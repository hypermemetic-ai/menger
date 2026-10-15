//! FILTER-22: a right-click on any tag builds a filter from it — a fast
//! option that applies at once ("Filter DEMAND"), and a constructor that
//! opens the filter menu holding the tag's clause.
//!
//! - **A tag** ([`Tag`]) is a chip the app draws for a value the filter has
//!   a category for: a shortcode, a ticket id, a status, a kind, a severity.
//!   It carries `data-tag="<sc|id|status|kind|sev>:<value>"`, and the surface
//!   it sits on carries `data-tag-level`: `board` (the board's table), `nav`
//!   (the nav, tree and flat), `waves` (the wave board's cards), `global`
//!   (the ticket page, which has no filter of its own). Tags inside a framed
//!   body are the frame's, not the app's: they carry nothing.
//! - **Its clauses** ([`offers`]): a shortcode's is the one checking it in
//!   the menu's Shortcode category makes — its tree ([`shortcode_clause`]);
//!   a ticket id's, its shortcode's, then `under` the ticket; a status, a
//!   kind or a severity, that category's with that value.
//! - **The menu** ([`items`]), at the pointer: **Filter …** for each clause
//!   — the level's filter becomes exactly it, at once (a surface's own
//!   categories cleared, as a saved filter clears them; [`only`]); **Add …
//!   to the filter** — the first clause joins the filter as it stands
//!   ([`joined`]); **Build a filter from …** — the filter becomes the first
//!   clause and the level's menu opens on its category, ready for more.
//! - **Which filter** ([`tag_at`]): the surface's own where the tag sits on
//!   one; on the ticket page, the global filter.
//! - **One menu**, as AGS-16's: one document-level `contextmenu` listener.
//!   ACT-4: a tag inside an element the ticket menu answers (a nav row, a
//!   board row, a card, a ticket page's heading or relation) opens the ticket
//!   menu — the one menu (src/menu/) — with these items in its filter group,
//!   a status, kind, severity or label chip's first; this menu stands down
//!   for it, and answers only a tag drawn outside any ticket (a shortcode's
//!   rootless row, a label on the inputs page).
//! - **Over a selection** ([`many_items`], ACT-4): the shortcodes the
//!   tickets share, or `under` each of them ([`TagClause::UnderAny`]).
//! - **The palette** takes `filter <shortcode>` (src/keys.rs): the global
//!   filter becomes that shortcode's clause — the Filter item's ([`run`]).
//!
//! - **Filter everywhere** (FILTER-25; owner, 2026-10-13: *"I need to be
//!   able to click a chip and set the global filter"*): where a tag sits on
//!   a surface with a filter of its own (the nav, the board, the wave
//!   board), its menu — this one, or the one menu's filter group — also
//!   offers **Filter everywhere: …** for each clause ([`everywhere`],
//!   [`many_everywhere`]), ruled off after the surface's items: the global
//!   filter becomes exactly that clause. On the ticket page the Filter items
//!   already build the global filter, so nothing doubles; a framed chip's
//!   menu (whose level is the global filter) names them **Filter
//!   everywhere: …** ([`as_everywhere`]).
//! - **Alt+click** (FILTER-25's decision `click`: Alt+click, plus the
//!   menu): on any tag the app draws, the global filter becomes its first
//!   clause at once ([`alt_pick`]) — one capture-phase `click` listener on
//!   the document, which takes the click (no navigation, no download of a
//!   link); a plain click is unchanged. A ticket chip in a framed body does
//!   the same through the SDK (1.17: `menger:filter-everywhere { id }`,
//!   src/frame.rs → [`filter_everywhere`]).
//!
//! Everything that decides — a tag read from its attribute, its clauses,
//! the menu's items, what each does to a level's filter — is a pure function
//! here, pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::filter::{Field, Filter, Group, Predicate};
use menger_tickets_api::{Kind, Severity, Status};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::bindings::{self, Context};
use crate::filter::{Cat, Clausal, Clause, Clauses, Op, ScTree};
use crate::lens::{self, Kept, Lens, Level};
use crate::waves::epic_of;
use crate::{dom, menu, nav, saved, st_class};

// ---------------------------------------------------------------------
// The tags

/// What a tag names. Values are canonical: a shortcode and a ticket id upper
/// case, a status as the wire spells it, a kind and a severity as the
/// language does (lower case).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    Shortcode(String),
    Ticket(String),
    Status(String),
    Kind(String),
    Severity(String),
    /// MENGER-12: a label (src/labels.rs's chips).
    Label(String),
}

impl Tag {
    pub(crate) fn shortcode(sc: &str) -> Tag {
        Tag::Shortcode(sc.to_ascii_uppercase())
    }

    pub(crate) fn ticket(id: &str) -> Tag {
        Tag::Ticket(id.to_ascii_uppercase())
    }

    pub(crate) fn status(s: Status) -> Tag {
        Tag::Status(s.as_str().to_string())
    }

    pub(crate) fn kind(k: Kind) -> Tag {
        Tag::Kind(k.as_str().to_string())
    }

    pub(crate) fn severity(s: Severity) -> Tag {
        Tag::Severity(s.as_str().to_ascii_lowercase())
    }

    /// The tag as its element's `data-tag` says it: `id:DEMAND-1`.
    pub(crate) fn attr(&self) -> String {
        let (k, v) = match self {
            Tag::Shortcode(v) => ("sc", v),
            Tag::Ticket(v) => ("id", v),
            Tag::Status(v) => ("status", v),
            Tag::Kind(v) => ("kind", v),
            Tag::Severity(v) => ("sev", v),
            Tag::Label(v) => ("label", v),
        };
        format!("{k}:{v}")
    }

