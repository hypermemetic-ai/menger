//! V2-41: a person's own organization — made with their first grant (by
//! name, through a team, as a new organization's admin), never for a key;
//! always editor in it; never deleted; backfilled at a start.

use super::*;
use crate::address::OrgId;
use crate::members::{Grant, Role, Scope};
use crate::vocab::{GrantedBy, Grantee, Principal, TeamName};

const ADA: &str = "oidc:workos/user_01ADA";
const GRACE: &str = "idp:0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0";
const LIN: &str = "nostr:1111111111111111111111111111111111111111111111111111111111111111";
const BUILDER: &str = "key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28";

fn hyper() -> OrgId {
    OrgId::new(OrgId::HYPERMEMETIC).unwrap()
}

fn sc(id: &str) -> Scope {
    Scope::Shortcode { org: hyper(), id: crate::address::ShortcodeId::new(id) }
}

fn grant(principal: &str, scope: Scope, role: Role) -> Grant {
    Grant { principal: principal.parse().unwrap(), scope, role, granted_by: GrantedBy::AuthGrant, at: 1 }
}

fn mine(p: &str) -> Scope {
    Scope::Org(personal_name(p, 10))
}

/// A store on a file of its own; the directory goes with the test.
async fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("t.db")).unwrap();
    let new = NewTicket { epic: "MENGER".into(), title: "menger".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() };
    s.create(SYS, new).await.unwrap();
    (dir, s)
}

/// The organizations someone owns, by name.
fn owned(s: &Store, p: &str) -> Vec<String> {
    s.orgs().into_iter().filter(|o| o.owner.as_ref().is_some_and(|w| w.as_str() == p)).map(|o| o.name.to_string()).collect()
}

fn role_on(s: &Store, p: &str, scope: &Scope) -> Option<Role> {
    s.grants(SYS).into_iter().find(|g| g.principal.to_string() == p && g.scope == *scope).map(|g| g.role)
}

#[test]
fn the_name_is_the_principal_hashed() {
    let name = personal_name(ADA, 10);
    assert!(name.as_str().starts_with("u-") && name.as_str().len() == 12, "{name}");
    assert_eq!(name, personal_name(ADA, 10));
    assert_ne!(name, personal_name(GRACE, 10));
    assert!(is_person(ADA) && is_person(GRACE) && is_person(LIN));
    assert!(!is_person(BUILDER) && !is_person("apikey:ci"));
}

#[tokio::test]
async fn a_persons_first_grant_makes_their_own_organization_once_and_they_are_editor_in_it() {
    let (_dir, s) = store().await;
    let before = s.orgs().len();
    s.grant(SYS, grant(ADA, sc("MENGER"), Role::Viewer)).await.unwrap();
    assert_eq!(owned(&s, ADA), vec![personal_name(ADA, 10).to_string()]);
    assert_eq!(role_on(&s, ADA, &mine(ADA)), Some(Role::Editor));
    let g = s.grants(SYS).into_iter().find(|g| g.scope == mine(ADA)).unwrap();
    assert_eq!(g.granted_by, GrantedBy::Personal);
    // A second grant makes nothing more.
    s.grant(SYS, grant(ADA, Scope::Org(hyper()), Role::Editor)).await.unwrap();
    assert_eq!(s.orgs().len(), before + 1);
    // And it is a real organization: editor on it roots a shortcode there.
    let home = personal_name(ADA, 10);
    let new = NewTicket { epic: format!("{home}:NOTES"), title: "notes".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() };
    s.create(SYS, new).await.unwrap();
    assert_eq!(s.org(home.as_str()).unwrap().tickets, 1);
}

#[tokio::test]
async fn a_key_gets_no_organization_of_its_own() {
    let (_dir, s) = store().await;
    let before = s.orgs().len();
    s.grant(SYS, grant(BUILDER, Scope::Everywhere, Role::Editor)).await.unwrap();
    assert_eq!(s.orgs().len(), before);
}

#[tokio::test]
async fn a_team_grant_and_a_team_add_accept_its_people() {
    let (_dir, s) = store().await;
    let crew: TeamName = "crew".parse().unwrap();
    s.create_team(SYS, crew.clone(), &hyper(), GrantedBy::AuthGrant).await.unwrap();
    // Into a team that holds nothing: not accepted yet.
    s.add_to_team(SYS, &crew, &ADA.parse().unwrap(), GrantedBy::AuthGrant).await.unwrap();
    assert!(owned(&s, ADA).is_empty());
    // The team is granted: its people are.
    s.grant(SYS, Grant { principal: Grantee::Team(crew.clone()), scope: sc("MENGER"), role: Role::Viewer, granted_by: GrantedBy::AuthGrant, at: 1 }).await.unwrap();
    assert_eq!(owned(&s, ADA).len(), 1);
    // Added to a team that holds a grant: accepted.
    s.add_to_team(SYS, &crew, &GRACE.parse().unwrap(), GrantedBy::AuthGrant).await.unwrap();
    assert_eq!(owned(&s, GRACE).len(), 1);
}

