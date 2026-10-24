//! TICKETS-23: **a task opens in a sheet that is a full ticket body** (the
//! owner, 2026-10-24: "the subtask sheet should support full ticket bodies.
//! the ticket body renderer should be reused completely").
//!
//! An `<m-task id>` (TICKETS-10's block in the body) is a row in its
//! ticket's frame. A click or Enter on the row asks the SDK
//! `menger.task.open(id)`, which posts `menger:task { id }` inside the
//! viewer's gesture; the host (src/frame.rs) takes it only from the ticket
//! page's own body frame and opens this sheet. Nothing of the task comes
//! from the frame: the sheet reads it out of the ticket's **stored body**
//! by its id (`menger_tickets_api::tasks`, the scan the store reads inputs
//! with: raw HTML only, so a fenced example is never a task), live, so an
//! `amended` body re-draws it in place.
//!
//! **The renderer is the ticket page's, whole**: the task's direction goes
//! through [`notes::TicketBody`], the very component the ticket page draws
//! its body with, so the same markdown pass, the same frame (`/app/frame`,
//! its CSP, the SDK), the same `present::Framed` box and host-drawn
//! mockups, `media:` images, chips, inputs and theme repaint. The frame is
//! stamped with the ticket, so an input inside a task answers through that
//! ticket's `tickets.inputs.answer`, its id unique in the ticket as ever.
//!
//! **The sheet**: a bottom sheet on a phone (≤ 820; its handle and head
//! drag it down, src/sheet.rs, padded by the safe-area insets), a side
//! sheet beside the page above. The page behind is held still; Esc, the
//! scrim, × or a swipe down close it; ‹ › step through the ticket's tasks.
//! The route says which is open — `#/ticket/T?task=ID`, rewritten in place
//! (`?rev=`'s way: no history entry, no hashchange) — so a deep link opens
//! it.
//!
//! **One level**: a frame inside a sheet is told so (`task` in its init
//! block, read here off its container, [`sheet_of`]); its `m-task` rows
//! fold in place and open no second sheet.
//!
//! What decides — the route's query, the step — is pure and pinned by
//! native tests; the rest is DOM.

use leptos::prelude::*;
use menger_tickets_api::tasks::{self, Task};
use wasm_bindgen::JsCast;

use crate::{dom, notes, sheet};

// -- the route ------------------------------------------------------------------

/// Whether `id` may be a task's id in a route: a short slug.
fn is_task_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The task the route's query opens (`task=ID`).
pub(crate) fn task_in(hash: &str) -> Option<String> {
    let (_, q) = hash.split_once('?')?;
    q.split('&').find_map(|p| p.strip_prefix("task=")).filter(|v| is_task_id(v)).map(str::to_string)
}

/// The hash with `task=ID` in its query (`Some`) or out of it, the rest kept.
pub(crate) fn with_task(hash: &str, task: Option<&str>) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<String> = query.split('&').filter(|p| !p.is_empty() && !p.starts_with("task=")).map(str::to_string).collect();
    if let Some(t) = task.filter(|t| is_task_id(t)) {
        params.push(format!("task={t}"));
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

/// The task `by` steps from `at` among `ids` (‹ is -1, › is +1); `None`
/// past either end, or when `at` is not one of them.
pub(crate) fn step(ids: &[String], at: &str, by: i32) -> Option<String> {
    let i = ids.iter().position(|x| x == at)? as i64 + i64::from(by);
    usize::try_from(i).ok().and_then(|i| ids.get(i)).cloned()
}

/// "2 of 5": where `at` is among `ids` (`None`: not among them).
pub(crate) fn place_of(ids: &[String], at: &str) -> Option<String> {
    ids.iter().position(|x| x == at).map(|i| format!("{} of {}", i + 1, ids.len()))
}

thread_local! {
    static OPEN: std::cell::RefCell<Option<ArcRwSignal<Option<String>>>> = const { std::cell::RefCell::new(None) };
    /// The ticket whose page holds the sheet, while one does.
    static PAGE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn hash_now() -> String {
    dom::window().location().hash().unwrap_or_default()
}

/// The task open on the ticket page — the route's, followed. Owned by no
/// view: the sheet that reads it re-renders as it moves.
pub(crate) fn open_task() -> ArcRwSignal<Option<String>> {
    if let Some(s) = OPEN.with(|v| v.borrow().clone()) {
        return s;
    }
    let s = ArcRwSignal::new(task_in(&hash_now()));
    OPEN.with(|v| *v.borrow_mut() = Some(s.clone()));
    let follow = s.clone();
    let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || follow.set(task_in(&hash_now())));
    let _ = dom::window().add_event_listener_with_callback("hashchange", cb.as_ref().unchecked_ref());
    cb.forget();
    s
}

/// Opens task `id` (`None`: closes the sheet) — the query rewritten in place.
pub(crate) fn open(id: Option<&str>) {
    let w = dom::window();
    let hash = hash_now();
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&with_task(&hash, id)));
    }
    open_task().set(id.filter(|t| is_task_id(t)).map(str::to_string));
}