    /// A `data-tag` read back — `None` when it names nothing a filter has a
    /// category for (a severity of none is not drawn as a tag).
    pub(crate) fn parse(attr: &str) -> Option<Tag> {
        let (k, v) = attr.split_once(':')?;
        Some(match k {
            "sc" => Tag::Shortcode(Cat::Shortcode.token(v)?),
            "id" => Tag::Ticket(ticket_id(v)?),
            "status" => Tag::Status(Cat::Status.token(v)?),
            "kind" => Tag::Kind(Cat::Kind.token(v)?),
            "sev" => Tag::Severity(Cat::Severity.token(v).filter(|s| s != "none")?),
            "label" => Tag::Label(Cat::Label.token(v)?),
            _ => return None,
        })
    }
}

/// `raw` as a ticket id — a shortcode, `-`, a number — upper case.
fn ticket_id(raw: &str) -> Option<String> {
    let id = raw.trim().to_ascii_uppercase();
    let (sc, seq) = id.rsplit_once('-')?;
    (Cat::Shortcode.token(sc).is_some() && !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

// ---------------------------------------------------------------------
// Its clauses

/// A clause a tag gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TagClause {
    /// A category's clause — the one the menu makes.
    Of(Cat, Clause),
    /// `under` a ticket: the ticket and its descendants — a group no
    /// category draws (a custom pill).
    Under(String),
    /// ACT-4: `under` any of several tickets — a selection's subtrees, one
    /// group (`under A-1 or under B-4`).
    UnderAny(Vec<String>),
}

impl TagClause {
    /// The category whose values hold it — none for `under`.
    pub(crate) fn cat(&self) -> Option<Cat> {
        match self {
            TagClause::Of(c, _) => Some(*c),
            TagClause::Under(_) | TagClause::UnderAny(_) => None,
        }
    }

    /// The clause as the language's group.
    pub(crate) fn group(&self) -> Option<Group> {
        match self {
            TagClause::Of(c, cl) => lens::group_of(*c, cl),
            TagClause::Under(id) => Some(Group::one(Predicate::is(Field::Under(id.to_ascii_uppercase())))),
            TagClause::UnderAny(ids) if ids.is_empty() => None,
            TagClause::UnderAny(ids) => {
                Some(Group::any(ids.iter().map(|id| Predicate::is(Field::Under(id.to_ascii_uppercase()))).collect()))
            }
        }
    }
}

/// A clause a tag offers, and how the menu names it — `name` after Filter
/// and Add (`DEMAND`, `under DEMAND-1`, `status Pending`), `value` after
/// Build a filter from (`DEMAND`, `Pending`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) clause: TagClause,
    pub(crate) name: String,
    pub(crate) value: String,
}

/// The Shortcode clause checking `code` in the menu makes, nothing checked
/// yet: its whole tree — `code` and every shortcode hosted under it (the
/// code alone while the groups are not known).
pub(crate) fn shortcode_clause(code: &str, tree: Option<&ScTree>) -> Option<Clause> {
    let mut k = Clauses::default();
    match tree {
        Some(t) => k.toggle_code(t, code),
        None => k.toggle(Cat::Shortcode, code),
    }
    k.get(Cat::Shortcode).cloned()
}

fn shortcode_offer(code: &str, tree: Option<&ScTree>) -> Option<Offer> {
    let code = Cat::Shortcode.token(code)?;
    Some(Offer { clause: TagClause::Of(Cat::Shortcode, shortcode_clause(&code, tree)?), name: code.clone(), value: code })
}

fn value_offer(c: Cat, v: &str) -> Option<Offer> {
    let label = c.value_label(v);
    Some(Offer {
        clause: TagClause::Of(c, Clause::new(c, Op::Is, [v.to_string()])?),
        name: format!("{} {label}", c.label().to_ascii_lowercase()),
        value: label,
    })
}

/// The clauses a tag gives, the one its menu leads with first: a shortcode's
/// tree; a ticket id's shortcode's, then `under` the ticket; a status, a
/// kind or a severity, that category's with that value.
pub(crate) fn offers(tag: &Tag, tree: Option<&ScTree>) -> Vec<Offer> {
    match tag {
        Tag::Shortcode(sc) => shortcode_offer(sc, tree).into_iter().collect(),
        Tag::Ticket(id) => {
            let under = Offer { clause: TagClause::Under(id.clone()), name: format!("under {id}"), value: id.clone() };
            epic_of(id).and_then(|sc| shortcode_offer(sc, tree)).into_iter().chain([under]).collect()
        }
        Tag::Status(v) => value_offer(Cat::Status, v).into_iter().collect(),
        Tag::Kind(v) => value_offer(Cat::Kind, v).into_iter().collect(),
        Tag::Severity(v) => value_offer(Cat::Severity, v).into_iter().collect(),
        Tag::Label(v) => value_offer(Cat::Label, v).into_iter().collect(),
    }
}

// ---------------------------------------------------------------------
// The menu's items

/// What an item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    /// The filter becomes exactly the clause, at once.
    Filter,
    /// The clause joins the filter as it stands.
    Add,
    /// The filter becomes the clause, and the menu opens on its category.
    Build,
    /// FILTER-25: the *global* filter becomes exactly the clause, at once —
    /// whatever surface the tag sits on.
    Everywhere,
}

impl Act {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Act::Filter => "filter",
            Act::Add => "add",
            Act::Build => "build",
            Act::Everywhere => "everywhere",
        }
    }
}

/// One item of a tag's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) act: Act,
    pub(crate) clause: TagClause,
    pub(crate) words: String,
}

