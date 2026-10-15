//! WAVE-11: the board annotation — a markdown document below the wave
//! board that discusses its waves (WAVE-1 decision 8: "the plexus board
//! itself gets the annotation … a viewer that can show up below the
//! board"), written and read like a ticket body.
//!
//! - **Where it lives**: the board's root shortcode keeps it at the reserved
//!   path `_MENGER/board` (WAVE-10) — a doc under the shortcode's scope root,
//!   hidden from the nav — so `#/waves/PLEXUS` reads `get { id:
//!   "PLEXUS/_MENGER/board" }`. None yet is an empty panel that says "write
//!   notes for this board"; the first save creates it (`create { epic, kind:
//!   doc, path, title: "board: ROOT", body }`), every later one amends it.
//! - **What it shows**: the body through the ticket page's renderer
//!   (`md.rs`), and each `{wave: ID}` in it as that ticket's current ordinal
//!   ([`Refs`]): from the board's cards when the ticket is one (its link
//!   scrolls to the card's column), else from the ticket's row — its kept
//!   placement (WAVE-6), fetched and held while the panel is open. Both move
//!   live: the cards with `placements_changed`, a row with the same event.
//! - **Editing** ([`BodyEditor`]): a markdown textarea beside a live
//!   preview through the same renderer; save is `amend { id, body }` (or the
//!   first save's `create`), cancel discards. An `amended` from elsewhere
//!   updates the panel — or, mid-edit, says "changed elsewhere — reload?".
//!   The editor takes an id and a body signal and nothing of the board's,
//!   so the ticket page can adopt it.
//!
//! - **Framed** (COMP-4): a note that holds HTML renders in a sandboxed
//!   frame below the board ([`BodyView`], src/frame.rs), and so does the
//!   editor's preview of one. A wave reference in a framed note is a link
//!   that opens the ticket in a new tab: the board cannot see a click inside
//!   the frame, so it does not scroll to the card.
//!
//! What decides — the address, how a reference is placed, create or amend —
//! is pure and pinned by native tests.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::{Placement, Ticket, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::md::{self, WaveRef};
use crate::{cache, dom, mermaid, present, session};

/// Where a board keeps its annotation inside its root shortcode (WAVE-1
/// decision 11, WAVE-10's reserved prefix).
pub(crate) const BOARD_PATH: &str = "_MENGER/board";

/// The annotation's path address for the board of `root`.
pub(crate) fn board_address(root: &str) -> String {
    format!("{}/{BOARD_PATH}", root.to_ascii_uppercase())
}

/// How a kept placement reads as a wave reference.
pub(crate) fn wave_ref(p: Placement) -> WaveRef {
    match p {
        Placement::Wave { n } => WaveRef::Wave(n),
        Placement::Done => WaveRef::Done,
        Placement::MissingBlocker => WaveRef::Missing,
    }
}

/// A `{wave: ID}` placed: from the board's cards when the ticket is one of
/// them, else from its row's kept placement (`row`: `None` when no row is
/// held — not fetched yet, or no such ticket; `Some(None)` for a row with no
/// placement, a doc). What cannot be placed is left as written.
pub(crate) fn resolve_ref(id: &str, cards: Option<&HashMap<String, Placement>>, row: Option<Option<Placement>>) -> Option<WaveRef> {
    if let Some(p) = cards.and_then(|c| c.get(id)) {
        return Some(wave_ref(*p));
    }
    row.flatten().map(wave_ref)
}

/// What a first save creates: the board's annotation in its root shortcode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Create {
    pub(crate) epic: String,
    pub(crate) title: String,
    pub(crate) path: String,
}

impl Create {
    pub(crate) fn board(root: &str) -> Self {
        let root = root.to_ascii_uppercase();
        Self {
            title: format!("board: {root}"),
            epic: root,
            path: BOARD_PATH.to_string(),
        }
    }
}

/// What a save does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Save {
    Amend { id: String, body: String },
    Create { create: Create, body: String },
}

