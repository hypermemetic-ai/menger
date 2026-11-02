//! ORG-13: epics — a file gains `genesis` and reads exactly as before; a new
//! epic starts fresh (`MENGER-1` born again, numbered from 1, its address
//! naming the epic); a short form resolves in the caller's current epic
//! first, then as before (pins, then every match); a move in renumbers and
//! the old id still answers; a whole shortcode moves in keeping its
//! numbers; `epic <name>` / `epic any` in a filter, and a reader's default;
//! the writes' refusals; and the migration of a file from before ORG-13,
//! on a copy, additive.

use super::*;
use crate::access::On;
use crate::address::{EpicName, OrgId};
use crate::members::Scope;
use std::path::Path;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        body: String::new(),
    }
}

const ADA: &str = "key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BO: &str = "key:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn hm() -> OrgId {
    OrgId::hypermemetic()
}

fn name(n: &str) -> EpicName {
    EpicName::new(n).unwrap()
}

/// `f` as a call by `who` — the call's act names its caller, as every
/// method's does.
async fn as_<T>(who: &str, f: impl std::future::Future<Output = T>) -> T {
    acting(Act { actor: who.into(), ..Act::default() }, f).await
}

/// MENGER-1 a scope with MENGER-2, MENGER-3 builds; IDENTITY-1 a scope
/// hosted under MENGER-1 with IDENTITY-2 a build.
async fn fixture(store: &Store) {
    store.create(SYS, new("MENGER", "menger", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("MENGER", "two", Kind::Build, None)).await.unwrap();
    store.create(SYS, new("MENGER", "three", Kind::Build, None)).await.unwrap();
    store.create(SYS, new("IDENTITY", "identity", Kind::Scope, Some("MENGER-1"))).await.unwrap();
    store.create(SYS, new("IDENTITY", "id two", Kind::Build, None)).await.unwrap();
}

fn held(store: &Store, text: &str) -> u64 {
    store.counts(SYS, &Lens::parse(text).unwrap()).into_iter().map(|(_, n)| n).sum()
}

async fn key_of(store: &Store, text: &str) -> String {
    store.get(SYS, text).await.unwrap().id.to_string()
}

#[tokio::test]
async fn a_store_has_genesis_and_reads_as_before() {
    let store = Store::new();
    fixture(&store).await;
    let epics = store.epics(&hm(), ADA, false).unwrap();
    assert_eq!(epics.len(), 1);
    let g = &epics[0];
    assert_eq!((g.name.as_str(), g.first, g.current, g.shortcodes, g.tickets), ("genesis", true, true, 2, 5));
    // Its addresses name no epic; every id is its own key.
    let t = store.get(SYS, "IDENTITY-2").await.unwrap();
    assert_eq!((t.id.as_str(), t.address.as_str()), ("IDENTITY-2", "hypermemetic:MENGER/IDENTITY-2"));
    assert_eq!(key_of(&store, "hypermemetic:genesis:MENGER/IDENTITY-2").await, "IDENTITY-2", "naming the first epic is allowed");
    assert_eq!(held(&store, "epic genesis"), 5);
    assert_eq!(held(&store, "epic any"), 5);
}

#[tokio::test]
async fn a_new_epic_starts_fresh_and_menger_1_is_born_again() {
    let store = Store::new();
    fixture(&store).await;
    let fresh = store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.unwrap();
    assert_eq!((fresh.first, fresh.current, fresh.shortcodes, fresh.tickets), (false, false, 0, 0), "empty; ORG-23: making it moves nobody");
    assert_eq!(store.current_epic(&hm(), ADA).unwrap().name.as_str(), "genesis", "ADA is on the default still");
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "fresh", ADA).await.unwrap();
    assert!(store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.is_err(), "a name is unique in its org");
    let root = store.create(SYS, new("hypermemetic:fresh:MENGER", "menger again", Kind::Scope, None)).await.unwrap();
    assert_eq!((root.seq, root.epic.as_str(), root.address.as_str()), (1, "MENGER", "hypermemetic:fresh:MENGER-1"));
    assert_ne!(root.id.as_str(), "MENGER-1", "its key is its own");
    let two = store.create(SYS, new("hypermemetic:fresh:MENGER", "fresh two", Kind::Build, None)).await.unwrap();
    assert_eq!(two.address, "hypermemetic:fresh:MENGER-2");
    // The full addresses: genesis's name no epic; fresh's name it.
    assert_eq!(store.get(SYS, "hypermemetic:MENGER-1").await.unwrap().title, "menger");
    assert_eq!(store.get(SYS, "hypermemetic:fresh:MENGER-1").await.unwrap().title, "menger again");
    // Outside a call (no current epic), a bare id that fits both is
    // ambiguous, each by its full address — as two orgs' namesakes are; an
    // id the file held before (a pin) would be its ticket.
    match store.get(SYS, "MENGER-1").await {
        Err(StoreError::Ambiguous { candidates, .. }) => assert_eq!(candidates, ["hypermemetic:MENGER-1", "hypermemetic:fresh:MENGER-1"]),
        other => panic!("expected ambiguous, got {other:?}"),
    }
    // ADA is in fresh (she switched): its bare MENGER-1 is fresh's; MENGER-3,
    // which fresh has not, is genesis's by its pin.
    assert_eq!(as_(ADA, store.get(SYS, "MENGER-1")).await.unwrap().title, "menger again");
    assert_eq!(as_(ADA, store.get(SYS, "MENGER-3")).await.unwrap().title, "three");
    assert_eq!(as_(ADA, store.get(SYS, "IDENTITY-2")).await.unwrap().title, "id two");
    // A new shortcode ADA files with no parent is born in its epic.
    let kiln = as_(ADA, store.create(SYS, new("KILN", "kiln", Kind::Scope, None))).await.unwrap();
    assert_eq!(kiln.address, "hypermemetic:fresh:KILN-1");
    // BO switches back to genesis: its bare id is genesis's again.
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "genesis", BO).await.unwrap();
    assert_eq!(as_(BO, store.get(SYS, "MENGER-1")).await.unwrap().title, "menger");
    assert_eq!(store.current_epic(&hm(), BO).unwrap().name.as_str(), "genesis");
    assert_eq!(store.current_epic(&hm(), ADA).unwrap().name.as_str(), "fresh");
    let counted: Vec<(String, u32)> = store.epics(&hm(), ADA, false).unwrap().into_iter().map(|e| (e.name.to_string(), e.tickets)).collect();
    assert_eq!(counted, [("genesis".to_string(), 5), ("fresh".to_string(), 3)]);
}

