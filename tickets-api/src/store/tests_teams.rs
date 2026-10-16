//! ACCESS-22: the teams store — made, joined, left, renamed and deleted,
//! each durable and in the cache at once; a team's grants go with its name
//! and its end; a change needs admin on every scope it holds, checked under
//! the write lock; a form-12 file gains the tables in place.

use super::*;
use crate::access::AdminOf;
use crate::address::OrgId;
use crate::members::{Grant, Role, Scope};
use crate::vocab::{GrantedBy, Grantee, Principal, TeamName};
use std::path::Path;

const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "key:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn new(epic: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket { epic: epic.into(), title: format!("a {kind} in {epic}"), kind, severity: None, blocked_by: Vec::new(), parent: parent.map(str::to_string), path: None, body: String::new() }
}

fn hyper() -> OrgId {
    OrgId::new(OrgId::HYPERMEMETIC).unwrap()
}

fn sc(id: &str) -> Scope {
    Scope::Shortcode { org: hyper(), id: crate::address::ShortcodeId::new(id) }
}

fn team(name: &str) -> TeamName {
    name.parse().unwrap()
}

fn who(p: &str) -> Principal {
    p.parse().unwrap()
}

fn to_team(name: &str, scope: Scope, role: Role) -> Grant {
    Grant { principal: Grantee::Team(team(name)), scope, role, granted_by: GrantedBy::Operator, at: 1 }
}

/// IDENTITY and OUGHT, each a root and a build.
async fn store_at(path: &Path) -> Store {
    let s = Store::open(path).unwrap();
    for epic in ["IDENTITY", "OUGHT"] {
        s.create(SYS, new(epic, Kind::Scope, None)).await.unwrap();
        s.create(SYS, new(epic, Kind::Build, Some(&format!("{epic}-1")))).await.unwrap();
    }
    s
}

#[tokio::test]
async fn a_team_is_made_joined_renamed_and_deleted_durably() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let s = store_at(&path).await;
    let made = s.create_team(SYS, team("crew"), &hyper(), GrantedBy::Operator).await.unwrap();
    assert_eq!((made.name.as_str(), made.org.as_str(), made.members), ("crew", "hypermemetic", 0));
    assert!(s.create_team(SYS, team("crew"), &hyper(), GrantedBy::Operator).await.is_err(), "a name is unique");
    assert!(s.create_team(SYS, team("other"), &OrgId::new("nope").unwrap(), GrantedBy::Operator).await.is_err(), "an org there is");
    s.grant(SYS, to_team("crew", sc("OUGHT"), Role::Editor)).await.unwrap();
    let joined = s.add_to_team(SYS, &team("crew"), &who(ADA), GrantedBy::Operator).await.unwrap();
    assert_eq!(joined.principal.as_str(), ADA);
    assert_eq!(s.add_to_team(SYS, &team("crew"), &who(ADA), GrantedBy::Operator).await.unwrap(), joined, "in it already: as it was");
    s.add_to_team(SYS, &team("crew"), &who(BOB), GrantedBy::Operator).await.unwrap();
    assert_eq!(s.teams_of(ADA), [team("crew")]);
    drop(s);

    // Durable: the next open reads every row.
    let s = Store::open(&path).unwrap();
    let (t, members, grants) = s.team(SYS, &team("crew")).unwrap();
    assert_eq!(t.members, 2);
    assert_eq!(members.iter().map(|m| m.principal.as_str()).collect::<Vec<_>>(), [ADA, BOB]);
    assert_eq!(grants.iter().map(|g| (g.scope.text(), g.role)).collect::<Vec<_>>(), [("hypermemetic:OUGHT".to_string(), Role::Editor)]);
    assert_eq!(s.teams_of(BOB), [team("crew")]);

    // A rename takes its members and its grants.
    let renamed = s.rename_team(SYS, &team("crew"), team("release-crew")).await.unwrap();
    assert_eq!((renamed.name.as_str(), renamed.members), ("release-crew", 2));
    assert!(s.team(SYS, &team("crew")).is_none());
    assert_eq!(s.teams_of(ADA), [team("release-crew")]);
    assert!(s.grants(SYS).iter().all(|g| g.principal.to_string() != "team:crew"));
    assert_eq!(s.grants_of(SYS, "team:release-crew").len(), 1);
    assert!(s.remove_from_team(SYS, &team("release-crew"), &who(BOB)).await.unwrap());
    assert!(!s.remove_from_team(SYS, &team("release-crew"), &who(BOB)).await.unwrap(), "not in it");
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.team(SYS, &team("release-crew")).unwrap().1.len(), 1);
    assert_eq!(s.grants_of(SYS, "team:release-crew").len(), 1, "the grant moved with the name, durably");

    // A delete takes its grants and its members.
    assert_eq!(s.delete_team(SYS, &team("release-crew")).await.unwrap(), (1, 1));
    assert!(s.teams(SYS, None).is_empty() && s.teams_of(ADA).is_empty() && s.grants(SYS).is_empty());
    drop(s);
    let s = Store::open(&path).unwrap();
    assert!(s.teams(SYS, None).is_empty() && s.grants(SYS).is_empty(), "gone, durably");
    assert!(s.grant(SYS, to_team("release-crew", sc("OUGHT"), Role::Viewer)).await.is_err(), "no such team");
}