/// Create or amend: the document that exists (`id`) is amended; with none,
/// it is created (`create`) — or, with nothing to create, there is nothing
/// to save to.
pub(crate) fn save_action(id: Option<&str>, create: Option<&Create>, body: String) -> Option<Save> {
    match (id, create) {
        (Some(id), _) => Some(Save::Amend { id: id.to_string(), body }),
        (None, Some(c)) => Some(Save::Create { create: c.clone(), body }),
        (None, None) => None,
    }
}

/// How a view places its `{wave: ID}`s: the board's cards (none off the
/// board), and the rows of the tickets named that are not cards — fetched
/// once each and held while the view is.
#[derive(Clone, Copy)]
pub(crate) struct Refs {
    cards: Option<Memo<HashMap<String, Placement>>>,
    holder: StoredValue<cache::Holder>,
    asked: StoredValue<HashSet<String>>,
}

impl Refs {
    pub(crate) fn new(cards: Option<Memo<HashMap<String, Placement>>>, holder: StoredValue<cache::Holder>) -> Self {
        Self {
            cards,
            holder,
            asked: StoredValue::new(HashSet::new()),
        }
    }

    /// `id` placed — tracked: a card moving, a row arriving or its placement
    /// moving renders again.
    pub(crate) fn resolve(&self, id: &str) -> Option<WaveRef> {
        if let Some(r) = self.cards.and_then(|c| c.with(|c| resolve_ref(id, Some(c), None))) {
            return Some(r);
        }
        resolve_ref(id, None, cache::find(id).map(|row| row.with(|t| t.placement)))
    }

    /// Fetch the rows of `ids` that are neither cards nor held nor asked for
    /// already.
    pub(crate) fn want(&self, ids: &[String]) {
        let holder = self.holder.get_value();
        let on_board = |id: &str| self.cards.is_some_and(|c| c.with_untracked(|c| c.contains_key(id)));
        let ask: Vec<String> = ids
            .iter()
            .filter(|id| !on_board(id) && !holder.holds(id) && !self.asked.with_value(|a| a.contains(*id)) && !holder.hold_id(id))
            .cloned()
            .collect();
        if !ask.is_empty() {
            self.asked.update_value(|a| a.extend(ask.iter().cloned()));
            session::load_rows(ask, holder);
        }
    }

    /// A new connection or a lag: what was fetched may be stale — ask again.
    pub(crate) fn forget(&self) {
        self.asked.update_value(HashSet::clear);
    }
}

/// A body as the ticket page draws one — its frontmatter folded, the rest
/// through `md.rs` (a diagram handed to the mermaid renderer once mounted) —
/// with its `{wave: ID}`s placed by `refs` (WAVE-11; a body with none
/// renders as it always did).
pub(crate) fn body_view(raw: &str, refs: Option<Refs>) -> AnyView {
    let (fm, md_src) = md::split_frontmatter(raw);
    let fm_view = fm.map(|f| {
        let f = f.to_string();
        view! {
            <details class="frontmatter">
                <summary>"frontmatter"</summary>
                <pre>{f}</pre>
            </details>
        }
    });
    let md::Rendered { html, diagrams } = match refs {
        Some(refs) => md::render_body_with(md_src, &|id| refs.resolve(id)),
        None => md::render_body(md_src),
    };
    // AGS-14: once this div is in the DOM, hand its mermaid placeholders to
    // the renderer; a diagram-free body never touches it.
    let md_node = NodeRef::<leptos::html::Div>::new();
    md_node.on_load(move |el: web_sys::HtmlDivElement| {
        if diagrams > 0 {
            mermaid::mount(&el);
        }
        // MENGER-22: and its code highlighted — fetched only if it holds any.
        crate::highlight::mount(&el);
    });
    view! {
        {fm_view}
        <div class="md" node_ref=md_node inner_html=html></div>
    }
    .into_any()
}

