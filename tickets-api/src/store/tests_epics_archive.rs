//! ORG-23: each person stays on their own epic — the default, never the
//! newest; making an epic moves nobody — and an admin archives one: the
//! people on it move to the next newer epic (else the next older) in one
//! write, the last live epic is refused, nothing switches to, is made in or
//! moves into an archived epic, restore moves nobody, the views drop its
//! tickets unless a filter names it or `archived`, and the marks are kept.

use super::*;
use crate::access::On;
use crate::address::{EpicName, OrgId};
use crate::members::{Grant, Role, Scope};

const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BO: &str = "key:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CY: &str = "key:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn hm() -> OrgId {
    OrgId::hypermemetic()
}

fn name(n: &str) -> EpicName {
    EpicName::new(n).unwrap()
}

fn new(epic: &str, title: &str, kind: Kind) -> NewTicket {
    NewTicket { path: None, epic: epic.into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: None, body: String::new() }
}

fn held(store: &Store, text: &str) -> u64 {
    store.counts(SYS, &Lens::parse(text).unwrap()).into_iter().map(|(_, n)| n).sum()
}

async fn switch(store: &Store, who: &str, to: &str) -> Result<crate::epics::Epic, StoreError> {
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), to, who).await
}

fn on(store: &Store, who: &str) -> String {
    store.current_epic(&hm(), who).unwrap().name.to_string()
}

/// genesis (MENGER-1..2), spring (MENGER-1 again, a scope and a build),
/// fresh (KILN-1); ADA, BO and CY editors of the org. ADA on spring, CY on
/// fresh, BO never switched (the default, genesis).
async fn fixture(store: &Store) {
    store.create(SYS, new("MENGER", "menger", Kind::Scope)).await.unwrap();
    store.create(SYS, new("MENGER", "two", Kind::Build)).await.unwrap();
    for e in ["spring", "fresh"] {
        store.create_epic(SYS, &hm(), &name(e), ADA).await.unwrap();
    }
    store.create(SYS, new("hypermemetic:spring:MENGER", "menger, spring", Kind::Scope)).await.unwrap();
    store.create(SYS, new("hypermemetic:spring:MENGER", "spring two", Kind::Build)).await.unwrap();
    store.create(SYS, new("hypermemetic:fresh:KILN", "kiln", Kind::Scope)).await.unwrap();
    for p in [ADA, BO, CY] {
        let g = Grant { principal: p.parse().unwrap(), scope: Scope::Org(hm()), role: Role::Editor, granted_by: crate::vocab::GrantedBy::AuthGrant, at: 1 };
        store.grant(SYS, g).await.unwrap();
    }
    switch(store, ADA, "spring").await.unwrap();
    switch(store, CY, "fresh").await.unwrap();
}

fn counts_on(store: &Store) -> Vec<(String, u32, bool)> {
    store.epics(&hm(), ADA, true).unwrap().into_iter().map(|e| (e.name.to_string(), e.members_on, e.default)).collect()
}

#[tokio::test]
async fn a_principal_who_never_switched_is_on_the_default_and_making_an_epic_moves_nobody() {
    let store = Store::new();
    fixture(&store).await;
    assert_eq!((on(&store, BO), on(&store, ADA), on(&store, CY)), ("genesis".into(), "spring".into(), "fresh".into()));
    // A new epic, by ADA: nobody moves — ADA neither.
    let summer = store.create_epic(SYS, &hm(), &name("summer"), ADA).await.unwrap();
    assert!(!summer.current && !summer.default && summer.members_on == 0);
    assert_eq!((on(&store, BO), on(&store, ADA), on(&store, CY)), ("genesis".into(), "spring".into(), "fresh".into()));
    assert_eq!(counts_on(&store), [("genesis".into(), 1, true), ("spring".into(), 1, false), ("fresh".into(), 1, false), ("summer".into(), 0, false)]);
    // An admin moves the default: those on it by default move with it.
    let d = store.set_default_epic(SYS, &hm(), "fresh", ADA).await.unwrap();
    assert!(d.default);
    assert_eq!(on(&store, BO), "fresh");
    assert_eq!(on(&store, ADA), "spring", "a choice stays");
}

