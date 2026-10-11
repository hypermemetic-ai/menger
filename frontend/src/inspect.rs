//! COMP-9: what the host answers a frame about a component instance — the
//! tooltip's content — and where a frame may send the app.
//!
//! **The tooltip.** Every instance in a frame wears a corner badge (the SDK);
//! hovering or focusing it asks the host, `menger:inspect { id, name, attrs
//! }`, and the host answers `menger:inspected { id, inspection }` with
//! [`inspect`]'s model, which the SDK draws: the name, its summary (or "no
//! docs yet"), each **declared** attribute with this instance's value and
//! its state — a required one missing, a value its `AttrKind` refuses
//! (`AttrKind::check`: a number, a boolean, an existing shortcode, an
//! existing ticket id, one of an enum's values) — and the routes of
//! "definition ↗" (the owner's card, [`definition_route`]; an orphan's help
//! page) and "docs ↗" (the help page). The host answers because it holds
//! what decides — the registry, and the extents that say which shortcodes
//! and tickets exist (the board's, SCALE-26) — and so the checks are Rust,
//! pinned by native tests, not script in every frame.
//!
//! **Navigation.** `menger.navigate(route)` (SDK → `menger:navigate`) moves
//! the app to an app route — [`app_route`]: `#/ticket/<id>` (with the card
//! anchor's `?component=<tag>`), `#/components`, `#/components/<tag>`,
//! `#/waves`, `#/waves/<SHORTCODE>` (with a board's query); anything else
//! is ignored. [`anchor`] reads the anchor back off the hash.
//! COMP-14: a ticket route may name a **section** instead of a card —
//! `?field=<name>` (the element carrying `data-field="<name>"`) or
//! `?anchor=<id>` (the element with that id) — what a cite window's header
//! opens; [`anchor`] reads whichever the hash names.

use menger_tickets_api::component_docs::is_component_name;
use menger_tickets_api::{AttrState, Component, Known};
use serde::Serialize;

/// One declared attribute on the instance, as the tooltip shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AttrRow {
    pub(crate) name: String,
    /// The type as written (`shortcode`, `enum(a|b)`).
    pub(crate) kind: String,
    pub(crate) required: bool,
    /// The instance's value — `None`: not set.
    pub(crate) value: Option<String>,
    /// `fits`, `unset`, `missing` or `unfit`.
    pub(crate) state: &'static str,
    /// What is wrong, for `missing` and `unfit`.
    pub(crate) problem: Option<String>,
}

/// The tooltip's content (sent to the frame as the wire spells it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Inspection {
    pub(crate) name: String,
    /// The defining ticket — `None` for an orphan, or a name not in the
    /// registry.
    pub(crate) owner: Option<String>,
    /// Whether the registry has the name at all.
    pub(crate) registered: bool,
    /// Whether the source holds `<docs>`.
    pub(crate) documented: bool,
    pub(crate) summary: String,
    pub(crate) attrs: Vec<AttrRow>,
    /// How many of the attributes are flagged (missing or unfit).
    pub(crate) flagged: usize,
    /// "definition ↗": the owner's card, or — an orphan — the help page.
    pub(crate) definition: String,
    /// "docs ↗": the help page.
    pub(crate) docs: String,
}

/// The help page of a component: `#/components/<tag>`.
pub(crate) fn docs_route(name: &str) -> String {
    format!("#/components/{name}")
}

/// Where a component is defined: its owner's ticket, scrolled to its card
/// (`#/ticket/<OWNER>?component=<tag>`) — or, for an orphan (no ticket
/// defines it any more), its help page.
pub(crate) fn definition_route(name: &str, owner: Option<&str>) -> String {
    match owner {
        Some(owner) => format!("#/ticket/{owner}?component={name}"),
        None => docs_route(name),
    }
}