/// COMP-4: a body, framed or not — the one renderer the ticket page, the
/// board's notes and the editor's preview share. A body that holds HTML
/// (`md::is_framed`, frontmatter split off first) renders in a sandboxed
/// [`crate::frame::Frame`]: its frontmatter folded outside it as ever, its markup from
/// `md::render_body_framed` with each `{wave: ID}` placed here, by `refs`.
/// While a body stays framed the frame persists, and each change of the
/// body (an `amended`, a keystroke, a reference placed) re-renders inside it
/// without a flash. Any other body is [`body_view`]'s — the markup it always
/// was, rebuilt on each change as it always was. INPUT-4: `ticket` is the
/// ticket whose stored body `body` is — a click in its frame may answer that
/// ticket's inputs; `None` for a draft (the editor's preview), which answers
/// nothing. IDENTITY-36: a frame reads only its ticket's shortcode and what
/// it hosts — or, for a draft, `scope`'s (the ticket being edited, or the
/// shortcode a first save creates in). MENGER-15: a framed body sits in a
/// [`present::Framed`] box — full screen, and its declared view (`view:`,
/// `widths:` in its frontmatter); `title` is the ticket page's, which makes
/// it the box `z`, `?view=full` and `view: full` open.
#[component]
pub(crate) fn BodyView(
    #[prop(into)] body: Signal<String>,
    refs: Option<Refs>,
    ticket: Option<String>,
    #[prop(optional)] scope: Option<String>,
    #[prop(optional)] title: Option<Signal<String>>,
) -> impl IntoView {
    let framed = Memo::new(move |_| body.with(|b| md::is_framed(md::split_frontmatter(b).1)));
    move || {
        if framed.get() {
            framed_view(body, refs, ticket.clone(), scope.clone(), title)
        } else {
            body.with(|b| body_view(b, refs))
        }
    }
}

/// A framed body: the frontmatter fold, then the frame. Reads `body` only
/// inside its own reactive children, so the caller's closure re-runs only
/// when the body stops being framed. MENGER-15: a whole document is the
/// frame's as written — not markdown, nothing placed in it.
fn framed_view(body: Signal<String>, refs: Option<Refs>, ticket: Option<String>, scope: Option<String>, title: Option<Signal<String>>) -> AnyView {
    let fm = Memo::new(move |_| body.with(|b| md::split_frontmatter(b).0.map(str::to_string)));
    let html = Memo::new(move |_| {
        body.with(|b| {
            let src = md::split_frontmatter(b).1;
            if md::is_page(src) {
                return src.to_string();
            }
            match refs {
                Some(refs) => md::render_body_framed(src, &|id| refs.resolve(id)).html,
                None => md::render_body_framed(src, &|_| None).html,
            }
        })
    });
    let decl = Memo::new(move |_| {
        body.with(|b| {
            let (fm, src) = md::split_frontmatter(b);
            present::declared(fm, src)
        })
    });
    let scope = scope.unwrap_or_default();
    let framed = match title {
        Some(title) => view! { <present::Framed html decl ticket scope title /> }.into_any(),
        None => view! { <present::Framed html decl ticket scope /> }.into_any(),
    };
    view! {
        {move || fm.get().map(|f| view! {
            <details class="frontmatter">
                <summary>"frontmatter"</summary>
                <pre>{f}</pre>
            </details>
        })}
        {framed}
    }
    .into_any()
}

/// The first event of a call that answers with a ticket: the ticket, or
/// what the server said instead.
fn ticket_of(events: Vec<TicketsEvent>) -> Result<Ticket, String> {
    for ev in events {
        match ev {
            TicketsEvent::Amended { ticket, .. } | TicketsEvent::Created { ticket } | TicketsEvent::Ticket { ticket } => {
                return Ok(ticket)
            }
            TicketsEvent::NotFound { id } => return Err(format!("{id}: not found")),
            TicketsEvent::Error { message } => return Err(message),
            // MENGER-15: the body past the server's cap — said with the cap.
            TicketsEvent::BodyTooLarge { bytes, max, .. } => return Err(too_large_words(bytes, max)),
            _ => {}
        }
    }
    Err("no answer".into())
}

