//! COMP-5: the app's copy of the component registry (COMP-3) — what every
//! frame is handed, and what `#/components` lists (COMP-6).
//!
//! **Asked for by what shows it.** Nothing at startup asks `components`
//! (SCALE-26: the session asks only for what the first screen shows). The
//! first view that needs the registry — a framed body ([`crate::frame`]),
//! or the components page — calls [`want`]; from then on it is fetched on
//! every new connection and after every `lagged` ([`refresh`]), and kept in
//! between by `watch`: `component_changed` upserts the whole component,
//! `component_removed` drops it ([`heard`]).
//!
//! **No change is lost to the fetch.** `watch` is subscribed before anything
//! is asked (session.rs), so a `components {}` answer may be older than an
//! event already heard, or newer: the events heard while the fetch is out
//! are kept and replayed onto its answer, in order. An upsert of a whole
//! component, and a removal, give the newest state whichever side of the
//! answer they fell on.
//!
//! **Frames.** When a fetch lands, every open frame is handed the whole
//! registry (`menger:components`, [`crate::frame::broadcast_registry`]); a
//! frame born later finds it in its srcdoc. Each `watch` event reaches the
//! frames as a change notice ([`crate::frame::broadcast_change`]), which
//! carries the component events too — the SDK applies those itself.
//!
//! What decides — applying an event, the snapshot's order — is pure and
//! pinned by native tests; the rest is the session's socket.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use leptos::prelude::*;
use menger_tickets_api::{Component, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::{frame, session};

/// The registry as the app holds it: every component, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Registry {
    by_name: BTreeMap<String, Component>,
}

impl Registry {
    /// A `components {}` answer: one `component` per entry; anything else
    /// (an in-band error) contributes nothing.
    pub(crate) fn from_events(events: impl IntoIterator<Item = TicketsEvent>) -> Self {
        let by_name = events
            .into_iter()
            .filter_map(|ev| match ev {
                TicketsEvent::Component { component } => Some((component.name.clone(), component)),
                _ => None,
            })
            .collect();
        Self { by_name }
    }

    /// Whether `ev` is one the registry takes.
    pub(crate) fn concerns(ev: &TicketsEvent) -> bool {
        matches!(ev, TicketsEvent::ComponentChanged { .. } | TicketsEvent::ComponentRemoved { .. })
    }

    /// Apply a `watch` event; whether the registry moved.
    pub(crate) fn apply(&mut self, ev: &TicketsEvent) -> bool {
        match ev {
            TicketsEvent::ComponentChanged { component } => {
                if self.by_name.get(&component.name) == Some(component) {
                    return false;
                }
                self.by_name.insert(component.name.clone(), component.clone());
                true
            }
            TicketsEvent::ComponentRemoved { name } => self.by_name.remove(name).is_some(),
            _ => false,
        }
    }

    /// Every component, in name order (the wire's).
    pub(crate) fn list(&self) -> Vec<Component> {
        self.by_name.values().cloned().collect()
    }
}

thread_local! {
    /// The registry — `None` until the first fetch lands.
    static REGISTRY: ArcRwSignal<Option<Registry>> = ArcRwSignal::new(None);
    /// Whether some view has asked for it ([`want`]); from then on it is
    /// kept for the app's lifetime.
    static WANTED: Cell<bool> = const { Cell::new(false) };
    /// The newest fetch; an older one's answer is dropped.
    static FETCH: Cell<u32> = const { Cell::new(0) };
    /// While a fetch is out: the component events heard since it went.
    static HEARD: RefCell<Option<Vec<TicketsEvent>>> = const { RefCell::new(None) };
}

/// The registry, as a signal — what `#/components` (COMP-6) draws from.
pub(crate) fn signal() -> ArcRwSignal<Option<Registry>> {
    REGISTRY.with(Clone::clone)
}

/// The registry now, untracked — what a frame's srcdoc is built with.
pub(crate) fn snapshot() -> Option<Vec<Component>> {
    REGISTRY.with(|r| r.with_untracked(|r| r.as_ref().map(Registry::list)))
}

/// A view needs the registry: fetch it now if this is the first ask (and
/// the socket is up — else the session's [`refresh`] will).
pub(crate) fn want() {
    if !WANTED.with(|w| w.replace(true)) {
        fetch();
    }
}

/// A new connection, or a `lagged`: what is held may be stale — ask again,
/// if anything wants it.
pub(crate) fn refresh() {
    if WANTED.with(Cell::get) {
        fetch();
    }
}