#[tokio::test]
async fn a_new_organizations_admin_is_accepted_beside_it() {
    let (_dir, s) = store().await;
    let ada: Principal = ADA.parse().unwrap();
    s.create_org(SYS, "acme", Some((ada, GrantedBy::AuthGrant))).await.unwrap();
    assert_eq!(owned(&s, ADA).len(), 1);
    assert!(s.org("acme").unwrap().owner.is_none());
    assert_eq!(role_on(&s, ADA, &mine(ADA)), Some(Role::Editor));
}

#[tokio::test]
async fn the_owner_is_always_editor_and_the_organization_is_never_deleted() {
    let (_dir, s) = store().await;
    s.grant(SYS, grant(ADA, sc("MENGER"), Role::Viewer)).await.unwrap();
    let home = mine(ADA);
    // Below editor: refused.
    for role in [Role::Viewer, Role::Deployer] {
        let e = s.grant(SYS, grant(ADA, home.clone(), role)).await.unwrap_err().to_string();
        assert!(e.contains("always editor"), "{e}");
    }
    // Editor's revoke: refused.
    let e = s.revoke(SYS, ADA.parse().unwrap(), home.clone()).await.unwrap_err().to_string();
    assert!(e.contains("always editor"), "{e}");
    assert_eq!(role_on(&s, ADA, &home), Some(Role::Editor));
    // Admin may be given; its revoke puts editor back.
    s.grant(SYS, grant(ADA, home.clone(), Role::Admin)).await.unwrap();
    assert!(s.revoke(SYS, ADA.parse().unwrap(), home.clone()).await.unwrap());
    assert_eq!(role_on(&s, ADA, &home), Some(Role::Editor));
    // Someone else on it is anyone's grant.
    s.grant(SYS, grant(GRACE, home.clone(), Role::Viewer)).await.unwrap();
    assert!(s.revoke(SYS, GRACE.parse().unwrap(), home.clone()).await.unwrap());
    // Never deleted, empty as it is; renamed, still theirs.
    let name = personal_name(ADA, 10);
    let e = s.delete_org(SYS, name.as_str()).await.unwrap_err().to_string();
    assert!(e.contains("never deleted"), "{e}");
    s.rename_org(SYS, name.as_str(), "ada").await.unwrap();
    assert_eq!(owned(&s, ADA), vec!["ada".to_string()]);
    let e = s.revoke(SYS, ADA.parse().unwrap(), Scope::Org(OrgId::new("ada").unwrap())).await.unwrap_err().to_string();
    assert!(e.contains("always editor"), "{e}");
}

#[tokio::test]
async fn a_taken_name_takes_more_of_the_hash() {
    let (_dir, s) = store().await;
    let short = personal_name(ADA, 10);
    s.create_org(SYS, short.as_str(), None).await.unwrap();
    s.grant(SYS, grant(ADA, sc("MENGER"), Role::Viewer)).await.unwrap();
    assert_eq!(owned(&s, ADA), vec![personal_name(ADA, 12).to_string()]);
}

#[tokio::test]
async fn the_backfill_gives_everyone_accepted_theirs_once_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let s = Store::open(&path).unwrap();
        let new = NewTicket { epic: "MENGER".into(), title: "menger".into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: String::new() };
        s.create(SYS, new).await.unwrap();
        // As a file from before V2-41 holds them: grants, no personal_orgs.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for (p, scope) in [(ADA, "hypermemetic:MENGER"), (BUILDER, "*")] {
            members::store_row(&conn, p, scope, Role::Viewer, "auth.grant", 1).unwrap();
        }
    }
    let s = Store::open(&path).unwrap();
    let made = s.ensure_personal_orgs(SYS, &[LIN.to_string(), BUILDER.to_string()]).await.unwrap();
    let mut names: Vec<String> = made.iter().map(|o| o.to_string()).collect();
    names.sort();
    let mut want = vec![personal_name(ADA, 10).to_string(), personal_name(LIN, 10).to_string()];
    want.sort();
    assert_eq!(names, want);
    assert!(s.ensure_personal_orgs(SYS, &[LIN.to_string()]).await.unwrap().is_empty());
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(owned(&s, ADA).len(), 1);
    assert_eq!(role_on(&s, ADA, &mine(ADA)), Some(Role::Editor));
    assert!(s.ensure_personal_orgs(SYS, &[]).await.unwrap().is_empty());
}
