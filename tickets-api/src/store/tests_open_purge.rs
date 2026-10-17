//! ACCESS-24: the purge of what open mode planted — every grant and team
//! membership signed `--admin` that no `--admin` principal accounts for,
//! removed once, durably, with its mark; the grants made under login (a
//! principal's, the dev sign-in's, `auth.grant`'s) untouched.

use super::*;
use crate::address::OrgId;
use crate::members::{Grant, Role, Scope};
use crate::vocab::{GrantedBy, Grantee, Principal, TeamName};
use std::path::Path;

/// The operator's `--admin` at this start.
const OPERATOR_KEY: &str = "key:ba720a3d0cfbbf974cf6d57fe57832ad2c2c6198375691be98e28afbe3974b8e";
/// An agent the operator granted under login.
const BUILDER: &str = "key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28";
/// A stranger who granted themselves admin while open (ACCESS-4 C2).
const STRANGER: &str = "nostr:0000000000000000000000000000000000000000000000000000000000000001";
const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn hyper() -> OrgId {
    OrgId::new(OrgId::HYPERMEMETIC).unwrap()
}

fn sc(id: &str) -> Scope {
    Scope::Shortcode { org: hyper(), id: crate::address::ShortcodeId::new(id) }
}

fn grant(principal: &str, scope: Scope, role: Role, by: GrantedBy) -> Grant {
    Grant { principal: principal.parse().unwrap(), scope, role, granted_by: by, at: 1 }
}

fn by(p: &str) -> GrantedBy {
    GrantedBy::Principal(p.parse::<Principal>().unwrap())
}

async fn store_at(path: &Path) -> Store {
    let s = Store::open(path).unwrap();
    let new = NewTicket { epic: "MENGER".into(), title: "menger".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() };
    s.create(SYS, new).await.unwrap();
    s
}

/// Each stored grant as `principal role on scope`, sorted.
fn held(s: &Store) -> Vec<String> {
    let mut out: Vec<String> = s.grants(SYS).iter().map(|g| format!("{} {} on {}", g.principal, g.role.as_str(), g.scope.text())).collect();
    out.sort();
    out
}

#[tokio::test]
async fn what_open_mode_planted_is_purged_once_and_what_login_wrote_stays() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let s = store_at(&path).await;
        // Under login: signed by the operator's key (ACCESS-47's grants), the
        // dev sign-in, auth.grant.
        s.grant(SYS, grant(BUILDER, Scope::Everywhere, Role::Editor, by(OPERATOR_KEY))).await.unwrap();
        s.grant(SYS, grant(ADA, sc("MENGER"), Role::Viewer, GrantedBy::DevLogin)).await.unwrap();
        s.grant(SYS, grant(ADA, Scope::Everywhere, Role::Viewer, GrantedBy::AuthGrant)).await.unwrap();
        // While open: signed `--admin` — a stranger's admin on *, an org's
        // admin (ACCESS-51), and one the operator's own flag accounts for.
        s.grant(SYS, grant(STRANGER, Scope::Everywhere, Role::Admin, GrantedBy::Operator)).await.unwrap();
        s.grant(SYS, grant(STRANGER, Scope::Org(hyper()), Role::Admin, GrantedBy::Operator)).await.unwrap();
        s.grant(SYS, grant(OPERATOR_KEY, Scope::Everywhere, Role::Admin, GrantedBy::Operator)).await.unwrap();
        // A team made under login, a member added to it while open.
        let crew: TeamName = "crew".parse().unwrap();
        s.create_team(SYS, crew.clone(), &hyper(), by(OPERATOR_KEY)).await.unwrap();
        s.grant(SYS, Grant { principal: Grantee::Team(crew.clone()), scope: sc("MENGER"), role: Role::Editor, granted_by: by(OPERATOR_KEY), at: 1 }).await.unwrap();
        s.add_to_team(SYS, &crew, &STRANGER.parse().unwrap(), GrantedBy::Operator).await.unwrap();
        s.add_to_team(SYS, &crew, &ADA.parse().unwrap(), by(OPERATOR_KEY)).await.unwrap();
        assert_eq!(s.teams_of(STRANGER), vec![crew.clone()]);

        let before = s.grants_generation();
        let ran = s.purge_open_grants(SYS, &[OPERATOR_KEY.to_string()], 1_792_200_000).await.unwrap();
        assert_eq!(
            ran,
            OpenPurge::Ran {
                removed: vec![
                    format!("{STRANGER} admin on *"),
                    format!("{STRANGER} admin on hypermemetic:"),
                    format!("{STRANGER} in team:crew"),
                ],
                kept: vec![format!("{OPERATOR_KEY} admin on *")],
            }
        );
        assert!(s.grants_generation() > before, "an open watch verifies its caller again");
        // The cache holds what the file holds: no grant and no team for the stranger.
        assert!(s.grants_of(SYS, STRANGER).is_empty());
        assert!(s.teams_of(STRANGER).is_empty());
        assert_eq!(s.teams_of(ADA), vec![crew]);
    }
    let want = vec![
        format!("{BUILDER} editor on *"),
        format!("{ADA} viewer on *"),
        format!("{ADA} viewer on hypermemetic:MENGER"),
        format!("{OPERATOR_KEY} admin on *"),
        "team:crew editor on hypermemetic:MENGER".to_string(),
    ];
    // Reopened: the purge was durable, and it does not run again — not even
    // for a row signed `--admin` that a later open is handed.
    let s = Store::open(&path).unwrap();
    assert_eq!(held(&s), want);
    assert!(s.teams_of(STRANGER).is_empty());
    assert_eq!(s.purge_open_grants(SYS, &[], 1_792_300_000).await.unwrap(), OpenPurge::Done { at: 1_792_200_000 });
    assert_eq!(held(&s), want, "once: the operator's row stays though no --admin names it now");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (removed, kept): (String, String) = conn.query_row("SELECT removed, kept FROM open_purge", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(removed.lines().count(), 3, "{removed}");
    assert_eq!(kept, format!("{OPERATOR_KEY} admin on *"));
}

#[tokio::test]
async fn a_database_with_nothing_planted_is_marked_and_keeps_every_grant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let s = store_at(&path).await;
    s.grant(SYS, grant(BUILDER, Scope::Everywhere, Role::Editor, by(OPERATOR_KEY))).await.unwrap();
    assert_eq!(s.purge_open_grants(SYS, &[OPERATOR_KEY.to_string()], 7).await.unwrap(), OpenPurge::Ran { removed: vec![], kept: vec![] });
    assert_eq!(held(&s), vec![format!("{BUILDER} editor on *")]);
    assert_eq!(s.purge_open_grants(SYS, &[], 8).await.unwrap(), OpenPurge::Done { at: 7 });
}