/// A `watch` event: a component event moves the registry (and is kept for
/// replay while a fetch is out). Called by the session for every event.
pub(crate) fn heard(ev: &TicketsEvent) {
    if !Registry::concerns(ev) {
        return;
    }
    HEARD.with(|h| {
        if let Some(buf) = h.borrow_mut().as_mut() {
            buf.push(ev.clone());
        }
    });
    REGISTRY.with(|r| {
        r.maybe_update(|reg| reg.as_mut().is_some_and(|reg| reg.apply(ev)));
    });
}

fn fetch() {
    let Some(client) = session::client() else {
        return; // not connected: the session refreshes once it is
    };
    let n = FETCH.with(|f| {
        let n = f.get().wrapping_add(1);
        f.set(n);
        n
    });
    HEARD.with(|h| *h.borrow_mut() = Some(Vec::new()));
    spawn_local(async move {
        let answer = client.components().list_all().await;
        if FETCH.with(Cell::get) != n {
            return; // a newer fetch went out; its answer is the one kept
        }
        let heard = HEARD.with(|h| h.borrow_mut().take()).unwrap_or_default();
        match answer {
            Ok(events) => {
                let mut reg = Registry::from_events(events);
                for ev in &heard {
                    reg.apply(ev);
                }
                REGISTRY.with(|r| r.set(Some(reg)));
                frame::broadcast_registry();
            }
            Err(e) => leptos::logging::log!("components: {}", session::describe(&e)),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(name: &str, source: &str, owner: Option<&str>, references: &[&str]) -> Component {
        Component {
            name: name.into(),
            source: source.into(),
            owner: owner.map(str::to_string),
            references: references.iter().map(|r| r.to_string()).collect(),
            docs: None,
            preview: Vec::new(),
        }
    }

    #[test]
    fn an_answer_is_its_components_in_name_order() {
        let reg = Registry::from_events([
            TicketsEvent::Component { component: component("m-b", "<b>b</b>", Some("A-2"), &[]) },
            TicketsEvent::Component { component: component("m-a", "<i>a</i>", None, &["A-3"]) },
            TicketsEvent::Error { message: "ignored".into() },
        ]);
        let list = reg.list();
        let names: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["m-a", "m-b"]);
        assert_eq!(list[0].owner, None);
    }

    /// `component_changed` is an upsert of the whole component; one that
    /// changes nothing moves nothing; `component_removed` drops it; every
    /// other event is not the registry's.
    #[test]
    fn events_upsert_and_remove_whole_components() {
        let mut reg = Registry::default();
        let v1 = component("m-x", "<b>1</b>", Some("A-2"), &["A-3"]);
        assert!(reg.apply(&TicketsEvent::ComponentChanged { component: v1.clone() }));
        assert!(!reg.apply(&TicketsEvent::ComponentChanged { component: v1.clone() }), "the same component again");
        let orphan = component("m-x", "<b>1</b>", None, &["A-3"]);
        assert!(reg.apply(&TicketsEvent::ComponentChanged { component: orphan.clone() }), "orphaned");
        assert_eq!(reg.list(), vec![orphan]);
        assert!(!reg.apply(&TicketsEvent::Lagged { missed: 1 }));
        assert!(!Registry::concerns(&TicketsEvent::Lagged { missed: 1 }));
        assert!(reg.apply(&TicketsEvent::ComponentRemoved { name: "m-x".into() }));
        assert!(!reg.apply(&TicketsEvent::ComponentRemoved { name: "m-x".into() }), "already gone");
        assert_eq!(reg, Registry::default());
    }

    /// The fetch's rule: the events heard while it was out, replayed in
    /// order onto its answer, give the newest state whether the answer was
    /// taken before them or after.
    #[test]
    fn events_replayed_onto_an_answer_give_the_newest_state_either_way() {
        let v1 = component("m-x", "<b>1</b>", Some("A-2"), &[]);
        let v2 = component("m-x", "<b>2</b>", Some("A-2"), &["A-3"]);
        let heard = [
            TicketsEvent::ComponentChanged { component: v2.clone() },
            TicketsEvent::ComponentRemoved { name: "m-y".into() },
        ];
        let older = [
            TicketsEvent::Component { component: v1 },
            TicketsEvent::Component { component: component("m-y", "y", None, &["A-4"]) },
        ];
        let newer = [TicketsEvent::Component { component: v2.clone() }];
        for answer in [&older[..], &newer[..]] {
            let mut reg = Registry::from_events(answer.to_vec());
            for ev in &heard {
                reg.apply(ev);
            }
            assert_eq!(reg.list(), vec![v2.clone()]);
        }
    }
}