/// What the tooltip says about an instance of `name` setting `attrs`,
/// given the registry's `component` (if it has the name) and what is
/// `known` to exist.
pub(crate) fn inspect(name: &str, component: Option<&Component>, attrs: &[(String, String)], known: &dyn Known) -> Inspection {
    let owner = component.and_then(|c| c.owner.clone());
    let docs = component.and_then(|c| c.docs.as_ref());
    let value = |attr: &str| attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(attr)).map(|(_, v)| v.clone());
    let rows: Vec<AttrRow> = docs
        .map(|d| {
            d.attrs
                .iter()
                .map(|a| {
                    let v = value(&a.name);
                    let (state, problem) = match a.state(v.as_deref(), known) {
                        AttrState::Fits => ("fits", None),
                        AttrState::Unset => ("unset", None),
                        AttrState::Missing => ("missing", Some("required, and not set".to_string())),
                        AttrState::Unfit(why) => ("unfit", Some(why)),
                    };
                    AttrRow { name: a.name.clone(), kind: a.kind.to_string(), required: a.required, value: v, state, problem }
                })
                .collect()
        })
        .unwrap_or_default();
    Inspection {
        name: name.to_string(),
        definition: definition_route(name, owner.as_deref()),
        docs: docs_route(name),
        owner,
        registered: component.is_some(),
        documented: docs.is_some(),
        summary: docs.map(|d| d.summary.clone()).unwrap_or_default(),
        flagged: rows.iter().filter(|r| matches!(r.state, "missing" | "unfit")).count(),
        attrs: rows,
    }
}

/// A route's characters: what an id, a tag, a shortcode and a board's
/// query (`sc=is:RETURNS&status=not:Complete`, percent-escapes) are made of.
fn route_chars_ok(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:=&,%+!~*/".contains(c))
}

/// COMP-9: `menger.navigate`'s allow-list — the app route `route` names, as
/// the hash to set, or `None` (ignored): `#/ticket/<id>[?component=<tag>]`,
/// `#/components`, `#/components/<tag>`, `#/waves`, `#/waves/<SHORTCODE>[?query]`.
pub(crate) fn app_route(route: &str) -> Option<String> {
    let path = route.trim().strip_prefix("#/")?;
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    if !route_chars_ok(path) || query.is_some_and(|q| !route_chars_ok(q)) {
        return None;
    }
    let ok = match path.split_once('/') {
        Some(("ticket", id)) => {
            crate::kids::key(id).is_some_and(|(epic, _)| !epic.is_empty() && epic.chars().all(|c| c.is_ascii_alphanumeric()))
                && query.is_none_or(|q| {
                    q.strip_prefix("component=").is_some_and(is_component_name)
                        || q.strip_prefix("field=").is_some_and(section_name_ok)
                        || q.strip_prefix("anchor=").is_some_and(section_name_ok)
                })
        }
        Some(("components", tag)) => (tag.is_empty() || is_component_name(tag)) && query.is_none(),
        Some(("waves", sc)) => sc.is_empty() || sc.chars().all(|c| c.is_ascii_alphanumeric()),
        None => (path == "components" && query.is_none()) || path == "waves",
        _ => false,
    };
    ok.then(|| format!("#/{path}{}", query.map(|q| format!("?{q}")).unwrap_or_default()))
}

/// COMP-14: a section's name — a `data-field` value or an element id a cite
/// addresses: 1–128 of ASCII letters, digits, `_`, `-`, `.`, `:`. Nothing
/// that could end an attribute, a selector's string or a route.
pub(crate) fn section_name_ok(name: &str) -> bool {
    (1..=128).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
}

/// What a ticket route asks the frame showing it to scroll to and flash:
/// COMP-9's card (`?component=<tag>`), or COMP-14's section (`?field=<name>`,
/// `?anchor=<id>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Component(String),
    Field(String),
    Id(String),
}

impl Anchor {
    /// The key it travels under in `menger:reveal` / `menger:revealed`, and its value.
    pub(crate) fn pair(&self) -> (&'static str, &str) {
        match self {
            Anchor::Component(n) => ("component", n),
            Anchor::Field(n) => ("field", n),
            Anchor::Id(n) => ("anchor", n),
        }
    }
}

/// The anchor on a ticket route, if it names one ([`Anchor`]).
pub(crate) fn anchor(hash: &str) -> Option<Anchor> {
    let path = hash.strip_prefix('#').unwrap_or(hash);
    let path = path.strip_prefix('/').unwrap_or(path);
    let (path, query) = path.split_once('?')?;
    if !path.starts_with("ticket/") {
        return None;
    }
    query.split('&').find_map(|kv| {
        if let Some(n) = kv.strip_prefix("component=") {
            let n = n.to_ascii_lowercase();
            return is_component_name(&n).then_some(Anchor::Component(n));
        }
        if let Some(n) = kv.strip_prefix("field=") {
            return section_name_ok(n).then(|| Anchor::Field(n.to_string()));
        }
        kv.strip_prefix("anchor=").filter(|n| section_name_ok(n)).map(|n| Anchor::Id(n.to_string()))
    })
}

