//! TICKETS-35: every ticket a uid — minted at create, never another's and
//! never a shortcode's code; kept through every move; given once, by the
//! open, to every ticket and tombstone a file held without one. One
//! resolver for every name of a ticket, and a write reads its permit's key
//! as a key (TICKETS-32): two shortcodes named PERF in two namespaces, the
//! original's ticket keyed `PERF-19` beside a namesake's `PERF.N-19`, each
//! written by its address, its uid and a short form only it fits, and the
//! bare `PERF-19` ambiguous — answered with a choice per candidate. Rows
//! name the uids of what they name (`uid_of`); the filter words `under` and
//! `ticket` take a uid; `--verify-derived` holds the uids.

use super::delete::Deletion;
use super::reshortcode::{Rename, Reshortcode};
use super::tests_reach::viewer_of;
use super::update_many::Selection;
use super::*;
use crate::types::TicketsEvent;
use crate::vocab::Uid;
use rusqlite::Connection;
use std::collections::HashSet;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(str::to_string),
        path: None,
        body: String::new(),
    }
}

/// MENGER-1 → STORE-1 → PERF-1 and its builds PERF-2 … PERF-19 (the
/// original's); PLEXUS-1 → a namesake PERF hosted there, its root and its
/// gaps up to 19. Answers the namesake's shortcode identity.
async fn two_perfs(store: &Store) -> ShortcodeId {
    store.create(SYS, new("MENGER", "menger", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("STORE", "the store and the server", Kind::Scope, Some("MENGER-1"))).await.unwrap();
    store.create(SYS, new("PERF", "Magnitude", Kind::Scope, Some("STORE-1"))).await.unwrap();
    for n in 2..=19 {
        store.create(SYS, new("PERF", &format!("ours {n}"), Kind::Build, Some("PERF-1"))).await.unwrap();
    }
    store.create(SYS, new("PLEXUS", "the platform", Kind::Scope, None)).await.unwrap();
    let theirs = store.create(SYS, new("PERF", "Self-description cached", Kind::Scope, Some("PLEXUS-1"))).await.unwrap();
    assert_eq!(theirs.address, "hypermemetic:PLEXUS/PERF-1");
    for n in 2..=19 {
        store.create(SYS, new("PLEXUS/PERF", &format!("theirs {n}"), Kind::Gap, Some(&theirs.id))).await.unwrap();
    }
    theirs.shortcode
}

/// The ticket a name names, whole.
async fn named(store: &Store, name: &str) -> Ticket {
    store.get(SYS, name).await.unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[tokio::test]
async fn create_mints_a_uid_no_other_ticket_s_and_no_shortcode_s_code() {
    let store = Store::new();
    two_perfs(&store).await;
    let all = store.list(SYS, None, None, None).await.unwrap();
    assert_eq!(all.len(), 41);
    let uids: HashSet<Uid> = all.iter().map(|t| t.uid).collect();
    assert_eq!(uids.len(), 41, "every ticket its own");
    for t in &all {
        assert!(t.uid.is_some(), "{} has none", t.id);
        assert_eq!(t.uid.as_str().len(), 8, "{}", t.id);
        assert_eq!(t.uid.as_str().parse::<Uid>(), Ok(t.uid), "Crockford's base32, lower case: {:?}", t.uid);
    }
    let codes: Vec<String> = store.shortcodes().into_iter().map(|(s, _)| s.code.as_str().to_ascii_lowercase()).collect();
    assert!(uids.iter().all(|u| !codes.contains(&u.as_str().to_string())), "no shortcode's code");
    // A clash is minted again: a uid another ticket has, or a code's letters.
    let taken = all[0].uid;
    let code = Uid::from("abcdefgh");
    store.create(SYS, new("ABCDEFGH", "a code a uid could be", Kind::Scope, None)).await.unwrap();
    let cache = store.tickets.read().unwrap();
    assert!(cache.shortcodes.holds_letters(code.as_str()), "the letters of a code, any case");
    let mut draws = vec![taken.to_bits(), code.to_bits(), 7].into_iter();
    let minted = uids::mint(|u| cache.uids.holds(u) || cache.shortcodes.holds_letters(u.as_str()), || draws.next().expect("a third draw"));
    assert_eq!(minted, Uid::from_bits(7), "past another's uid and a code's letters");
    // And a minted one is none of them.
    let fresh = cache.mint_uid(&[taken]);
    assert!(!cache.uids.holds(&fresh) && fresh != taken && !cache.shortcodes.holds_letters(fresh.as_str()));
}

#[tokio::test]
async fn the_first_open_mints_one_for_every_ticket_and_tombstone_and_no_later_open_mints_any() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, new("KILN", "the kiln", Kind::Scope, None)).await.unwrap();
        for n in 2..=5 {
            store.create(SYS, new("KILN", &format!("firing {n}"), Kind::Build, Some("KILN-1"))).await.unwrap();
        }
        store.delete(SYS, Selection::Ids(vec!["KILN-5".into()]), Deletion::default(), None).await.unwrap();
    }
    // A file from before uids: no column, no index (a release before
    // TICKETS-35 wrote it).
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP INDEX tickets_uid; DROP INDEX tombstones_uid;
             ALTER TABLE tickets DROP COLUMN uid; ALTER TABLE tombstones DROP COLUMN uid;",
        )
        .unwrap();
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { uids: None, tickets: 4, .. }), "no column: the next open mints");
    let mut conn = Connection::open(&path).unwrap();
    assert_eq!(uids::migrate(&mut conn).unwrap(), uids::Minted { tickets: 4, tombstones: 1 }, "one each, in one transaction");
    assert_eq!(uids::migrate(&mut conn).unwrap(), uids::Minted::default(), "the second open mints none");
    let held: Vec<(String, Option<String>)> =
        conn.prepare("SELECT id, uid FROM tickets UNION ALL SELECT id, uid FROM tombstones ORDER BY id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(held.len(), 5);
    let uids: HashSet<&str> = held.iter().map(|(_, u)| u.as_deref().expect("every row its uid")).collect();
    assert_eq!(uids.len(), 5, "no two alike: {held:?}");
    // A row a release before TICKETS-35 writes after it (no uid): the next
    // open gives it one, and no other row's moves.
    conn.execute("UPDATE tickets SET uid = NULL WHERE id = 'KILN-4'", []).unwrap();
    assert_eq!(uids::migrate(&mut conn).unwrap(), uids::Minted { tickets: 1, tombstones: 0 });
    let after: HashSet<String> = conn.prepare("SELECT uid FROM tickets WHERE id != 'KILN-4' UNION ALL SELECT uid FROM tombstones").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(held.iter().filter(|(id, _)| id != "KILN-4").all(|(_, u)| after.contains(u.as_deref().unwrap())), "every other uid as it was");
    drop(conn);
    // The store reads them back, and the deleted one names its tombstone.
    let store = Store::open(&path).unwrap();
    let k2 = named(&store, "KILN-2").await;
    assert_eq!(named(&store, k2.uid.as_str()).await.id, "KILN-2");
    let gone = held.iter().find(|(id, _)| id == "KILN-5").and_then(|(_, u)| u.clone()).unwrap();
    assert_eq!(store.tombstone(SYS, &gone).map(|t| (t.id.to_string(), t.uid.into_string())), Some(("KILN-5".into(), gone.clone())));
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { uids: Some(5), .. }));
}

