//! UI-6: the router.

use super::*;
use serde_json::json;

/// `#/ticket/:id` gives the prop `id`; the more specific pattern wins; an
/// empty hash is `#/`; a hash no pattern fits is none.
#[test]
fn a_route_names_its_component_and_its_segments_are_props() {
    let routes = BTreeMap::from([
        ("#/".to_string(), "page-shell".to_string()),
        ("#/ticket/:id".to_string(), "ticket-page".to_string()),
        ("#/ticket/new".to_string(), "ticket-new".to_string()),
        ("#/:epic/:id".to_string(), "epic-ticket".to_string()),
    ]);
    let r = Router::new(&routes);
    let at = |h: &str| r.resolve(h).map(|x| (x.component, Value::Object(x.props)));
    assert_eq!(at("#/ticket/T-1"), Some(("ticket-page".into(), json!({ "id": "T-1" }))));
    assert_eq!(at("#/ticket/new"), Some(("ticket-new".into(), json!({}))), "the literal pattern wins over :id");
    assert_eq!(at("#/kiln/T-1"), Some(("epic-ticket".into(), json!({ "epic": "kiln", "id": "T-1" }))));
    assert_eq!(at(""), Some(("page-shell".into(), json!({}))), "an empty hash is #/");
    assert_eq!(at("#/"), Some(("page-shell".into(), json!({}))));
    assert_eq!(at("#/a/b/c"), None);
}

/// UI-8: a `:name` segment is the text, percent-decoded (`#/f/status%20Ready`
/// → `status Ready`); a stray `%` is kept.
#[test]
fn a_segment_is_percent_decoded() {
    let routes = BTreeMap::from([("#/f/:filter".to_string(), "board-layout".to_string())]);
    let r = Router::new(&routes);
    let at = |h: &str| r.resolve(h).map(|x| Value::Object(x.props));
    assert_eq!(at("#/f/status%20Ready"), Some(json!({ "filter": "status Ready" })));
    assert_eq!(at("#/f/a%2Fb%25c"), Some(json!({ "filter": "a/b%c" })));
    assert_eq!(at("#/f/100%"), Some(json!({ "filter": "100%" })));
    assert_eq!(at("#/f/%E2%86%92"), Some(json!({ "filter": "→" })));
}
