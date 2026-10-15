//! MENGER-43: **Needs you** is a filter on the nav, not a panel.
//!
//! - **What waits on the owner** is the result of a filter: by default
//!   [`DEFAULT`], `queued or asking or label needs-you` — every ticket in the
//!   owner's queue (MENGER-39), every ticket with an open input (`asking`,
//!   INPUT-1's questions) and every ticket labelled `needs-you`. The filter
//!   is a **saved filter** (FILTER-6, src/saved.rs): the one named "Needs
//!   you" (`VIEW/_MENGER/view/needs-you`) when one is saved, else the
//!   default. It is changed as any saved filter is: apply it, edit the
//!   pills, **Save as…** "Needs you" (replace) — the count follows the save
//!   ([`saved_changed`]), and the next `g n` applies the new text.
//! - **Where it is seen is the nav** ([`go`]): `g n`, or a click on the
//!   header's `needs you N`, puts the saved filter on the nav's level
//!   (`Level::Nav`, `n=` in the URL) exactly as `Saved ▾` puts one — the
//!   nav's own clauses replaced, its pills, text and URL following — and
//!   puts focus on the nav's first row; the route does not move. The nav's
//!   effective filter is `global and nav` (FILTER-21), so the global filter
//!   reaches Needs you by construction: filtered to one shortcode, the nav
//!   shows what waits on the owner there. The same press while the nav
//!   holds exactly that filter ([`is_applied`]) puts the nav's default back
//!   — a toggle ([`plan`]).
//! - **The header's count** ([`HeaderLink`]): `needs you N` beside
//!   `inputs`, live from the moment the app is up (`counts { filter }` kept
//!   by a membership watch — FILTER-12's [`LiveCounts`]), whatever the lens
//!   holds: it is the saved filter's count, never narrowed.
//! - **The questions** live where they are answered: the ticket page (its
//!   frame scrolls to the input, [`answer_href`], COMP-14), `#/inputs`, and
//!   the tray for a queued ticket (src/queue.rs). MENGER-12's panel on Home,
//!   which answered to no filter, is gone: Home is the board.
//!
//! What decides — the default, the saved filter's id, what the count asks,
//! the answer link, what a press does, the header's words — is pure and
//! pinned by native tests.

use std::cell::Cell;

use leptos::prelude::*;
use menger_tickets_api::filter::Filter;
use wasm_bindgen_futures::spawn_local;

use crate::counts::{wire_text, LiveCounts};
use crate::lens::{self, Lens, Level};
use crate::{dom, keys, saved, session};

/// The filter while none is saved: MENGER-39: every ticket in the owner's
/// queue, every ticket asking the owner something, and every ticket
/// labelled `needs-you`.
pub(crate) const DEFAULT: &str = "queued or asking or label needs-you";

/// The saved filter's name — its id (its doc's path) is the name's slug.
pub(crate) const SAVED_NAME: &str = "Needs you";

/// The default, parsed.
pub(crate) fn default_filter() -> Filter {
    Filter::parse(DEFAULT).expect("the default parses")
}

/// The saved filter's id.
pub(crate) fn saved_id() -> String {
    saved::slug(SAVED_NAME)
}

/// What the header's count asks with: the filter's wire text — `None` for a
/// filter that admits nothing (asked of no one). MENGER-13: a canceled
/// ticket needs no one, nor does what its cancel canceled — `not status
/// Canceled` is always beside the filter (its withdrawn inputs are no
/// longer `asking`; this takes out a `needs-you` label on a canceled
/// subtree too). The nav is given the saved filter itself, not this.
pub(crate) fn asked(f: &Filter) -> Option<String> {
    let f = f.and(&Filter::parse(NOT_CANCELED).expect("parses"));
    f.admits_nothing().is_none().then(|| wire_text(&f))
}

/// MENGER-13: what the count never counts.
pub(crate) const NOT_CANCELED: &str = "not status Canceled";

/// Where an input is answered: its ticket's page, its frame scrolled to the
/// input (COMP-14's `?anchor=`, the element with the input's id).
pub(crate) fn answer_href(ticket: &str, input: &str) -> String {
    format!("#/ticket/{ticket}?anchor={input}")
}

/// The header's title for `n` waiting.
pub(crate) fn badge_title(n: usize) -> String {
    match n {
        0 => "nothing needs you".into(),
        1 => "1 ticket needs you".into(),
        n => format!("{n} tickets need you"),
    }
}

/// The header badge's class: marked while anything waits.
fn badge_class(n: usize) -> &'static str {
    if n > 0 {
        "hbadge needs"
    } else {
        "hbadge zero"
    }
}