/// A tag's menu: **Filter …** by each clause it gives, then **Add … to the
/// filter** and **Build a filter from …** with the first —
///
/// ```text
/// Filter DEMAND · Filter under DEMAND-1 · Add DEMAND to the filter · Build a filter from DEMAND…
/// Filter status Pending · Add status Pending to the filter · Build a filter from Pending…
/// ```
pub(crate) fn items(tag: &Tag, tree: Option<&ScTree>) -> Vec<Item> {
    items_of(offers(tag, tree))
}

/// The items for `offers`, the first leading Add and Build.
fn items_of(offers: Vec<Offer>) -> Vec<Item> {
    let mut out: Vec<Item> =
        offers.iter().map(|o| Item { act: Act::Filter, clause: o.clause.clone(), words: format!("Filter {}", o.name) }).collect();
    if let Some(first) = offers.first() {
        out.push(Item { act: Act::Add, clause: first.clause.clone(), words: format!("Add {} to the filter", first.name) });
        out.push(Item { act: Act::Build, clause: first.clause.clone(), words: format!("Build a filter from {}…", first.value) });
    }
    out
}

/// ACT-4: a selection's filter items — **Filter** the shortcodes its
/// tickets share (each one's tree, as one clause), **Filter under each** of
/// them, then Add and Build with the shortcodes:
///
/// ```text
/// Filter DEMAND, MNG · Filter under each of 3 · Add DEMAND, MNG to the filter · Build a filter from DEMAND, MNG…
/// ```
pub(crate) fn many_items(ids: &[String], tree: Option<&ScTree>) -> Vec<Item> {
    items_of(many_offers(ids, tree))
}

/// A selection's clauses: the shortcodes its tickets share (each one's tree,
/// as one clause), then `under` each of them.
fn many_offers(ids: &[String], tree: Option<&ScTree>) -> Vec<Offer> {
    let mut codes: Vec<String> = Vec::new();
    for id in ids {
        if let Some(sc) = epic_of(id).and_then(|sc| Cat::Shortcode.token(sc)) {
            if !codes.contains(&sc) {
                codes.push(sc);
            }
        }
    }
    let values: Vec<String> = codes.iter().filter_map(|c| shortcode_clause(c, tree)).flat_map(|cl| cl.values).collect();
    let mut offers = Vec::new();
    if let Some(cl) = Clause::new(Cat::Shortcode, Op::Is, values) {
        let name = codes.join(", ");
        offers.push(Offer { clause: TagClause::Of(Cat::Shortcode, cl), name: name.clone(), value: name });
    }
    let under: Vec<String> = ids.iter().map(|id| id.to_ascii_uppercase()).collect();
    if !under.is_empty() {
        let n = under.len();
        offers.push(Offer { clause: TagClause::UnderAny(under), name: format!("under each of {n}"), value: format!("{n} tickets") });
    }
    offers
}

/// FILTER-25: **Filter everywhere: …** for each clause a tag gives — the
/// global filter becomes exactly it, whatever surface the tag sits on:
///
/// ```text
/// Filter everywhere: DEMAND · Filter everywhere: under DEMAND-1
/// ```
pub(crate) fn everywhere(tag: &Tag, tree: Option<&ScTree>) -> Vec<Item> {
    everywhere_of(offers(tag, tree))
}

/// FILTER-25: a selection's **Filter everywhere: …** — its shortcodes, and
/// `under` each of them.
pub(crate) fn many_everywhere(ids: &[String], tree: Option<&ScTree>) -> Vec<Item> {
    everywhere_of(many_offers(ids, tree))
}

fn everywhere_of(offers: Vec<Offer>) -> Vec<Item> {
    offers.into_iter().map(|o| Item { act: Act::Everywhere, words: format!("Filter everywhere: {}", o.name), clause: o.clause }).collect()
}

/// FILTER-25: a menu whose filter is already the global one, said as such —
/// each **Filter …** becomes **Filter everywhere: …** (what it does is
/// unchanged: the global filter becomes the clause); Add and Build stay. A
/// framed chip's menu: the reader is in a body, on no surface of the app's.
pub(crate) fn as_everywhere(items: Vec<Item>) -> Vec<Item> {
    items
        .into_iter()
        .map(|it| match it.act {
            Act::Filter => Item {
                act: Act::Everywhere,
                words: it.words.strip_prefix("Filter ").map(|w| format!("Filter everywhere: {w}")).unwrap_or(it.words),
                clause: it.clause,
            },
            _ => it,
        })
        .collect()
}

/// FILTER-25: what an Alt+click on a tag sets the global filter to — its
/// first clause, its menu's first item's: a ticket id's shortcode, a
/// shortcode's tree, a status's, a kind's, a severity's, a label's.
pub(crate) fn alt_pick(tag: &Tag, tree: Option<&ScTree>) -> Option<TagClause> {
    offers(tag, tree).into_iter().next().map(|o| o.clause)
}

/// What an item makes of a level's state `k`.
pub(crate) fn apply(act: Act, clause: &TagClause, k: &Kept) -> Kept {
    match act {
        Act::Filter | Act::Build | Act::Everywhere => only(clause, k),
        Act::Add => joined(clause, k),
    }
}

/// The level's filter as exactly `clause`: no other group, none of the
/// surface's own categories (as a saved filter is chosen, FILTER-6) — the
/// display part, which filters nothing, kept.
pub(crate) fn only(clause: &TagClause, k: &Kept) -> Kept {
    Kept {
        filter: Filter { groups: clause.group().into_iter().collect(), display: k.filter.display.clone() },
        local: Clauses::default(),
    }
}

