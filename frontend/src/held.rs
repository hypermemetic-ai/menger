//! ACCESS-26: **frames bounded — the registry as a frame is handed it.**
//!
//! IDENTITY-36 held every read a frame makes to its reach (src/bridge.rs),
//! but the component registry still went to every frame whole: every
//! component the viewer may read (ACCESS-18), each with its `references` —
//! the id of every ticket, in every shortcode the viewer sees, whose body
//! uses it. A body in shortcode A, opened by an admin, could read the ids
//! of every ticket anywhere that asks a decision (`m-decision`'s
//! references), and the source of every component in every shortcode.
//!
//! So a frame is handed only the components its ticket could show
//! ([`held_registry`]):
//!
//! - those defined inside its reach (the owner a ticket there),
//! - those a ticket inside its reach uses (a reference held by the reach),
//! - those its own markup names ([`names_in`] — the host reads the markup
//!   it hands over: an editor's draft that names a component before it is
//!   saved, a body that makes one by script, `createElement("m-x")`),
//! - and every component one of those uses, transitively (the server's
//!   references name each component whose source holds a name).
//!
//! Each with its references held: the tickets inside the reach and the
//! components handed with it, nothing else. Its owner stays — the code the
//! frame runs says where it is defined, as the frame's own tooltip does
//! ("definition ↗"). A frame with no reach (a help card for an orphan)
//! is handed what its markup names, and what that uses.
//!
//! What this bounds is enumeration: what exists elsewhere, and where it is
//! used, is never handed over. A body that names a component the viewer
//! may read is handed it, as it always rendered it.
//!
//! [`bounded_known`] holds the tooltip's checks (`menger:inspect`) to the
//! reach the same way: a ticket or shortcode outside it is "cannot say",
//! never "no such ticket" — the tooltip is no existence oracle.
//!
//! Pure, and pinned by native tests below; src/frame.rs applies it.

use std::collections::HashSet;

use menger_tickets_api::component_docs::is_component_name;
use menger_tickets_api::{Component, Known};

use crate::bridge::Reach;

/// Every word of `text` that could be a component's name — a run of
/// `[a-z0-9._-]` that starts with a letter and holds a hyphen, read with
/// ASCII case folded (an HTML tag's name is case-blind). A superset of the
/// names `text` uses, as tags or as strings in its scripts.
pub(crate) fn names_in(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_');
    for run in text.split(|c: char| !word(c)) {
        let run = run.trim_end_matches(['.', '-', '_']);
        if run.contains('-') && run.starts_with(|c: char| c.is_ascii_alphabetic()) {
            let name = run.to_ascii_lowercase();
            if is_component_name(&name) {
                out.insert(name);
            }
        }
    }
    out
}

/// The names [`names_in`] found, as the host keeps them on the `<iframe>`
/// (`data-names`, where nothing in the frame can reach): space-separated,
/// sorted.
pub(crate) fn names_attr(names: &HashSet<String>) -> String {
    let mut v: Vec<&str> = names.iter().map(String::as_str).collect();
    v.sort_unstable();
    v.join(" ")
}

/// [`names_attr`], read back.
pub(crate) fn names_from_attr(attr: Option<&str>) -> HashSet<String> {
    attr.map(|a| a.split_ascii_whitespace().map(str::to_string).collect()).unwrap_or_default()
}

/// The registry `all` as a frame held to `reach` (`None`: it renders no
/// ticket's body) whose markup names `names` is handed it — see the
/// module's doc. In `all`'s order.
pub(crate) fn held_registry(all: &[Component], reach: Option<&Reach>, names: &HashSet<String>) -> Vec<Component> {
    let inside = |id: &str| reach.is_some_and(|r| r.holds_id(id));
    let mut held: HashSet<&str> = all
        .iter()
        .filter(|c| {
            names.contains(&c.name)
                || c.owner.as_deref().is_some_and(inside)
                || c.references.iter().any(|r| !is_component_name(r) && inside(r))
        })
        .map(|c| c.name.as_str())
        .collect();
    // What a held component uses: each component whose references name a
    // held one (its source holds that name), until nothing more joins.
    loop {
        let more: Vec<&str> = all
            .iter()
            .filter(|c| !held.contains(c.name.as_str()) && c.references.iter().any(|r| held.contains(r.as_str())))
            .map(|c| c.name.as_str())
            .collect();
        if more.is_empty() {
            break;
        }
        held.extend(more);
    }
    all.iter()
        .filter(|c| held.contains(c.name.as_str()))
        .map(|c| {
            let mut c = c.clone();
            c.references.retain(|r| if is_component_name(r) { held.contains(r.as_str()) } else { inside(r) });
            c
        })
        .collect()
}

/// The names in a held registry — what the host keeps as handed
/// (`data-held`), to say a removal only to a frame that had the name.
pub(crate) fn held_names(held: &[Component]) -> HashSet<String> {
    held.iter().map(|c| c.name.clone()).collect()
}

/// What a frame's tooltip may know exists: `known`, held to `reach` — a
/// shortcode or ticket outside it is `None` ("cannot say"; the checks go
/// by form), whatever `known` says.
pub(crate) struct Bounded<'a> {
    pub(crate) known: &'a dyn Known,
    pub(crate) reach: Option<&'a Reach>,
}

impl Known for Bounded<'_> {
    fn shortcode(&self, epic: &str) -> Option<bool> {
        self.reach.filter(|r| r.holds(epic)).and_then(|_| self.known.shortcode(epic))
    }
    fn ticket(&self, id: &str) -> Option<bool> {
        self.reach.filter(|r| r.holds_id(id)).and_then(|_| self.known.ticket(id))
    }
}