#[tokio::test]
async fn verify_derived_finds_a_uid_blanked_or_doubled_by_hand() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, new("KILN", "the kiln", Kind::Scope, None)).await.unwrap();
        store.create(SYS, new("KILN", "a firing", Kind::Build, Some("KILN-1"))).await.unwrap();
        store.create(SYS, new("KILN", "a glaze", Kind::Build, Some("KILN-1"))).await.unwrap();
    }
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { uids: Some(3), .. }));
    let conn = Connection::open(&path).unwrap();
    conn.execute("UPDATE tickets SET uid = NULL WHERE id = 'KILN-2'", []).unwrap();
    match Store::verify_derived(&path).unwrap() {
        Verdict::Differs(lines) => assert_eq!(lines, ["uid of ticket KILN-2: none (the next open mints one)"]),
        other => panic!("expected Differs, got {other:?}"),
    }
    conn.execute_batch("DROP INDEX tickets_uid; UPDATE tickets SET uid = (SELECT uid FROM tickets WHERE id = 'KILN-1') WHERE id = 'KILN-2';").unwrap();
    match Store::verify_derived(&path).unwrap() {
        Verdict::Differs(lines) => {
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(lines[0].ends_with("held by ticket KILN-1 and ticket KILN-2"), "{lines:?}");
        }
        other => panic!("expected Differs, got {other:?}"),
    }
    // The next open gives the second its own, and builds the index again.
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_ne!(named(&store, "KILN-1").await.uid, named(&store, "KILN-2").await.uid);
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { uids: Some(3), .. }));
}

