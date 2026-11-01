//! FILTER-46: the saved filters through the store — saved, replaced,
//! refused (a filter the language cannot read, an unknown name, a cycle),
//! renamed, deleted, read again from the table on open; `saved "Name"`
//! (FILTER-36) following each change on a read and on a watch; and Needs
//! you's default before any is saved.

use super::*;
use crate::access::{Editor, On};
use crate::address::OrgId;
use crate::filter::Filter;
use crate::members::Scope;

fn org() -> OrgId {
    OrgId::hypermemetic()
}

fn on() -> On<Editor> {
    On::for_tests(Scope::Org(org()))
}

fn ticket(epic: &str, title: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket { epic: epic.into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: parent.map(Into::into), path: None, body: String::new() }
}

/// PLEXUS with three builds: -2 and -3 Ready, -3 and -4 labelled `needs-you`.
async fn work(store: &Store) {
    store.create(SYS, ticket("PLEXUS", "the platform", Kind::Scope, None)).await.unwrap();
    for _ in 0..3 {
        store.create(SYS, ticket("PLEXUS", "work", Kind::Build, Some("PLEXUS-1"))).await.unwrap();
    }
    store.set_status(SYS.permit("PLEXUS-2"), Status::Ready).await.unwrap();
    store.set_status(SYS.permit("PLEXUS-3"), Status::Ready).await.unwrap();
    store.label(SYS.permit("PLEXUS-3"), vec!["needs-you".into()], Vec::new()).await.unwrap();
    store.label(SYS.permit("PLEXUS-4"), vec!["needs-you".into()], Vec::new()).await.unwrap();
}

async fn save(store: &Store, name: &str, text: &str) -> Result<(crate::saved_filters::SavedFilter, bool), StoreError> {
    store.save_filter(on(), &org(), name, text, "key:test".into()).await
}