/// `menger:task { id }`, from the ticket page's own body frame (the host's
/// checks are src/frame.rs's), `ticket` the one the host stamped on it —
/// opened only on that ticket's page.
pub(crate) fn open_from_frame(ticket: &str, id: &str) {
    let here = PAGE.with(|p| p.borrow().as_deref().is_some_and(|p| p.eq_ignore_ascii_case(ticket)));
    if here && is_task_id(id.trim()) {
        open(Some(id.trim()));
    }
}

/// Esc: closes an open sheet (and says it did).
pub(crate) fn escape() -> bool {
    let up = OPEN.with(|v| v.borrow().as_ref().is_some_and(|s| s.get_untracked().is_some()));
    if up {
        open(None);
    }
    up
}

/// The task whose sheet holds `container` (a frame's), if one does.
pub(crate) fn sheet_of(container: &web_sys::Element) -> Option<String> {
    container.closest("[data-task-sheet]").ok().flatten().and_then(|s| s.get_attribute("data-task-sheet"))
}

// -- the sheet --------------------------------------------------------------------

/// What the sheet shows of the open task: the body not here yet, the task,
/// or no such task in the body.
#[derive(Clone, PartialEq)]
enum Seen {
    Loading,
    Found(Task),
    Missing,
}

/// TICKETS-23: the ticket page's task sheet — over the page, outside
/// `#ticket-page` (so the section dock never reads its frame as the
/// page's). `body` is the page's own body signal (the stored body, live);
/// `refs` places its `{wave: ID}`s as the page does.
#[component]
pub(crate) fn TaskSheet(ticket: String, body: RwSignal<Option<String>>, refs: notes::Refs) -> impl IntoView {
    PAGE.with(|p| *p.borrow_mut() = Some(ticket.clone()));
    let mine = ticket.clone();
    on_cleanup(move || PAGE.with(|p| {
        let mut p = p.borrow_mut();
        if p.as_deref() == Some(mine.as_str()) {
            *p = None;
        }
    }));
    let open_id = open_task();
    let id = {
        let o = open_id.clone();
        Memo::new(move |_| o.get())
    };
    move || {
        id.get().map(|task| {
            let ticket = ticket.clone();
            view! { <Sheet ticket task body refs /> }
        })
    }
}