#[tokio::test]
async fn the_uid_is_kept_through_every_move() {
    let store = Store::new();
    store.create(SYS, new("A", "a", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("A", "a two", Kind::Build, Some("A-1"))).await.unwrap();
    store.create(SYS, new("A", "a three", Kind::Build, Some("A-1"))).await.unwrap();
    store.create(SYS, new("B", "b", Kind::Scope, None)).await.unwrap();
    let uid = named(&store, "A-2").await.uid;
    let at = |store: &Store| -> (String, Uid) {
        let cache = store.tickets.read().unwrap();
        let key = cache.uids.key(&uid).expect("the uid names it").to_string();
        let row = cache.get(&key).map_or_else(Uid::default, |r| r.uid);
        (key, row)
    };
    // set_parent.
    store.set_parent(SYS.moving("A-2", "A-3")).await.unwrap();
    assert_eq!(at(&store), ("A-2".to_string(), uid));
    // reshortcode: the key rewritten, the uid not.
    let moved = store.reshortcode(SYS, Reshortcode { ids: vec!["A-2".into()], to: "B".into(), ..Reshortcode::default() }).await.unwrap();
    let key = moved.moves[0].to.clone();
    assert_eq!((key.as_str(), moved.tickets[0].uid), ("B-2", uid), "{moved:?}");
    assert_eq!(at(&store), ("B-2".to_string(), uid));
    assert_eq!(named(&store, uid.as_str()).await.id, "B-2", "get by the uid follows it");
    // rename into a subtask id under its parent.
    store.rename(SYS, Rename { id: "B-2".into(), to: "B-1.1".into(), orgs: None, dry_run: false }).await.unwrap();
    assert_eq!(at(&store), ("B-1.1".to_string(), uid));
    // archive, then restore.
    store.archive(SYS, Selection::Ids(vec!["B-1.1".into()]), false).await.unwrap();
    assert_eq!(at(&store), ("B-1.1".to_string(), uid));
    store.restore(SYS, Selection::Ids(vec!["B-1.1".into()]), false, false).await.unwrap();
    assert_eq!(at(&store), ("B-1.1".to_string(), uid));
    // delete — the tombstone keeps it — then undelete by it.
    store.delete(SYS, Selection::Ids(vec![uid.to_string()]), Deletion::default(), None).await.unwrap();
    assert_eq!(store.tombstone(SYS, uid.as_str()).map(|t| (t.id.to_string(), t.uid)), Some(("B-1.1".into(), uid)));
    assert!(matches!(store.get(SYS, uid.as_str()).await, Err(StoreError::NotFound(_))), "deleted: no ticket, its tombstone");
    let back = store.undelete(SYS, &[uid.to_string()]).await.unwrap();
    assert_eq!((back.tickets[0].id.as_str(), back.tickets[0].uid), ("B-1.1", uid));
    assert_eq!(at(&store), ("B-1.1".to_string(), uid));
    // An epic move: its shortcode moved whole into another epic.
    store.create_epic(SYS, &crate::address::OrgId::hypermemetic(), &crate::address::EpicName::new("fresh").unwrap(), "key:ada").await.unwrap();
    let to_fresh = Reshortcode { ids: vec!["B-1".into()], to: "hypermemetic:fresh:B".into(), with_children: true, ..Reshortcode::default() };
    store.reshortcode(SYS, to_fresh).await.unwrap();
    let (key, kept) = at(&store);
    assert_eq!(kept, uid, "in fresh as {key}");
    let there = named(&store, uid.as_str()).await;
    assert_eq!(there.id, key);
    assert!(there.address.starts_with("hypermemetic:fresh:B-"), "{}", there.address);
}

/// TICKETS-32's case: every write lands by the original's full address, by
/// its uid and by `STORE/PERF-19` — each resolved once, to the key `PERF-19`,
/// which the write then reads as the key — and the namesake's by its own;
/// the bare `PERF-19` fits both and is refused.
#[tokio::test]
async fn every_write_lands_by_the_address_the_uid_or_a_short_form_and_the_bare_id_is_ambiguous() {
    let store = Store::new();
    let theirs = two_perfs(&store).await;
    let ours = named(&store, "hypermemetic:MENGER/STORE/PERF-19").await;
    let namesake = named(&store, "hypermemetic:PLEXUS/PERF-19").await;
    assert_eq!((ours.id.as_str(), namesake.id.to_string()), ("PERF-19", theirs.key(19)));
    for name in ["hypermemetic:MENGER/STORE/PERF-19", ours.uid.as_str(), "STORE/PERF-19", "MENGER/PERF-19"] {
        let key = store.resolve(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(key, "PERF-19", "{name}");
        let amended = store.amend(SYS.permit(&key), Some(format!("ours, by {name}")), None).await.unwrap_or_else(|e| panic!("amend by {name}: {e}"));
        assert!(matches!(&amended, Change::Amended { ticket, .. } if ticket.id == "PERF-19" && ticket.uid == ours.uid), "{amended:?}");
        store.update(SYS.updating(&key), Update { severity: Some(Some(Severity::High)), ..Update::default() }).await.unwrap_or_else(|e| panic!("update by {name}: {e}"));
        store.set_status(SYS.permit(&key), Status::Ready).await.unwrap_or_else(|e| panic!("set_status by {name}: {e}"));
        store.label(SYS.permit(&key), vec!["needs-you".into()], vec![]).await.unwrap_or_else(|e| panic!("label by {name}: {e}"));
        store.link_commits(SYS.permit(&key), vec!["77df9a0".into()]).await.unwrap_or_else(|e| panic!("commits by {name}: {e}"));
        store.set_parent(SYS.moving(&key, "PERF-2")).await.unwrap_or_else(|e| panic!("set_parent by {name}: {e}"));
        store.set_parent(SYS.moving(&key, "PERF-1")).await.unwrap_or_else(|e| panic!("set_parent back by {name}: {e}"));
    }
    let after = named(&store, ours.uid.as_str()).await;
    assert_eq!(
        (after.title.as_str(), after.severity, after.status, after.labels.len(), after.commits.len(), after.parent.as_deref()),
        ("ours, by MENGER/PERF-19", Some(Severity::High), Status::Ready, 1, 1, Some("PERF-1"))
    );
    // The namesake, by its address and its uid.
    for name in ["hypermemetic:PLEXUS/PERF-19", namesake.uid.as_str(), "PLEXUS/PERF-19"] {
        let key = store.resolve(name).unwrap();
        assert_eq!(key, namesake.id.as_str());
        store.amend(SYS.permit(&key), Some(format!("theirs, by {name}")), None).await.unwrap();
        store.set_status(SYS.permit(&key), Status::Ready).await.unwrap();
    }
    assert_eq!(named(&store, namesake.uid.as_str()).await.title, "theirs, by PLEXUS/PERF-19");
    assert_eq!(named(&store, ours.uid.as_str()).await.title, "ours, by MENGER/PERF-19", "the original untouched");
    // The bare id fits both: refused, each by its full address.
    match store.resolve("PERF-19") {
        Err(StoreError::Ambiguous { candidates, .. }) => assert_eq!(candidates, ["hypermemetic:MENGER/STORE/PERF-19", "hypermemetic:PLEXUS/PERF-19"]),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    // A parent named by the original's key is that ticket, as a permit's is.
    let child = store.create(SYS, new("MENGER/STORE/PERF", "a child of ours", Kind::Build, Some(ours.uid.as_str()))).await.unwrap();
    assert_eq!(child.parent.as_deref(), Some("PERF-19"));
}

#[tokio::test]
async fn a_name_that_fits_several_answers_each_choice_with_its_parent() {
    let store = Store::new();
    let theirs = two_perfs(&store).await;
    let candidates: Vec<String> = match store.resolve("PERF-19") {
        Err(StoreError::Ambiguous { candidates, .. }) => candidates,
        other => panic!("expected Ambiguous, got {other:?}"),
    };
    let ours = named(&store, "hypermemetic:MENGER/STORE/PERF-19").await;
    let perf1 = named(&store, "MENGER/STORE/PERF-1").await;
    let namesake = named(&store, "PLEXUS/PERF-19").await;
    let choices = store.choices(SYS, &candidates);
    assert_eq!(choices.len(), 2);
    let a = &choices[0];
    assert_eq!(
        (a.uid, a.id.as_ref().map(|i| i.as_str()), a.address.as_str(), a.org.as_ref().map(|o| o.as_str()), a.epic.as_ref().map(|e| e.as_str()), a.kind, a.status, a.title.as_deref()),
        (Some(ours.uid), Some("PERF-19"), "hypermemetic:MENGER/STORE/PERF-19", Some("hypermemetic"), Some("genesis"), Some(Kind::Build), Some(Status::Draft), Some("ours 19"))
    );
    let p = a.parent.as_ref().expect("its parent");
    assert_eq!((p.uid, p.id.as_str(), p.address.as_str(), p.title.as_str()), (perf1.uid, "PERF-1", "hypermemetic:MENGER/STORE/PERF-1", "Magnitude"));
    assert_eq!((choices[1].uid, choices[1].kind, choices[1].parent.as_ref().map(|p| p.address.as_str())), (Some(namesake.uid), Some(Kind::Gap), Some("hypermemetic:PLEXUS/PERF-1")));
    // A reader of the original's PERF alone reads its choice whole, and the
    // namesake's address alone.
    let reader = viewer_of(&["PERF"]);
    let veiled = store.choices(&reader, &candidates);
    assert_eq!(veiled[0].uid, Some(ours.uid));
    assert_eq!(veiled[1], crate::types::Choice { address: "hypermemetic:PLEXUS/PERF-19".into(), ..Default::default() });
    // A reader of the namesake alone: the other way round — its own whole,
    // with its parent, and the original's address alone.
    let theirs_reader = viewer_of(&[theirs.as_str()]);
    let theirs_view = store.choices(&theirs_reader, &candidates);
    assert_eq!(theirs_view[0], crate::types::Choice { address: "hypermemetic:MENGER/STORE/PERF-19".into(), ..Default::default() });
    assert_eq!(theirs_view[1].parent.as_ref().map(|p| p.id.to_string()), Some(theirs.key(1)));
    // The wire: a choice the reader cannot read is its address alone.
    assert_eq!(serde_json::to_value(&veiled[1]).unwrap(), serde_json::json!({"address": "hypermemetic:PLEXUS/PERF-19"}));
}

#[tokio::test]
async fn a_row_names_the_uids_of_its_parent_blockers_and_unlocks() {
    let store = Store::new();
    store.create(SYS, new("KILN", "the kiln", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("KILN", "a firing", Kind::Build, Some("KILN-1"))).await.unwrap();
    let three = NewTicket { blocked_by: vec!["KILN-2".into(), "KILN-9".into()], ..new("KILN", "a glaze", Kind::Build, Some("KILN-1")) };
    store.create(SYS, three).await.unwrap();
    let uid = |id: &str| store.tickets.read().unwrap().get(id).unwrap().uid;
    let dressed = |t: Ticket| match store.dressed(TicketsEvent::Ticket { ticket: t }) {
        TicketsEvent::Ticket { ticket } => ticket,
        other => panic!("{other:?}"),
    };
    let k3 = dressed(named(&store, "KILN-3").await);
    let want: BTreeMap<TicketName, Uid> = [("KILN-1", uid("KILN-1")), ("KILN-2", uid("KILN-2"))].into_iter().map(|(k, u)| (TicketName::new(k), u)).collect();
    assert_eq!(k3.uid_of, want, "the parent and the blocker; KILN-9, not born yet, none");
    let k2 = dressed(named(&store, "KILN-2").await);
    assert_eq!(k2.uid_of.get("KILN-3"), Some(&uid("KILN-3")), "what it unlocks");
    let k1 = dressed(named(&store, "KILN-1").await);
    assert!(k1.uid_of.is_empty());
    assert!(serde_json::to_value(&k1).unwrap().get("uid_of").is_none(), "omitted when empty");
    // A summary too; and a page of them.
    let page = store.page(SYS, PageQuery { parent: Some("KILN-1"), ..PageQuery::default() }).unwrap();
    let summary = match store.dressed(TicketsEvent::Summary { summary: page.tickets[1].clone() }) {
        TicketsEvent::Summary { summary } => summary,
        other => panic!("{other:?}"),
    };
    assert_eq!(summary.uid_of, want);
    // Born, the forward reference names it.
    for n in 4..=9 {
        store.create(SYS, new("KILN", &format!("{n}"), Kind::Build, Some("KILN-1"))).await.unwrap();
    }
    assert_eq!(dressed(named(&store, "KILN-3").await).uid_of.get("KILN-9"), Some(&uid("KILN-9")));
}

#[tokio::test]
async fn a_uid_names_its_ticket_bare_in_a_link_and_in_the_filters() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("t.db")).unwrap();
    store.create(SYS, new("KILN", "the kiln", Kind::Scope, None)).await.unwrap();
    store.create(SYS, new("KILN", "a firing", Kind::Build, Some("KILN-1"))).await.unwrap();
    store.create(SYS, new("KILN", "a glaze", Kind::Build, Some("KILN-2"))).await.unwrap();
    let k2 = named(&store, "KILN-2").await.uid;
    for name in [k2.to_string(), k2.as_str().to_ascii_uppercase(), format!("/t/{k2}/KILN-2-a-firing"), format!("https://app.menger.sh/t/{k2}/x"), format!("#/t/{k2}")] {
        assert_eq!(named(&store, &name).await.id, "KILN-2", "{name}");
    }
    assert!(matches!(store.get(SYS, "zzzzzzzz").await, Err(StoreError::NotFound(n)) if n == "zzzzzzzz"), "a uid no ticket has");
    // The filter words.
    let ids = |text: &str| -> Result<Vec<String>, String> {
        let lens = Lens::parse(text).map_err(|e| e.to_string())?;
        let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
        store.filtered_page(SYS, &q, None, None, None).map(|p| p.tickets.iter().map(|t| t.id.to_string()).collect()).map_err(|e| e.to_string())
    };
    assert_eq!(ids(&format!("under {k2}")).unwrap(), ["KILN-2", "KILN-3"]);
    assert_eq!(ids(&format!("under {}", k2.as_str().to_ascii_uppercase())).unwrap(), ["KILN-2", "KILN-3"]);
    assert_eq!(ids(&format!("ticket {k2}")).unwrap(), ["KILN-2"]);
    assert_eq!(ids("under zzzzzzzz").unwrap(), Vec::<String>::new(), "a uid no ticket has holds nowhere");
    // A uid that is also a shortcode's code, in `under`: both, refused.
    let code = k2.as_str().to_ascii_uppercase();
    store.create(SYS, new(&code, "letters a uid has", Kind::Scope, None)).await.unwrap();
    let both = ids(&format!("under {k2}")).unwrap_err();
    assert!(both.contains("hypermemetic:KILN-2") && both.contains(&format!("hypermemetic:{code}")), "{both}");
    // `in` names letters and is unchanged.
    assert_eq!(ids(&format!("in {code}")).unwrap(), [format!("{code}-1")]);
    // Another organization's uid is not found to a reader of one.
    let (_, _) = store.create_org(SYS, "acme", None).await.unwrap();
    let acme = store.create(SYS, new("acme:MENGER", "acme's", Kind::Scope, None)).await.unwrap();
    let acme_reader = crate::access::VerifiedUser::for_tests(
        &crate::access::Access::for_tests(),
        "idp:a0000000-0000-4000-8000-0000000000ac".parse().unwrap(),
        vec![(crate::access::Held::Org(crate::address::OrgId::new("acme").unwrap()), crate::members::Role::Viewer)],
    );
    assert_eq!(store.get(&acme_reader, acme.uid.as_str()).await.unwrap().id, acme.id);
    assert!(matches!(store.get(&acme_reader, k2.as_str()).await, Err(StoreError::NotFound(_))), "hypermemetic's, to acme's reader");
}

/// TICKETS-35: a full address is never refused as ambiguous — a root's
/// (`acme:PERF-2`) is also a short form of a namesake's hosted below it in
/// the same organization (`acme:OPS/PERF-2`), and names its own.
#[tokio::test]
async fn a_root_s_full_address_names_its_own_beside_a_namesake_hosted_below() {
    let store = Store::new();
    store.create(SYS, new("MENGER", "menger", Kind::Scope, None)).await.unwrap();
    store.create_org(SYS, "acme", None).await.unwrap();
    store.create(SYS, new("acme:PERF", "acme's perf", Kind::Scope, None)).await.unwrap();
    let root2 = store.create(SYS, new("acme:PERF", "acme's PERF-2", Kind::Build, None)).await.unwrap();
    let ops = store.create(SYS, new("acme:OPS", "acme's ops", Kind::Scope, None)).await.unwrap();
    let hosted1 = store.create(SYS, new("PERF", "ops perf", Kind::Scope, Some(&ops.id))).await.unwrap();
    let hosted2 = store.create(SYS, new("acme:OPS/PERF", "ops PERF-2", Kind::Build, Some(&hosted1.id))).await.unwrap();
    assert_eq!((root2.address.as_str(), hosted2.address.as_str()), ("acme:PERF-2", "acme:OPS/PERF-2"));
    assert_eq!(store.resolve("acme:PERF-2").unwrap(), root2.id.as_str(), "the root's full address, its own");
    assert_eq!(store.resolve("acme:OPS/PERF-2").unwrap(), hosted2.id.as_str());
    assert!(matches!(store.resolve("PERF-2"), Err(StoreError::Ambiguous { candidates, .. }) if candidates == ["acme:PERF-2", "acme:OPS/PERF-2"]));
    // And a shortcode's: `acme:PERF` is the root's.
    let sc = store.shortcode("acme:PERF").unwrap().expect("a shortcode");
    assert_eq!(sc.id, root2.shortcode);
    // A write by it lands.
    store.amend(SYS.permit(&store.resolve("acme:PERF-2").unwrap()), Some("by its full address".into()), None).await.unwrap();
    assert_eq!(named(&store, root2.uid.as_str()).await.title, "by its full address");
}
