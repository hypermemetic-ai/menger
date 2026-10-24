//! ORG-13 / ORG-23: the epics of the reader's organization, the nav head's
//! picker between them, and the notice of a move the reader did not make.
//!
//! - **What is held**: `epics.list` on every connection ([`refresh`]) —
//!   the organization's live epics, each saying whether it is the reader's
//!   current one, whether it is the default (where a principal who never
//!   chose reads), and how many people are on it — and `epics.current`,
//!   whose `moved` is a move an archive made for the reader that they have
//!   not dismissed yet ([`notice`]).
//! - **The picker** ([`EpicPicker`]) is always in the nav's head (ORG-23:
//!   one epic is still an epic — the reader sees where they are). Its
//!   button names the current epic, *yours*; it opens a list of every live
//!   epic (✓ the current, *· default* the default, each with its tickets),
//!   **+ new epic…** for an admin (inline: a name, *create*) and **manage
//!   epics →** (`#/epics`). On a phone the list is a sheet from the bottom
//!   edge. Choosing one calls `epics.switch` — the reader's own setting,
//!   kept by the server, so only they move — and the page is read again in
//!   it; a toast says so after the reload ([`say_after_reload`]). Creating
//!   one moves nobody, and the toast says that too.
//! - **The notice** ([`EpicNotice`]): over every page while `current` says
//!   `moved` — "spring was archived by ana. You're on fresh now." with
//!   **open spring (archived)** (the board under `epic spring`), **pick
//!   another epic** (the picker) and **got it** (`epics.dismiss`: it is
//!   shown once, on every device, until then).
//! - **Live** ([`start_watch`]): `epics.watch` — another admin's create,
//!   rename, archive, restore or default move reads the list again; a move
//!   of the reader (`moved`) reads the page again in the new epic, where
//!   the notice waits.
//!
//! What decides — which epic is current, its line, the notice's words, a
//! namesake's key — is pure here, pinned by native tests.

use std::cell::RefCell;

use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::{Epic, EpicArchived, EpicMove, EpicsEvent};
use plexus_client::ClientEvent;

thread_local! {
    /// The organization's live epics — empty until the first answer.
    static HELD: ArcRwSignal<Vec<Epic>> = ArcRwSignal::new(Vec::new());
    /// ORG-23: a move the reader has not dismissed, from `current`.
    static NOTICE: ArcRwSignal<Option<EpicMove>> = ArcRwSignal::new(None);
    /// The picker whose list is open — `nav` or `phone` (a notice's *pick
    /// another epic* opens the one on screen).
    static OPEN: ArcRwSignal<Option<&'static str>> = ArcRwSignal::new(None);
    /// ORG-23: the tickets opened that live in an archived epic, and its mark.
    static ARCHIVED: ArcRwSignal<std::collections::HashMap<String, EpicArchived>> = ArcRwSignal::new(Default::default());
    /// The open `epics.watch`, aborted when the next session starts one.
    static PUMP: RefCell<Option<futures::future::AbortHandle>> = const { RefCell::new(None) };
}

/// The epics held, as a signal.
pub(crate) fn held() -> ArcRwSignal<Vec<Epic>> {
    HELD.with(Clone::clone)
}

/// The move waiting to be said, as a signal.
pub(crate) fn notice() -> ArcRwSignal<Option<EpicMove>> {
    NOTICE.with(Clone::clone)
}

fn open() -> ArcRwSignal<Option<&'static str>> {
    OPEN.with(Clone::clone)
}

/// The picker on screen: the phone head's at a phone's width, else the nav's.
fn on_screen() -> &'static str {
    let w = web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(1440.0);
    if crate::phonebar::is_phone(w) {
        "phone"
    } else {
        "nav"
    }
}

/// Asks `epics.list` and `epics.current` on the session's connection (every
/// connect), and opens `epics.watch`.
pub(crate) fn refresh() {
    start_watch();
    read();
    say_waiting();
}

/// The list and the current epic, read again (after a change from a page).
pub(crate) fn refresh_list() {
    read();
}