#[tokio::test]
async fn a_move_in_renumbers_in_order_and_the_old_id_still_answers() {
    let store = Store::new();
    fixture(&store).await;
    store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.unwrap();
    // A scope of its own there first, then two builds moved in, in the order given.
    let ask = Reshortcode {
        ids: vec!["MENGER-3".into(), "MENGER-2".into()],
        to: "hypermemetic:fresh:MENGER".into(),
        new_scope: Some("menger, fresh".into()),
        ..Reshortcode::default()
    };
    let moved = store.reshortcode(SYS, ask).await.unwrap();
    let addresses: Vec<&str> = moved.tickets.iter().map(|t| t.address.as_str()).collect();
    assert_eq!(addresses, ["hypermemetic:fresh:MENGER-1", "hypermemetic:fresh:MENGER-2", "hypermemetic:fresh:MENGER-3"]);
    assert_eq!(moved.tickets[0].parent, None, "the new root is at the epic's top");
    // The old ids answer the moved tickets.
    let three = store.get(SYS, "MENGER-3").await.unwrap();
    assert_eq!((three.title.as_str(), three.address.as_str()), ("three", "hypermemetic:fresh:MENGER-2"));
    assert!(three.moved_from.iter().any(|m| m == "MENGER-3"));
    assert_eq!(store.get(SYS, "hypermemetic:MENGER-1").await.unwrap().title, "menger", "genesis keeps its root");
    // Counted where they are now.
    assert_eq!(held(&store, "epic fresh"), 3);
    assert_eq!(held(&store, "epic genesis"), 3);
    // A parent in another epic is refused.
    let wrong = Reshortcode { ids: vec!["IDENTITY-2".into()], to: "hypermemetic:fresh:MENGER".into(), parent: Some("MENGER-1".into()), ..Reshortcode::default() };
    assert!(store.reshortcode(SYS, wrong).await.is_err());
}

#[tokio::test]
async fn a_whole_shortcode_moves_in_keeping_its_numbers_and_what_it_hosts_follows() {
    let store = Store::new();
    fixture(&store).await;
    store.create(SYS, new("THEME", "theme", Kind::Scope, Some("IDENTITY-1"))).await.unwrap();
    store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.unwrap();
    let ask = Reshortcode { ids: vec!["IDENTITY-1".into()], to: "hypermemetic:fresh:IDENTITY".into(), with_children: true, ..Reshortcode::default() };
    let moved = store.reshortcode(SYS, ask).await.unwrap();
    let addresses: Vec<&str> = moved.tickets.iter().map(|t| t.address.as_str()).collect();
    assert_eq!(addresses, ["hypermemetic:fresh:IDENTITY-1", "hypermemetic:fresh:IDENTITY-2"]);
    assert_eq!(store.get(SYS, "IDENTITY-1").await.unwrap().parent, None, "the root left MENGER-1 for the epic's top");
    let theme = store.get(SYS, "THEME-1").await.unwrap();
    assert_eq!(theme.address, "hypermemetic:fresh:IDENTITY/THEME-1", "a hosted shortcode is in its host's epic");
    assert_eq!(held(&store, "epic fresh"), 3);
}

