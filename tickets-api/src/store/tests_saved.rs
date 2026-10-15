//! FILTER-36: `saved "Name"` through the store — the registry read from the
//! docs the app writes (FILTER-6: `VIEW-1`, a doc per filter at
//! `_MENGER/view/<slug>`), resolved on every filtered read: the union and
//! the intersection of two names, a doc's text amended, a rename (the old
//! name keeps naming it), a delete and its undo, a doc that is not a saved
//! filter's, the registry rebuilt on open, a watch said what a doc's
//! amend did to its membership, and Needs you's default before any doc.

use super::*;
use crate::filter::Filter;

fn ticket(epic: &str, title: &str, kind: Kind, parent: Option<&str>, path: Option<&str>, body: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        path: path.map(Into::into),
        body: body.into(),
    }
}

/// FILTER-1 and the collection, as the app's first save makes them; then
/// two saved filters and the work they choose from.
async fn seeded(store: &Store) -> (String, String) {
    store.create(ticket("FILTER", "filters", Kind::Scope, None, None, "")).await.unwrap();
    let view = store.create(ticket("VIEW", "Saved filters", Kind::Doc, Some("FILTER-1"), None, "")).await.unwrap();
    assert_eq!(view.id, "VIEW-1");
    let needs = saved_doc(store, "Needs you", "needs-you", "(asking or label needs-you)").await;
    let plexus = saved_doc(store, "Plexus-Ready", "plexus-ready", "in PLEXUS and status Ready").await;
    store.amend("VIEW-1", None, Some("- <m-demand id=\"needs-you\">Needs you</m-demand>\n- <m-demand id=\"plexus-ready\">Plexus-Ready</m-demand>\n".into())).await.unwrap();
    store.create(ticket("PLEXUS", "the platform", Kind::Scope, None, None, "")).await.unwrap();
    for _ in 0..3 {
        store.create(ticket("PLEXUS", "work", Kind::Build, Some("PLEXUS-1"), None, "")).await.unwrap();
    }
    store.set_status("PLEXUS-2", Status::Ready).await.unwrap();
    store.set_status("PLEXUS-3", Status::Ready).await.unwrap();
    store.label("PLEXUS-3", vec!["needs-you".into()], Vec::new()).await.unwrap();
    store.label("PLEXUS-4", vec!["needs-you".into()], Vec::new()).await.unwrap();
    (needs, plexus)
}

async fn saved_doc(store: &Store, name: &str, slug: &str, text: &str) -> String {
    let path = crate::saved_filters::path_of(slug);
    let body = format!("filter: {}", Filter::parse(text).unwrap().format());
    store.create(ticket("VIEW", name, Kind::Doc, Some("VIEW-1"), Some(&path), &body)).await.unwrap().id.to_string()
}