/// The ids a filter admits, in the store's order.
fn ids(store: &Store, text: &str) -> Vec<String> {
    let lens = store.lens(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    let page = store.filtered_page(SYS, &Filtered { lens, epic: None, status: None, parent: None, keep: None }, None, None, Some(500)).unwrap();
    page.tickets.into_iter().map(|t| t.id.to_string()).collect()
}

fn refusal(store: &Store, text: &str) -> String {
    match store.lens(text) {
        Ok(_) => panic!("{text} resolves"),
        Err(e) => e.to_string(),
    }
}

#[tokio::test]
async fn a_saved_name_is_its_filter_and_two_join_by_or_and_and() {
    let store = Store::new();
    work(&store).await;
    let (row, replaced) = save(&store, "Needs you", "(asking or label needs-you)").await.unwrap();
    assert!(!replaced);
    assert_eq!((row.saved_by.as_str(), row.name.as_str()), ("key:test", "Needs you"));
    save(&store, "Plexus-Ready", "in PLEXUS and status Ready").await.unwrap();
    assert_eq!(ids(&store, "saved \"Needs you\""), ["PLEXUS-3", "PLEXUS-4"]);
    assert_eq!(ids(&store, "saved needs-you"), ["PLEXUS-3", "PLEXUS-4"], "by slug, any case");
    assert_eq!(ids(&store, "saved \"Needs you\" or saved Plexus-Ready"), ["PLEXUS-2", "PLEXUS-3", "PLEXUS-4"]);
    assert_eq!(ids(&store, "saved \"Needs you\" and saved Plexus-Ready"), ["PLEXUS-3"]);
    assert_eq!(ids(&store, "in PLEXUS and not saved \"Needs you\""), ["PLEXUS-1", "PLEXUS-2"]);
    let lens = store.lens("saved \"Needs you\" or saved Plexus-Ready").unwrap();
    assert_eq!(store.counts(SYS, &lens).iter().map(|(_, n)| n).sum::<u64>(), 3);
    assert_eq!(store.resolved(Some(&Filter::parse("saved Plexus-Ready").unwrap())).unwrap().format(), "in PLEXUS and status Ready");
    assert_eq!(refusal(&store, "saved Nope"), "not resolved: `saved Nope` — no saved filter named \"Nope\"");
}

#[tokio::test]
async fn a_save_replaces_a_rename_moves_and_a_delete_removes() {
    let store = Store::new();
    work(&store).await;
    let (first, _) = save(&store, "Plexus-Ready", "in PLEXUS and status Ready").await.unwrap();
    // the same name again replaces it (any case), keeping who first saved it and when.
    let (again, replaced) = store.save_filter(on(), &org(), "plexus ready", "in PLEXUS and status Draft", "key:other".into()).await.unwrap();
    assert!(replaced);
    assert_eq!((again.saved_by.as_str(), again.saved_at, again.name.as_str()), ("key:test", first.saved_at, "plexus ready"));
    assert_eq!(ids(&store, "saved Plexus-Ready"), ["PLEXUS-1", "PLEXUS-4"]);
    assert_eq!(store.saved_filters(&org()).iter().filter(|f| f.name != "Needs you").count(), 1);
    // renamed: the new name finds it, the old one no longer does.
    let moved = store.rename_filter(on(), &org(), "Plexus-Ready", "Pending work").await.unwrap();
    assert_eq!((moved.filter.as_str(), moved.saved_at), ("in PLEXUS and status Draft", first.saved_at));
    assert_eq!(ids(&store, "saved \"Pending work\""), ["PLEXUS-1", "PLEXUS-4"]);
    assert!(refusal(&store, "saved Plexus-Ready").contains("no saved filter named \"Plexus-Ready\""));
    // a name another filter holds is refused; so is one that is not there.
    save(&store, "Other", "blocked").await.unwrap();
    assert!(store.rename_filter(on(), &org(), "Other", "pending WORK").await.unwrap_err().to_string().contains("already named"));
    assert!(store.rename_filter(on(), &org(), "Nope", "x1").await.unwrap_err().to_string().contains("no saved filter named \"Nope\""));
    // a case change alone is a rename.
    assert_eq!(store.rename_filter(on(), &org(), "Other", "OTHER").await.unwrap().name, "OTHER");
    // deleted: nothing named so.
    assert_eq!(store.delete_filter(on(), &org(), "pending work").await.unwrap(), "Pending work");
    assert!(refusal(&store, "saved \"Pending work\"").contains("no saved filter named"));
    assert!(store.delete_filter(on(), &org(), "Pending work").await.is_err());
}

#[tokio::test]
async fn a_save_is_refused_for_a_bad_filter_a_missing_name_and_a_cycle() {
    let store = Store::new();
    work(&store).await;
    let bad = save(&store, "Bad", "kind wobbly").await.unwrap_err().to_string();
    assert!(bad.starts_with("not understood:") && bad.contains("kind wobbly") && bad.ends_with("(at byte 0)"), "{bad}");
    let later = save(&store, "Bad", "blocked and kind wobbly").await.unwrap_err().to_string();
    assert!(later.contains("kind wobbly") && later.contains("(at byte"), "{later}");
    assert!(save(&store, "Empty", "").await.unwrap_err().to_string().contains("no predicate"));
    assert!(save(&store, "Texty", "text hello").await.unwrap_err().to_string().contains("text"));
    assert!(save(&store, "--", "blocked").await.unwrap_err().to_string().contains("letter or a digit"));
    assert!(save(&store, &"n".repeat(65), "blocked").await.unwrap_err().to_string().contains("at most 64"));
    assert!(save(&store, "Names", "saved Nope").await.unwrap_err().to_string().contains("no saved filter named \"Nope\""));
    assert!(store.saved_filters(&org()).iter().all(|f| f.name == "Needs you"), "nothing was kept");
    // a cycle: B names A; A may not then name B (nor itself).
    save(&store, "A", "blocked").await.unwrap();
    save(&store, "B", "saved A or label needs-you").await.unwrap();
    let cycle = save(&store, "A", "saved B").await.unwrap_err().to_string();
    assert!(cycle.contains("a cycle: \"B\" names \"A\" names \"B\""), "{cycle}");
    assert!(save(&store, "Self", "saved self").await.unwrap_err().to_string().contains("a cycle"));
    assert_eq!(store.saved_filter(&org(), "A").unwrap().filter, "blocked", "the refused save changed nothing");
    // `check` reads without keeping.
    assert_eq!(store.check_filter(None, "label  needs-you").unwrap().format(), "label needs-you");
    assert!(store.check_filter(None, "saved Zed").is_err());
    assert!(store.saved_filter(&org(), "Zed").is_err());
    // the proof is for the organization written.
    let elsewhere = On::<Editor>::for_tests(Scope::Org(OrgId::new("acme").unwrap()));
    assert!(store.save_filter(elsewhere, &org(), "X1", "blocked", "k".into()).await.is_err());
}

#[tokio::test]
async fn saved_filters_are_read_again_from_the_table_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        work(&store).await;
        save(&store, "Plexus-Ready", "in PLEXUS and status Ready").await.unwrap();
        save(&store, "Gone", "blocked").await.unwrap();
        store.delete_filter(on(), &org(), "Gone").await.unwrap();
        store.rename_filter(on(), &org(), "Plexus-Ready", "Ready PLEXUS").await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.tickets.read().unwrap().saved.len(), 1);
    assert_eq!(ids(&store, "saved \"ready plexus\""), ["PLEXUS-2", "PLEXUS-3"]);
    assert_eq!(store.saved_filter(&org(), "Ready PLEXUS").unwrap().saved_by, "key:test");
}