#[tokio::test]
async fn a_filter_naming_an_epic_reads_it_and_a_reader_is_held_to_none() {
    let store = Store::new();
    fixture(&store).await;
    store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.unwrap();
    store.create(SYS, new("hypermemetic:fresh:MENGER", "m", Kind::Scope, None)).await.unwrap();
    // V2-25: a reader's choice narrows no read — ADA on `fresh` reads every
    // epic's scopes; a filter naming `epic` reads that one.
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "fresh", ADA).await.unwrap();
    assert_eq!(held(&store, "kind scope"), 3);
    assert_eq!(held(&store, "kind scope and epic fresh"), 1);
    assert_eq!(held(&store, "kind scope and epic any"), 3);
    assert_eq!(held(&store, "epic hypermemetic:genesis"), 5);
    assert_eq!(held(&store, "epic nowhere"), 0);
    assert!(crate::filter::Filter::parse("epic Not_A_Name").is_err());
    // The language says it back as written.
    assert_eq!(crate::filter::Filter::parse("not epic fresh").unwrap().format(), "not epic fresh");
}

#[tokio::test]
async fn an_epic_is_renamed_readdressing_and_deleted_only_empty_never_the_first() {
    let store = Store::new();
    fixture(&store).await;
    store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.unwrap();
    store.create(SYS, new("hypermemetic:fresh:MENGER", "m", Kind::Scope, None)).await.unwrap();
    let renamed = store.rename_epic(SYS, &hm(), "fresh", &name("second"), ADA).await.unwrap();
    assert_eq!(renamed.name.as_str(), "second");
    assert_eq!(store.get(SYS, "hypermemetic:second:MENGER-1").await.unwrap().address, "hypermemetic:second:MENGER-1");
    assert!(store.delete_epic(SYS, &hm(), "second").await.is_err(), "it holds a shortcode");
    assert!(store.delete_epic(SYS, &hm(), "genesis").await.is_err(), "the first is never deleted");
    store.create_epic(SYS, &hm(), &name("empty"), ADA).await.unwrap();
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "empty", BO).await.unwrap();
    let (org, gone) = store.delete_epic(SYS, &hm(), "empty").await.unwrap();
    assert_eq!((org.as_str(), gone.as_str()), ("hypermemetic", "empty"));
    assert_eq!(store.current_epic(&hm(), BO).unwrap().name.as_str(), "genesis", "its readers are on the default again");
    assert!(matches!(store.epic(&hm(), "empty", BO), Err(StoreError::NotFound(_))));
}

#[tokio::test]
async fn a_new_org_starts_with_its_own_genesis() {
    let store = Store::new();
    fixture(&store).await;
    store.create_org(SYS, "acme", None).await.unwrap();
    let acme = OrgId::new("acme").unwrap();
    let epics = store.epics(&acme, ADA, false).unwrap();
    assert_eq!(epics.iter().map(|e| (e.name.as_str(), e.first)).collect::<Vec<_>>(), [("genesis", true)]);
    let t = store.create(SYS, new("acme:MENGER", "acme's", Kind::Scope, None)).await.unwrap();
    assert_eq!(t.address, "acme:MENGER-1");
}

/// A file as a build before ORG-13 left it: the shortcodes without an
/// `era`, ORG-3's index, no epics' tables.
fn as_before_org_13(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "DROP INDEX shortcodes_ns_era;
         ALTER TABLE shortcodes DROP COLUMN era;
         CREATE UNIQUE INDEX shortcodes_ns ON shortcodes (org, ifnull(host, ''), code);
         DROP TABLE eras;
         DROP TABLE era_choices;",
    )
    .unwrap();
}

/// Every ticket's row, as the file stores it.
fn rows_of(path: &Path) -> Vec<(String, String, i64, String)> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn.prepare("SELECT id, epic, seq, title FROM tickets ORDER BY id").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect()
}

