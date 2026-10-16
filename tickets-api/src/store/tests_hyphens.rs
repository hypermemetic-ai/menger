//! ORG-9: a shortcode may hold hyphens (owner, 2026-10-16: *"synapse-cc-1
//! should be a valid name … only the final -N is protected"*). The ticket's
//! "done when", in the store: a scope `--epic SYNAPSE-CC --parent SYNAPSE-1`
//! is born SYNAPSE-CC-1, and get, list, the filters, the tree and a reopen
//! from disk all name it; SYNAPSE, the code it begins with, stays its own.

use super::lineage::TreeQuery;
use super::*;

fn new(epic: &str, title: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| (*b).into()).collect(),
        parent: parent.map(Into::into),
        body: String::new(),
    }
}

fn lens(text: &str) -> Lens {
    Lens::parse(text).unwrap()
}

fn ids(page: &Page) -> Vec<&str> {
    page.tickets.iter().map(|t| t.id.as_str()).collect()
}

/// SYNAPSE-1 (a root), SYNAPSE-2 under it; SYNAPSE-CC-1 hosted under
/// SYNAPSE-1, SYNAPSE-CC-2 under that, waiting on SYNAPSE-2; SYNAPSE-CC-3
/// waiting on SYNAPSE-CC-2.
async fn seeded(store: &Store) {
    store.create(SYS, new("SYNAPSE", "synapse", Kind::Scope, None, &[])).await.unwrap();
    store.create(SYS, new("SYNAPSE", "a build in synapse", Kind::Build, Some("SYNAPSE-1"), &[])).await.unwrap();
    let cc = store.create(SYS, new("synapse-cc", "synapse-cc and hub-codegen", Kind::Scope, Some("SYNAPSE-1"), &[])).await.unwrap();
    assert_eq!((cc.id.as_str(), cc.epic.as_str(), cc.seq), ("SYNAPSE-CC-1", "SYNAPSE-CC", 1));
    assert_eq!(cc.address, "hypermemetic:SYNAPSE/SYNAPSE-CC-1");
    let t = store.create(SYS, new("SYNAPSE-CC", "a gap in hub-codegen", Kind::Gap, Some("synapse-cc-1"), &["SYNAPSE-2"])).await.unwrap();
    assert_eq!((t.id.as_str(), t.parent.as_deref()), ("SYNAPSE-CC-2", Some("SYNAPSE-CC-1")));
    let t = store.create(SYS, new("SYNAPSE/SYNAPSE-CC", "after it", Kind::Build, Some("SYNAPSE-CC-1"), &["synapse-cc-2"])).await.unwrap();
    assert_eq!((t.id.as_str(), t.blocked_by.iter().map(|b| b.as_str()).collect::<Vec<_>>()), ("SYNAPSE-CC-3", vec!["SYNAPSE-CC-2"]));
}