/// The header link's title: where it goes.
pub(crate) const LINK_TITLE: &str =
    "what waits on you — the nav under the saved filter \u{201c}Needs you\u{201d}, within the global filter (g n); again, the nav's default";

// ---------------------------------------------------------------------
// What a press does

/// The canonical text of `text` — itself, trimmed, when it does not parse.
fn canonical(text: &str) -> String {
    Filter::parse(text).map(|f| f.format()).unwrap_or_else(|_| text.trim().to_string())
}

/// Whether the nav (its canonical text, [`Lens::text`]) holds exactly the
/// Needs you filter (`needs_text`, in any spelling the language reads).
pub(crate) fn is_applied(nav_text: &str, needs_text: &str) -> bool {
    canonical(nav_text) == canonical(needs_text)
}

/// What `g n` (or the header's click) does to the nav.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Go {
    /// The nav's filter becomes this canonical text, as `Saved ▾` applies it.
    Apply(String),
    /// The nav holds it already: back to the nav's default.
    Reset,
}

/// The press, decided: apply the Needs you filter to the nav — or, the nav
/// holding exactly it, put the nav's default back.
pub(crate) fn plan(nav_text: &str, needs_text: &str) -> Go {
    if is_applied(nav_text, needs_text) {
        Go::Reset
    } else {
        Go::Apply(canonical(needs_text))
    }
}

/// Where the filter came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// Not read yet (no connection).
    Unread,
    /// Nothing is saved under the name: the default.
    Default,
    /// The saved filter.
    Saved,
    /// The saved filter could not be read — what was held stands; why.
    Unreadable(String),
}

/// Needs you's state for the app's lifetime — every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct NeedsYou {
    pub(crate) filter: RwSignal<Filter>,
    pub(crate) source: RwSignal<Source>,
    pub(crate) count: LiveCounts,
}

thread_local! {
    /// The app's Needs you and its lens — what `g n` (a key, outside any
    /// view) acts through.
    static HOOK: Cell<Option<(NeedsYou, Lens)>> = const { Cell::new(None) };
}

/// Read the saved filter into `filter` / `source`: the saved one, else the
/// default; unreadable (or no connection), what was held stays.
async fn reread(filter: RwSignal<Filter>, source: RwSignal<Source>) {
    let next = match saved::load_if_saved(&saved_id()).await {
        Ok(Some(f)) => (Some(f), Source::Saved),
        Ok(None) => (Some(default_filter()), Source::Default),
        Err(why) => (None, Source::Unreadable(why)),
    };
    if let Some(f) = next.0 {
        if filter.try_get_untracked().is_some_and(|held| held != f) {
            filter.try_set(f);
        }
    }
    source.try_set(next.1);
}

impl NeedsYou {
    /// Made once by the App (after the lens): the default at first, the
    /// saved filter read on every new connection, the count live from then
    /// on.
    pub(crate) fn install() -> Self {
        let filter = RwSignal::new(default_filter());
        let source = RwSignal::new(Source::Unread);
        // ORG-4: asked within the organization the reader chose.
        let org = crate::lens::use_lens().org();
        let text = Memo::new(move |_| org.with(|o| filter.with(|f| asked(&crate::orgs::within(o.as_deref(), f)))));
        let count = LiveCounts::new(text.into());
        let link = session::link();
        Effect::new(move |_| {
            if !link.get() {
                return;
            }
            spawn_local(reread(filter, source));
        });
        let me = NeedsYou { filter, source, count };
        provide_context(me);
        HOOK.with(|h| h.set(Some((me, lens::use_lens()))));
        me
    }
}

/// The app's Needs you.
pub(crate) fn use_needs() -> NeedsYou {
    use_context::<NeedsYou>().expect("Needs you is provided by the App")
}

/// FILTER-6's Save as… wrote saved filter `id` as `filter`: when it is
/// Needs you's, the count follows at once.
pub(crate) fn saved_changed(id: &str, filter: &Filter) {
    if id != saved_id() {
        return;
    }
    if let Some((needs, _)) = HOOK.with(Cell::get) {
        needs.filter.try_set(filter.clone());
        needs.source.try_set(Source::Saved);
    }
}