/// The list and the current epic, read again.
fn read() {
    let Some(client) = crate::session::epics_client() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(events) = client.list_all(None, None).await {
            held().set(epics_of(events));
        }
        if let Ok(events) = client.current_all(None).await {
            for ev in events {
                if let EpicsEvent::Current { moved, .. } = ev {
                    notice().set(moved);
                }
            }
        }
    });
}

/// `epics.watch` for this connection: a change to the epics reads the list
/// again; a move of the reader reads the page again (in the new epic).
fn start_watch() {
    let Some(client) = crate::session::epics_client() else { return };
    let (abort, reg) = futures::future::AbortHandle::new_pair();
    if let Some(old) = PUMP.with(|p| p.borrow_mut().replace(abort)) {
        old.abort();
    }
    let pump = async move {
        let mut stream = client.watch();
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(ClientEvent::Data(ev)) => match heard(&ev) {
                    Heard::Read => read(),
                    Heard::Reload => reload(),
                    Heard::Nothing => {}
                },
                Ok(_) => {}
                Err(_) => break, // the socket: the next session starts it again
            }
        }
    };
    wasm_bindgen_futures::spawn_local(async move {
        let _ = futures::future::Abortable::new(pump, reg).await;
    });
}

/// What a watch event asks of the app.
#[derive(Debug, PartialEq, Eq)]
enum Heard {
    Read,
    Reload,
    Nothing,
}

fn heard(ev: &EpicsEvent) -> Heard {
    match ev {
        EpicsEvent::Moved { .. } => Heard::Reload,
        EpicsEvent::EpicChanged { .. } | EpicsEvent::Lagged { .. } | EpicsEvent::Dismissed { .. } => Heard::Read,
        _ => Heard::Nothing,
    }
}

/// The epics a `list` answered.
fn epics_of(events: Vec<EpicsEvent>) -> Vec<Epic> {
    events
        .into_iter()
        .filter_map(|e| match e {
            EpicsEvent::Epic { epic } => Some(epic),
            _ => None,
        })
        .collect()
}

/// The reader's current epic among `epics` — the one the server says, else
/// the default, else the newest; `None` only before the first answer.
pub(crate) fn current_of(epics: &[Epic]) -> Option<String> {
    epics
        .iter()
        .find(|e| e.current)
        .or_else(|| epics.iter().find(|e| e.default))
        .or_else(|| epics.last())
        .map(|e| e.name.to_string())
}

/// A picker line's words: the name, `· default` on the default.
pub(crate) fn line_of(e: &Epic) -> (String, Option<&'static str>, u32) {
    (e.name.to_string(), e.default.then_some("· default"), e.tickets)
}

/// The toast after a switch.
pub(crate) fn switched_words(to: &str) -> String {
    format!("You are on {to} now. Only you moved: everyone else stays on their own epic.")
}

/// The toast after a create.
pub(crate) fn created_words(name: &str, on: &str) -> String {
    format!("{name} created. Nobody moved: you are still on {on}.")
}

/// The notice's words: who archived what, and where the reader is now.
pub(crate) fn notice_words(m: &EpicMove, name: impl Fn(&str) -> String) -> (String, String, String) {
    (m.from.to_string(), name(&m.by), m.to.to_string())
}

/// The board under an archived epic: `epic <name>` reaches it.
pub(crate) fn old_link(from: &str) -> String {
    format!("#/?f={}", crate::lens::enc(&format!("epic {from}")))
}