/// [`Bounded`], built.
pub(crate) fn bounded_known<'a>(known: &'a dyn Known, reach: Option<&'a Reach>) -> Bounded<'a> {
    Bounded { known, reach }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(name: &str, owner: Option<&str>, refs: &[&str]) -> Component {
        Component {
            name: name.into(),
            source: format!("<template>{name}</template>"),
            owner: owner.map(str::to_string),
            references: refs.iter().map(|r| r.to_string()).collect(),
            docs: None,
            preview: vec![],
        }
    }

    /// AREA, and SUB hosted under it.
    fn area() -> Reach {
        let mut r = Reach::only("AREA");
        r.epics.insert("SUB".into());
        r
    }

    fn registry() -> Vec<Component> {
        vec![
            // A house component, defined elsewhere, used everywhere.
            comp("m-decision", Some("ART-3"), &["AREA-2", "SECRET-4", "OTHER-9", "m-poll"]),
            // Defined inside the reach (in a hosted shortcode).
            comp("m-area-chart", Some("SUB-1"), &[]),
            // Defined elsewhere, used only elsewhere.
            comp("m-secret", Some("SECRET-1"), &["SECRET-2"]),
            // Uses m-decision (its source names it) — used only elsewhere.
            comp("m-poll", Some("OTHER-1"), &["OTHER-3"]),
            // Used by m-area-chart's source (its references name it).
            comp("m-badge", Some("ART-1"), &["m-area-chart", "SECRET-2"]),
            // An orphan, used by no one the reach holds.
            comp("m-old", None, &["SECRET-7"]),
        ]
    }

    fn names(v: &[Component]) -> Vec<&str> {
        v.iter().map(|c| c.name.as_str()).collect()
    }

    /// A frame is handed what its reach defines and uses, and what those
    /// use — never what is only defined and used elsewhere; each with
    /// only the references inside.
    #[test]
    fn a_frame_is_handed_only_what_its_ticket_could_show() {
        let all = registry();
        let held = held_registry(&all, Some(&area()), &HashSet::new());
        assert_eq!(names(&held), ["m-decision", "m-area-chart", "m-badge"]);
        let decision = &held[0];
        assert_eq!(decision.references, ["AREA-2"], "no ticket outside, no component not handed");
        assert_eq!(decision.owner.as_deref(), Some("ART-3"), "the code says where it is defined");
        assert_eq!(held[2].references, ["m-area-chart"]);
        // The source is the registry's own.
        assert_eq!(decision.source, all[0].source);
    }

    /// What the frame's own markup names is handed, with what it uses — an
    /// editor's draft that names a component before any ticket does.
    #[test]
    fn a_frame_is_handed_what_its_markup_names() {
        let all = registry();
        let held = held_registry(&all, Some(&area()), &names_in("<p>draft</p><m-poll q=\"x\"></m-poll>"));
        assert_eq!(names(&held), ["m-decision", "m-area-chart", "m-poll", "m-badge"]);
        let poll = held.iter().find(|c| c.name == "m-poll").unwrap();
        assert!(poll.references.is_empty(), "{:?}", poll.references);
        let decision = &held[0];
        assert_eq!(decision.references, ["AREA-2", "m-poll"]);
    }

    /// A frame with no reach (a help card) is handed what its markup names,
    /// and what that uses — and no ticket's id.
    #[test]
    fn a_frame_with_no_reach_is_handed_what_it_names() {
        let all = registry();
        assert!(held_registry(&all, None, &HashSet::new()).is_empty());
        let held = held_registry(&all, None, &names_in("<m-old></m-old>"));
        assert_eq!(names(&held), ["m-old"]);
        assert!(held[0].references.is_empty());
        let held = held_registry(&all, None, &names_in("<m-area-chart>"));
        assert_eq!(names(&held), ["m-area-chart", "m-badge"]);
        assert_eq!(held[1].references, ["m-area-chart"]);
    }

    /// The words that may be names: tags and strings alike, case folded,
    /// trailing punctuation off; attribute names come along (a superset).
    #[test]
    fn names_are_read_from_tags_and_scripts() {
        let n = names_in("<M-Decision data-field=\"x\"></m-decision><script>document.createElement('m-chart.v2');</script> m-end. 9-x -x");
        let mut v: Vec<&str> = n.iter().map(String::as_str).collect();
        v.sort_unstable();
        assert_eq!(v, ["data-field", "m-chart.v2", "m-decision", "m-end"]);
        assert_eq!(names_from_attr(Some(&names_attr(&n))), n);
        assert!(names_from_attr(None).is_empty());
    }

    struct All;
    impl Known for All {
        fn shortcode(&self, _: &str) -> Option<bool> {
            Some(false)
        }
        fn ticket(&self, _: &str) -> Option<bool> {
            Some(true)
        }
    }

    /// The tooltip's checks: inside the reach as the app knows; outside it,
    /// or with no reach, "cannot say".
    #[test]
    fn the_tooltip_knows_only_the_reach() {
        let reach = area();
        let k = bounded_known(&All, Some(&reach));
        assert_eq!((k.ticket("SUB-4"), k.shortcode("AREA")), (Some(true), Some(false)));
        assert_eq!((k.ticket("SECRET-4"), k.shortcode("SECRET")), (None, None));
        let k = bounded_known(&All, None);
        assert_eq!((k.ticket("SUB-4"), k.shortcode("AREA")), (None, None));
    }
}