/// `g n` and the header's click: the saved filter (read again, so a save
/// made elsewhere is taken; else the default) put on the nav as `Saved ▾`
/// puts one — or, the nav holding exactly it, the nav's default — then
/// focus on the nav's first row. The route stays.
pub(crate) fn go() {
    let Some((needs, lens)) = HOOK.with(Cell::get) else { return };
    spawn_local(async move {
        reread(needs.filter, needs.source).await;
        let Some(f) = needs.filter.try_get_untracked() else { return };
        let said = match plan(&lens.text(Level::Nav), &f.format()) {
            Go::Apply(_) => {
                lens.commit(Level::Nav, Some(saved::chosen(f)));
                format!("\u{201c}{SAVED_NAME}\u{201d} applied")
            }
            Go::Reset => {
                lens.reset(Level::Nav);
                "the nav's default filter".to_string()
            }
        };
        if let Some(s) = saved::hook() {
            s.say(Level::Nav, said);
        }
        focus_first_row().await;
    });
}

/// The nav's rows.
const ROWS: &str = "#nav .tree [role=treeitem]";

/// Focus on the nav's first row once the tree is drawn under the new
/// filter: polled (the tree's answer comes from the server, as the panel's
/// focus once was), the first visible row focused once it has held still
/// for three polls — and again, for the rest of the window, if the tree
/// replaced it and focus fell to nowhere.
async fn focus_first_row() {
    let first = || keys::query_all(ROWS).into_iter().find(keys::is_visible);
    let mut last: Option<web_sys::HtmlElement> = None;
    let mut still = 0;
    let mut focused = false;
    for _ in 0..60 {
        dom::sleep_ms(50).await;
        let now = first();
        still = match (&now, &last) {
            (Some(a), Some(b)) if a.is_same_node(Some(b)) => still + 1,
            _ => 0,
        };
        last = now.clone();
        let Some(row) = now else { continue };
        let lost = dom::window()
            .document()
            .and_then(|d| d.active_element())
            .is_none_or(|a| a.tag_name().eq_ignore_ascii_case("body") || !a.is_connected());
        if (!focused && still >= 3) || (focused && lost) {
            keys::focus_and_rove(&keys::query_all(ROWS), &row);
            focused = true;
        }
    }
}