#[tokio::test]
async fn archive_moves_exactly_the_people_on_it_to_the_next_newer_in_one_write() {
    let store = Store::new();
    fixture(&store).await;
    // Dry: the answer, and nothing written.
    let dry = store.archive_epic(SYS, &hm(), "spring", true, BO).await.unwrap();
    assert_eq!((dry.moved.clone(), dry.to.as_str(), dry.tickets), (vec![ADA.to_string()], "fresh", 2));
    assert_eq!(on(&store, ADA), "spring");
    assert!(store.epic(&hm(), "spring", ADA).unwrap().archived.is_none());
    // The write.
    let done = store.archive_epic(SYS, &hm(), "spring", false, BO).await.unwrap();
    assert_eq!((done.moved, done.to.as_str()), (vec![ADA.to_string()], "fresh"));
    let mark = done.epic.archived.expect("marked");
    assert_eq!(mark.by, BO);
    assert_eq!((on(&store, ADA), on(&store, BO), on(&store, CY)), ("fresh".into(), "genesis".into(), "fresh".into()));
    // The notice: ADA's alone, until dismissed.
    let mv = store.epic_move(&hm(), ADA).expect("ADA was moved");
    assert_eq!((mv.from.as_str(), mv.to.as_str(), mv.by.as_str(), mv.reason.as_str()), ("spring", "fresh", BO, "archived"));
    assert_eq!(store.epic_move(&hm(), CY), None);
    store.dismiss_epic_move(On::for_tests(Scope::Org(hm())), &hm(), ADA).await.unwrap();
    assert_eq!(store.epic_move(&hm(), ADA), None);
    // Listed only with archived; counted where they are now.
    assert_eq!(store.epics(&hm(), ADA, false).unwrap().iter().map(|e| e.name.to_string()).collect::<Vec<_>>(), ["genesis", "fresh"]);
    assert_eq!(counts_on(&store), [("genesis".into(), 1, true), ("spring".into(), 0, false), ("fresh".into(), 2, false)]);
    assert!(store.archive_epic(SYS, &hm(), "spring", false, BO).await.is_err(), "archived already");
}