/// MENGER-15: the body cap's refusal, as the editor says it.
pub(crate) fn too_large_words(bytes: u64, max: u64) -> String {
    let mb = |n: u64| n as f64 / (1024.0 * 1024.0);
    format!("not saved: the body is {:.1} MB ({bytes} bytes), past the server's {:.1} MB cap on a ticket body ({max} bytes)", mb(bytes), mb(max))
}

/// WAVE-11: a body's editor — a markdown textarea beside a live preview
/// through the same renderer. It edits `body`, the body as the server holds
/// it, of the ticket `id`: save is `amend { id, body }`; with no ticket yet,
/// `create` says what the first save creates (the board's annotation), and
/// `id` is the new ticket's after it. Cancel discards. While `editing`, a
/// `body` that moves under the edit (an `amended` from elsewhere) is said —
/// "changed elsewhere — reload?" — and never overwrites the draft.
#[component]
pub(crate) fn BodyEditor(
    id: RwSignal<Option<String>>,
    body: RwSignal<Option<String>>,
    editing: RwSignal<bool>,
    refs: Refs,
    #[prop(optional)] create: Option<Create>,
) -> impl IntoView {
    // What the textarea was given (the body when editing began, or when it
    // was reloaded), and what it holds now. The textarea takes `seed` only:
    // writing what it typed back into it would move the caret.
    let seed = RwSignal::new(body.get_untracked().unwrap_or_default());
    let draft = RwSignal::new(seed.get_untracked());
    let base = RwSignal::new(body.get_untracked());
    let saving = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    // IDENTITY-36: the preview's frame reads as the edited ticket's would —
    // its shortcode (or, before a first save, the one it creates in).
    let draft_scope = id
        .get_untracked()
        .map(|t| crate::bridge::shortcode_of(&t))
        .or_else(|| create.as_ref().map(|c| c.epic.clone()))
        .unwrap_or_default();
    let elsewhere = Memo::new(move |_| {
        editing.get() && !saving.get() && body.with(|b| *b != base.get() && b.as_deref() != Some(draft.get().as_str()))
    });
    let reload = move |_| {
        let b = body.get_untracked();
        seed.set(b.clone().unwrap_or_default());
        draft.set(b.clone().unwrap_or_default());
        base.set(b);
    };
    // The rows the draft's references name, as it is typed.
    Effect::new(move |_| refs.want(&md::wave_ids(&draft.get())));
    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        let Some(action) = save_action(id.get_untracked().as_deref(), create.as_ref(), draft.get_untracked()) else {
            problem.set(Some("there is no document to save to".into()));
            return;
        };
        let Some(client) = session::client() else {
            problem.set(Some("not connected yet".into()));
            return;
        };
        saving.set(true);
        problem.set(None);
        spawn_local(async move {
            let answer = match action {
                Save::Amend { id: tid, body: b } => client.amend_all(tid, None, Some(b)).await,
                Save::Create { create: c, body: b } => {
                    client.create_all(c.epic, c.title, Some("doc".into()), None, None, Some(b), None, Some(c.path)).await
                }
            };
            let answer = answer.map_err(|e| session::describe(&e)).and_then(ticket_of);
            saving.try_set(false);
            match answer {
                Ok(t) => {
                    id.try_set(Some(t.id.clone()));
                    base.try_set(Some(t.body.clone()));
                    body.try_set(Some(t.body));
                    editing.try_set(false);
                }
                Err(m) => {
                    problem.try_set(Some(format!("not saved: {m}")));
                }
            }
        });
    };
    view! {
        <div class="editor">
            {move || elsewhere.get().then(|| view! {
                <p class="notice">"changed elsewhere — " <button class="reload" on:click=reload>"reload?"</button></p>
            })}
            {move || problem.get().map(|p| view! { <p class="problem">{p}</p> })}
            <div class="panes">
                <textarea
                    class="src"
                    spellcheck="false"
                    aria-label="markdown"
                    prop:value=move || seed.get()
                    on:input=move |ev| draft.set(event_target_value(&ev))
                ></textarea>
                <div class="preview"><BodyView body=Signal::derive(move || draft.get()) refs=Some(refs) ticket=None scope=draft_scope.clone() /></div>
            </div>
            <div class="actions">
                <button class="save" on:click=save disabled=move || saving.get()>
                    {move || if saving.get() { "saving…" } else { "save" }}
                </button>
                <button class="cancel" on:click=move |_| editing.set(false)>"cancel"</button>
            </div>
        </div>
    }
}