/// `clause` joined to the filter as it stands: where its category has an
/// `is` clause, its values join that clause's (as checking them in the menu
/// does — one value of a field cannot be two); otherwise the filter `and`
/// it — a new clause beside the rest, an `is not` of its category narrowed
/// by FILTER-10's merge (`not status Complete` and `status Pending` is
/// `status Pending`), `under` a group of its own.
pub(crate) fn joined(clause: &TagClause, k: &Kept) -> Kept {
    let mut next = k.clone();
    if let TagClause::Of(c, want) = clause {
        if let Some(have) = k.clause(*c).filter(|h| h.op == Op::Is) {
            next.set(*c, Op::Is, have.values.into_iter().chain(want.values.iter().cloned()));
            return next;
        }
    }
    if let Some(g) = clause.group() {
        next.filter = k.filter.and(&Filter { groups: vec![g], display: Default::default() });
    }
    next
}

/// The level an item chosen at `level` builds: its own — FILTER-25's
/// Filter everywhere, the global filter's.
pub(crate) fn level_of(act: Act, level: Level) -> Level {
    match act {
        Act::Everywhere => Level::Global,
        _ => level,
    }
}

/// An item chosen at `level`: the level's filter made (Filter everywhere:
/// the global filter's), and — Build — its menu opened on the clause's
/// category.
pub(crate) fn run(lens: Lens, level: Level, act: Act, clause: &TagClause) {
    let level = level_of(act, level);
    lens.edit(level, |k| *k = apply(act, clause, k));
    if act == Act::Build {
        if let Some(c) = clause.cat() {
            lens.open_menu(level, Some(c), None);
        }
    }
}

/// FILTER-22's palette: the global filter becomes `code`'s clause — what
/// the Filter item of its tag does on the ticket page.
pub(crate) fn filter_globally(lens: Lens, code: &str) {
    let tree = lens.sctree().get_untracked();
    if let Some(o) = offers(&Tag::shortcode(code), tree.as_ref()).into_iter().next() {
        run(lens, Level::Global, Act::Filter, &o.clause);
    }
}

/// FILTER-25: the global filter becomes `tag`'s first clause — an
/// Alt+click on it, in the app or in a framed body.
pub(crate) fn alt_filter(lens: Lens, tag: &Tag) {
    let tree = lens.sctree().get_untracked();
    if let Some(clause) = alt_pick(tag, tree.as_ref()) {
        run(lens, Level::Global, Act::Everywhere, &clause);
    }
}

thread_local! {
    /// FILTER-25: the lens, for the frame bridge's Alt+click — set when the
    /// tag menu mounts (a window message has no reactive owner to find it).
    static LENS: std::cell::Cell<Option<Lens>> = const { std::cell::Cell::new(None) };
}

/// FILTER-25: a ticket chip Alt+clicked in a framed body (src/frame.rs has
/// checked `id`'s form): as on an app chip.
pub(crate) fn filter_everywhere(id: &str) {
    if let Some(lens) = LENS.with(std::cell::Cell::get) {
        alt_filter(lens, &Tag::ticket(id));
    }
}

/// FILTER-25: the tag an Alt+click on `el` names, wherever the app drew it
/// (its surface need not have a filter: the global filter is the one set).
fn alt_tag(el: &web_sys::Element) -> Option<Tag> {
    let host = el.closest("[data-tag]").ok().flatten()?;
    Tag::parse(&host.get_attribute("data-tag")?)
}

// ---------------------------------------------------------------------
// The DOM

/// The tag `el` is (or sits in) and the level its surface names — `None`
/// where it is no tag the app drew. ACT-4: inside a ticket element the one
/// menu (src/menu/) takes it as its chip.
pub(crate) fn tag_at(el: &web_sys::Element) -> Option<(Tag, Level)> {
    let host = el.closest("[data-tag]").ok().flatten()?;
    let tag = Tag::parse(&host.get_attribute("data-tag")?)?;
    let surface = host.closest("[data-tag-level]").ok().flatten()?;
    let level = Level::of_name(&surface.get_attribute("data-tag-level")?)?;
    Some((tag, level))
}

/// The menu's DOM id.
const MENU_ID: &str = "tagmenu";

/// What was right-clicked, where, and whose filter it builds.
#[derive(Clone, Debug, PartialEq)]
struct Open {
    tag: Tag,
    level: Level,
    x: f64,
    y: f64,
}

fn close(open: RwSignal<Option<Open>>) {
    if open.try_with_untracked(Option::is_some) == Some(true) {
        open.set(None);
    }
}

/// The document-level listeners, for the app's lifetime: open on a
/// right-click over a tag; close on Escape, a mousedown outside the menu, a
/// scroll, or a route change — as the ticket menu does.
fn install_listeners(open: RwSignal<Option<Open>>, lens: Lens) {
    let Some(doc) = dom::window().document() else { return };
    // FILTER-25: Alt+click on a tag — capture, so the row, the link or the
    // chip's own handler never sees it (no navigation; an Alt+click on a
    // link would download it otherwise). Any other click passes untouched.
    let on_click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if bindings::hit_mouse(&ev, Context::Chip) != Some("chip.filter-everywhere") {
            return;
        }
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        // A menu's own chip, an editor's field: not a tag to filter by.
        if el.closest(&format!("#{MENU_ID}, #tmenu, input, textarea, select, [contenteditable]")).ok().flatten().is_some() {
            return;
        }
        let Some(tag) = alt_tag(&el) else { return };
        ev.prevent_default();
        ev.stop_immediate_propagation();
        close(open);
        alt_filter(lens, &tag);
    });
    let _ = doc.add_event_listener_with_callback_and_bool("click", on_click.as_ref().unchecked_ref(), true);
    on_click.forget();

    let on_context = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if bindings::hit_mouse(&ev, Context::Chip) != Some("menu.open") {
            return;
        }
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        // ACT-4: inside a ticket the one menu answers, with these items.
        if menu::host_of(&el).is_some() {
            return;
        }
        if let Some((tag, level)) = tag_at(&el) {
            ev.prevent_default();
            open.set(Some(Open { tag, level, x: ev.client_x() as f64, y: ev.client_y() as f64 }));
        }
    });
    let _ = doc.add_event_listener_with_callback("contextmenu", on_context.as_ref().unchecked_ref());
    on_context.forget();

    let on_down = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if open.with_untracked(Option::is_none) {
            return;
        }
        let inside = dom::window().document().and_then(|d| d.get_element_by_id(MENU_ID)).is_some_and(|m| {
            ev.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()).is_some_and(|n| m.contains(Some(&n)))
        });
        if !inside {
            close(open);
        }
    });
    // Capture: nothing swallows the mousedown before this sees it.
    let _ = doc.add_event_listener_with_callback_and_bool("mousedown", on_down.as_ref().unchecked_ref(), true);
    on_down.forget();

    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if bindings::is(&ev, Context::Menu, "dialog.close") {
            close(open);
        }
    });
    let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    let on_scroll = Closure::<dyn FnMut()>::new(move || close(open));
    let _ = doc.add_event_listener_with_callback_and_bool("scroll", on_scroll.as_ref().unchecked_ref(), true);
    on_scroll.forget();

    let on_route = Closure::<dyn FnMut()>::new(move || close(open));
    let _ = dom::window().add_event_listener_with_callback("hashchange", on_route.as_ref().unchecked_ref());
    on_route.forget();
}

