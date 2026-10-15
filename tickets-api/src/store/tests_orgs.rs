//! ORG-4: organizations, their grants and their reach. An organization is
//! one write to make, empty, its first admin granted on it; two
//! organizations each have a root MENGER; a grant's scope is `*`, `org:` or
//! `org:ID`, kept by identity; the filter's `under acme:` is every ticket in
//! acme and none of another's; the gate's chain names the org; a rename
//! keeps every address and grant; a delete is refused while the org holds a
//! ticket; a file's grants from before ORG-4 move into hypermemetic once.

use super::*;
use crate::members::{Grant, Role};
use std::path::Path;

fn new(epic: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {kind} in {epic}"),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(str::to_string),
        path: None,
        body: String::new(),
    }
}

const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// hypermemetic's MENGER (with IDENTITY hosted under MENGER-1) and acme's
/// own MENGER, each with a build; acme administered by ADA.
async fn two_orgs(path: &Path) -> Store {
    let store = Store::open(path).unwrap();
    store.create(new("MENGER", Kind::Scope, None)).await.unwrap();
    store.create(new("MENGER", Kind::Build, None)).await.unwrap();
    store.create(new("IDENTITY", Kind::Scope, Some("MENGER-1"))).await.unwrap();
    let (org, admin) = store.create_org("acme", Some((ADA.into(), "--admin".into()))).await.unwrap();
    assert_eq!((org.name.as_str(), org.tickets, org.roots.len()), ("acme", 0, 0), "an organization is born empty");
    assert_eq!(admin.unwrap().scope, "acme:");
    let root = store.create(new("acme:MENGER", Kind::Scope, None)).await.unwrap();
    assert_eq!(root.address, "acme:MENGER-1");
    store.create(new("acme:MENGER", Kind::Build, None)).await.unwrap();
    store
}

fn lens(text: &str) -> Lens {
    Lens::parse(text).unwrap()
}

#[tokio::test]
async fn both_orgs_have_a_root_menger_each_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let store = two_orgs(&dir.path().join("t.db")).await;
    let mengers: Vec<String> = store.shortcodes().into_iter().filter(|(s, _)| s.code == "MENGER").map(|(_, a)| a).collect();
    assert_eq!(mengers, ["hypermemetic:MENGER", "acme:MENGER"]);
    let theirs = store.get(SYS, "acme:MENGER-2").await.unwrap();
    assert_eq!((theirs.epic.as_str(), theirs.address.as_str()), ("MENGER", "acme:MENGER-2"));
    assert_ne!(theirs.shortcode.as_str(), "MENGER", "a namesake's identity is its own");
    assert_eq!(store.get(SYS, "hypermemetic:MENGER-2").await.unwrap().title, "a build in MENGER");
    // The bare id fits both: the resolver names each by its full address
    // (the activation keeps those in the caller's organizations).
    match store.get(SYS, "MENGER-2").await {
        Err(StoreError::Ambiguous { candidates, .. }) => assert_eq!(candidates, ["hypermemetic:MENGER-2", "acme:MENGER-2"]),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    let orgs: Vec<(String, u32, u32)> = store.orgs().into_iter().map(|o| (o.name.to_string(), o.shortcodes, o.tickets)).collect();
    assert_eq!(orgs, [("hypermemetic".to_string(), 2, 3), ("acme".to_string(), 1, 2)]);
    assert!(matches!(Store::verify_derived(&dir.path().join("t.db")).unwrap(), Verdict::Consistent { .. }));
}

#[tokio::test]
async fn an_org_is_made_once_by_a_name_no_other_has_or_had() {
    let dir = tempfile::tempdir().unwrap();
    let store = two_orgs(&dir.path().join("t.db")).await;
    for bad in ["acme", "hypermemetic", "Acme", "ac me", "-x", ""] {
        assert!(store.create_org(bad, None).await.is_err(), "{bad:?}");
    }
    let (made, admin) = store.create_org("beta", None).await.unwrap();
    assert_eq!((made.name.as_str(), admin), ("beta", None));
    // A new root in an organization that is not there is refused.
    assert!(store.create(new("nope:X", Kind::Scope, None)).await.is_err());
}