#[tokio::test]
async fn a_hyphenated_shortcode_is_born_and_named_everywhere() {
    let store = Store::new();
    seeded(&store).await;

    // get, by every short form of the address.
    for name in ["SYNAPSE-CC-1", "synapse-cc-1", "SYNAPSE/SYNAPSE-CC-1", "hypermemetic:SYNAPSE/SYNAPSE-CC-1", "hypermemetic:SYNAPSE-CC-1"] {
        assert_eq!(store.get(SYS, name).await.unwrap().id, "SYNAPSE-CC-1", "{name}");
    }
    // A code that is no shortcode is not found, not mistaken for another.
    assert!(store.get(SYS, "SYNAPSE-C-1").await.is_err());

    // list by the shortcode, and the shortcode it begins with stays its own.
    let cc: Vec<String> = store.list(SYS, Some("SYNAPSE-CC"), None, None).await.unwrap().into_iter().map(|t| t.id.into_string()).collect();
    assert_eq!(cc, ["SYNAPSE-CC-1", "SYNAPSE-CC-2", "SYNAPSE-CC-3"]);
    let sy: Vec<String> = store.list(SYS, Some("SYNAPSE"), None, None).await.unwrap().into_iter().map(|t| t.id.into_string()).collect();
    assert_eq!(sy, ["SYNAPSE-1", "SYNAPSE-2"]);

    // The filter language: `in` is the letters, `under` a ticket or a
    // shortcode and what it hosts.
    let page = |text: &str| {
        let q = Filtered { lens: lens(text), epic: None, status: None, parent: None, keep: None };
        store.filtered_page(SYS, &q, None, None, None).unwrap()
    };
    assert_eq!(ids(&page("in SYNAPSE-CC")), ["SYNAPSE-CC-1", "SYNAPSE-CC-2", "SYNAPSE-CC-3"]);
    assert_eq!(ids(&page("in synapse-cc and kind gap")), ["SYNAPSE-CC-2"]);
    assert_eq!(ids(&page("in SYNAPSE")), ["SYNAPSE-1", "SYNAPSE-2"]);
    assert_eq!(ids(&page("under SYNAPSE-CC-1")), ["SYNAPSE-CC-1", "SYNAPSE-CC-2", "SYNAPSE-CC-3"]);
    assert_eq!(ids(&page("under SYNAPSE-CC")), ["SYNAPSE-CC-1", "SYNAPSE-CC-2", "SYNAPSE-CC-3"]);
    assert_eq!(page("under SYNAPSE").tickets.len(), 5, "SYNAPSE hosts SYNAPSE-CC");
    assert_eq!(ids(&page("in (SYNAPSE-CC, SYNAPSE) and blocked")), ["SYNAPSE-CC-2", "SYNAPSE-CC-3"]);
    let counts: Vec<(String, u64)> = store.counts(SYS, &lens("under SYNAPSE")).into_iter().map(|(s, n)| (s.to_string(), n)).collect();
    assert!(counts.contains(&("SYNAPSE-CC".to_string(), 3)) && counts.contains(&("SYNAPSE".to_string(), 2)), "{counts:?}");
    let member = store.member(SYS, &["SYNAPSE-CC-2".to_string(), "SYNAPSE-2".to_string()], &lens("in SYNAPSE-CC"));
    assert_eq!(member, [true, false]);

    // The tree: SYNAPSE-CC's top row is its root, and its children below it.
    let tree = |epic: Option<&str>, parent: Option<&str>| {
        let q = TreeQuery { parent: parent.map(String::from), epic: epic.map(String::from), lens: lens("not category done"), matches_only: false, keep: None };
        store.tree_page(SYS, &q, None, None).unwrap().nodes.into_iter().map(|n| n.summary.id.into_string()).collect::<Vec<_>>()
    };
    assert_eq!(tree(Some("SYNAPSE-CC"), None), ["SYNAPSE-CC-1"]);
    assert_eq!(tree(None, Some("SYNAPSE-CC-1")), ["SYNAPSE-CC-2", "SYNAPSE-CC-3"]);

    // A write by the hyphenated name moves the ticket it names.
    store.set_status(SYS.permit("SYNAPSE-CC-2"), Status::Ready).await.unwrap();
    assert_eq!(ids(&page("in SYNAPSE-CC and status Ready")), ["SYNAPSE-CC-2"]);
}

#[tokio::test]
async fn a_hyphenated_shortcode_is_kept_in_the_database_as_its_epic_and_seq() {
    // No migration: the rows keep epic and seq apart, and a reopen reads
    // SYNAPSE-CC-1 back whole.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        seeded(&store).await;
    }
    let store = Store::open(&path).unwrap();
    let t = store.get(SYS, "synapse-cc-3").await.unwrap();
    assert_eq!((t.id.as_str(), t.epic.as_str(), t.seq, t.parent.as_deref()), ("SYNAPSE-CC-3", "SYNAPSE-CC", 3, Some("SYNAPSE-CC-1")));
    assert_eq!(t.address, "hypermemetic:SYNAPSE/SYNAPSE-CC-3");
    // The next ticket in SYNAPSE-CC takes the next number.
    let t = store.create(SYS, new("SYNAPSE-CC", "four", Kind::Build, Some("SYNAPSE-CC-1"), &[])).await.unwrap();
    assert_eq!(t.id, "SYNAPSE-CC-4");
    // A second SYNAPSE-CC in SYNAPSE's namespace is refused, as any is.
    assert!(store.create(SYS, new("SYNAPSE-CC", "again", Kind::Scope, Some("SYNAPSE-2"), &[])).await.is_err());
}