/// ORG-13: a ticket's key as a row shows it — a namesake's (`MENGER.3-1`,
/// a shortcode born again in a new epic, or another host's) as its letters
/// and its number, `MENGER-1`: the address says which. Any other key as it is.
pub(crate) fn shown_id(id: &str) -> String {
    match id.rsplit_once('-') {
        Some((code, n)) => match code.split_once('.') {
            Some((letters, k)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => format!("{letters}-{n}"),
            _ => id.to_string(),
        },
        None => id.to_string(),
    }
}

/// ORG-23: a shortcode's identity as a reader sees it — a namesake's
/// (`MENGER.3`) as its letters.
pub(crate) fn shown_code(code: &str) -> String {
    match code.split_once('.') {
        Some((letters, k)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => letters.to_string(),
        _ => code.to_string(),
    }
}

/// ORG-23: a key as a reader in another epic sees it — `spring:MENGER-12`.
pub(crate) fn shown_in(epic: &str, id: &str) -> String {
    format!("{epic}:{}", shown_id(id))
}

const SAY: &str = "menger.epics.say";

/// A toast to show once the page has been read again (a switch reloads).
fn say_after_reload(text: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) {
        let _ = s.set_item(SAY, text);
    }
}

/// The toast a reload carried, shown now.
fn say_waiting() {
    let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) else { return };
    if let Ok(Some(text)) = s.get_item(SAY) {
        let _ = s.remove_item(SAY);
        crate::menu::toast(text);
    }
}

fn reload() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// `epics.switch` to `name`, then the page read again in it.
pub(crate) fn switch(name: String) {
    let Some(client) = crate::session::epics_client() else { return };
    let Ok(typed) = name.parse() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(evs) = client.switch_all(typed, None).await {
            if let Some(m) = refused(&evs) {
                crate::menu::toast(m);
                return;
            }
            say_after_reload(&switched_words(&name));
            reload();
        }
    });
}

/// The server's words when an epics call refused.
pub(crate) fn refused(evs: &[EpicsEvent]) -> Option<String> {
    evs.iter().find_map(|e| match e {
        EpicsEvent::Error { message } => Some(message.clone()),
        EpicsEvent::NotFound { name } => Some(format!("no epic `{name}`")),
        _ => None,
    })
}

/// `epics.create`; the toast says nobody moved.
pub(crate) fn create(name: String, then: impl FnOnce() + 'static) {
    let Some(client) = crate::session::epics_client() else { return };
    let typed = match crate::typed::typed(&name) {
        Ok(t) => t,
        Err(e) => return crate::menu::toast(e),
    };
    wasm_bindgen_futures::spawn_local(async move {
        match client.create_all(typed, None).await {
            Ok(evs) => match refused(&evs) {
                Some(m) => crate::menu::toast(m),
                None => {
                    let on = held().with_untracked(|e| current_of(e)).unwrap_or_default();
                    crate::menu::toast(created_words(&name, &on));
                    read();
                    then();
                }
            },
            Err(e) => crate::menu::toast(crate::session::describe(&e)),
        }
    });
}

/// ORG-23: the epic picker — always shown: in the nav's head, and
/// (`phone`) in the phone's head line, where it is one button, `epic NAME ▾`
/// whose list is a sheet from the bottom edge ([`EpicSheet`]).
#[component]
pub(crate) fn EpicPicker(#[prop(optional)] phone: bool) -> impl IntoView {
    let me: &'static str = if phone { "phone" } else { "nav" };
    let epics = held();
    let open = open();
    let now = move || epics.with(|e| current_of(e));
    let toggle = {
        let open = open.clone();
        move |ev: web_sys::MouseEvent| {
            ev.stop_propagation();
            open.update(|o| *o = if *o == Some(me) { None } else { Some(me) });
        }
    };
    let expanded = {
        let open = open.clone();
        move || (open.get() == Some(me)).to_string()
    };
    let list = move || (!phone && open.get() == Some(me)).then(list_view);
    view! {
        <div class=if phone { "nepic pepic" } else { "nepic" } title="the epic the board, the nav and every filtered read answer from — yours, kept by the server">
            {(!phone).then(|| view! { <span class="nepic-k">"epic"</span> })}
            <button class="epick" aria-haspopup="true" aria-expanded=expanded on:click=toggle>
                {phone.then(|| view! { <span class="nepic-k">"epic"</span> })}
                <b class="epick-n">{now}</b>
                {(!phone).then(|| view! { <span class="you">"yours"</span> })}
                <span class="caret" aria-hidden="true">"▾"</span>
            </button>
            {list}
        </div>
    }
}

/// ORG-23: the phone's picker list — a sheet over the page and the bar,
/// mounted once by the App (the head line is its own stacking context).
#[component]
pub(crate) fn EpicSheet() -> impl IntoView {
    let open = open();
    move || (open.get() == Some("phone")).then(|| view! { <div class="epic-sheet">{list_view()}</div> })
}