/// The ids a filter admits, in the store's order.
fn ids(store: &Store, text: &str) -> Vec<String> {
    let lens = store.lens(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    let page = store.filtered_page(&Filtered { lens, epic: None, status: None, parent: None, keep: None }, None, None, Some(500)).unwrap();
    page.tickets.into_iter().map(|t| t.id.to_string()).collect()
}

fn refusal(store: &Store, text: &str) -> String {
    match store.lens(text) {
        Ok(_) => panic!("{text} resolves"),
        Err(e) => e.to_string(),
    }
}

#[tokio::test]
async fn a_name_is_its_saved_filter_and_two_join_by_or_and_and() {
    let store = Store::new();
    seeded(&store).await;
    let needs = ids(&store, "(asking or label needs-you)");
    let plexus = ids(&store, "in PLEXUS and status Ready");
    assert_eq!(needs, ["PLEXUS-3", "PLEXUS-4"]);
    assert_eq!(plexus, ["PLEXUS-2", "PLEXUS-3"]);
    assert_eq!(ids(&store, "saved \"Needs you\""), needs);
    assert_eq!(ids(&store, "saved needs-you"), needs, "by slug, any case");
    assert_eq!(ids(&store, "saved Plexus-Ready"), plexus);
    assert_eq!(ids(&store, "saved \"Needs you\" or saved Plexus-Ready"), ["PLEXUS-2", "PLEXUS-3", "PLEXUS-4"]);
    assert_eq!(ids(&store, "saved \"Needs you\" and saved Plexus-Ready"), ["PLEXUS-3"]);
    assert_eq!(ids(&store, "in PLEXUS and not saved \"Needs you\""), ["PLEXUS-1", "PLEXUS-2"]);
    // counts agree with the page.
    let lens = store.lens("saved \"Needs you\" or saved Plexus-Ready").unwrap();
    assert_eq!(store.counts(&lens).iter().map(|(_, n)| n).sum::<u64>(), 3);
    // a read narrowed by it (FILTER-15) and facets' filter resolve too.
    assert!(store.narrowing(Some("saved Plexus-Ready")).unwrap().is_some_and(|f| f.saved_names().is_empty()));
    assert_eq!(store.resolved(Some("saved Plexus-Ready")).unwrap().format(), "in PLEXUS and status Ready");
    // refused in words: a name nothing holds; the static lens names it unresolved.
    assert_eq!(refusal(&store, "saved Nope"), "not resolved: `saved Nope` — no saved filter named \"Nope\"");
    assert!(Lens::parse("saved Plexus-Ready").unwrap_err().to_string().contains("resolved by the store first"));
}

#[tokio::test]
async fn an_amend_a_rename_and_a_delete_move_what_a_name_finds() {
    let store = Store::new();
    let (needs, plexus) = seeded(&store).await;
    // the text amended: the next read sees it.
    store.amend(&plexus, None, Some("filter: in PLEXUS and status Pending".into())).await.unwrap();
    assert_eq!(ids(&store, "saved Plexus-Ready"), ["PLEXUS-1", "PLEXUS-4"]);
    // renamed (a title alone): the new name finds it, and so — no other
    // filter holding it — does the old one, by its path.
    store.amend(&plexus, Some("Plexus pending".into()), None).await.unwrap();
    assert_eq!(ids(&store, "saved \"Plexus pending\""), ["PLEXUS-1", "PLEXUS-4"]);
    assert_eq!(ids(&store, "saved Plexus-Ready"), ["PLEXUS-1", "PLEXUS-4"]);
    // a new filter taking the old name takes it over.
    saved_doc(&store, "Plexus-Ready", "plexus-ready-2", "in PLEXUS and status Ready").await;
    assert_eq!(ids(&store, "saved Plexus-Ready"), ["PLEXUS-2", "PLEXUS-3"]);
    assert_eq!(ids(&store, "saved \"Plexus pending\""), ["PLEXUS-1", "PLEXUS-4"]);
    // a cycle, through the docs.
    store.amend(&needs, None, Some("filter: saved \"Plexus pending\" or asking".into())).await.unwrap();
    store.amend(&plexus, None, Some("filter: saved \"Needs you\"".into())).await.unwrap();
    assert!(refusal(&store, "saved \"Needs you\"").contains("a cycle: \"Needs you\" names \"Plexus pending\" names \"Needs you\""));
    // deleted: nothing named so; undeleted: back.
    store.amend(&plexus, None, Some("filter: label needs-you".into())).await.unwrap();
    store.delete(Selection::Ids(vec![plexus.clone()]), Deletion::default(), None).await.unwrap();
    assert!(refusal(&store, "saved \"Plexus pending\"").contains("no saved filter named \"Plexus pending\""));
    store.undelete(std::slice::from_ref(&plexus)).await.unwrap();
    assert_eq!(ids(&store, "saved \"Plexus pending\""), ["PLEXUS-3", "PLEXUS-4"]);
    // unlisted (the collection's line taken out, the doc kept): still found.
    store.amend("VIEW-1", None, Some(String::new())).await.unwrap();
    assert_eq!(ids(&store, "saved \"Plexus pending\""), ["PLEXUS-3", "PLEXUS-4"]);
}

#[tokio::test]
async fn only_a_saved_filters_doc_is_named() {
    let store = Store::new();
    seeded(&store).await;
    // a `filter:` line in a ticket that is not a saved filter's doc.
    store.create(ticket("PLEXUS", "Sneaky", Kind::Build, Some("PLEXUS-1"), None, "filter: blocked")).await.unwrap();
    store.create(ticket("VIEW", "Other", Kind::Doc, Some("VIEW-1"), Some("_MENGER/wave/other"), "filter: blocked")).await.unwrap();
    assert!(refusal(&store, "saved Sneaky").contains("no saved filter named"));
    assert!(refusal(&store, "saved Other").contains("no saved filter named"));
}

#[tokio::test]
async fn the_registry_is_read_again_from_the_bodies_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let plexus = {
        let store = Store::open(&path).unwrap();
        let (_, plexus) = seeded(&store).await;
        store.amend(&plexus, None, Some("intro line\nfilter: in PLEXUS and status Pending\n".into())).await.unwrap();
        plexus
    };
    let store = Store::open(&path).unwrap();
    assert_eq!(store.tickets.read().unwrap().saved.len(), 2, "{plexus} and Needs you");
    assert_eq!(ids(&store, "saved Plexus-Ready"), ["PLEXUS-1", "PLEXUS-4"]);
    assert_eq!(ids(&store, "saved \"Needs you\" or saved Plexus-Ready"), ["PLEXUS-1", "PLEXUS-3", "PLEXUS-4"]);
}