#[tokio::test]
async fn under_an_org_is_every_ticket_in_it_and_none_of_another() {
    let dir = tempfile::tempdir().unwrap();
    let store = two_orgs(&dir.path().join("t.db")).await;
    let held = |text: &str| -> u64 { store.counts(SYS, &lens(text)).into_iter().map(|(_, n)| n).sum() };
    assert_eq!(held("under acme:"), 2);
    assert_eq!(held("under hypermemetic:"), 3, "hosted IDENTITY with it");
    assert_eq!(held("under beta:"), 0, "an org there is not holds nowhere");
    assert_eq!(held("kind build and under acme:"), 1);
    let ids = ["MENGER-2".to_string(), "IDENTITY-1".to_string()];
    assert_eq!(store.member(SYS, &ids, &lens("under acme:")), [false, false]);
    assert_eq!(store.member(SYS, &ids, &lens("under hypermemetic:")), [true, true]);
    // A reach of two orgs reads both.
    assert_eq!(held("under in (acme:, hypermemetic:)"), 5);
}

#[tokio::test]
async fn a_scope_is_kept_by_its_org_and_its_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = two_orgs(&dir.path().join("t.db")).await;
    let acme_menger = store.get(SYS, "acme:MENGER-1").await.unwrap().shortcode;
    assert_eq!(store.scope_named("*").unwrap(), "*");
    assert_eq!(store.scope_named("identity").unwrap(), "hypermemetic:IDENTITY");
    assert_eq!(store.scope_named("MENGER/IDENTITY").unwrap(), "hypermemetic:IDENTITY");
    assert_eq!(store.scope_named("acme:").unwrap(), "acme:");
    assert_eq!(store.scope_named("acme:MENGER").unwrap(), format!("acme:{acme_menger}"));
    assert_eq!(store.scope_named(&format!("acme:{acme_menger}")).unwrap(), format!("acme:{acme_menger}"));
    // A shortcode not born yet: its code in the org it names, else the first.
    assert_eq!(store.scope_named("acme:LATER").unwrap(), "acme:LATER");
    assert_eq!(store.scope_named("LATER").unwrap(), "hypermemetic:LATER");
    assert!(matches!(store.scope_named("nope:"), Err(StoreError::NoOrg(_))));
    assert!(matches!(store.scope_named("MENGER"), Err(StoreError::Ambiguous { .. })));
    assert!(store.scope_named("MENGER-1").is_err(), "a ticket is no grant's scope yet (ACCESS-21)");
}

#[tokio::test]
async fn the_gate_chain_names_the_org_and_a_ticket_as_written() {
    let dir = tempfile::tempdir().unwrap();
    let store = two_orgs(&dir.path().join("t.db")).await;
    let sc = store.get(SYS, "acme:MENGER-1").await.unwrap().shortcode;
    let chain = store.gate_chain("acme:MENGER-2").unwrap();
    assert_eq!(chain.first().map(String::as_str), Some("acme:MENGER-2"), "what a refusal names: as written");
    assert!(chain.contains(&format!("acme:{sc}")) && chain.last().map(String::as_str) == Some("acme:"), "{chain:?}");
    assert!(!chain.iter().any(|s| s.starts_with("hypermemetic")), "{chain:?}");
    let hyper = store.gate_chain("IDENTITY-1").unwrap();
    assert_eq!(hyper, ["IDENTITY-1", "hypermemetic:IDENTITY", "IDENTITY", "MENGER-1", "hypermemetic:MENGER", "MENGER", "hypermemetic:"]);
    // A missing ticket in acme is placed by its shortcode, in acme.
    assert_eq!(store.gate_chain("acme:MENGER-99").unwrap().last().map(String::as_str), Some("acme:"));
    assert_eq!(store.gate_chain("acme:").unwrap(), ["acme:"]);
    assert_eq!(store.gate_chain(&format!("acme:{sc}")).unwrap(), [format!("acme:{sc}"), sc.to_string(), "acme:".to_string()]);
    assert!(store.gate_chain("nope:").is_none());
}