/// A click on a wave reference on the board: to the card, in its column —
/// or, for a ticket the board does not show, to the wave's column — instead
/// of the ticket's page. Anywhere else the link is followed.
fn scroll_to_ref(ev: web_sys::MouseEvent) {
    let Some(a) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.closest("a.wave-ref").ok().flatten())
    else {
        return;
    };
    let id = a.get_attribute("data-wave-ref").unwrap_or_default();
    let n = a.get_attribute("data-wave").unwrap_or_default();
    if reveal_on_board(&id, &n) {
        ev.prevent_default();
    }
}

/// The card of the ticket `id` on the board on screen — or, for one it does
/// not show, its wave `n`'s column — scrolled into view and flashed; whether
/// there was one. COMP-9: also what a `{wave: ID}` clicked inside a framed
/// board note does (src/frame.rs, `menger:navigate` with `reveal`).
pub(crate) fn reveal_on_board(id: &str, n: &str) -> bool {
    let ok = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok(id) || !ok(n) {
        return false;
    }
    let Some(doc) = dom::window().document() else { return false };
    let target = doc
        .query_selector(&format!("#waves .card[data-ticket-id=\"{id}\"]"))
        .ok()
        .flatten()
        .or_else(|| doc.query_selector(&format!("#waves .lane[data-lane=\"wave-{n}\"]")).ok().flatten());
    let Some(el) = target else { return false };
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(web_sys::ScrollLogicalPosition::Center);
    opts.set_inline(web_sys::ScrollLogicalPosition::Center);
    el.scroll_into_view_with_scroll_into_view_options(&opts);
    let _ = el.class_list().add_1("flash");
    spawn_local(async move {
        dom::sleep_ms(1200).await;
        let _ = el.class_list().remove_1("flash");
    });
    true
}

/// Where the annotation stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doc {
    Loading,
    /// `get` answered `not_found`: none written yet.
    Missing,
    Here,
}

/// `get` the annotation by its address: the document, or none yet.
async fn load(address: String, id: RwSignal<Option<String>>, body: RwSignal<Option<String>>, doc: RwSignal<Doc>, last_error: RwSignal<Option<String>>) {
    let Some(client) = session::client() else { return };
    match client.get_all(address.clone()).await {
        Ok(events) => {
            for ev in events {
                match ev {
                    TicketsEvent::Ticket { ticket } => {
                        id.try_set(Some(ticket.id));
                        body.try_set(Some(ticket.body));
                        doc.try_set(Doc::Here);
                    }
                    TicketsEvent::NotFound { .. } => {
                        // Not known to exist: a document seen before stays.
                        if id.try_get_untracked().flatten().is_none() {
                            doc.try_set(Doc::Missing);
                        }
                    }
                    TicketsEvent::Error { message } => last_error.set(Some(format!("get {address}: {message}"))),
                    _ => {}
                }
            }
        }
        Err(e) => last_error.set(Some(format!("get {address}: {}", session::describe(&e)))),
    }
}