#[tokio::test]
async fn a_watch_naming_a_filter_hears_what_its_amend_did() {
    let store = Store::new();
    let (_, plexus) = seeded(&store).await;
    let text = "saved \"Needs you\" or saved Plexus-Ready";
    let mut watcher = store.watcher_in(store.lens(text).unwrap(), None).naming(Some(text));
    assert!(watcher.names_saved());
    assert!(watcher.resaved(&store, None).unwrap().is_empty(), "nothing moved yet");
    // Plexus-Ready now says Pending: PLEXUS-2 leaves, PLEXUS-1 (the root, Pending) enters; 3 and 4 stay, through Needs you.
    store.amend(&plexus, None, Some("filter: in PLEXUS and status Pending".into())).await.unwrap();
    let said = watcher.resaved(&store, None).unwrap();
    let sc = |id: &str| store.tickets.read().unwrap().at.get(id).cloned().expect("a ticket").shortcode;
    assert_eq!(
        said,
        [
            Membership::Entered { id: "PLEXUS-1".into(), shortcode: sc("PLEXUS-1"), seq: 1 },
            Membership::Left { id: "PLEXUS-2".into(), shortcode: sc("PLEXUS-2"), seq: 2 },
        ]
    );
    assert!(watcher.resaved(&store, None).unwrap().is_empty(), "said once");
    // the compiled watch follows the new filter: a cell move is judged by it.
    store.set_status("PLEXUS-2", Status::Pending).await.unwrap();
    // a name that stops resolving: the watch's refusal.
    store.delete(Selection::Ids(vec![plexus]), Deletion::default(), None).await.unwrap();
    assert!(watcher.resaved(&store, None).unwrap_err().contains("no saved filter named \"Plexus-Ready\""));
    // a watch naming none is never read again.
    let plain = store.watcher_in(store.lens("blocked").unwrap(), None).naming(Some("blocked"));
    assert!(!plain.names_saved());
}

#[tokio::test]
async fn needs_you_means_the_apps_default_until_one_is_saved() {
    let store = Store::new();
    store.create(ticket("PLEXUS", "the platform", Kind::Scope, None, None, "")).await.unwrap();
    store.create(ticket("PLEXUS", "work", Kind::Build, Some("PLEXUS-1"), None, "")).await.unwrap();
    store.label("PLEXUS-2", vec!["needs-you".into()], Vec::new()).await.unwrap();
    assert_eq!(store.resolved(Some("saved \"Needs you\"")).unwrap().format(), "(queued or asking or label needs-you)");
    assert_eq!(ids(&store, "saved \"needs you\""), ["PLEXUS-2"]);
    assert!(refusal(&store, "saved Plexus-Ready").contains("no saved filter named"), "no other name has a default");
}