/// The picker's list: every live epic, + new epic… (an admin), manage epics →.
fn list_view() -> impl IntoView {
    let naming = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let admin = move || crate::signin::account().with(|a| a.is_admin());
    let close = move || {
        open().set(None);
        naming.set(false);
    };
    let options = move || {
        held().with(|all| {
            all.iter()
                .map(|e| {
                    let (n, dflt, tickets) = line_of(e);
                    let cur = e.current;
                    let pick = n.clone();
                    view! {
                        <button class="eopt" class:cur=cur role="menuitemradio" aria-checked=cur.to_string() data-epic=n.clone()
                            on:click=move |_| {
                                close();
                                if !cur {
                                    switch(pick.clone());
                                }
                            }>
                            <span class="etick">{cur.then_some("✓")}</span>
                            <span><span class="en">{n.clone()}</span>{dflt.map(|d| view! { " " <span class="ec">{d}</span> })}</span>
                            <span class="ec">{tickets}</span>
                        </button>
                    }
                })
                .collect_view()
        })
    };
    let go = move || {
        let n = name.get_untracked().trim().to_string();
        if n.is_empty() {
            return;
        }
        create(n, move || {
            name.set(String::new());
            naming.set(false);
        });
    };
    view! {
        <div class="epop-scrim" on:click=move |_| close()></div>
        <div class="epop" role="menu" aria-label="epics" on:click=|ev| ev.stop_propagation()
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { close() }>
            <div class="ehint">"your epic — only you move"</div>
            {options}
            <div class="esep"></div>
            {move || naming.get().then(|| view! {
                <div class="enew">
                    <input placeholder="name" aria-label="new epic name" prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev))
                        on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { go() } />
                    <button class="primary" on:click=move |_| go()>"create"</button>
                </div>
            })}
            {move || (admin() && !naming.get()).then(|| view! {
                <button class="eact" data-act="new" on:click=move |_| naming.set(true)>"+ new epic…"</button>
            })}
            <a class="eact" href="#/epics" on:click=move |_| close()>"manage epics →"</a>
        </div>
    }
}

/// ORG-23: what `get` said of a ticket's epic — archived, or not.
pub(crate) fn note_archived(id: &str, mark: Option<EpicArchived>) {
    let a = ARCHIVED.with(Clone::clone);
    let had = a.with_untracked(|m| m.get(id).cloned());
    if had != mark {
        a.update(|m| {
            match mark {
                Some(x) => m.insert(id.to_string(), x),
                None => m.remove(id),
            };
        });
    }
}

/// ORG-23: a ticket page's line for a ticket in an archived epic — reached
/// by its link, still: "in spring (archived 2026-10-30)".
#[component]
pub(crate) fn ArchivedMark(id: String) -> impl IntoView {
    let a = ARCHIVED.with(Clone::clone);
    move || {
        a.with(|m| m.get(&id).cloned()).map(|x| {
            let name = x.name.to_string();
            view! {
                <p class="earchived" role="note">
                    "in the archived epic "<a class="mono" href=old_link(&name)>{name.clone()}</a>
                    " — it is in no view; links and "<span class="mono">{format!("epic {name}")}</span>" still reach it."
                </p>
            }
        })
    }
}

/// ORG-23: the notice of a move an archive made — over every page until
/// *got it*.
#[component]
pub(crate) fn EpicNotice() -> impl IntoView {
    let n = notice();
    move || {
        n.get().map(|m| {
            let (from, by, to) = notice_words(&m, crate::chat_pane::names());
            let old = old_link(&from);
            view! {
                <div id="epic-notice" role="status">
                    <span class="enote-k" aria-hidden="true">"↪"</span>
                    <div class="enote-t">
                        <b class="mono">{from.clone()}</b>" was archived by "{by}". You're on "<b class="mono">{to}</b>" now."
                        <div class="enote-acts">
                            <a class="lnk" href=old>{format!("open {from} (archived)")}</a>
                            <button class="lnk" on:click=move |ev| {
                                ev.stop_propagation();
                                open().set(Some(on_screen()));
                            }>"pick another epic"</button>
                        </div>
                    </div>
                    <button class="enote-ok" on:click=move |_| dismiss()>"got it"</button>
                </div>
            }
        })
    }
}