#[tokio::test]
async fn a_rename_keeps_every_address_and_grant_and_the_old_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = two_orgs(&path).await;
        store.grant(Grant { principal: ADA.into(), scope: store.scope_named("acme:MENGER").unwrap(), role: Role::Editor, granted_by: "t".into(), at: 1 }).await.unwrap();
        let renamed = store.rename_org("acme", "acme-co").await.unwrap();
        assert_eq!((renamed.name.as_str(), renamed.aliases.clone()), ("acme-co", vec!["acme".to_string()]));
        assert_eq!(store.get(SYS, "acme:MENGER-2").await.unwrap().address, "acme-co:MENGER-2", "the old name resolves");
        let scopes: Vec<String> = store.grants_of(SYS, ADA).into_iter().map(|g| g.scope).collect();
        assert!(scopes.iter().all(|s| s.starts_with("acme-co:")), "{scopes:?}");
        assert!(store.create_org("acme", None).await.is_err(), "an old name still names it");
        assert_eq!(store.scope_named("acme:").unwrap(), "acme-co:");
        assert!(store.rename_org("acme-co", "hypermemetic").await.is_err());
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.org("acme").map(|o| o.name.to_string()).as_deref(), Some("acme-co"), "kept across a reopen");
    assert_eq!(store.get(SYS, "acme:MENGER-2").await.unwrap().address, "acme-co:MENGER-2");
    // Back to the old name: it is its own again, and no longer an alias.
    let back = store.rename_org("acme-co", "acme").await.unwrap();
    assert_eq!((back.name.as_str(), back.aliases), ("acme", vec!["acme-co".to_string()]));
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}

#[tokio::test]
async fn a_delete_is_refused_while_the_org_holds_a_ticket_and_takes_its_grants() {
    let dir = tempfile::tempdir().unwrap();
    let store = two_orgs(&dir.path().join("t.db")).await;
    let refused = store.delete_org("acme").await.unwrap_err().to_string();
    assert!(refused.contains("holds 1 shortcode(s) and 2 ticket(s)"), "{refused}");
    assert!(store.delete_org("hypermemetic").await.unwrap_err().to_string().contains("first organization"));
    store.create_org("beta", Some((ADA.into(), "t".into()))).await.unwrap();
    assert_eq!(store.delete_org("beta").await.unwrap(), (crate::address::OrgId::new("beta").unwrap(), 1));
    assert!(store.org("beta").is_none());
    assert!(!store.grants_of(SYS, ADA).iter().any(|g| g.scope.starts_with("beta:")));
    assert!(matches!(store.delete_org("beta").await, Err(StoreError::NoOrg(_))));
}

#[test]
fn a_file_s_grants_from_before_orgs_move_into_hypermemetic_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let rt = tokio::runtime::Runtime::new().unwrap();
    {
        let store = Store::open(&path).unwrap();
        rt.block_on(store.create(new("MENGER", Kind::Scope, None))).unwrap();
        rt.block_on(store.create(new("IDENTITY", Kind::Scope, Some("MENGER-1")))).unwrap();
        // As an ORG-3 build kept them: a bare shortcode, and `*`.
        for (scope, role) in [("IDENTITY", Role::Editor), ("*", Role::Admin), ("LATER", Role::Viewer)] {
            let g = Grant { principal: ADA.into(), scope: scope.into(), role, granted_by: "t".into(), at: 1 };
            rt.block_on(store.grant(g)).unwrap();
        }
    }
    let scopes = |s: &Store| -> Vec<String> { s.grants_of(SYS, ADA).into_iter().map(|g| g.scope).collect() };
    let store = Store::open(&path).unwrap();
    assert_eq!(scopes(&store), ["*", "hypermemetic:IDENTITY", "hypermemetic:LATER"]);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(scopes(&store), ["*", "hypermemetic:IDENTITY", "hypermemetic:LATER"], "once: the next open reads them");
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));
}