/// The owner's rule (2026-10-09), as the store holds it: a change needs
/// admin on every scope the team holds — checked under the write lock
/// against the grants as they stand.
#[tokio::test]
async fn a_change_needs_admin_on_every_scope_the_team_holds() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_at(&dir.path().join("t.db")).await;
    s.create_team(SYS, team("crew"), &hyper(), GrantedBy::Operator).await.unwrap();
    s.grant(SYS, to_team("crew", sc("OUGHT"), Role::Viewer)).await.unwrap();
    s.grant(SYS, to_team("crew", sc("IDENTITY"), Role::Viewer)).await.unwrap();
    let admin = |scope: Scope| AdminOf::for_tests(scope);
    // An admin of IDENTITY only cannot add anyone to a team holding OUGHT.
    let only = OnTeam::new(vec![admin(sc("IDENTITY"))]);
    let e = s.add_to_team(only, &team("crew"), &who(BOB), GrantedBy::Operator).await.unwrap_err();
    assert!(e.to_string().contains("hypermemetic:OUGHT"), "{e}");
    let both = || OnTeam::new(vec![admin(sc("IDENTITY")), admin(sc("OUGHT"))]);
    s.add_to_team(both(), &team("crew"), &who(BOB), GrantedBy::Operator).await.unwrap();
    // Admin of `*` covers any.
    s.remove_from_team(OnTeam::new(vec![admin(Scope::Everywhere)]), &team("crew"), &who(BOB)).await.unwrap();
    // A proof minted before the team gained a scope covers no more than it did.
    s.grant(SYS, to_team("crew", Scope::Ticket(crate::address::TicketId::new("OUGHT-2")), Role::Editor)).await.unwrap();
    let stale = s.rename_team(both(), &team("crew"), team("crew-2")).await.unwrap_err();
    assert!(stale.to_string().contains("OUGHT-2"), "{stale}");
    assert!(s.delete_team(OnTeam::new(Vec::new()), &team("crew")).await.is_err(), "no proof is none");
    assert_eq!(s.team(SYS, &team("crew")).unwrap().0.members, 0, "nothing written");
}

/// ACCESS-22: an organization holding a team is not deleted.
#[tokio::test]
async fn an_org_holding_a_team_is_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_at(&dir.path().join("t.db")).await;
    s.create_org(SYS, "acme", None).await.unwrap();
    let acme = OrgId::new("acme").unwrap();
    s.create_team(SYS, team("acme-crew"), &acme, GrantedBy::Operator).await.unwrap();
    assert_eq!(s.teams(SYS, Some(&[acme.clone()])).len(), 1);
    assert!(s.teams(SYS, Some(&[hyper()])).is_empty(), "in its own org");
    assert!(s.delete_org(SYS, "acme").await.unwrap_err().to_string().contains("team"));
    s.delete_team(SYS, &team("acme-crew")).await.unwrap();
    s.delete_org(SYS, "acme").await.unwrap();
}

/// ACCESS-22: stored form 13 — a form-12 file (no teams tables) gains them,
/// empty, in place on its first open, keeps its grants, builds nothing
/// again; its teams are kept from then on.
#[test]
fn a_form_12_file_gains_the_teams_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form12.db");
    let rt = tokio::runtime::Runtime::new().unwrap();
    {
        let s = rt.block_on(store_at(&path));
        rt.block_on(s.grant(SYS, Grant { principal: Grantee::Principal(who(ADA)), scope: sc("OUGHT"), role: Role::Admin, granted_by: GrantedBy::Operator, at: 1 })).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE teams; DROP TABLE team_members; UPDATE derived_state SET form = 12;").unwrap();
    }
    let state = |p: &Path| stored::state(&rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()).unwrap();
    assert_eq!(state(&path), stored::State::Older(12));
    let s = Store::open(&path).unwrap();
    assert_eq!(state(&path), stored::State::Current);
    assert_eq!(s.grants_of(SYS, ADA).len(), 1, "its grants kept");
    assert!(s.teams(SYS, None).is_empty());
    rt.block_on(s.create_team(SYS, team("crew"), &hyper(), GrantedBy::Operator)).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.teams(SYS, None).len(), 1);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}