/// COMP-9: the card anchor on a ticket route — `#/ticket/<OWNER>?component=<tag>`
/// names the card to scroll to and flash.
#[cfg(test)]
pub(crate) fn card_anchor(hash: &str) -> Option<String> {
    match anchor(hash)? {
        Anchor::Component(n) => Some(n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{AttrDoc, AttrKind, ComponentDocs};

    struct Board;
    impl Known for Board {
        fn shortcode(&self, epic: &str) -> Option<bool> {
            Some(epic == "RETURNS")
        }
        fn ticket(&self, id: &str) -> Option<bool> {
            Some(id == "RETURNS-6")
        }
    }

    fn summary_component(owner: Option<&str>) -> Component {
        Component {
            name: "m-wave-summary".into(),
            source: "<template></template>".into(),
            owner: owner.map(str::to_string),
            references: vec!["DEMO-6".into()],
            docs: Some(ComponentDocs {
                summary: "Wave 1, counted.".into(),
                attrs: vec![
                    AttrDoc { name: "board".into(), kind: AttrKind::Shortcode, required: true, description: String::new() },
                    AttrDoc { name: "focus".into(), kind: AttrKind::Ticket, required: false, description: String::new() },
                    AttrDoc { name: "limit".into(), kind: AttrKind::Number, required: false, description: String::new() },
                ],
                ..Default::default()
            }),
            preview: vec![],
        }
    }

    fn states(i: &Inspection) -> Vec<(&str, &str, Option<&str>)> {
        i.attrs.iter().map(|r| (r.name.as_str(), r.state, r.value.as_deref())).collect()
    }

    /// COMP-9 acceptance 1 (and 2's tooltip): `board=RETURNS` fits; a second
    /// instance's `board="NOPE"` is flagged; a missing required one too; an
    /// undeclared attribute is not shown; the routes to the owner's card and
    /// the help page.
    #[test]
    fn an_instance_is_inspected_against_its_docs() {
        let c = summary_component(Some("DEMO-5"));
        let ok = inspect("m-wave-summary", Some(&c), &[("board".into(), "RETURNS".into()), ("class".into(), "x".into())], &Board);
        assert_eq!(states(&ok), vec![("board", "fits", Some("RETURNS")), ("focus", "unset", None), ("limit", "unset", None)]);
        assert_eq!((ok.flagged, ok.summary.as_str(), ok.documented, ok.registered), (0, "Wave 1, counted.", true, true));
        assert_eq!(ok.definition, "#/ticket/DEMO-5?component=m-wave-summary");
        assert_eq!(ok.docs, "#/components/m-wave-summary");
        let nope = inspect("m-wave-summary", Some(&c), &[("board".into(), "NOPE".into()), ("focus".into(), "RETURNS-99".into())], &Board);
        assert_eq!(nope.flagged, 2);
        assert_eq!(nope.attrs[0].problem.as_deref(), Some("no shortcode 'NOPE'"));
        assert_eq!(nope.attrs[1].problem.as_deref(), Some("no ticket 'RETURNS-99'"));
        let bare = inspect("m-wave-summary", Some(&c), &[("limit".into(), "lots".into()), ("BOARD".into(), "returns".into())], &Board);
        assert_eq!(states(&bare)[0], ("board", "fits", Some("returns")), "an attribute's name folds, as HTML's does");
        assert_eq!(bare.attrs[2].problem.as_deref(), Some("'lots' is not a number"));
        let missing = inspect("m-wave-summary", Some(&c), &[], &Board);
        assert_eq!((missing.attrs[0].state, missing.attrs[0].problem.as_deref()), ("missing", Some("required, and not set")));
    }

    /// An orphan's "definition ↗" is its help page; a component without
    /// docs has no rows ("no docs yet"); a name not in the registry says so.
    #[test]
    fn an_orphan_one_without_docs_and_an_unknown_name() {
        let orphan = inspect("m-wave-summary", Some(&summary_component(None)), &[], &Board);
        assert_eq!((orphan.owner.as_deref(), orphan.definition.as_str()), (None, "#/components/m-wave-summary"));
        let mut plain = summary_component(Some("A-2"));
        plain.docs = None;
        let undocumented = inspect("m-wave-summary", Some(&plain), &[("board".into(), "X".into())], &Board);
        assert!(!undocumented.documented && undocumented.attrs.is_empty() && undocumented.summary.is_empty());
        let unknown = inspect("m-gone", None, &[], &Board);
        assert!(!unknown.registered && !unknown.documented);
        assert_eq!(unknown.definition, "#/components/m-gone");
    }

    /// COMP-9 acceptance 1: `navigate`'s route allow-list.
    #[test]
    fn navigate_takes_app_routes_only() {
        for (route, want) in [
            ("#/ticket/DEMO-5", "#/ticket/DEMO-5"),
            ("#/ticket/DEMO-5?component=m-wave-summary", "#/ticket/DEMO-5?component=m-wave-summary"),
            ("#/components", "#/components"),
            ("#/components/m-wave-summary", "#/components/m-wave-summary"),
            ("#/waves", "#/waves"),
            ("#/waves/PLEXUS", "#/waves/PLEXUS"),
            ("#/waves/PLEXUS?sc=is:RETURNS&status=not:Complete", "#/waves/PLEXUS?sc=is:RETURNS&status=not:Complete"),
            (" #/ticket/MNG-2 ", "#/ticket/MNG-2"),
            // COMP-14: a cite window's header opens its section.
            ("#/ticket/AUTH-10?field=findings", "#/ticket/AUTH-10?field=findings"),
            ("#/ticket/AUTH-10?anchor=the-table", "#/ticket/AUTH-10?anchor=the-table"),
        ] {
            assert_eq!(app_route(route).as_deref(), Some(want), "{route}");
        }
        for route in [
            "", "#", "#/", "/app/#/ticket/A-1", "https://example.com", "javascript:alert(1)", "#/ticket/", "#/ticket/A",
            "#/ticket/A-1/x", "#/ticket/A-1?component=Bad", "#/ticket/A-1?evil=1", "#/components/Not-A-Tag", "#/components?x",
            "#/settings", "#/ticket/A-1\"><script>", "#/waves/PLEX US", "#/waves/../x", "#//evil.example",
            "#/ticket/A-1?field=", "#/ticket/A-1?field=a/b", "#/ticket/A-1?anchor=%22x", "#/components/m-x?field=a",
        ] {
            assert_eq!(app_route(route), None, "{route}");
        }
    }

    #[test]
    fn the_card_anchor_is_read_off_a_ticket_route() {
        assert_eq!(card_anchor("#/ticket/DEMO-5?component=m-wave-summary").as_deref(), Some("m-wave-summary"));
        assert_eq!(card_anchor("#/ticket/DEMO-5?x=1&component=M-X").as_deref(), Some("m-x"));
        assert_eq!(card_anchor("#/ticket/DEMO-5"), None);
        assert_eq!(card_anchor("#/components/m-x?component=m-x"), None);
        assert_eq!(card_anchor("#/ticket/DEMO-5?component=nohyphen"), None);
        assert_eq!(card_anchor("#/ticket/DEMO-5?field=findings"), None);
    }

    /// COMP-14: a section anchor — by `data-field` or by id — read off a
    /// ticket route; a name that could escape a selector is none.
    #[test]
    fn a_section_anchor_is_read_off_a_ticket_route() {
        assert_eq!(anchor("#/ticket/AUTH-10?field=findings"), Some(Anchor::Field("findings".into())));
        assert_eq!(anchor("#/ticket/AUTH-10?anchor=t.1:x"), Some(Anchor::Id("t.1:x".into())));
        assert_eq!(anchor("#/ticket/AUTH-10?component=m-x"), Some(Anchor::Component("m-x".into())));
        assert_eq!(anchor("#/ticket/AUTH-10?field=a\"]"), None);
        assert_eq!(anchor("#/waves?field=a"), None);
        assert_eq!(Anchor::Field("f".into()).pair(), ("field", "f"));
        assert_eq!(Anchor::Id("i".into()).pair(), ("anchor", "i"));
        assert!(!section_name_ok("") && !section_name_ok(&"x".repeat(129)) && !section_name_ok("a b"));
    }
}