/// Mounted once by the App, outside every tag: the open menu, if any.
#[component]
pub(crate) fn TagMenu() -> impl IntoView {
    let open = RwSignal::new(None::<Open>);
    let lens = lens::use_lens();
    LENS.with(|l| l.set(Some(lens)));
    install_listeners(open, lens);
    move || open.get().map(|o| menu_view(o, open))
}

/// The tag as the menu's header draws it — its chip.
fn chip(tag: &Tag) -> AnyView {
    match tag {
        Tag::Shortcode(v) | Tag::Ticket(v) => view! { <span class="code">{v.clone()}</span> }.into_any(),
        Tag::Status(v) => view! { <span class=format!("tag {}", st_class(v))>{v.clone()}</span> }.into_any(),
        Tag::Kind(v) => view! { <span class="kind">{nav::kind_glyph(v)}" "{v.clone()}</span> }.into_any(),
        Tag::Severity(v) => view! { <span class="sev">{Cat::Severity.value_label(v)}</span> }.into_any(),
        Tag::Label(v) => view! { <span class=crate::labels::chip_class(v)>{v.clone()}</span> }.into_any(),
    }
}

/// One open menu: the tag and whose filter it builds, then its items — each
/// a button, the group it makes on hover. Built fresh per open; focused so
/// the arrows move through it at once.
fn menu_view(o: Open, open: RwSignal<Option<Open>>) -> impl IntoView {
    let lens = lens::use_lens();
    let Open { tag, level, x, y } = o;
    let tree = lens.sctree().get_untracked();
    let mut items = items(&tag, tree.as_ref());
    // FILTER-25: on a surface with a filter of its own, the global filter
    // too — ruled off after the surface's items.
    let rule_at = (level != Level::Global).then_some(items.len());
    if rule_at.is_some() {
        items.extend(everywhere(&tag, tree.as_ref()));
    }
    let root = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        let Some(el) = root.get() else { return };
        menu::place(&el, x, y);
        let _ = el.focus();
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        let Some(el) = root.get_untracked() else { return };
        let id = bindings::hit(&ev, Context::Menu);
        let list = menu::items(&el);
        let active = dom::window().document().and_then(|d| d.active_element());
        let current = list.iter().position(|i| active.as_ref() == Some(i.as_ref()));
        if matches!(id, Some("menu.leave" | "menu.leave-back")) {
            close(open);
        } else if let Some(n) = menu::step(current, list.len(), id) {
            let _ = list[n].focus();
            ev.prevent_default();
        }
    };
    let label = format!("filter by {}", tag.attr());
    view! {
        <div
            id=MENU_ID
            role="menu"
            tabindex="-1"
            node_ref=root
            data-tag-of=tag.attr()
            data-level=level.name()
            aria-label=label
            style=format!("left: {x}px; top: {y}px")
            on:contextmenu=|ev| ev.prevent_default()
            on:keydown=on_key
        >
            <div class="h">
                {chip(&tag)}
                <span class="title">{saved::level_words(level)}</span>
            </div>
            {items
                .into_iter()
                .enumerate()
                .map(|(n, it)| {
                    let text = it.clause.group().map(|g| g.format());
                    let Item { act, clause, words } = it;
                    let rule = (rule_at == Some(n)).then(|| view! { <div class="rule" role="separator"></div> });
                    view! {
                        {rule}
                        <button
                            class="item"
                            role="menuitem"
                            data-act=act.name()
                            data-filter=text.clone()
                            title=text
                            on:click=move |_| {
                                close(open);
                                run(lens, level, act, &clause);
                            }
                        >
                            {words}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::tests::{platform, strs};
    use menger_tickets_api::Group as Roll;
    use std::collections::BTreeMap;

    /// The demand tree of the ticket's example: DEMAND, and SUB hosted under
    /// a DEMAND ticket; MNG beside them.
    fn demand() -> ScTree {
        let group = |epic: &str, host: Option<&str>| {
            (
                epic.to_string(),
                Roll { epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(), total: 2, done: 0, docs: 0, root: Some(format!("{epic}-1")), root_row: None, host: host.map(Into::into) },
            )
        };
        let g: BTreeMap<String, Roll> = [group("DEMAND", None), group("SUB", Some("DEMAND-3")), group("MNG", None)].into_iter().collect();
        ScTree::of(&g)
    }

    fn words(tag: &Tag, tree: Option<&ScTree>) -> Vec<String> {
        items(tag, tree).into_iter().map(|i| i.words).collect()
    }

    fn acts(tag: &Tag) -> Vec<Act> {
        items(tag, Some(&demand())).into_iter().map(|i| i.act).collect()
    }

    fn text(k: &Kept) -> String {
        k.filter.format()
    }

    fn kept(text: &str) -> Kept {
        Kept { filter: Filter::parse(text).unwrap(), local: Clauses::default() }
    }

    /// A tag rides its element as `data-tag`, canonical both ways; what is
    /// no tag reads as none.
    #[test]
    fn a_tag_rides_its_element() {
        let all = [
            Tag::shortcode("demand"),
            Tag::ticket("demand-1"),
            Tag::status(Status::InReview),
            Tag::kind(Kind::Build),
            Tag::severity(Severity::High),
        ];
        assert_eq!(all.clone().map(|t| t.attr()), ["sc:DEMAND", "id:DEMAND-1", "status:In Review", "kind:build", "sev:high"]);
        for t in all {
            assert_eq!(Tag::parse(&t.attr()), Some(t.clone()), "{t:?}");
        }
        // Read as the menu reads its values: any case.
        assert_eq!(Tag::parse("status:pending"), Some(Tag::Status("Pending".into())));
        assert_eq!(Tag::parse("sev:Critical"), Some(Tag::Severity("critical".into())));
        assert_eq!(Tag::parse("id:Sub-12"), Some(Tag::Ticket("SUB-12".into())));
        for bad in ["", "sc", "sc:", "sc:A B", "id:DEMAND", "id:DEMAND-", "id:-3", "id:DEMAND-x", "status:Nope", "kind:epic", "sev:none", "sev:", "wave:2", ":x"] {
            assert_eq!(Tag::parse(bad), None, "{bad}");
        }
    }

    /// A shortcode's clause is the one checking it in the menu's Shortcode
    /// category makes — its tree (the platform's: PLEXUS and the nine it
    /// hosts); a leaf shortcode, itself; with no groups known yet, itself.
    #[test]
    fn a_shortcodes_clause_is_the_menus() {
        let t = platform();
        // What a click on its box in the menu makes, on a level with nothing
        // checked (`Lens::toggle`, under the switch's `is`).
        let mut by_menu = Kept::default();
        by_menu.edit_as(Cat::Shortcode, Op::Is, |k| k.toggle_code(&t, "PLEXUS"));
        let cl = shortcode_clause("PLEXUS", Some(&t)).unwrap();
        assert_eq!(Some(cl.clone()), by_menu.clause(Cat::Shortcode));
        assert_eq!(cl.values.len(), 10);
        assert_eq!(offers(&Tag::shortcode("PLEXUS"), Some(&t))[0].clause, TagClause::Of(Cat::Shortcode, cl));
        assert_eq!(shortcode_clause("WAVE", Some(&t)), Some(Clause { op: Op::Is, values: strs(&["WAVE"]) }));
        assert_eq!(shortcode_clause("NOPE", None), Some(Clause { op: Op::Is, values: strs(&["NOPE"]) }));
        assert_eq!(
            only(&offers(&Tag::shortcode("DEMAND"), Some(&demand()))[0].clause, &Kept::default()).filter.format(),
            "in (DEMAND, SUB)"
        );
    }

    /// A ticket id gives its shortcode's clause first, then `under` it; a
    /// status, a kind, a severity, that category's clause with that value.
    #[test]
    fn a_tag_gives_its_clauses() {
        let t = demand();
        let of = |tag: Tag| offers(&tag, Some(&t)).into_iter().map(|o| o.clause).collect::<Vec<_>>();
        assert_eq!(
            of(Tag::ticket("DEMAND-1")),
            [
                TagClause::Of(Cat::Shortcode, Clause { op: Op::Is, values: strs(&["DEMAND", "SUB"]) }),
                TagClause::Under("DEMAND-1".into())
            ]
        );
        assert_eq!(of(Tag::ticket("SUB-4"))[0], TagClause::Of(Cat::Shortcode, Clause { op: Op::Is, values: strs(&["SUB"]) }));
        assert_eq!(of(Tag::status(Status::Pending)), [TagClause::Of(Cat::Status, Clause { op: Op::Is, values: strs(&["Pending"]) })]);
        assert_eq!(of(Tag::kind(Kind::Spike)), [TagClause::Of(Cat::Kind, Clause { op: Op::Is, values: strs(&["spike"]) })]);
        assert_eq!(of(Tag::severity(Severity::Low)), [TagClause::Of(Cat::Severity, Clause { op: Op::Is, values: strs(&["low"]) })]);

        // Each as the language writes it — the pills a menu would draw.
        let groups: Vec<String> = [Tag::ticket("DEMAND-1"), Tag::status(Status::InReview), Tag::kind(Kind::Doc), Tag::severity(Severity::High)]
            .iter()
            .flat_map(|tag| offers(tag, Some(&t)))
            .map(|o| o.clause.group().unwrap().format())
            .collect();
        assert_eq!(groups, ["in (DEMAND, SUB)", "under DEMAND-1", "status \"In Review\"", "kind doc", "severity high"]);
        // The category a Build opens on; `under` has none.
        assert_eq!(of(Tag::ticket("DEMAND-1")).iter().map(TagClause::cat).collect::<Vec<_>>(), [Some(Cat::Shortcode), None]);
    }

    /// The ticket's example, word for word: a ticket id's menu leads with its
    /// shortcode, then `under` it, then Add and Build with the shortcode; a
    /// status's and a shortcode's, Filter · Add · Build.
    #[test]
    fn the_menu_reads_as_the_example() {
        let t = demand();
        assert_eq!(
            words(&Tag::ticket("DEMAND-1"), Some(&t)),
            ["Filter DEMAND", "Filter under DEMAND-1", "Add DEMAND to the filter", "Build a filter from DEMAND…"]
        );
        assert_eq!(acts(&Tag::ticket("DEMAND-1")), [Act::Filter, Act::Filter, Act::Add, Act::Build]);
        assert_eq!(
            words(&Tag::status(Status::Pending), Some(&t)),
            ["Filter status Pending", "Add status Pending to the filter", "Build a filter from Pending…"]
        );
        assert_eq!(acts(&Tag::status(Status::Pending)), [Act::Filter, Act::Add, Act::Build]);
        assert_eq!(words(&Tag::shortcode("DEMAND"), Some(&t)), ["Filter DEMAND", "Add DEMAND to the filter", "Build a filter from DEMAND…"]);
        assert_eq!(words(&Tag::kind(Kind::Build), None), ["Filter kind build", "Add kind build to the filter", "Build a filter from build…"]);
        assert_eq!(
            words(&Tag::severity(Severity::Critical), None),
            ["Filter severity critical", "Add severity critical to the filter", "Build a filter from critical…"]
        );
        // Add and Build carry the first clause.
        let it = items(&Tag::ticket("DEMAND-1"), Some(&t));
        assert_eq!(it[2].clause, it[0].clause);
        assert_eq!(it[3].clause, it[0].clause);
        assert_eq!(Act::Build.name(), "build");
    }

    /// Filter and Build: the level's filter becomes exactly the clause — no
    /// other group, none of the surface's own categories — its display kept.
    #[test]
    fn filter_makes_the_filter_exactly_the_clause() {
        let t = demand();
        let mut k = kept("kind in (build, spike) and not status Complete · flat");
        k.local.set(Cat::Wave, Op::Is, ["2".to_string()]);
        let under = TagClause::Under("DEMAND-1".into());
        let sc = offers(&Tag::shortcode("DEMAND"), Some(&t)).remove(0).clause;
        for act in [Act::Filter, Act::Build] {
            let next = apply(act, &sc, &k);
            assert_eq!(next.filter.groups, Filter::parse("in (DEMAND, SUB)").unwrap().groups, "{act:?}");
            assert_eq!(next.filter.display, k.filter.display);
            assert_eq!(next.local, Clauses::default());
        }
        assert_eq!(only(&under, &Kept::default()).filter.format(), "under DEMAND-1");
        let pending = offers(&Tag::status(Status::Pending), None).remove(0).clause;
        assert_eq!(text(&only(&pending, &k)), "status Pending · flat");
        // Its pill: the Status clause, `is Pending`.
        assert_eq!(only(&pending, &Kept::default()).clause(Cat::Status), Some(Clause { op: Op::Is, values: strs(&["Pending"]) }));
    }

    /// Add: the clause joins the filter as it stands — beside the other
    /// categories; into its category's `is` clause as more values; narrowing
    /// an `is not` of its category; `under`, a group of its own; twice, as
    /// once.
    #[test]
    fn add_joins_the_filter_as_it_stands() {
        let t = demand();
        let pending = offers(&Tag::status(Status::Pending), None).remove(0).clause;
        let sc = offers(&Tag::shortcode("DEMAND"), Some(&t)).remove(0).clause;
        let under = TagClause::Under("DEMAND-1".into());

        assert_eq!(text(&joined(&pending, &Kept::default())), "status Pending");
        assert_eq!(text(&joined(&pending, &kept("kind build"))), "kind build and status Pending");
        assert_eq!(text(&joined(&pending, &kept("kind build and status Ready"))), "kind build and status in (Ready, Pending)");
        assert_eq!(text(&joined(&sc, &kept("in MNG and ready"))), "in (DEMAND, MNG, SUB) and ready");
        assert_eq!(text(&joined(&pending, &kept("not status in (Complete, Superseded)"))), "status Pending");
        assert_eq!(text(&joined(&under, &kept("in MNG"))), "in MNG and under DEMAND-1");
        let once = joined(&pending, &kept("kind build"));
        assert_eq!(joined(&pending, &once), once, "twice is once");
        assert_eq!(joined(&under, &joined(&under, &Kept::default())), joined(&under, &Kept::default()));
        // What is already there stays: the surface's own categories, the display.
        let mut k = kept("kind build · flat");
        k.local.set(Cat::Readiness, Op::Is, ["workable".to_string()]);
        let next = apply(Act::Add, &pending, &k);
        assert_eq!(next.local, k.local);
        assert_eq!(text(&next), "kind build and status Pending · flat");
    }

    /// ACT-4: a selection's items — the shortcodes its tickets share, each
    /// one's tree, then `under` each of them; Add and Build with the first.
    #[test]
    fn a_selection_filters_its_shortcodes_or_under_each() {
        let t = demand();
        let ids = strs(&["DEMAND-4", "MNG-2", "DEMAND-7"]);
        let it = many_items(&ids, Some(&t));
        assert_eq!(
            it.iter().map(|i| i.words.as_str()).collect::<Vec<_>>(),
            ["Filter DEMAND, MNG", "Filter under each of 3", "Add DEMAND, MNG to the filter", "Build a filter from DEMAND, MNG…"]
        );
        assert_eq!(only(&it[0].clause, &Kept::default()).filter.format(), "in (DEMAND, MNG, SUB)");
        let under = only(&it[1].clause, &Kept::default()).filter.format();
        assert_eq!(under, "under in (DEMAND-4, MNG-2, DEMAND-7)");
        assert_eq!(Filter::parse(&under).unwrap().format(), under, "it reads back");
        assert_eq!(it[1].clause.cat(), None);
        assert!(many_items(&[], None).is_empty());
    }

    /// FILTER-25: Filter everywhere — each clause a tag gives, the global
    /// filter's, whatever surface the menu opened on.
    #[test]
    fn the_global_filter_from_any_surface() {
        // FILTER-25: the ticket's example, on a nav row's DEMAND-1 chip — the
        // nav's items, then the global filter's, one per clause.
        let t = demand();
        let ev = |tag: &Tag| everywhere(tag, Some(&t)).into_iter().map(|i| i.words).collect::<Vec<_>>();
        assert_eq!(ev(&Tag::ticket("DEMAND-1")), ["Filter everywhere: DEMAND", "Filter everywhere: under DEMAND-1"]);
        assert_eq!(ev(&Tag::status(Status::Pending)), ["Filter everywhere: status Pending"]);
        assert_eq!(ev(&Tag::shortcode("SUB")), ["Filter everywhere: SUB"]);
        assert_eq!(ev(&Tag::Label("needs-you".into())), ["Filter everywhere: label needs-you"]);
        let it = everywhere(&Tag::ticket("DEMAND-1"), Some(&t));
        assert!(it.iter().all(|i| i.act == Act::Everywhere));
        assert_eq!(Act::Everywhere.name(), "everywhere");
        // The same clauses as the surface's Filter items.
        let here: Vec<_> = items(&Tag::ticket("DEMAND-1"), Some(&t)).into_iter().filter(|i| i.act == Act::Filter).map(|i| i.clause).collect();
        assert_eq!(it.iter().map(|i| i.clause.clone()).collect::<Vec<_>>(), here);
        // The global filter becomes exactly the clause — whatever level the
        // menu opened at.
        for level in [Level::Nav, Level::Board, Level::Waves, Level::Global] {
            assert_eq!(level_of(Act::Everywhere, level), Level::Global, "{level:?}");
        }
        assert_eq!(level_of(Act::Filter, Level::Nav), Level::Nav);
        assert_eq!(level_of(Act::Build, Level::Board), Level::Board);
        let k = kept("kind build and ready · flat");
        assert_eq!(text(&apply(Act::Everywhere, &it[1].clause, &k)), "under DEMAND-1 · flat");
        assert_eq!(apply(Act::Everywhere, &it[0].clause, &k), only(&it[0].clause, &k));
        // A selection: its shortcodes, and `under` each.
        let ids = strs(&["DEMAND-4", "MNG-2"]);
        assert_eq!(
            many_everywhere(&ids, Some(&t)).into_iter().map(|i| i.words).collect::<Vec<_>>(),
            ["Filter everywhere: DEMAND, MNG", "Filter everywhere: under each of 2"]
        );
    }

    /// FILTER-25: Alt+click picks a tag's first clause — a ticket id's
    /// shortcode (its tree), a shortcode's, a value chip's category's.
    #[test]
    fn alt_click_picks_the_first_clause() {
        let t = demand();
        let sc = |v: &[&str]| Some(TagClause::Of(Cat::Shortcode, Clause { op: Op::Is, values: strs(v) }));
        assert_eq!(alt_pick(&Tag::ticket("DEMAND-1"), Some(&t)), sc(&["DEMAND", "SUB"]));
        assert_eq!(alt_pick(&Tag::ticket("SUB-4"), Some(&t)), sc(&["SUB"]));
        assert_eq!(alt_pick(&Tag::shortcode("DEMAND"), Some(&t)), sc(&["DEMAND", "SUB"]));
        assert_eq!(alt_pick(&Tag::ticket("MNG-9"), None), sc(&["MNG"]), "groups not known yet: the code alone");
        assert_eq!(
            alt_pick(&Tag::status(Status::InReview), None),
            Some(TagClause::Of(Cat::Status, Clause { op: Op::Is, values: strs(&["In Review"]) }))
        );
        assert_eq!(alt_pick(&Tag::kind(Kind::Spike), None).and_then(|c| c.group()).map(|g| g.format()).as_deref(), Some("kind spike"));
        // The first item of its menu, always.
        for tag in [Tag::ticket("DEMAND-1"), Tag::severity(Severity::High), Tag::Label("stale".into())] {
            assert_eq!(alt_pick(&tag, Some(&t)), items(&tag, Some(&t)).first().map(|i| i.clause.clone()), "{tag:?}");
        }
        // The global filter becomes exactly it.
        let pick = alt_pick(&Tag::ticket("DEMAND-1"), Some(&t)).unwrap();
        assert_eq!(text(&apply(Act::Everywhere, &pick, &kept("status Ready"))), "in (DEMAND, SUB)");
    }

    /// FILTER-25: a menu that builds the global filter already says its
    /// Filter items as Filter everywhere (a framed chip's); Add and Build stay.
    #[test]
    fn a_global_menu_says_everywhere() {
        let t = demand();
        let it = as_everywhere(items(&Tag::ticket("DEMAND-1"), Some(&t)));
        assert_eq!(
            it.iter().map(|i| i.words.as_str()).collect::<Vec<_>>(),
            ["Filter everywhere: DEMAND", "Filter everywhere: under DEMAND-1", "Add DEMAND to the filter", "Build a filter from DEMAND…"]
        );
        assert_eq!(it.iter().map(|i| i.act).collect::<Vec<_>>(), [Act::Everywhere, Act::Everywhere, Act::Add, Act::Build]);
    }

    /// The level a tag's surface names: the board, the nav, the wave board —
    /// and the ticket page's, the global filter.
    #[test]
    fn a_surface_names_its_level() {
        assert_eq!(Level::of_name("board"), Some(Level::Board));
        assert_eq!(Level::of_name("nav"), Some(Level::Nav));
        assert_eq!(Level::of_name("waves"), Some(Level::Waves));
        assert_eq!(Level::of_name("global"), Some(Level::Global));
        assert_eq!(Level::of_name("ticket"), None);
    }
}
