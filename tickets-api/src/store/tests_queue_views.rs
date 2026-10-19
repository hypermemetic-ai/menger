//! APP-85: a queue entry carries views of mockups — `--mockup T#m1` (or
//! APP-45's `mockup:T#N@W`): each checked against the body (a mockup it
//! holds, a whole page for `T`), refused in band naming it; kept on the
//! entry and its records, added on a requeue, across a reopen; an entry
//! without them as before.

use super::*;
use crate::queue::{MockupView, QueueAction, Urgency};
use crate::types::TransitionSource;
use crate::vocab::MockupRef;

fn ticket(kind: Kind, body: &str) -> NewTicket {
    NewTicket { epic: "K".into(), title: format!("a {}", kind.as_str()), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: body.into() }
}

fn act() -> Act {
    Act { actor: "agent:kiln".into(), name: None, source: TransitionSource::Api, message: None, cause: None }
}

const PAGE: &str = "<!doctype html>\n<html><body><main>a look</main></body></html>\n";

/// A decision between three looks, one mockup in each option.
fn looks() -> String {
    let opt = |name: &str, value: &str| format!("<m-option data-field=\"option\" name=\"{name}\" value=\"{value}\">\n\n```mockup widths=390,1440 title=\"{name}\"\n{PAGE}```\n\n</m-option>\n");
    format!(
        "Three layouts are drawn.\n\n<m-decision data-field=\"decision\" id=\"layout\">\n<m-question data-field=\"question\">Which shelf layout ships?</m-question>\n\n{}{}{}</m-decision>\n",
        opt("Grid", "grid"),
        opt("Ring", "ring"),
        opt("List", "list")
    )
}

/// K-1 a scope; K-2 the decision with three looks; K-3 a whole page; K-4 plain.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, ticket(Kind::Scope, "root")).await.unwrap();
    store.create(SYS, ticket(Kind::Build, &looks())).await.unwrap();
    store.create(SYS, ticket(Kind::Build, &format!("---\nview: mockup\n---\n{PAGE}"))).await.unwrap();
    store.create(SYS, ticket(Kind::Build, "no mockups here")).await.unwrap();
    store
}

fn refs(names: &[&str]) -> Vec<MockupRef> {
    names.iter().map(|n| n.parse().unwrap()).collect()
}

fn view(t: &str, mock: Option<u32>, width: Option<u32>) -> MockupView {
    MockupView { ticket: t.into(), mock, width }
}

#[test]
fn a_body_counts_its_mockups_as_the_app_numbers_them() {
    assert_eq!(crate::mockups::count(&looks()), 3);
    assert_eq!(crate::mockups::count("<m-mockup ref=\"DESIGN-49\"></m-mockup>\n\n<m-mockup widths=\"390\">\n<p>x</p>\n</m-mockup>\n"), 2, "a ref and an element");
    assert_eq!(crate::mockups::count("`<m-mockup>` in code\n\n```html\n<m-mockup ref=\"X-1\"></m-mockup>\n```\n\n<!-- <m-mockup ref=\"X-1\"></m-mockup> -->\n"), 0, "code and comments hold none");
    assert_eq!(crate::mockups::count("```mockup\n<!doctype html><p>a</p>\n```\n"), 1, "a fence");
    assert!(crate::mockups::whole_page(&format!("---\nview: mockup\n---\n{PAGE}")) && !crate::mockups::whole_page(&looks()));
}

#[tokio::test]
async fn views_are_checked_against_the_body() {
    let store = store().await;
    let got = store.mockup_views(SYS, &refs(&["k-2#m1", "mockup:K-2#3@390", "K-3"])).await.unwrap();
    assert_eq!(got, [view("K-2", Some(1), None), view("K-2", Some(3), Some(390)), view("K-3", None, None)]);
    for (bad, says) in [
        ("K-2#m4", "mockup `K-2#m4`: K-2 holds 3 — no mockup 4"),
        ("K-4#m1", "mockup `K-4#m1`: K-4 holds none — no mockup 1"),
        ("K-99#m1", "mockup `K-99#m1`: no ticket K-99 you can read"),
        ("K-2", "mockup `K-2`: K-2 is no whole page"),
    ] {
        let e = store.mockup_views(SYS, &refs(&[bad])).await.unwrap_err();
        assert!(e.to_string().contains(says), "{bad}: {e}");
    }
}

#[tokio::test]
async fn an_entry_carries_its_views_and_a_requeue_adds() {
    let store = store().await;
    let views = store.mockup_views(SYS, &refs(&["K-2#m1", "K-2#m2"])).await.unwrap();
    let ask = QueueAsk { urgency: Urgency::Now, reason: "pick a look".into(), inputs: vec!["layout".into()], mockups: views, ..QueueAsk::default() };
    let QueueChange::Queued { entry, .. } = store.queue_with(SYS.permit("K-2"), ask, act()).await.unwrap() else { panic!() };
    assert_eq!(entry.mockups, [view("K-2", Some(1), None), view("K-2", Some(2), None)]);
    assert_eq!(entry.input.as_deref(), Some("layout"));
    // Again with the third and one it has: added once, in order.
    let more = store.mockup_views(SYS, &refs(&["K-2#m2", "K-2#m3"])).await.unwrap();
    let ask = QueueAsk { urgency: Urgency::Today, reason: "and the list".into(), mockups: more, ..QueueAsk::default() };
    let QueueChange::Queued { entry, action, .. } = store.queue_with(SYS.permit("K-2"), ask, act()).await.unwrap() else { panic!() };
    assert_eq!((entry.mockups.len(), action, entry.urgency), (3, QueueAction::Requeued, Urgency::Now));
    assert_eq!(entry.input.as_deref(), Some("layout"), "its input kept");
    let records = store.queue_history(SYS, "K-2");
    assert_eq!((records[0].mockups.len(), records[1].mockups.len()), (2, 3), "the records carry them");
    // Past the cap: refused, nothing written.
    let nine: Vec<MockupView> = (1..=9).map(|n| view("K-2", Some(n), Some(390))).collect();
    let ask = QueueAsk { urgency: Urgency::Now, reason: "too many".into(), mockups: nine, ..QueueAsk::default() };
    assert!(store.queue_with(SYS.permit("K-2"), ask, act()).await.unwrap_err().to_string().contains("at most 8 views"));
    assert_eq!(store.queue_entry(SYS, "K-2").unwrap().mockups.len(), 3);
}

#[tokio::test]
async fn views_survive_a_reopen_and_an_entry_without_them_is_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket(Kind::Scope, "root")).await.unwrap();
        store.create(SYS, ticket(Kind::Build, &looks())).await.unwrap();
        let views = store.mockup_views(SYS, &refs(&["K-2#m1@390", "K-2#m2"])).await.unwrap();
        let ask = QueueAsk { urgency: Urgency::Now, reason: "pick".into(), inputs: vec!["layout".into()], mockups: views, ..QueueAsk::default() };
        store.queue_with(SYS.permit("K-2"), ask, act()).await.unwrap();
        store.queue_by(SYS.permit("K-1"), Urgency::Today, "plain", None, act()).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.queue_entry(SYS, "K-2").unwrap().mockups, [view("K-2", Some(1), Some(390)), view("K-2", Some(2), None)]);
    assert_eq!(store.queue_history(SYS, "K-2")[0].mockups.len(), 2);
    let plain = store.queue_entry(SYS, "K-1").unwrap();
    assert!(plain.mockups.is_empty());
    assert!(serde_json::to_value(&plain).unwrap().get("mockups").is_none(), "no views: no field on the wire");
}