/// The header's link: `needs you` and the live count; its address is the
/// nav under the filter (a click toggles it in place).
#[component]
pub(crate) fn HeaderLink() -> impl IntoView {
    let needs = use_needs();
    // DESIGN-39: the count ticks when it moves.
    crate::motion::tick_on_change("#needs-link .hbadge", move || needs.count.n());
    let href = move || format!("#/?n={}", lens::enc(&needs.filter.with(Filter::format)));
    view! {
        <a
            class="hlink"
            id="needs-link"
            href=href
            title=LINK_TITLE
            on:click=move |ev| {
                ev.prevent_default();
                go();
            }
        >
            "needs you"
            {move || {
                needs.count.n().map(|n| view! {
                    <span class=badge_class(n) title=badge_title(n)>{n}</span>
                })
            }}
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::filter::Facts;
    use menger_tickets_api::{Kind, Status};

    fn facts(asking: bool, labels: &[&str]) -> Facts {
        Facts {
            id: "T-1".into(),
            epic: "T".into(),
            title: String::new(),
            status: Status::Ready,
            kind: Kind::Build,
            severity: None,
            blocked: false,
            has_commits: false,
            menger_doc: false,
            asking,
            labels: labels.iter().map(|l| l.to_string()).collect(),
            lineage: vec!["T-1".into(), "T".into()],
            history: Vec::new(),
        }
    }

    /// The default is the ticket's: MENGER-39: queued, asking, or labelled
    /// `needs-you` — and the count asks it beside `not status Canceled`.
    #[test]
    fn the_default_is_queued_asking_or_needs_you() {
        let f = default_filter();
        // Canonical, an `or` group is parenthesized.
        assert_eq!(f.format(), "(queued or asking or label needs-you)");
        assert_eq!(
            asked(&f).as_deref(),
            Some("(queued or asking or label needs-you) and not status Canceled"),
            "MENGER-13: never a canceled ticket"
        );
        assert_eq!(Filter::parse(&f.format()).unwrap(), f, "the canonical text reads back");
        assert!(f.matches(&facts(true, &[])));
        assert!(f.matches(&facts(false, &["needs-you"])));
        assert!(f.matches(&facts(true, &["p1", "needs-you"])));
        assert!(!f.matches(&facts(false, &["p1"])));
        assert!(!f.matches(&facts(false, &[])));
        let mut queued = facts(false, &[]);
        queued.history = vec!["queued".into()];
        assert!(f.matches(&queued), "queued: the store says so, as for every history predicate");
        assert_eq!(asked(&Filter::parse("asking and not asking").unwrap()), None, "admits nothing: asked of no one");
    }

    /// The saved filter is FILTER-6's, named "Needs you".
    #[test]
    fn the_saved_filter_is_named_needs_you() {
        assert_eq!(saved_id(), "needs-you");
        assert_eq!(saved::address_of(&saved_id()), "VIEW/_MENGER/view/needs-you");
    }

    /// An input is answered in its ticket's frame, scrolled to it.
    #[test]
    fn an_input_is_answered_at_its_anchor() {
        let href = answer_href("MENGER-12", "landing");
        assert_eq!(href, "#/ticket/MENGER-12?anchor=landing");
        assert_eq!(crate::inspect::anchor(&href), Some(crate::inspect::Anchor::Id("landing".into())));
    }

    #[test]
    fn the_badge_says_how_many() {
        assert_eq!(badge_title(0), "nothing needs you");
        assert_eq!(badge_title(1), "1 ticket needs you");
        assert_eq!(badge_title(7), "7 tickets need you");
    }

    /// MENGER-43: the nav holds Needs you when its canonical text is the
    /// filter's — whichever spelling either was written in.
    #[test]
    fn applied_is_the_same_canonical_text() {
        let canon = default_filter().format();
        assert!(is_applied(&canon, DEFAULT), "the lens's canonical text against the default as written");
        assert!(is_applied(DEFAULT, &canon));
        assert!(is_applied("  queued or asking or label needs-you ", DEFAULT));
        assert!(is_applied("label Needs-You", "label needs-you"), "a label is read in its canonical case");
        // The nav's default, another filter, or Needs you narrowed is not it.
        let nav_default = lens::nav_default().filter.format();
        assert_eq!(nav_default, "not menger doc");
        assert!(!is_applied(&nav_default, DEFAULT));
        assert!(!is_applied("", DEFAULT), "every ticket is not Needs you");
        assert!(!is_applied(&format!("{canon} and kind build"), DEFAULT));
        assert!(!is_applied("asking", DEFAULT));
        // A shown display is part of the filter.
        assert!(!is_applied(&format!("{canon} · flat"), DEFAULT));
    }

    /// MENGER-43: a press applies the filter (as canonical text); the same
    /// press on a nav holding it puts the default back.
    #[test]
    fn a_press_applies_then_resets() {
        let canon = default_filter().format();
        let nav_default = lens::nav_default().filter.format();
        assert_eq!(plan(&nav_default, DEFAULT), Go::Apply(canon.clone()));
        assert_eq!(plan("", DEFAULT), Go::Apply(canon.clone()));
        assert_eq!(plan("kind build", DEFAULT), Go::Apply(canon.clone()), "the nav's own clauses are replaced, as Saved ▾ replaces them");
        assert_eq!(plan(&canon, DEFAULT), Go::Reset);
        // A saved filter's text is applied, not the count's wire text.
        let saved = "label needs-you or label blocked";
        let applied = match plan(&nav_default, saved) {
            Go::Apply(t) => t,
            Go::Reset => panic!("not held yet"),
        };
        assert_eq!(applied, Filter::parse(saved).unwrap().format());
        assert!(!applied.contains("Canceled"));
        assert_eq!(plan(&applied, saved), Go::Reset);
        // Applied, read back as the lens reads `n=`: still held.
        let read = lens::kept_of(&applied, &[], Level::Nav).expect("reads").filter.format();
        assert_eq!(plan(&read, saved), Go::Reset);
    }

    /// MENGER-43: the nav shows `global and nav` (FILTER-21) — under a
    /// global filter of one shortcode, Needs you admits only what both
    /// admit, as the lens's effective filter composes them.
    #[test]
    fn the_global_filter_narrows_needs_you() {
        let global = Filter::parse("in T").expect("parses");
        let effective = global.and(&default_filter());
        let mut there = facts(true, &[]);
        assert!(effective.matches(&there), "asking, in T");
        there.labels = vec!["needs-you".into()];
        there.asking = false;
        assert!(effective.matches(&there), "labelled, in T");
        let mut elsewhere = facts(true, &["needs-you"]);
        elsewhere.id = "U-1".into();
        elsewhere.epic = "U".into();
        elsewhere.lineage = vec!["U-1".into(), "U".into()];
        assert!(default_filter().matches(&elsewhere), "Needs you alone admits it");
        assert!(!effective.matches(&elsewhere), "the global filter takes it out");
        assert!(!effective.matches(&facts(false, &[])), "in T, waiting on no one");
    }
}