#[tokio::test]
async fn a_watch_naming_a_filter_hears_what_a_save_a_rename_and_a_delete_did() {
    let store = Store::new();
    work(&store).await;
    save(&store, "Needs you", "(asking or label needs-you)").await.unwrap();
    save(&store, "Plexus-Ready", "in PLEXUS and status Ready").await.unwrap();
    let text = "saved \"Needs you\" or saved Plexus-Ready";
    let mut rx = store.subscribe();
    let mut watcher = store.watcher_in(store.lens(text).unwrap(), None).naming(Some(&Filter::parse(text).unwrap()));
    assert!(watcher.names_saved());
    assert!(watcher.resaved(&store, None).unwrap().is_empty(), "nothing moved yet");
    // Plexus-Ready now says Draft: PLEXUS-2 leaves, PLEXUS-1 enters; 3 and 4 stay, through Needs you.
    save(&store, "Plexus-Ready", "in PLEXUS and status Draft").await.unwrap();
    let said = watcher.resaved(&store, None).unwrap();
    let sc = |id: &str| store.tickets.read().unwrap().at.get(id).cloned().expect("a ticket").shortcode;
    assert_eq!(
        said,
        [Membership::Entered { id: "PLEXUS-1".into(), shortcode: sc("PLEXUS-1"), seq: 1 }, Membership::Left { id: "PLEXUS-2".into(), shortcode: sc("PLEXUS-2"), seq: 2 }]
    );
    assert!(watcher.resaved(&store, None).unwrap().is_empty(), "said once");
    // the change itself is on the wire for the apps that follow it.
    assert!(matches!(rx.try_recv(), Ok(Change::Filters(FilterChange::Saved { renamed_from: None, .. }))));
    // a rename of a name it uses: the watch's refusal; a delete likewise.
    store.rename_filter(on(), &org(), "Plexus-Ready", "Pending").await.unwrap();
    assert!(matches!(rx.try_recv(), Ok(Change::Filters(FilterChange::Saved { renamed_from: Some(_), .. }))));
    // SURFACE-6: and the old name said gone, right after.
    assert!(matches!(rx.try_recv(), Ok(Change::Filters(FilterChange::Deleted { name })) if name == "Plexus-Ready"));
    assert!(watcher.resaved(&store, None).unwrap_err().contains("no saved filter named \"Plexus-Ready\""));
    store.delete_filter(on(), &org(), "Pending").await.unwrap();
    assert!(matches!(rx.try_recv(), Ok(Change::Filters(FilterChange::Deleted { name })) if name == "Pending"));
    // a watch naming none is never read again.
    let plain = store.watcher_in(store.lens("blocked").unwrap(), None).naming(Some(&Filter::parse("blocked").unwrap()));
    assert!(!plain.names_saved());
}

#[tokio::test]
async fn needs_you_means_the_apps_default_until_one_is_saved() {
    let store = Store::new();
    work(&store).await;
    let default = "(queued or asking or asked or label needs-you)";
    assert_eq!(store.resolved(Some(&Filter::parse("saved \"Needs you\"").unwrap())).unwrap().format(), default);
    assert_eq!(ids(&store, "saved \"needs you\""), ["PLEXUS-3", "PLEXUS-4"]);
    assert_eq!(store.saved_filter(&org(), "needs-you").unwrap().filter, default.trim_matches(['(', ')']));
    assert!(refusal(&store, "saved Plexus-Ready").contains("no saved filter named"), "no other name has a default");
    // saved, it means what was saved; deleted, the default again.
    save(&store, "Needs you", "blocked").await.unwrap();
    assert!(ids(&store, "saved \"Needs you\"").is_empty());
    store.delete_filter(on(), &org(), "needs you").await.unwrap();
    assert_eq!(ids(&store, "saved \"Needs you\""), ["PLEXUS-3", "PLEXUS-4"]);
}

/// SURFACE-6: a rename is said as the new row and then the old name gone —
/// so a reader keyed by name follows a rename, a rename back and a case-only
/// rename; a rename to the very name it has says no delete.
#[tokio::test]
async fn a_rename_says_the_new_row_and_then_the_old_name_gone() {
    let store = Store::new();
    work(&store).await;
    save(&store, "Plexus-Ready", "in PLEXUS and status Ready").await.unwrap();
    let mut rx = store.subscribe();
    let mut heard = Vec::new();
    for (from, to) in [("Plexus-Ready", "Pending"), ("Pending", "Plexus-Ready"), ("Plexus-Ready", "PLEXUS-READY"), ("PLEXUS-READY", "PLEXUS-READY")] {
        store.rename_filter(on(), &org(), from, to).await.unwrap();
        while let Ok(c) = rx.try_recv() {
            match c {
                Change::Filters(FilterChange::Saved { filter, renamed_from }) => heard.push(format!("saved {} from {}", filter.name, renamed_from.unwrap_or_default())),
                Change::Filters(FilterChange::Deleted { name }) => heard.push(format!("deleted {name}")),
                _ => {}
            }
        }
    }
    assert_eq!(
        heard,
        [
            "saved Pending from Plexus-Ready",
            "deleted Plexus-Ready",
            "saved Plexus-Ready from Pending",
            "deleted Pending",
            "saved PLEXUS-READY from Plexus-Ready",
            "deleted Plexus-Ready",
            "saved PLEXUS-READY from PLEXUS-READY",
        ]
    );
}