#[tokio::test]
async fn the_newest_archived_sends_its_people_to_the_next_older_and_the_last_is_refused() {
    let store = Store::new();
    fixture(&store).await;
    let a = store.archive_epic(SYS, &hm(), "fresh", false, BO).await.unwrap();
    assert_eq!((a.moved, a.to.as_str()), (vec![CY.to_string()], "spring"));
    assert_eq!(on(&store, CY), "spring");
    // spring next: the next newer is archived, so the next older live one.
    let b = store.archive_epic(SYS, &hm(), "spring", false, BO).await.unwrap();
    assert_eq!(b.to.as_str(), "genesis");
    let mut moved = b.moved.clone();
    moved.sort();
    assert_eq!(moved, [ADA.to_string(), CY.to_string()]);
    // genesis is the last live epic.
    match store.archive_epic(SYS, &hm(), "genesis", true, BO).await {
        Err(StoreError::Invalid(why)) => assert!(why.contains("genesis is hypermemetic's last epic: create another first"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn archiving_the_default_moves_the_default_and_those_on_it_by_default() {
    let store = Store::new();
    fixture(&store).await;
    let a = store.archive_epic(SYS, &hm(), "genesis", false, ADA).await.unwrap();
    assert_eq!((a.moved, a.to.as_str()), (vec![BO.to_string()], "spring"), "BO was on it by default");
    assert_eq!(on(&store, BO), "spring");
    assert!(store.epic(&hm(), "spring", BO).unwrap().default);
    assert_eq!(store.epic_move(&hm(), BO).unwrap().from.as_str(), "genesis");
}

#[tokio::test]
async fn nothing_switches_to_is_made_in_or_moves_into_an_archived_epic_and_restore_moves_nobody() {
    let store = Store::new();
    fixture(&store).await;
    store.archive_epic(SYS, &hm(), "spring", false, BO).await.unwrap();
    let refused = |r: Result<String, StoreError>| match r {
        Err(StoreError::Invalid(why)) => assert!(why.contains("is archived: restore it first"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    };
    refused(switch(&store, BO, "spring").await.map(|e| e.name.to_string()));
    refused(store.create(SYS, new("hypermemetic:spring:MENGER", "x", Kind::Build)).await.map(|t| t.id.to_string()));
    refused(store.create(SYS, new("hypermemetic:spring:NEW", "x", Kind::Scope)).await.map(|t| t.id.to_string()));
    let into = Reshortcode { ids: vec!["hypermemetic:MENGER-2".into()], to: "hypermemetic:spring:MENGER".into(), ..Reshortcode::default() };
    refused(store.reshortcode(SYS, into).await.map(|_| String::new()));
    refused(store.set_default_epic(SYS, &hm(), "spring", BO).await.map(|e| e.name.to_string()));
    // Restore: live again, nobody moves back.
    let back = store.restore_epic(SYS, &hm(), "spring", BO).await.unwrap();
    assert!(back.archived.is_none());
    assert_eq!(on(&store, ADA), "fresh");
    assert!(store.restore_epic(SYS, &hm(), "spring", BO).await.is_err(), "not archived");
    switch(&store, ADA, "spring").await.unwrap();
}

#[tokio::test]
async fn an_archived_epics_tickets_leave_the_views_and_answer_by_name() {
    let store = Store::new();
    fixture(&store).await;
    assert_eq!(held(&store, "epic any"), 5);
    store.archive_epic(SYS, &hm(), "spring", false, BO).await.unwrap();
    assert_eq!(held(&store, "epic any"), 3, "every live epic");
    assert_eq!(held(&store, "epic spring"), 2, "named, it answers");
    // A reader's view: its epic; with `archived`, the archived epics too.
    let plain = crate::filter::Filter::parse("kind scope").unwrap();
    assert_eq!(store.era_filter(ADA, None, &plain).unwrap().format(), "epic hypermemetic:fresh");
    let asks = crate::filter::Filter::parse("kind scope and archived").unwrap();
    assert_eq!(store.era_filter(ADA, None, &asks).unwrap().format(), "epic in (hypermemetic:spring, hypermemetic:fresh)");
    // get still answers, with the mark.
    let t = store.get(SYS, "hypermemetic:spring:MENGER-2").await.unwrap();
    let mark = t.epic_archived.expect("its epic's mark");
    assert_eq!((mark.name.as_str(), mark.by.as_str()), ("spring", BO));
    assert_eq!(store.get(SYS, "hypermemetic:MENGER-2").await.unwrap().epic_archived, None, "genesis's is live");
    assert!(!store.in_current_era(ADA, &store.get(SYS, "hypermemetic:spring:MENGER-1").await.unwrap().shortcode));
}

#[tokio::test]
async fn a_renamed_epic_answers_to_its_old_name() {
    let store = Store::new();
    fixture(&store).await;
    store.rename_epic(SYS, &hm(), "spring", &name("vernal"), ADA).await.unwrap();
    assert_eq!(held(&store, "epic spring"), 2, "a saved filter naming it still works");
    assert_eq!(store.get(SYS, "hypermemetic:spring:MENGER-2").await.unwrap().address, "hypermemetic:vernal:MENGER-2");
    assert_eq!(store.epic(&hm(), "spring", ADA).unwrap().name.as_str(), "vernal");
    assert!(store.create_epic(SYS, &hm(), &name("spring"), ADA).await.is_err(), "the alias holds the name");
    // Renamed back: the name is a name again.
    store.rename_epic(SYS, &hm(), "vernal", &name("spring"), ADA).await.unwrap();
    assert_eq!(store.epic(&hm(), "vernal", ADA).unwrap().name.as_str(), "spring");
}

#[tokio::test]
async fn the_marks_are_kept_and_the_derived_state_is_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.db");
    {
        let store = Store::open(&path).unwrap();
        fixture(&store).await;
        store.archive_epic(SYS, &hm(), "genesis", false, ADA).await.unwrap();
        store.rename_epic(SYS, &hm(), "fresh", &name("summer"), ADA).await.unwrap();
    }
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "{:?}", Store::verify_derived(&path).unwrap());
    let store = Store::open(&path).unwrap();
    let genesis = store.epic(&hm(), "genesis", ADA).unwrap();
    assert!(genesis.archived.is_some() && !genesis.default);
    assert!(store.epic(&hm(), "spring", ADA).unwrap().default);
    assert_eq!(on(&store, BO), "spring");
    assert_eq!(store.epic_move(&hm(), BO).unwrap().to.as_str(), "spring");
    assert_eq!(held(&store, "epic fresh"), 1, "the alias is kept");
    assert_eq!(held(&store, "epic any"), 3);
}

#[tokio::test]
async fn a_move_to_epic_previews_the_keys_it_will_give_and_writes_nothing() {
    let store = Store::new();
    fixture(&store).await;
    let ask = Reshortcode { ids: vec!["hypermemetic:spring:MENGER-2".into(), "hypermemetic:MENGER-2".into()], to: "hypermemetic:fresh:KILN".into(), ..Reshortcode::default() };
    let dry = store.reshortcode(SYS, Reshortcode { dry_run: true, ..ask.clone() }).await.unwrap();
    assert!(dry.tickets.is_empty());
    let shown: Vec<String> = dry.moves.iter().map(|m| m.to.rsplit_once('-').unwrap().1.to_string()).collect();
    assert_eq!(shown, ["2", "3"], "after KILN-1, in the order given");
    assert_eq!(store.get(SYS, "hypermemetic:spring:MENGER-2").await.unwrap().title, "spring two", "nothing moved");
    let done = store.reshortcode(SYS, ask).await.unwrap();
    assert_eq!(done.moves.iter().map(|m| &m.to).collect::<Vec<_>>(), dry.moves.iter().map(|m| &m.to).collect::<Vec<_>>(), "the preview is the move");
}

#[tokio::test]
async fn a_key_a_namesake_shares_still_has_its_gate_chain() {
    // genesis's MENGER-1 is also spring's MENGER.3-1 by short form: the key
    // is the key, so a permit for it (a move into genesis) is not refused.
    let store = Store::new();
    fixture(&store).await;
    let chain = store.gate_chain("MENGER-1").expect("genesis's MENGER-1 has a chain");
    assert!(chain.iter().any(|c| c == "MENGER-1"), "{chain:?}");
}