/// `epics.dismiss`: the notice is gone, on every device.
fn dismiss() {
    notice().set(None);
    let Some(client) = crate::session::epics_client() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        let _ = client.dismiss_all(None).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{EpicName, OrgId};

    fn epic(name: &str, current: bool) -> Epic {
        Epic {
            org: OrgId::hypermemetic(),
            name: EpicName::new(name).unwrap(),
            first: name == "genesis",
            current,
            shortcodes: 0,
            tickets: 0,
            created_unix: 0,
            default: name == "genesis",
            members_on: 0,
            archived: None,
        }
    }

    #[test]
    fn the_picker_always_names_the_current() {
        assert_eq!(current_of(&[epic("genesis", true)]).as_deref(), Some("genesis"), "one epic is still shown");
        assert_eq!(current_of(&[epic("genesis", false), epic("fresh", true)]).as_deref(), Some("fresh"));
        assert_eq!(current_of(&[epic("genesis", true), epic("fresh", false)]).as_deref(), Some("genesis"));
        assert_eq!(current_of(&[epic("genesis", false), epic("fresh", false)]).as_deref(), Some("genesis"), "none said: the default");
        assert_eq!(current_of(&[]), None);
    }

    #[test]
    fn the_notice_names_who_and_where() {
        let m = EpicMove { from: EpicName::new("spring").unwrap(), to: EpicName::new("fresh").unwrap(), by: "key:ab".into(), at_unix: 0, reason: "archived".into() };
        assert_eq!(notice_words(&m, |_| "ana".into()), ("spring".into(), "ana".into(), "fresh".into()));
    }

    #[test]
    fn a_line_marks_the_default() {
        assert_eq!(line_of(&epic("genesis", false)).1, Some("· default"));
        assert_eq!(line_of(&epic("fresh", false)).1, None);
    }

    #[test]
    fn the_toasts_say_who_moved() {
        assert!(switched_words("fresh").starts_with("You are on fresh now. Only you moved"));
        assert_eq!(created_words("summer", "fresh"), "summer created. Nobody moved: you are still on fresh.");
    }

    #[test]
    fn a_move_of_the_reader_reads_the_page_again_and_a_change_the_list() {
        let m = EpicMove { from: EpicName::new("spring").unwrap(), to: EpicName::new("fresh").unwrap(), by: "key:ab".into(), at_unix: 0, reason: "archived".into() };
        assert_eq!(heard(&EpicsEvent::Moved { org: OrgId::hypermemetic(), moved: m }), Heard::Reload);
        assert_eq!(heard(&EpicsEvent::Lagged { missed: 3 }), Heard::Read);
        assert_eq!(heard(&EpicsEvent::Error { message: "x".into() }), Heard::Nothing);
    }

    #[test]
    fn a_namesakes_key_shows_as_its_letters_and_number() {
        assert_eq!(shown_id("MENGER.3-1"), "MENGER-1");
        assert_eq!(shown_id("SYNAPSE-CC.4-12"), "SYNAPSE-CC-12");
        assert_eq!(shown_id("MENGER-7"), "MENGER-7");
        assert_eq!(shown_id("SYNAPSE-CC-2"), "SYNAPSE-CC-2");
        assert_eq!(shown_in("spring", "MENGER.3-12"), "spring:MENGER-12");
        assert_eq!((shown_code("MENGER.3"), shown_code("MENGER"), shown_code("SYNAPSE-CC.4")), ("MENGER".into(), "MENGER".into(), "SYNAPSE-CC".into()));
    }

    #[test]
    fn a_list_is_its_epics() {
        let evs = vec![EpicsEvent::Epic { epic: epic("genesis", false) }, EpicsEvent::Error { message: "x".into() }];
        assert_eq!(epics_of(evs).len(), 1);
    }
}