/// WAVE-11: the board's annotation panel, below the board in both layouts —
/// `<ROOT>/_MENGER/board`, folded or open, read or edited.
#[component]
pub(crate) fn BoardNotes(
    root: String,
    /// The board's cards' placements — what an on-board `{wave: ID}` reads.
    cards: Memo<HashMap<String, Placement>>,
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let root = root.to_ascii_uppercase();
    let address = board_address(&root);
    let id = RwSignal::new(None::<String>);
    let body = RwSignal::new(None::<String>);
    let doc = RwSignal::new(Doc::Loading);
    let open = RwSignal::new(true);
    let editing = RwSignal::new(false);
    let refs = Refs::new(Some(cards), StoredValue::new(cache::Holder::new()));

    // On every connection (and after a lag): the document, and the rows its
    // references name, again.
    let fetch = {
        let address = address.clone();
        move || {
            refs.forget();
            spawn_local(load(address.clone(), id, body, doc, last_error));
        }
    };
    {
        let fetch = fetch.clone();
        Effect::new(move |_| {
            if socket.get() > 0 {
                fetch();
            }
        });
    }
    {
        let root = root.clone();
        session::listen(move |ev| match ev {
            TicketsEvent::Amended { ticket, .. } if id.try_get_untracked().flatten().as_deref() == Some(ticket.id.as_str()) => {
                body.try_set(Some(ticket.body.clone()));
            }
            // Written elsewhere (or by this panel's own first save) while
            // none was here.
            TicketsEvent::Created { ticket }
                if ticket.shortcode.as_str() == root.as_str() && ticket.path.as_deref() == Some(BOARD_PATH) && id.try_get_untracked().flatten().is_none() =>
            {
                id.try_set(Some(ticket.id.clone()));
                body.try_set(Some(ticket.body.clone()));
                doc.try_set(Doc::Here);
            }
            TicketsEvent::Lagged { .. } => fetch(),
            _ => {}
        });
    }
    Effect::new(move |_| body.with(|b| b.as_deref().map(|b| refs.want(&md::wave_ids(b)))));
    // A first save is the document's birth.
    Effect::new(move |_| {
        if id.with(Option::is_some) && doc.get_untracked() != Doc::Here {
            doc.set(Doc::Here);
        }
    });

    // Whether there is anything to show — a memo, so the panel is rebuilt
    // when that flips and not on every edit of the body (COMP-4: a framed
    // note's frame re-renders in place).
    let has_body = Memo::new(move |_| body.with(|b| b.as_deref().map(str::trim).is_some_and(|b| !b.is_empty())));
    let content = {
        let root = root.clone();
        move || {
            if editing.get() {
                return view! { <BodyEditor id body editing refs create=Create::board(&root) /> }.into_any();
            }
            match (doc.get(), has_body.get()) {
                (Doc::Loading, _) => view! { <p class="loading">"loading the board's notes…"</p> }.into_any(),
                // INPUT-4: the note is its doc's stored body — its inputs answer.
                (_, true) => view! {
                    <BodyView body=Signal::derive(move || body.get().unwrap_or_default()) refs=Some(refs) ticket=id.get_untracked() />
                }
                .into_any(),
                (Doc::Here, false) => view! { <p class="none">"(no notes yet — " <button class="link" on:click=move |_| editing.set(true)>"write some"</button>")"</p> }.into_any(),
                (Doc::Missing, false) => view! {
                    <p class="none"><button class="link" on:click=move |_| editing.set(true)>"write notes for this board"</button></p>
                }
                .into_any(),
            }
        }
    };
    view! {
        <section class="notes" data-address=address.clone() data-doc=move || id.get() on:click=scroll_to_ref>
            <div class="nh">
                <button class="fold" aria-expanded=move || open.get().to_string() on:click=move |_| open.update(|o| *o = !*o)>
                    <span class="arrow" aria-hidden="true">{move || if open.get() { "▾" } else { "▸" }}</span>
                    " "
                    <span class="code">{address.clone()}</span>
                </button>
                {move || (!editing.get()).then(|| view! {
                    <button class="edit" on:click=move |_| { open.set(true); editing.set(true); }>"[ edit ]"</button>
                })}
            </div>
            {move || open.get().then(content.clone())}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address of a board's annotation: its root, upper-cased, and the
    /// reserved path.
    #[test]
    fn a_boards_annotation_lives_at_its_roots_menger_board() {
        assert_eq!(board_address("plexus"), "PLEXUS/_MENGER/board");
        assert_eq!(BOARD_PATH, format!("{}board", menger_tickets_api::MENGER_PATH_PREFIX));
        assert_eq!(
            Create::board("plexus"),
            Create { epic: "PLEXUS".into(), title: "board: PLEXUS".into(), path: "_MENGER/board".into() }
        );
    }

    /// WAVE-11 acceptance 1: a reference placed — on the board from its
    /// card, off it from the row's kept placement (done and missing too);
    /// one with no card and no row (an unknown id, a row not fetched yet), or
    /// a row with no placement (a doc), is left as written.
    #[test]
    fn a_wave_reference_is_placed_from_the_card_else_the_row() {
        let cards: HashMap<String, Placement> = [
            ("RETURNS-6".to_string(), Placement::Wave { n: 2 }),
            ("RETURNS-4".to_string(), Placement::Done),
        ]
        .into();
        assert_eq!(resolve_ref("RETURNS-6", Some(&cards), None), Some(WaveRef::Wave(2)), "on the board");
        assert_eq!(resolve_ref("RETURNS-4", Some(&cards), None), Some(WaveRef::Done));
        assert_eq!(
            resolve_ref("RETURNS-6", Some(&cards), Some(Some(Placement::Wave { n: 9 }))),
            Some(WaveRef::Wave(2)),
            "the card wins: it is what the board shows"
        );
        assert_eq!(resolve_ref("MNG-4", Some(&cards), Some(Some(Placement::Wave { n: 3 }))), Some(WaveRef::Wave(3)), "off the board");
        assert_eq!(resolve_ref("MNG-5", None, Some(Some(Placement::Done))), Some(WaveRef::Done), "off any board");
        assert_eq!(resolve_ref("MNG-6", None, Some(Some(Placement::MissingBlocker))), Some(WaveRef::Missing));
        assert_eq!(resolve_ref("NOPE-1", Some(&cards), None), None, "unknown: left as written");
        assert_eq!(resolve_ref("DOCS-2", None, Some(None)), None, "a doc has no wave");
        // Through the renderer: the board's placements, then the rows'.
        let rendered = md::render_body_with("{wave: RETURNS-6} · {wave: MNG-4} · {wave: NOPE-1}", &|id| {
            let row = (id == "MNG-4").then_some(Some(Placement::Wave { n: 3 }));
            resolve_ref(id, Some(&cards), row)
        });
        assert!(rendered.html.contains(">wave 2</a> · <a"), "{}", rendered.html);
        assert!(rendered.html.contains(">wave 3</a> · {wave: NOPE-1}"), "{}", rendered.html);
    }

    /// WAVE-11 acceptance 1: the panel's create-vs-amend choice — a document
    /// that exists is amended; none yet is created as the board's
    /// annotation; with neither, nothing is saved.
    #[test]
    fn the_first_save_creates_and_every_later_one_amends() {
        let c = Create::board("PLEXUS");
        assert_eq!(
            save_action(None, Some(&c), "notes".into()),
            Some(Save::Create { create: c.clone(), body: "notes".into() }),
            "none yet: create"
        );
        assert_eq!(
            save_action(Some("PLEXUS-35"), Some(&c), "more".into()),
            Some(Save::Amend { id: "PLEXUS-35".into(), body: "more".into() }),
            "there: amend"
        );
        assert_eq!(
            save_action(Some("MNG-4"), None, "a ticket's body".into()),
            Some(Save::Amend { id: "MNG-4".into(), body: "a ticket's body".into() }),
            "the ticket page's editor: always an amend"
        );
        assert_eq!(save_action(None, None, "x".into()), None);
    }
}