#[component]
fn Sheet(ticket: String, task: String, body: RwSignal<Option<String>>, refs: notes::Refs) -> impl IntoView {
    let seen = {
        let task = task.clone();
        Memo::new(move |_| {
            body.with(|b| match b.as_deref() {
                None => Seen::Loading,
                Some(b) => tasks::find(b, &task).map_or(Seen::Missing, Seen::Found),
            })
        })
    };
    let ids = Memo::new(move |_| body.with(|b| b.as_deref().map(|b| tasks::read(b).into_iter().map(|t| t.id).collect::<Vec<_>>()).unwrap_or_default()));
    // The ticket page's renderer: the direction as the body it is.
    let direction = Signal::derive(move || {
        seen.with(|s| match s {
            Seen::Loading => None,
            Seen::Found(t) => Some(t.direction.clone()),
            Seen::Missing => Some(String::new()),
        })
    });
    // The page behind held still, at every width (src/sheet.rs counts it).
    sheet::hold_page(true);
    on_cleanup(|| sheet::hold_page(false));
    let close = Callback::new(|_: ()| open(None));
    let at = task.clone();
    let go = move |by: i32| {
        if let Some(next) = ids.with_untracked(|ids| step(ids, &at, by)) {
            open(Some(&next));
        }
    };
    let (go_prev, go_next) = (go.clone(), go);
    let place = {
        let task = task.clone();
        move || ids.with(|ids| place_of(ids, &task)).unwrap_or_default()
    };
    let (t1, t2) = (task.clone(), task.clone());
    let at_first = move || ids.with(|ids| step(ids, &t1, -1).is_none());
    let at_last = move || ids.with(|ids| step(ids, &t2, 1).is_none());
    let status = move || seen.with(|s| match s {
        Seen::Found(t) => t.status.clone().unwrap_or_else(|| "Draft".into()),
        _ => String::new(),
    });
    let name = move || seen.with(|s| match s {
        Seen::Found(t) => t.name.clone().unwrap_or_default(),
        Seen::Missing => "No such task in this ticket".into(),
        Seen::Loading => "loading…".into(),
    });
    let params = move || {
        seen.with(|s| match s {
            Seen::Found(t) => t
                .params
                .iter()
                .map(|(k, v)| view! { <span class="tsp"><b>{k.clone()}</b>{v.clone()}</span> })
                .collect_view()
                .into_any(),
            _ => ().into_any(),
        })
    };
    let missing = move || seen.with(|s| matches!(s, Seen::Missing));
    let label = format!("Task {task}");
    let shown = task.clone();
    let close_btn = NodeRef::<leptos::html::Button>::new();
    // Focus moves into the sheet, whose own Esc closes it: the app's key
    // map defers to a dialog (keys::defer_to_focus). Annotation mode and
    // full screen first, as on the page.
    let on_key = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" && !ev.default_prevented() {
            ev.prevent_default();
            ev.stop_propagation();
            if !crate::annotate::escape() && !crate::present::escape() {
                close.run(());
            }
        }
    };
    Effect::new(move |_| {
        if let Some(b) = close_btn.get() {
            let _ = b.focus();
        }
    });
    view! {
        <div class="tsheet-scrim" on:click=move |_| close.run(())></div>
        <section id="tsheet" class="tsheet" role="dialog" aria-modal="true" aria-label=label data-task-sheet=task.clone() on:keydown=on_key>
            <sheet::Grip target="tsheet" head=".tshead" class="tgrab" open=Signal::derive(|| true) dismiss=close />
            <header class="tshead">
                <code class="tsid">{shown}</code>
                {move || {
                    let s = status();
                    (!s.is_empty()).then(|| view! { <span class=crate::st_class(&s)>{s.clone()}</span> })
                }}
                <span class="tsnav">
                    <button class="tsstep" aria-label="Previous task" title="Previous task" disabled=at_first on:click=move |_| go_prev(-1)>"‹"</button>
                    <span class="tsplace">{place}</span>
                    <button class="tsstep" aria-label="Next task" title="Next task" disabled=at_last on:click=move |_| go_next(1)>"›"</button>
                </span>
                <button class="tsx" node_ref=close_btn aria-label="Close the task" title="Close (Esc)" on:click=move |_| close.run(())>"×"</button>
            </header>
            <div class="tsscroll">
                <h3 class="tstitle" class:missing=missing>{name}</h3>
                <div class="tsparams">{params}</div>
                <div class="tsbody">
                    <notes::TicketBody body=direction refs=Some(refs) ticket=ticket />
                </div>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_says_which_task_is_open() {
        assert_eq!(task_in("#/ticket/KILN-7?task=sheet-lock").as_deref(), Some("sheet-lock"));
        assert_eq!(task_in("#/ticket/KILN-7?view=full&task=a_1.b").as_deref(), Some("a_1.b"));
        assert_eq!(task_in("#/ticket/KILN-7"), None);
        assert_eq!(task_in("#/ticket/KILN-7?task="), None);
        assert_eq!(task_in("#/ticket/KILN-7?task=<x>"), None, "a slug, nothing else");
        assert_eq!(with_task("#/ticket/KILN-7", Some("a")), "#/ticket/KILN-7?task=a");
        assert_eq!(with_task("#/ticket/KILN-7?view=full&task=a", Some("b")), "#/ticket/KILN-7?view=full&task=b");
        assert_eq!(with_task("#/ticket/KILN-7?task=a", None), "#/ticket/KILN-7");
        assert_eq!(with_task("#/ticket/KILN-7?rev=3&task=a", None), "#/ticket/KILN-7?rev=3");
    }

    #[test]
    fn the_arrows_step_through_the_tickets_tasks() {
        let ids: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        assert_eq!(step(&ids, "b", 1).as_deref(), Some("c"));
        assert_eq!(step(&ids, "b", -1).as_deref(), Some("a"));
        assert_eq!(step(&ids, "a", -1), None);
        assert_eq!(step(&ids, "c", 1), None);
        assert_eq!(step(&ids, "z", 1), None);
        assert_eq!(place_of(&ids, "b").as_deref(), Some("2 of 3"));
        assert_eq!(place_of(&ids, "z"), None);
    }
}