#[tokio::test]
async fn a_file_from_before_epics_gains_genesis_on_a_copy_and_nothing_else_moves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before.db");
    {
        let store = Store::open(&path).unwrap();
        fixture(&store).await;
        // A namesake THEME under each of two hosts, and a moved ticket's pin.
        store.create(SYS, new("THEME", "theme a", Kind::Scope, Some("MENGER-1"))).await.unwrap();
        store.create(SYS, new("THEME", "theme b", Kind::Scope, Some("IDENTITY-1"))).await.unwrap();
        store.reshortcode(SYS, Reshortcode { ids: vec!["MENGER-3".into()], to: "IDENTITY".into(), ..Reshortcode::default() }).await.unwrap();
    }
    let copy = dir.path().join("copy.db");
    let names = [
        "MENGER-1", "MENGER-2", "MENGER-3", "IDENTITY-1", "IDENTITY-2", "IDENTITY-3", "THEME-1", "MENGER/THEME-1", "IDENTITY/THEME-1",
        "hypermemetic:MENGER/IDENTITY-3", "hypermemetic:MENGER/IDENTITY/THEME-1", "menger-2", "NOPE-1",
    ];
    let resolved = |store: &Store| -> Vec<String> {
        let cache = store.tickets.read().unwrap();
        names.iter().map(|n| format!("{:?}", cache.find_ticket(n, super::shortcodes::OrgReach::All))).collect()
    };
    // What each name resolved to before (the same code, one epic), and on the
    // copy after.
    let was = {
        let store = Store::open(&path).unwrap();
        resolved(&store)
    };
    as_before_org_13(&path);
    std::fs::copy(&path, &copy).unwrap();
    let before = rows_of(&copy);
    {
        let store = Store::open(&copy).unwrap();
        let epics = store.epics(&hm(), ADA, false).unwrap();
        assert_eq!(epics.iter().map(|e| (e.name.as_str(), e.first, e.tickets)).collect::<Vec<_>>(), [("genesis", true, 7)]);
        assert_eq!(resolved(&store), was, "every name resolves as it did");
        assert_eq!(store.get(SYS, "MENGER-3").await.unwrap().address, "hypermemetic:MENGER/IDENTITY-3", "a move's pin still answers");
        assert_eq!(store.get(SYS, "IDENTITY/THEME-1").await.unwrap().title, "theme b");
    }
    assert_eq!(rows_of(&copy), before, "no ticket row was written");
    assert!(matches!(Store::verify_derived(&copy).unwrap(), Verdict::Consistent { .. }));
    // Opened again: the same, nothing to do.
    let store = Store::open(&copy).unwrap();
    assert_eq!(store.epics(&hm(), ADA, false).unwrap().len(), 1);
    let conn = rusqlite::Connection::open(&copy).unwrap();
    let unplaced: i64 = conn.query_row("SELECT count(*) FROM shortcodes WHERE era IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(unplaced, 0);
}

#[tokio::test]
async fn a_pinned_id_is_its_ticket_outside_a_call_and_the_current_epic_wins_inside_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pins.db");
    {
        let store = Store::open(&path).unwrap();
        fixture(&store).await;
    }
    // Today's ids pinned, as a file that gained ORG-3's tables with them holds.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("INSERT INTO pins (text, ticket) VALUES ('MENGER-1', 'MENGER-1'); UPDATE shortcodes SET pinned = 1 WHERE id = 'MENGER';").unwrap();
    }
    let store = Store::open(&path).unwrap();
    store.create_epic(SYS, &hm(), &name("fresh"), ADA).await.unwrap();
    store.create(SYS, new("hypermemetic:fresh:MENGER", "menger again", Kind::Scope, None)).await.unwrap();
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "fresh", ADA).await.unwrap();
    assert_eq!(store.get(SYS, "MENGER-1").await.unwrap().title, "menger", "the pin, outside a call");
    assert_eq!(as_(ADA, store.get(SYS, "MENGER-1")).await.unwrap().title, "menger again", "ADA's epic first");
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "genesis", ADA).await.unwrap();
    assert_eq!(as_(ADA, store.get(SYS, "MENGER-1")).await.unwrap().title, "menger");
    // A bare code: the pinned MENGER outside a call; ADA back in fresh files there.
    store.switch_epic(On::for_tests(Scope::Org(hm())), &hm(), "fresh", ADA).await.unwrap();
    let two = as_(ADA, store.create(SYS, new("MENGER", "into fresh", Kind::Build, None))).await.unwrap();
    assert_eq!(two.address, "hypermemetic:fresh:MENGER-2");
    let three = store.create(SYS, new("MENGER", "into genesis", Kind::Build, None)).await.unwrap();
    assert_eq!(three.address, "hypermemetic:MENGER-4");
}
