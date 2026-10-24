//! FILTER-53: the card on a saved chip — what a saved filter is made of,
//! as a rule builder, with the nav it would draw beside it (FILTER-50's
//! design, option B).
//!
//! - **Where it opens**: every saved chip — the chips line, the nav's
//!   head, the empty line's offers. A hover opens it after a pause, a press
//!   pins it; on a phone it is a sheet with *Rules* and *Nav* tabs. One card
//!   for the app ([`Card`], `Copy`), drawn by [`CardHost`] over the page.
//! - **Its head**: the name, who saved it and when, **on this board** (a
//!   switch: the saved filter and-ed onto the level's filter, or taken
//!   off), **More ⌄** (as text, save a copy…, keep open) and ×.
//! - **The rules** (src/parts.rs's model): a row per clause — the field,
//!   the operator (pressed: negated), the values as tokens (*+N* past four),
//!   the count it admits alone, a bin; a bracket per group with **And ⌄ /
//!   Or ⌄** (pressed: the group flips); inner groups as cards with *+ Add
//!   condition / + Add group* and *+ Name this group*, which saves the group
//!   as a saved filter and puts its name in its place; a saved filter it
//!   names a row that opens in place. Read only (no adds, bins, presses)
//!   when the filter is not on this board.
//! - **Edits are drafts**: a change lands in the saved filter whose text
//!   holds the part — the one on the board, or one it names — and is held
//!   by the card (counted, and asked of the nav, inlined: the server reads
//!   only what is saved) until **Save back**, **Save as…** or **Undo**.
//! - **Dropdowns** (FILTER-55, src/fcard/drop.rs) for every add and change:
//!   a rule's field, its operator, its values ticked — each choice with the
//!   count the card's filter would then have; a part being added is a ghost
//!   row until its first choice.
//! - **The builder** (FILTER-55): every level's own filter
//!   ([`Subject::Line`]) — the board's, the nav's, a wave board's, the
//!   inputs' and ⌂ everywhere's (its chip, `F`, the phone bar's Filter) —
//!   opened from its line's Filter button, its name or `f`. A change is the
//!   level's filter at once (no draft; a change inside a saved filter it
//!   names is that one's draft, as anywhere); More holds as text (editable
//!   here), Save as filter… (FILTER-37's naming), Reset and, on the nav's,
//!   Make global. A surface's own rules (Wave, Readiness, Off-board, Title;
//!   State, Removed) are rows of their own under *This page*, and-ed with
//!   the rest. The line says no more than the filter's name (the owner,
//!   2026-10-24).
//! - **The nav under it**: `nav tree` with the card's filter, drawn as the
//!   nav draws a row (the kind's mark, the id, the title, the matches below,
//!   the status; a holder dimmed), ▸ / ▾, *tree / flat*.
//!
//! What decides — the picker's words, the values shown, the switch's
//! filter, a copy's name — is a pure function here, pinned by native tests.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use menger_tickets_api::filter::{Conj, Display, Field, Filter, Predicate, Unit};
use menger_tickets_api::TicketsEvent;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::filter::{Cat, Clausal, Op};
use crate::lens::{Lens, Level};
use crate::parts::{self, At, Got, Inner, Node, Part, Shape, Time, TimeOf};
use crate::saved::{self, slug, Entry};
use crate::{dom, session, st_class};

mod drop;

/// How long a hover waits before the card opens, and a leave before it closes.
const HOVER_MS: i32 = 300;
const LEAVE_MS: i32 = 250;
/// The nav beside it: rows asked per level.
const NAV_PAGE: u32 = 60;
/// The global filter's mark (its chip's `data-fcard` is its line's key).
pub(crate) const HOME: &str = "\u{2302}";

// ---------------------------------------------------------------------
// The pure parts

/// What a card is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Subject {
    Saved(String),
    /// FILTER-55: a level's own filter — ⌂ everywhere's, the nav's, the
    /// board's, the wave board's, the inputs' — edited in place: a change is
    /// the level's filter at once.
    Line(Level),
}

/// Whether the card's counts are within ⌂ (every card but ⌂'s own).
fn within_home(s: &Subject) -> bool {
    !matches!(s, Subject::Line(Level::Global))
}

/// The owner a line's own parts say they are in (no saved filter's name:
/// one cannot begin with ▦).
fn line_owner(level: Level) -> String {
    format!("\u{25a6} {}", level.name())
}

fn line_of(owner: &str) -> Option<Level> {
    Level::ALL.into_iter().find(|l| line_owner(*l) == owner)
}

/// What the builder's head and the line call a level's own filter.
pub(crate) fn line_words(level: Level) -> &'static str {
    match level {
        Level::Global => "\u{2302} The filter everywhere",
        Level::Nav => "The nav's filter",
        Level::Board => "This board's filter",
        Level::Waves => "This wave board's filter",
        Level::Inputs => "The inputs' filter",
    }
}

/// FILTER-55: what the line says of its filter (the owner, 2026-10-24: *"just
/// show the filter name if available"*) — the name it is saved under, else
/// the one saved filter it is; nothing for any other.
pub(crate) fn line_name(f: &Filter, bound: Option<&str>) -> Option<String> {
    if let Some(b) = bound.filter(|b| !b.is_empty()) {
        return Some(b.to_string());
    }
    match f.groups.as_slice() {
        [g] => match g.preds.as_slice() {
            [Predicate { field: Field::Saved(n), not: false }] => Some(n.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// How many rules a filter holds, as the builder draws them (a list one
/// rule, a saved filter one) — the Filter button's badge.
pub(crate) fn rules_of(f: &Filter) -> usize {
    if f.groups.is_empty() {
        return 0;
    }
    parts::resolve_node("", &parts::of_filter(f), &|_| Got::Unknown).rules()
}

/// FILTER-55: the rules a level's filter holds — its own and its surface's
/// (the wave board's Wave, the inputs' State) — the Filter button's badge.
/// Tracked.
pub(crate) fn line_rules(lens: Lens, level: Level) -> usize {
    lens.filter(level).with(rules_of) + lens.kept(level).map_or(0, |k| k.local.iter().count())
}

/// FILTER-55: where a dropdown's choice lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// A new part at the end of the group at the path (its row a ghost until
    /// a choice is made) — `group`: as the first part of a new inner group.
    Add { group: bool },
    /// The part at the path (a surface's own rule: its category's clause).
    At,
    /// A surface's own rule, by its category, its field being changed: the
    /// first choice takes its place.
    Local(Cat),
}

/// FILTER-55: which dropdown — a rule's field, its operator, its values; a
/// surface's own rule's values and operator (its category).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum What {
    Field,
    Op,
    Values(PCat),
    Local(Cat),
    LocalOp(Cat),
}

/// The fields whose value is ticked from a list (or, a label, typed).
pub(crate) const LISTED: [PCat; 5] = [PCat::Status, PCat::Kind, PCat::Severity, PCat::Shortcode, PCat::Label];

/// The field a category's rules say (`Status`), and the language's word.
pub(crate) fn field_of(c: PCat) -> (&'static str, &'static str) {
    match c {
        PCat::Status => ("Status", "status"),
        PCat::Kind => ("Kind", "kind"),
        PCat::Severity => ("Severity", "severity"),
        PCat::Shortcode => ("Shortcode", "in"),
        PCat::Label => ("Label", "label"),
        PCat::Saved => ("Saved filter", "saved"),
        PCat::Flags => ("Flag", ""),
        PCat::Dates => ("Created", "created"),
    }
}

/// A rule's field as the language's word — `None` for one not ticked.
pub(crate) fn word_of(field: &str) -> Option<&'static str> {
    Some(match field {
        "Status" => "status",
        "Category" => "category",
        "Kind" => "kind",
        "Severity" => "severity",
        "Shortcode" => "in",
        "Under" => "under",
        "Label" => "label",
        _ => return None,
    })
}

/// The values a ticked field offers.
pub(crate) fn raw_values(c: PCat, codes: &[String]) -> Vec<String> {
    match c {
        PCat::Status => crate::filter::STATUS_GROUPS.iter().flat_map(|(_, ss)| ss.iter()).map(|s| s.to_string()).collect(),
        PCat::Kind => crate::filter::KINDS.iter().map(|k| k.to_string()).collect(),
        PCat::Severity => crate::filter::SEVERITIES.iter().map(|s| s.to_string()).collect(),
        PCat::Shortcode => codes.to_vec(),
        _ => Vec::new(),
    }
}

/// A value ticked or unticked on a rule: the rule's word and its values
/// after — the rule's own when it is of the field (`Under` is Shortcode's),
/// else the field's word and that value alone. Values compare ignoring case.
pub(crate) fn ticked(rule: Option<(&str, &[String])>, c: PCat, v: &str) -> (&'static str, Vec<String>) {
    let (field, word) = field_of(c);
    match rule {
        Some((f, values)) if f == field || (c == PCat::Shortcode && f == "Under") => {
            let mut out: Vec<String> = values.to_vec();
            match out.iter().position(|x| x.eq_ignore_ascii_case(v)) {
                Some(i) => {
                    out.remove(i);
                }
                None => out.push(v.to_string()),
            }
            (word_of(f).unwrap_or(word), out)
        }
        _ => (word, vec![v.to_string()]),
    }
}

/// The picker's categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PCat {
    Saved,
    Status,
    Kind,
    Severity,
    Shortcode,
    Flags,
    Label,
    /// FILTER-54: Created and Changed, fields whose value is a time.
    Dates,
}

impl PCat {
    #[cfg(test)]
    pub(crate) const ALL: [PCat; 8] = [PCat::Saved, PCat::Status, PCat::Kind, PCat::Severity, PCat::Shortcode, PCat::Flags, PCat::Label, PCat::Dates];

    /// The category a rule's field is changed in.
    pub(crate) fn of_field(field: &str) -> PCat {
        match field {
            "Saved filter" => PCat::Saved,
            "Status" | "Category" => PCat::Status,
            "Kind" => PCat::Kind,
            "Severity" => PCat::Severity,
            "Shortcode" | "Under" => PCat::Shortcode,
            "Label" => PCat::Label,
            "Created" | "Changed" => PCat::Dates,
            _ => PCat::Flags,
        }
    }
}

/// The flags the picker offers: what it says, and its clause.
pub(crate) const FLAGS: [(&str, &str); 7] = [
    ("ready", "ready"),
    ("blocked", "blocked"),
    ("has commits", "has commits"),
    ("asking", "asking"),
    ("queued", "queued"),
    ("asked", "asked"),
    ("menger doc", "menger doc"),
];

/// A category's values: what each says, and its clause's text.
pub(crate) fn values(c: PCat, codes: &[String], saved: &[String]) -> Vec<(String, String)> {
    let q = |v: &str| if v.contains(' ') { format!("\"{v}\"") } else { v.to_string() };
    match c {
        PCat::Saved => saved.iter().map(|n| (format!("\u{2317} {n}"), crate::naming::saved_text_of(n))).collect(),
        PCat::Status => crate::filter::STATUS_GROUPS.iter().flat_map(|(_, ss)| ss.iter()).map(|s| (s.to_string(), format!("status {}", q(s)))).collect(),
        PCat::Kind => crate::filter::KINDS.iter().map(|k| (k.to_string(), format!("kind {k}"))).collect(),
        PCat::Severity => crate::filter::SEVERITIES.iter().map(|s| (s.to_string(), format!("severity {s}"))).collect(),
        PCat::Shortcode => codes.iter().map(|c| (c.clone(), format!("in {c}"))).collect(),
        PCat::Flags => FLAGS.iter().map(|(w, t)| (w.to_string(), t.to_string())).collect(),
        PCat::Label => Vec::new(),
        PCat::Dates => vec![("Created".into(), "created this week".into()), ("Changed".into(), "changed this week".into())],
    }
}

/// FILTER-54: the times a rule's ◷ offers — what each says and the time
/// (`None`: any time). A date field's (`date`) has no *any time*; a status's
/// (`held`) adds how long it has been so. `today` names the reader's day,
/// for the dates of this month and the last.
pub(crate) fn time_choices(date: bool, held: bool, today: crate::calendar::Day) -> (Vec<(String, Option<Time>)>, Vec<(String, Time)>) {
    let month = crate::calendar::Day::new(today.y, today.m, 1);
    let last = month.plus_months(-1);
    let iso = |d: crate::calendar::Day| format!("{:04}-{:02}-{:02}", d.y, d.m, d.d);
    let mut became: Vec<(String, Option<Time>)> = if date { Vec::new() } else { vec![("any time".into(), None)] };
    let windows = [
        "today".to_string(),
        "this week".into(),
        "within 3d".into(),
        "within 2w".into(),
        "within 30d".into(),
        "after 2w ago".into(),
        "before 1w ago".into(),
        format!("after {}", iso(month)),
        format!("before {}", iso(month)),
        format!("between {} and {}", iso(last), iso(month.plus_days(-1))),
    ];
    became.extend(windows.into_iter().filter_map(|w| crate::when::window_of(&w).map(|win| (w, Some(Time::Became(win))))));
    let lengths = if held {
        [(true, 3, Unit::Day), (true, 1, Unit::Week), (true, 2, Unit::Week), (false, 3, Unit::Day)]
            .into_iter()
            .map(|(over, n, unit)| (format!("for {} {n}{}", if over { "over" } else { "under" }, unit.suffix()), Time::Held { over, n, unit }))
            .collect()
    } else {
        Vec::new()
    };
    (became, lengths)
}

/// A label typed, as its clause — `None` for nothing typed.
pub(crate) fn label_clause(raw: &str) -> Option<String> {
    let v = raw.trim().to_lowercase();
    (!v.is_empty() && !v.contains('"')).then(|| format!("label \"{v}\""))
}

/// One clause's text as a node (`not`: negated) — `None` when it does not read.
pub(crate) fn node_of(text: &str, not: bool) -> Option<Node> {
    let (n, _) = parts::of_text(text).ok()?;
    let mut n = match n {
        Node::Group { conj: Conj::All, not: false, mut items } if items.len() == 1 => items.remove(0),
        n => n,
    };
    if not {
        match &mut n {
            Node::Leaf(p) => p.not = !p.not,
            Node::Group { not, .. } => *not = !*not,
        }
    }
    Some(n)
}

/// FILTER-55: one field's values as a rule's node — one value its clause,
/// several a list (`any of`; negated, `none of`: each negated, joined by and).
pub(crate) fn list_node(word: &str, values: &[String], not: bool) -> Option<Node> {
    let mut items: Vec<Node> = values.iter().map(|v| node_of(&parts::clause_text(word, std::slice::from_ref(v)), not)).collect::<Option<_>>()?;
    match items.len() {
        0 => None,
        1 => items.pop(),
        _ => Some(Node::Group { conj: if not { Conj::All } else { Conj::Any }, not: false, items }),
    }
}

/// The key a card's subject is known by on the page (`data-fcard`).
pub(crate) fn key_of(s: &Subject) -> String {
    match s {
        Subject::Saved(n) => n.clone(),
        Subject::Line(l) => format!("line:{}", l.name()),
    }
}

/// A rule's values as drawn: past four, the first three and `+N` (`open`:
/// all of them, and `fewer`).
pub(crate) fn shown(values: &[String], open: bool) -> (Vec<String>, Option<String>) {
    match (values.len() > 4, open) {
        (false, _) => (values.to_vec(), None),
        (true, false) => (values[..3].to_vec(), Some(format!("+{}", values.len() - 3))),
        (true, true) => (values.to_vec(), Some("fewer".into())),
    }
}

fn names(p: &Predicate, name: &str) -> bool {
    matches!(&p.field, Field::Saved(n) if slug(n) == slug(name))
}

/// Whether `f` names the saved filter `name` — at any depth.
pub(crate) fn on_board(f: &Filter, name: &str) -> bool {
    f.leaves().into_iter().any(|p| names(p, name))
}

/// `f` with the saved filter `name` and-ed on (the switch turned on).
pub(crate) fn with_saved(f: &Filter, name: &str) -> Filter {
    match Filter::parse(&crate::naming::saved_text_of(name)) {
        Ok(s) if !on_board(f, name) => f.and(&s),
        _ => f.clone(),
    }
}

/// `f` with every clause naming `name` taken out of its groups — a group
/// left empty goes (the switch turned off).
pub(crate) fn without_saved(f: &Filter, name: &str) -> Filter {
    let groups = f
        .groups
        .iter()
        .filter_map(|g| {
            let preds: Vec<Predicate> = g.preds.iter().filter(|p| !names(p, name)).cloned().collect();
            (!preds.is_empty()).then(|| menger_tickets_api::filter::Group { conj: g.conj, preds })
        })
        .collect();
    let raw = Filter { groups, display: f.display.clone() };
    Filter::parse(&raw.format()).unwrap_or(raw)
}

/// A copy's name: `Name copy`, else `Name copy 2`, `3`… — one no listed
/// filter holds.
pub(crate) fn copy_name(listed: &[Entry], name: &str) -> String {
    let base = match name.rsplit_once(" copy") {
        Some((b, tail)) if tail.is_empty() || tail.trim().parse::<u32>().is_ok() => b,
        _ => name,
    };
    let taken = |n: &str| listed.iter().any(|e| slug(&e.name) == slug(n));
    let mut n = format!("{base} copy");
    let mut i = 1;
    while taken(&n) {
        i += 1;
        n = format!("{base} copy {i}");
    }
    n
}

/// A filter's text with its joiners marked, as the card's *as text* draws it.
pub(crate) fn marked(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    let mut quoted = false;
    for (i, word) in text.split(' ').enumerate() {
        let mark = !quoted && matches!(word.trim_start_matches('('), "and" | "or" | "not");
        let piece = if i == 0 { word.to_string() } else { format!(" {word}") };
        quoted ^= word.matches('"').count() % 2 == 1;
        match out.last_mut() {
            Some((false, s)) if !mark => s.push_str(&piece),
            _ => out.push((mark, piece)),
        }
    }
    out
}

/// Who saved it, as the card's head says: a key's principal shortened.
pub(crate) fn who_words(by: &str) -> String {
    match by.strip_prefix("key:") {
        Some(k) if k.len() > 8 => format!("key:{}\u{2026}", &k[..8]),
        _ if by.is_empty() => "the server".into(),
        _ => by.to_string(),
    }
}

// ---------------------------------------------------------------------
// The state

#[derive(Clone, Debug, PartialEq)]
struct Open {
    subject: Subject,
    level: Level,
    pinned: bool,
    /// The chip's rect: left, top, bottom.
    at: (f64, f64, f64),
}

/// FILTER-55: an open dropdown — the part it changes (its owner and path;
/// a ghost row's group), what it is, and the control it hangs from.
#[derive(Clone, Debug, PartialEq)]
struct Dd {
    owner: String,
    path: Vec<usize>,
    target: Target,
    /// The joiner of the group a new part lands in.
    conj: Conj,
    /// A ghost row's operator.
    not: bool,
    what: What,
    /// The control's box: left, top, bottom, width.
    at: (f64, f64, f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Rules,
    Nav,
}

/// One row of the nav beside the rules.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NavRow {
    id: String,
    title: String,
    kind: String,
    status: String,
    matched: bool,
    below: u64,
}

/// The card, for the app's lifetime — every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct Card {
    lens: Lens,
    open: RwSignal<Option<Open>>,
    /// Edits held, by saved filter: its name, its tree.
    drafts: RwSignal<Vec<(String, Node)>>,
    unfolded: RwSignal<HashSet<String>>,
    more: RwSignal<HashSet<String>>,
    naming: RwSignal<Option<(String, Vec<usize>)>>,
    typed: StoredValue<String>,
    /// A draft being saved under another name.
    copying: RwSignal<Option<String>>,
    note: RwSignal<Option<(bool, String)>>,
    text: RwSignal<bool>,
    menu: RwSignal<bool>,
    tab: RwSignal<Tab>,
    /// FILTER-55: the dropdown open, and its search.
    dd: RwSignal<Option<Dd>>,
    dq: RwSignal<String>,
    label: StoredValue<String>,
    last: RwSignal<Option<(String, Vec<usize>)>>,
    counts: RwSignal<HashMap<String, Option<usize>>>,
    meta: RwSignal<HashMap<String, (String, i64)>>,
    flat: RwSignal<bool>,
    folds: RwSignal<HashSet<String>>,
    groups: RwSignal<Option<Vec<menger_tickets_api::Group>>>,
    hover: StoredValue<u32>,
    busy: RwSignal<bool>,
    /// FILTER-54: the rule whose time menu is open — its owner and path.
    tm: RwSignal<Option<(String, Vec<usize>)>>,
    /// The time menu's own date: after or before, and the day.
    tm_own: StoredValue<(bool, String)>,
    /// Where the time menu hangs: its button's box (left, top, bottom).
    tm_at: StoredValue<(f64, f64, f64)>,
}

thread_local! {
    static HOOK: std::cell::Cell<Option<Card>> = const { std::cell::Cell::new(None) };
}

/// The app's card (`None` before the App made it).
pub(crate) fn hook() -> Option<Card> {
    HOOK.with(std::cell::Cell::get)
}

fn rect_of(ev: &web_sys::Event) -> (f64, f64, f64) {
    ev.current_target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .map(|el| {
            let r = el.get_bounding_client_rect();
            (r.left(), r.top(), r.bottom())
        })
        .unwrap_or((8.0, 0.0, 48.0))
}

fn inside(ev: &web_sys::Event, sel: &str) -> bool {
    ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|el| el.closest(sel).ok().flatten().is_some())
}

impl Card {
    /// The card, made once by the App; a press outside closes it, Escape
    /// closes what is innermost.
    pub(crate) fn new(lens: Lens) -> Self {
        let c = Card {
            lens,
            open: RwSignal::new(None),
            drafts: RwSignal::new(Vec::new()),
            unfolded: RwSignal::new(HashSet::new()),
            more: RwSignal::new(HashSet::new()),
            naming: RwSignal::new(None),
            typed: StoredValue::new(String::new()),
            copying: RwSignal::new(None),
            note: RwSignal::new(None),
            text: RwSignal::new(false),
            menu: RwSignal::new(false),
            tab: RwSignal::new(Tab::Rules),
            dd: RwSignal::new(None),
            dq: RwSignal::new(String::new()),
            label: StoredValue::new(String::new()),
            last: RwSignal::new(None),
            counts: RwSignal::new(HashMap::new()),
            meta: RwSignal::new(HashMap::new()),
            flat: RwSignal::new(false),
            folds: RwSignal::new(HashSet::new()),
            groups: RwSignal::new(None),
            hover: StoredValue::new(0),
            busy: RwSignal::new(false),
            tm: RwSignal::new(None),
            tm_own: StoredValue::new((true, String::new())),
            tm_at: StoredValue::new((0.0, 0.0, 0.0)),
        };
        window_event_listener(leptos::ev::pointerdown, move |ev: web_sys::PointerEvent| {
            if c.open.with_untracked(Option::is_none) {
                return;
            }
            let ev: &web_sys::Event = ev.as_ref();
            if c.menu.get_untracked() && !inside(ev, ".fc-menu, .fc-mb") {
                c.menu.set(false);
            }
            if c.tm.with_untracked(Option::is_some) && !inside(ev, ".fc-tm, [data-fc=\"time\"], [data-fc=\"time-add\"]") {
                c.tm.set(None);
            }
            if c.dd.with_untracked(Option::is_some) && !inside(ev, drop::INSIDE) {
                c.dd.set(None);
            }
            if !inside(ev, ".fcard, [data-fcard]") {
                c.close();
            }
        });
        window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if c.open.with_untracked(Option::is_none) || ev.default_prevented() || !bindings::is(&ev, Context::Global, "app.escape") {
                return;
            }
            ev.prevent_default();
            ev.stop_propagation();
            if c.tm.with_untracked(Option::is_some) {
                c.tm.set(None);
            } else if c.dd.with_untracked(Option::is_some) {
                c.dd.set(None);
            } else if c.menu.get_untracked() {
                c.menu.set(false);
            } else {
                c.close();
            }
        });
        // A saved filter written elsewhere: its text and counts read again.
        session::listen(move |ev| {
            if matches!(ev, TicketsEvent::FilterSaved { .. } | TicketsEvent::FilterDeleted { .. }) {
                c.counts.try_set(HashMap::new());
            }
        });
        HOOK.with(|h| h.set(Some(c)));
        c
    }

    /// The card opened on `subject` at `level` (`pinned`: by a press).
    fn show(self, subject: Subject, level: Level, pinned: bool, at: (f64, f64, f64)) {
        let same = self.open.with_untracked(|o| o.as_ref().is_some_and(|o| o.subject == subject));
        if !same {
            self.menu.set(false);
            self.dd.set(None);
            self.naming.set(None);
            self.copying.set(None);
            self.tab.set(Tab::Rules);
            self.note.set(None);
        }
        if let Subject::Saved(name) = &subject {
            self.read_meta(name);
        }
        self.read_groups();
        self.open.set(Some(Open { subject, level, pinned, at }));
    }

    /// Closed.
    pub(crate) fn close(self) {
        self.hover.update_value(|n| *n += 1);
        self.open.set(None);
        self.dd.set(None);
        self.menu.set(false);
        self.naming.set(None);
        self.copying.set(None);
        self.tm.set(None);
    }

    /// A press on a chip: pinned open, or — pinned already on it — closed.
    pub(crate) fn press(self, subject: Subject, level: Level, ev: &web_sys::Event) {
        // A peek still pending from the hover before this press would
        // reopen it unpinned.
        self.stay();
        let pinned_here = self.open.with_untracked(|o| o.as_ref().is_some_and(|o| o.pinned && o.subject == subject));
        if pinned_here {
            self.close();
        } else {
            self.show(subject, level, true, rect_of(ev));
        }
    }

    /// A pointer over a chip: open after a pause (never on a phone, never
    /// over a pinned card).
    pub(crate) fn hover(self, subject: Subject, level: Level, ev: &web_sys::Event) {
        if self.lens.phone_untracked() || self.open.with_untracked(|o| o.as_ref().is_some_and(|o| o.pinned)) {
            return;
        }
        let at = rect_of(ev);
        let n = self.hover.get_value() + 1;
        self.hover.set_value(n);
        spawn_local(async move {
            dom::sleep_ms(HOVER_MS).await;
            if self.hover.try_get_value() == Some(n) {
                self.show(subject, level, false, at);
            }
        });
    }

    /// The pointer left a chip or the card: an unpinned card closes after a
    /// moment, unless the pointer came back.
    pub(crate) fn leave(self) {
        let n = self.hover.get_value() + 1;
        self.hover.set_value(n);
        spawn_local(async move {
            dom::sleep_ms(LEAVE_MS).await;
            if self.hover.try_get_value() == Some(n) && self.open.try_with_untracked(|o| o.as_ref().is_some_and(|o| !o.pinned)) == Some(true) {
                self.close();
            }
        });
    }

    fn stay(self) {
        self.hover.update_value(|n| *n += 1);
    }

    /// Whether the card is open on `subject`. Tracked.
    pub(crate) fn is_on(self, subject: &Subject) -> bool {
        self.open.with(|o| o.as_ref().is_some_and(|o| o.subject == *subject))
    }

    fn read_meta(self, name: &str) {
        let key = slug(name);
        if self.meta.with_untracked(|m| m.contains_key(&key)) {
            return;
        }
        let name = name.to_string();
        spawn_local(async move {
            if let Ok(Some(row)) = saved::fetch_row(&name).await {
                self.meta.try_update(|m| {
                    m.insert(key, (row.saved_by, row.saved_at));
                });
            }
        });
    }

    fn read_groups(self) {
        if self.groups.with_untracked(Option::is_some) {
            return;
        }
        self.groups.set(Some(Vec::new()));
        let Some(client) = session::client() else { return };
        spawn_local(async move {
            let Ok(events) = client.nav().groups_all().await else { return };
            let all = events
                .into_iter()
                .filter_map(|ev| match ev {
                    TicketsEvent::Group { group } => Some(group),
                    _ => None,
                })
                .filter(menger_tickets_api::Group::drawn)
                .collect();
            self.groups.try_set(Some(all));
        });
    }

    // -- trees

    fn draft(self, name: &str) -> Option<Node> {
        self.drafts.with(|d| d.iter().find(|(k, _)| slug(k) == slug(name)).map(|(_, n)| n.clone()))
    }

    fn edited(self, name: &str) -> bool {
        self.drafts.with(|d| d.iter().any(|(k, _)| slug(k) == slug(name)))
    }

    /// `name`'s saved tree, as fetched. Tracked.
    fn saved_got(self, name: &str) -> Got {
        let Some(naming) = crate::naming::hook() else { return Got::Unknown };
        match naming.text(name) {
            None | Some(crate::naming::Fetch::Loading) => Got::Unknown,
            Some(crate::naming::Fetch::Missing) => Got::Missing,
            Some(crate::naming::Fetch::Failed(why)) => Got::Bad(why),
            Some(crate::naming::Fetch::Text(t)) => match parts::of_text(&t) {
                Ok((n, _)) => Got::Node(n),
                Err(e) => Got::Bad(e),
            },
        }
    }

    /// What the parts read for `name`: an edit held, else its saved tree.
    fn lookup(self, name: &str) -> Got {
        if let Some(l) = line_of(name) {
            return Got::Node(self.lens.filter(l).with(parts::of_filter));
        }
        match self.draft(name) {
            Some(n) => Got::Node(n),
            None => self.saved_got(name),
        }
    }

    fn owner_node(self, owner: &str) -> Option<Node> {
        match self.lookup(owner) {
            Got::Node(n) => Some(n),
            _ => None,
        }
    }

    /// The subject as a tree: a saved filter's name, or ⌂'s own.
    fn top(self, s: &Subject) -> Node {
        match s {
            Subject::Saved(n) => Node::Leaf(Predicate::is(Field::Saved(n.clone()))),
            Subject::Line(l) => self.lens.filter(*l).with(parts::of_filter),
        }
    }

    fn resolved(self, s: &Subject) -> Part {
        match s {
            Subject::Saved(n) => parts::resolve(n, &|m| self.lookup(m)),
            Subject::Line(l) => parts::resolve_node(&line_owner(*l), &self.top(s), &|m| self.lookup(m)),
        }
    }

    /// A tree as the wire's text — every draft inlined, within ⌂ (but for
    /// ⌂'s own card) and the organization.
    fn wire(self, s: &Subject, n: &Node, swap: Option<(&str, &Node)>) -> String {
        let drafted = |name: &str| match swap {
            Some((o, d)) if slug(o) == slug(name) => Some(d.clone()),
            _ => self.draft(name),
        };
        // A line's own tree swapped is the tree itself.
        let n = match (s, swap) {
            (Subject::Line(l), Some((o, d))) if o == line_owner(*l) => d,
            _ => n,
        };
        let own = parts::to_filter(&parts::inlined(n, &drafted), &Display::default());
        let eff = if within_home(s) { self.lens.filter(Level::Global).with(|g| g.and(&own)) } else { own };
        let eff = self.lens.org().with(|o| crate::orgs::within(o.as_deref(), &eff));
        crate::counts::wire_text(&eff)
    }

    /// How many tickets `text` admits — asked once. Tracked.
    fn count(self, text: String) -> Option<usize> {
        if let Some(n) = self.counts.with(|m| m.get(&text).copied()) {
            return n;
        }
        self.counts.update_untracked(|m| {
            m.insert(text.clone(), None);
        });
        spawn_local(async move {
            if let Some(n) = crate::counts::count_once(text.clone()).await {
                self.counts.try_update(|m| {
                    m.insert(text, Some(n));
                });
            }
        });
        None
    }

    // -- edits

    /// `owner`'s tree changed by `f` — held as a draft (none when it is back
    /// to its saved text); `at` the part to outline.
    fn edit(self, owner: &str, at: Vec<usize>, f: impl FnOnce(&mut Node) -> bool) {
        let Some(mut n) = untrack(|| self.owner_node(owner)) else { return };
        if !f(&mut n) {
            return;
        }
        // FILTER-55: a line's own tree is its level's filter, at once.
        if let Some(l) = line_of(owner) {
            let display = self.lens.filter(l).with_untracked(|f| f.display.clone());
            self.lens.set(l, parts::to_filter(&n, &display));
            self.last.set(Some((owner.to_string(), at)));
            self.note.set(None);
            return;
        }
        let saved = untrack(|| match self.saved_got(owner) {
            Got::Node(s) => Some(s),
            _ => None,
        });
        let canon = |x: &Node| parts::to_filter(x, &Display::default()).format();
        let back = saved.as_ref().is_some_and(|s| canon(s) == canon(&n));
        self.drafts.update(|d| {
            d.retain(|(k, _)| slug(k) != slug(owner));
            if !back {
                d.push((owner.to_string(), n));
            }
        });
        self.last.set(Some((owner.to_string(), at)));
        self.note.set(None);
    }

    fn undo(self, name: &str) {
        self.drafts.update(|d| d.retain(|(k, _)| slug(k) != slug(name)));
        self.last.set(None);
    }

    fn forget_text(self, name: &str) {
        if let Some(n) = crate::naming::hook() {
            n.refetch(name);
        }
        self.counts.set(HashMap::new());
        self.meta.update(|m| {
            m.remove(&slug(name));
        });
    }

    /// The draft of `name` saved back under its name, for everyone.
    fn save_back(self, name: String) {
        let Some(n) = untrack(|| self.draft(&name)) else { return };
        let f = parts::to_filter(&n, &Display::default());
        self.busy.set(true);
        spawn_local(async move {
            let got = saved::save(f, name.clone(), true).await;
            self.busy.try_set(false);
            match got {
                Ok((_, list)) => {
                    if let Some(s) = saved::hook() {
                        s.set_list(list);
                    }
                    self.undo(&name);
                    self.forget_text(&name);
                    self.note.try_set(Some((false, format!("\u{2317} {name} saved"))));
                }
                Err(e) => {
                    self.note.try_set(Some((true, failure_words(&name, e))));
                }
            }
        });
    }

    /// The draft of `name` (or, with none, its saved tree) saved as a new
    /// filter `to`; a filter that named it names the copy (the card's own
    /// subject: the card opens on the copy).
    fn save_as(self, name: String, to: String) {
        let Some(n) = untrack(|| self.owner_node(&name)) else { return };
        let f = parts::to_filter(&n, &Display::default());
        self.busy.set(true);
        spawn_local(async move {
            let got = saved::save(f, to.clone(), false).await;
            self.busy.try_set(false);
            match got {
                Ok((entry, list)) => {
                    if let Some(s) = saved::hook() {
                        s.set_list(list);
                    }
                    self.copying.try_set(None);
                    self.undo(&name);
                    let open = self.open.get_untracked();
                    match open.as_ref().map(|o| &o.subject) {
                        Some(Subject::Saved(top)) if slug(top) == slug(&name) => {
                            if let Some(o) = open.clone() {
                                self.open.set(Some(Open { subject: Subject::Saved(entry.name.clone()), ..o }));
                            }
                        }
                        Some(s) => {
                            let s = s.clone();
                            let holder = untrack(|| holder_of(&self.resolved(&s), &name));
                            if let Some((owner, path)) = holder {
                                let to = Node::Leaf(Predicate::is(Field::Saved(entry.name.clone())));
                                self.edit(&owner, path.clone(), |t| parts::replace(t, &path, to));
                            }
                        }
                        None => {}
                    }
                    self.note.try_set(Some((false, format!("saved as \u{2317} {}", entry.name))));
                }
                Err(e) => {
                    self.note.try_set(Some((true, failure_words(&to, e))));
                }
            }
        });
    }

    /// The group at `path` in `owner` saved as the filter `name`, its place
    /// that filter's row. A name a saved filter holds is refused.
    fn name_group(self, owner: String, path: Vec<usize>, raw: String) {
        let name = saved::clean_name(&raw);
        if name.is_empty() {
            self.naming.set(None);
            return;
        }
        let listed = saved::hook().map(|s| s.listed_untracked()).unwrap_or_default();
        if saved::taken_by(&listed, &name).is_some() {
            self.note.set(Some((true, format!("\u{2317} {name} is a saved filter already \u{2014} pick another name"))));
            return;
        }
        let Some(group) = untrack(|| self.owner_node(&owner)).and_then(|t| parts::get(&t, &path).cloned()) else { return };
        let f = parts::to_filter(&group, &Display::default());
        self.busy.set(true);
        spawn_local(async move {
            let got = saved::save(f, name.clone(), false).await;
            self.busy.try_set(false);
            match got {
                Ok((entry, list)) => {
                    if let Some(s) = saved::hook() {
                        s.set_list(list);
                    }
                    self.naming.try_set(None);
                    let to = Node::Leaf(Predicate::is(Field::Saved(entry.name.clone())));
                    self.edit(&owner, path.clone(), |t| parts::replace(t, &path, to));
                    self.note.try_set(Some((false, format!("saved as \u{2317} {} \u{2014} {owner} names it now; save back to keep that", entry.name))));
                }
                Err(e) => {
                    self.note.try_set(Some((true, failure_words(&name, e))));
                }
            }
        });
    }

    /// The switch: the saved filter and-ed onto the level's filter, or off it.
    fn switch(self, level: Level, name: &str) {
        let f = self.lens.filter(level).get_untracked();
        let next = if on_board(&f, name) { without_saved(&f, name) } else { with_saved(&f, name) };
        self.lens.set(level, next);
    }

    /// FILTER-55: `owner`'s tree with `text`'s clause put where the dropdown
    /// says — the rule at its path changed (its time kept), or a new part at
    /// its group's end (in a new inner group) — and the new part's path.
    fn placed(self, d: &Dd, text: &str) -> Option<(Node, Vec<usize>)> {
        self.placed_node(d, node_of(text, d.not)?)
    }

    fn placed_node(self, d: &Dd, node: Node) -> Option<(Node, Vec<usize>)> {
        let mut t = self.owner_node(&d.owner)?;
        match d.target {
            Target::At => keep_time(&mut t, &d.path, node).then(|| (t, d.path.clone())),
            Target::Local(_) => {
                let at = parts::add(&mut t, &d.path, node)?;
                Some((t, at))
            }
            Target::Add { group } => {
                let item = if group { Node::Group { conj: parts::other(d.conj), not: false, items: vec![node] } } else { node };
                let at = parts::add(&mut t, &d.path, item)?;
                let at = if group { at.into_iter().chain([0]).collect() } else { at };
                Some((t, at))
            }
        }
    }

    /// FILTER-55: a value ticked on the dropdown's rule — the owner's tree
    /// after and the rule's path (`None`: the rule is gone, its last value
    /// unticked).
    fn ticked_tree(self, d: &Dd, c: PCat, v: &str) -> Option<(Node, Option<Vec<usize>>)> {
        if d.target != Target::At {
            let (word, vals) = ticked(None, c, v);
            return self.placed_node(d, list_node(word, &vals, d.not)?).map(|(t, at)| (t, Some(at)));
        }
        let t = self.owner_node(&d.owner)?;
        let rule = parts::get(&t, &d.path).and_then(parts::rule_at);
        let (word, vals) = ticked(rule.as_ref().map(|r| (r.field.as_str(), r.values.as_slice())), c, v);
        if vals.is_empty() {
            let mut t = t;
            return parts::remove(&mut t, &d.path).then_some((t, None));
        }
        let not = rule.as_ref().map_or(d.not, |r| r.not);
        let node = list_node(word, &vals, not)?;
        let mut t = t;
        keep_time(&mut t, &d.path, node).then_some((t, Some(d.path.clone())))
    }

    /// A surface's own rule whose field is being changed: taken out as the
    /// choice takes its place.
    fn unswap(self, d: &Dd, keep: Option<Cat>) {
        if let (Target::Local(c), Some(l)) = (&d.target, line_of(&d.owner)) {
            if keep != Some(*c) {
                let c = *c;
                self.lens.edit(l, |k| k.remove(c));
            }
        }
    }

    /// A choice that is the whole part (a flag, a date, a saved filter):
    /// put in, the dropdown closed.
    fn choose(self, d: &Dd, text: &str) {
        if let Some((t, at)) = untrack(|| self.placed(d, text)) {
            self.unswap(d, None);
            self.edit(&d.owner, at, |n| {
                *n = t;
                true
            });
        }
        self.dd.set(None);
    }

    /// A value ticked: the rule follows at once; a ghost row becomes the rule.
    fn tick(self, d: &Dd, c: PCat, v: &str) {
        let Some((t, at)) = untrack(|| self.ticked_tree(d, c, v)) else { return };
        self.unswap(d, None);
        self.edit(&d.owner, at.clone().unwrap_or_default(), |n| {
            *n = t;
            true
        });
        match at {
            None => self.dd.set(None),
            Some(at) => self.dd.update(|o| {
                if let Some(o) = o {
                    o.target = Target::At;
                    o.path = at;
                }
            }),
        }
    }

    /// FILTER-55: a surface's own value ticked (the wave board's Wave, the
    /// inputs' State) — its clause beside the rules, at once; a ghost row's
    /// first tick takes its operator, and the row is the rule after.
    fn tick_local(self, d: &Dd, c: Cat, v: &str) {
        let Some(l) = line_of(&d.owner) else { return };
        self.unswap(d, Some(c));
        let op = if d.not { Op::Not } else { Op::Is };
        self.lens.edit(l, |k| k.edit_as(c, op, |k| k.toggle(c, v)));
        self.at_local(c);
    }

    /// FILTER-55: a surface's own Title set to `text` (empty: no rule).
    fn title_local(self, d: &Dd, text: &str) {
        let Some(l) = line_of(&d.owner) else { return };
        self.unswap(d, Some(Cat::Title));
        let not = d.not;
        let text = text.trim().to_string();
        self.lens.edit(l, |k| {
            let op = k.clause(Cat::Title).map_or(if not { Op::Not } else { Op::Is }, |cl| cl.op);
            k.set(Cat::Title, op, (!text.is_empty()).then_some(text));
        });
        self.dd.set(None);
    }

    /// The dropdown's rule is now the surface's own `c`.
    fn at_local(self, c: Cat) {
        self.dd.update(|o| {
            if let Some(o) = o {
                o.target = Target::At;
                o.path = Vec::new();
                o.what = What::Local(c);
            }
        });
    }

    /// The operator chosen: the rule negated when it differs (a ghost row:
    /// what its first choice will be).
    fn set_not(self, d: &Dd, not: bool) {
        if d.target == Target::At {
            let now = untrack(|| self.owner_node(&d.owner)).and_then(|t| parts::get(&t, &d.path).map(|n| match n {
                Node::Leaf(p) => p.not,
                g => parts::list_of(g).is_some_and(|(_, n, _)| n),
            }));
            if now.is_some_and(|n| n != not) {
                let at = d.path.clone();
                self.edit(&d.owner, at.clone(), |t| parts::negate(t, &at));
            }
            self.dd.set(None);
        } else {
            self.dd.update(|o| {
                if let Some(o) = o {
                    o.not = not;
                    o.what = What::Field;
                }
            });
        }
    }

    /// What the card's filter would admit with `t` as `owner`'s tree — the
    /// wire text.
    fn would_tree(self, s: &Subject, owner: &str, t: &Node) -> String {
        self.wire(s, &self.top(s), Some((owner, t)))
    }

    /// A dropdown opened from `ev`'s control.
    #[allow(clippy::too_many_arguments)]
    fn open_dd(self, owner: String, path: Vec<usize>, target: Target, conj: Conj, not: bool, what: What, ev: &web_sys::Event) {
        let (l, t, b) = rect_of(ev);
        let w = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).map_or(180.0, |e| e.get_bounding_client_rect().width());
        self.set_dd(owner, path, target, conj, not, what, (l, t, b, w));
    }

    #[allow(clippy::too_many_arguments)]
    fn set_dd(self, owner: String, path: Vec<usize>, target: Target, conj: Conj, not: bool, what: What, at: (f64, f64, f64, f64)) {
        self.tm.set(None);
        self.dq.set(String::new());
        self.label.set_value(String::new());
        let ghost = matches!(target, Target::Add { .. });
        self.dd.set(Some(Dd { owner, path, target, conj, not, what, at }));
        if ghost {
            self.ghost_at();
        }
    }

    /// The dropdown hung from the ghost row, once that is drawn.
    fn ghost_at(self) {
        {
            spawn_local(async move {
                dom::sleep_ms(30).await;
                let Some(el) = dom::window().document().and_then(|d| d.query_selector(".fcard .fc-rule.ghost").ok().flatten()) else { return };
                let r = el.get_bounding_client_rect();
                self.dd.try_update(|o| {
                    if let Some(o) = o {
                        o.at = (r.left(), r.top(), r.bottom(), r.width().min(320.0));
                    }
                });
            });
        }
    }
}

impl Card {
    /// FILTER-55: the builder on `level`'s own filter, hung from its Filter
    /// button (⌂'s: its chip) — an empty filter opens on a new rule's fields
    /// (`add`: always).
    pub(crate) fn open_line(self, level: Level, add: bool) {
        self.stay();
        let doc = dom::window().document();
        let at = doc
            .and_then(|d| {
                let key = key_of(&Subject::Line(level));
                d.query_selector(&format!("[data-fbtn=\"{}\"]:not([hidden]), [data-fcard=\"{key}\"]", level.name())).ok().flatten()
            })
            .map(|el| {
                let r = el.get_bounding_client_rect();
                (r.left(), r.top(), r.bottom())
            })
            .unwrap_or((12.0, 0.0, 56.0));
        self.show(Subject::Line(level), level, true, at);
        self.first_rule(level, add);
    }

    /// The builder on `level`'s filter, its name field open (naming's
    /// *Choose another name*).
    pub(crate) fn name_line(self, level: Level) {
        self.open_line(level, false);
        self.dd.set(None);
        self.copying.set(Some(line_owner(level)));
    }

    /// Open on the line, or — open on it already — closed (the `f` key).
    pub(crate) fn toggle_line(self, level: Level) {
        if self.open.with_untracked(|o| o.as_ref().is_some_and(|o| o.subject == Subject::Line(level))) {
            self.close();
        } else {
            self.open_line(level, false);
        }
    }

    /// A press on the line's Filter button or its name: the builder pinned
    /// open (`unfold`: that saved filter's row opened in place), or — open
    /// already and nothing to unfold — closed.
    pub(crate) fn press_line(self, level: Level, ev: &web_sys::Event, unfold: Option<String>) {
        self.stay();
        let s = Subject::Line(level);
        let here = self.open.with_untracked(|o| o.as_ref().is_some_and(|o| o.subject == s));
        if here && unfold.is_none() {
            self.close();
            return;
        }
        if let Some(n) = unfold {
            self.unfolded.update(|u| {
                u.insert(format!("{}/{:?}/{}", line_owner(level), vec![0usize], slug(&n)));
            });
        }
        self.show(s, level, true, rect_of(ev));
        self.first_rule(level, false);
    }

    fn first_rule(self, level: Level, add: bool) {
        let empty = self.lens.filter(level).with_untracked(|f| f.groups.is_empty());
        if add || empty {
            let conj = match self.top(&Subject::Line(level)) {
                Node::Group { conj, .. } => conj,
                _ => Conj::All,
            };
            self.set_dd(line_owner(level), Vec::new(), Target::Add { group: false }, conj, false, What::Field, (12.0, 0.0, 56.0, 260.0));
            self.ghost_at();
        }
    }
}

/// The node at `path` replaced by `with`, keeping the time it had when
/// `with` takes one (a value changed keeps *became so within 3d*).
fn keep_time(t: &mut Node, path: &[usize], with: Node) -> bool {
    let had = parts::get(t, path).and_then(parts::time_of);
    if !parts::replace(t, path, with) {
        return false;
    }
    if let Some(time) = had {
        parts::set_time(t, path, Some(time));
    }
    true
}

impl Card {
    /// FILTER-54: the rule at `owner`/`path` with time `t` (`None`: off).
    fn set_time(self, owner: &str, path: Vec<usize>, t: Option<Time>) {
        let at = path.clone();
        self.edit(owner, path, |n| parts::set_time(n, &at, t));
        self.tm.set(None);
    }

    /// What the card's filter would admit with that time — the wire text.
    fn would_time(self, s: &Subject, owner: &str, path: &[usize], t: Option<Time>) -> Option<String> {
        let mut n = self.owner_node(owner)?;
        parts::set_time(&mut n, path, t).then(|| self.wire(s, &self.top(s), Some((owner, &n))))
    }
}

fn failure_words(name: &str, e: saved::Failure) -> String {
    match e {
        saved::Failure::Refused(r) => format!("\u{2317} {name} not saved: {}", r.words()),
        saved::Failure::Failed(why) => format!("\u{2317} {name} not saved: {why}"),
    }
}

/// The part that names `name` from another filter: its owner and path.
fn holder_of(p: &Part, name: &str) -> Option<(String, Vec<usize>)> {
    match &p.shape {
        Shape::Named { name: n, inner, .. } => {
            if slug(n) == slug(name) && !p.owner.is_empty() {
                return Some((p.owner.clone(), p.path.clone()));
            }
            match inner {
                Inner::Parts(q) => holder_of(q, name),
                _ => None,
            }
        }
        Shape::Group { items, .. } => items.iter().find_map(|i| holder_of(i, name)),
        Shape::Rule(_) => None,
    }
}

// ---------------------------------------------------------------------
// The chips' side

/// A saved chip's hover, leave and press, for `on:` handlers.
pub(crate) fn chip_on(name: &str, level: Level) -> (impl Fn(web_sys::PointerEvent) + Clone, impl Fn(web_sys::PointerEvent) + Clone, impl Fn(web_sys::MouseEvent) + Clone) {
    let card = hook();
    let (a, b) = (name.to_string(), name.to_string());
    let subject = |n: &str| if n == HOME { Subject::Line(Level::Global) } else { Subject::Saved(n.to_string()) };
    (
        move |ev: web_sys::PointerEvent| {
            if let Some(c) = card {
                c.hover(subject(&a), level, ev.as_ref());
            }
        },
        move |_| {
            if let Some(c) = card {
                c.leave();
            }
        },
        move |ev: web_sys::MouseEvent| {
            if let Some(c) = card {
                ev.stop_propagation();
                c.press(subject(&b), level, ev.as_ref());
            }
        },
    )
}

/// How many rules a saved filter holds (`4 rules`), once its texts are in.
/// Tracked.
pub(crate) fn rules_words(name: &str) -> Option<String> {
    let c = hook()?;
    let p = parts::resolve(name, &|m| c.lookup(m));
    let missing = parts::unknown(&p);
    if !missing.is_empty() {
        if let Some(n) = crate::naming::hook() {
            untrack(|| missing.iter().for_each(|m| n.fetch(m)));
        }
        return None;
    }
    match &p.shape {
        Shape::Named { inner: Inner::Parts(top), .. } => {
            let n = top.rules();
            Some(format!("{n} rule{}", if n == 1 { "" } else { "s" }))
        }
        _ => None,
    }
}

/// Whether the card holds an edit of `name`. Tracked.
pub(crate) fn is_edited(name: &str) -> bool {
    hook().is_some_and(|c| c.edited(name))
}

// ---------------------------------------------------------------------
// The view

/// The card, over the page — drawn once by the app.
#[component]
pub(crate) fn CardHost() -> impl IntoView {
    let Some(card) = hook() else { return ().into_any() };
    let shown = Memo::new(move |_| card.open.with(|o| o.as_ref().map(|o| (o.subject.clone(), o.level))));
    // Every name the open card reaches is fetched, at any depth.
    Effect::new(move |_| {
        let Some((s, _)) = shown.get() else { return };
        let missing = parts::unknown(&card.resolved(&s));
        if let Some(n) = crate::naming::hook() {
            untrack(|| missing.iter().for_each(|m| n.fetch(m)));
        }
    });
    view! {
        {move || {
            let phone = card.lens.phone();
            (shown.get().is_some() && phone).then(|| view! { <div class="fc-scrim" on:click=move |_| card.close()></div> })
        }}
        {move || shown.get().map(|(s, level)| card_view(card, s, level))}
    }
    .into_any()
}

fn place(at: (f64, f64, f64), phone: bool) -> String {
    if phone {
        return String::new();
    }
    let win = dom::window();
    let vw = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1280.0);
    let vh = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let w = (vw - 24.0).min(980.0);
    let left = at.0.clamp(12.0, (vw - w - 12.0).max(12.0));
    let below = vh - at.2 - 18.0;
    let (top, h) = if below >= 320.0 || below >= at.1 - 18.0 { (at.2 + 6.0, below) } else { ((at.1 - 6.0 - (at.1 - 18.0).min(640.0)).max(12.0), (at.1 - 18.0).min(640.0)) };
    format!("left:{left:.0}px;top:{top:.0}px;width:{w:.0}px;max-height:{:.0}px", h.min(680.0))
}

fn card_view(card: Card, s: Subject, level: Level) -> AnyView {
    let lens = card.lens;
    let name = key_of(&s);
    let ro = {
        let (s, name) = (s.clone(), name.clone());
        Memo::new(move |_| match &s {
            Subject::Line(_) => false,
            Subject::Saved(_) => !lens.filter(level).with(|f| on_board(f, &name)),
        })
    };
    let phone = lens.phone_untracked();
    let style = move || card.open.with(|o| o.as_ref().map(|o| place(o.at, phone)).unwrap_or_default());
    let head = head_view(card, s.clone(), level, ro);
    let rules = rules_view(card, s.clone(), ro);
    let nav = nav_section(card, s.clone());
    let n_rules = {
        let s = s.clone();
        move || match &s {
            Subject::Line(l) => line_rules(lens, *l),
            _ => match &card.resolved(&s).shape {
                Shape::Named { inner: Inner::Parts(p), .. } => p.rules(),
                Shape::Group { .. } => card.resolved(&s).rules(),
                _ => 0,
            },
        }
    };
    let n_nav = {
        let s = s.clone();
        move || card.count(card.wire(&s, &card.top(&s), None)).map(|n| n.to_string()).unwrap_or_else(|| "\u{2026}".into())
    };
    view! {
        <div
            class="fcard"
            class:sheet=phone
            role="dialog"
            aria-label=match &s { Subject::Line(l) => line_words(*l).to_string(), _ => format!("what {name} is made of") }
            data-fcard-open=name.clone()
            data-ro=move || ro.get().to_string()
            style=style
            on:pointerenter=move |_| card.stay()
            on:pointerleave=move |_| {
                if !phone && card.dd.with_untracked(Option::is_none) && card.tm.with_untracked(Option::is_none) {
                    card.leave();
                }
            }
        >
            {head}
            <div class="fc-tabs" role="tablist">
                <button type="button" role="tab" data-tab="rules" aria-selected=move || (card.tab.get() == Tab::Rules).to_string() on:click=move |_| card.tab.set(Tab::Rules)>
                    "Rules \u{00b7} "{n_rules.clone()}
                </button>
                <button type="button" role="tab" data-tab="nav" aria-selected=move || (card.tab.get() == Tab::Nav).to_string() on:click=move |_| card.tab.set(Tab::Nav)>
                    "Nav \u{00b7} "{n_nav.clone()}
                </button>
            </div>
            <div class="fc-main" data-tab=move || if card.tab.get() == Tab::Rules { "rules" } else { "nav" }>
                {rules}
                {nav}
            </div>
            {drop::view(card, s.clone())}
        </div>
    }
    .into_any()
}

fn head_view(card: Card, s: Subject, level: Level, ro: Memo<bool>) -> AnyView {
    let close = view! { <button type="button" class="fc-x" aria-label="close" title="close (Esc)" on:click=move |_| card.close()>"\u{d7}"</button> };
    if let Subject::Line(l) = s {
        return line_head(card, l, close.into_any());
    }
    let Subject::Saved(name) = s else { return ().into_any() };
    let who = {
        let name = name.clone();
        move || {
            card.meta.with(|m| m.get(&slug(&name)).cloned()).map(|(by, at)| {
                if at == 0 {
                    "the default \u{2014} not saved yet".to_string()
                } else {
                    format!("saved by {} \u{00b7} {}", who_words(&by), crate::queue::ago(crate::now_unix(), at))
                }
            })
        }
    };
    let n2 = name.clone();
    let n3 = name.clone();
    let n4 = name.clone();
    let edited = move || card.edited(&n4).then(|| view! { <i class="fc-ed">"edited"</i> });
    view! {
        <div class="fc-head">
            <span class="fc-nm">"\u{2317} "{name.clone()}</span>
            {edited}
            <span class="fc-who">{who}</span>
            <span class="fc-sp"></span>
            <button
                type="button"
                class="fc-hb fc-sw"
                role="switch"
                data-fc-switch=""
                aria-checked=move || (!ro.get()).to_string()
                title=move || if ro.get() { "add it to this board's filter" } else { "take it off this board" }
                on:click=move |_| card.switch(level, &n2)
            >
                <span class="fc-swk" aria-hidden="true"></span>
                "on this board"
            </button>
            <span class="fc-mw">
                <button type="button" class="fc-hb fc-mb" aria-expanded=move || card.menu.get().to_string() on:click=move |_| card.menu.update(|m| *m = !*m)>"More \u{2304}"</button>
                {move || card.menu.get().then(|| {
                    let n3 = n3.clone();
                    view! {
                        <div class="fc-menu" role="menu">
                            <button type="button" role="menuitem" data-fc="text" on:click=move |_| {
                                card.text.update(|t| *t = !*t);
                                card.menu.set(false);
                            }>{move || if card.text.get() { "Hide as text" } else { "Show as text" }}</button>
                            <button type="button" role="menuitem" data-fc="copy" on:click=move |_| {
                                card.menu.set(false);
                                let listed = saved::hook().map(|s| s.listed_untracked()).unwrap_or_default();
                                card.save_as(n3.clone(), copy_name(&listed, &n3));
                            }>"Save a copy\u{2026}"</button>
                            <button type="button" role="menuitem" data-fc="pin" on:click=move |_| {
                                card.open.update(|o| {
                                    if let Some(o) = o {
                                        o.pinned = !o.pinned;
                                    }
                                });
                                card.menu.set(false);
                            }>{move || if card.open.with(|o| o.as_ref().is_some_and(|o| o.pinned)) { "Unpin" } else { "Keep open" }}</button>
                        </div>
                    }
                })}
            </span>
            {close}
        </div>
    }
    .into_any()
}

/// FILTER-55: a line's builder's head — whose filter, its rules and what
/// it admits, its name when saved; **More ⌄**: as text, Save as filter… (or
/// rename it), Reset, Keep open.
fn line_head(card: Card, level: Level, close: AnyView) -> AnyView {
    let lens = card.lens;
    let naming = crate::naming::hook();
    let bound = move || naming.and_then(|n| n.bound(level)).map(|b| b.name);
    let s = Subject::Line(level);
    let who = move || {
        let n = line_rules(lens, level);
        let m = card.count(card.wire(&s, &card.top(&s), None)).map_or("\u{2026}".to_string(), |m| m.to_string());
        format!("{n} rule{} \u{00b7} {m} match", if n == 1 { "" } else { "s" })
    };
    let owner = line_owner(level);
    view! {
        <div class="fc-head">
            <span class="fc-nm">{line_words(level)}</span>
            {move || bound().map(|b| view! { <span class="fc-tok sv" title="saved under this name \u{2014} every edit keeps it saved">"\u{2317} "{b}</span> })}
            <span class="fc-who">{who}</span>
            <span class="fc-sp"></span>
            <span class="fc-mw">
                <button type="button" class="fc-hb fc-mb" aria-expanded=move || card.menu.get().to_string() on:click=move |_| card.menu.update(|m| *m = !*m)>"More \u{2304}"</button>
                {move || card.menu.get().then(|| {
                    let owner = owner.clone();
                    view! {
                        <div class="fc-menu" role="menu">
                            <button type="button" role="menuitem" data-fc="text" on:click=move |_| {
                                card.text.update(|t| *t = !*t);
                                card.menu.set(false);
                            }>{move || if card.text.get() { "Hide as text" } else { "Show as text" }}</button>
                            <button type="button" role="menuitem" data-fc="save-line" on:click=move |_| {
                                card.menu.set(false);
                                card.copying.set(Some(owner.clone()));
                            }>{move || if bound().is_some() { "Rename\u{2026}" } else { "Save as filter\u{2026}" }}</button>
                            {move || lens.differs(level).then(|| view! {
                                <button type="button" role="menuitem" data-fc="reset" title="back to this view's default" on:click=move |_| {
                                    card.menu.set(false);
                                    card.dd.set(None);
                                    lens.reset(level);
                                }>"Reset"</button>
                            })}
                            // FILTER-26: the nav's filter made everyone's (the menu's, before FILTER-55).
                            {move || (level == Level::Nav && lens.can_promote()).then(|| view! {
                                <button type="button" role="menuitem" data-fc="make-global" data-act="make-global"
                                    title="the global filter becomes the global filter and the nav's; the nav goes back to its default"
                                    on:click=move |_| {
                                        card.menu.set(false);
                                        crate::promote::make_global(lens);
                                    }>"Make global"</button>
                            })}
                            <button type="button" role="menuitem" data-fc="pin" on:click=move |_| {
                                card.open.update(|o| {
                                    if let Some(o) = o {
                                        o.pinned = !o.pinned;
                                    }
                                });
                                card.menu.set(false);
                            }>{move || if card.open.with(|o| o.as_ref().is_some_and(|o| o.pinned)) { "Unpin" } else { "Keep open" }}</button>
                        </div>
                    }
                })}
            </span>
            {close}
        </div>
    }
    .into_any()
}

/// FILTER-55: a line's name field (Save as filter…, Rename…) and naming's
/// prompt and note — a name binds the level's filter (src/naming.rs): every
/// edit after keeps it saved.
fn line_name_view(card: Card, level: Level) -> impl IntoView {
    let naming = crate::naming::hook();
    let owner = line_owner(level);
    move || {
        let naming = naming?;
        let editing = card.copying.with(|c| c.as_deref() == Some(owner.as_str()));
        let field = NodeRef::<leptos::html::Input>::new();
        Effect::new(move |_| {
            if let Some(el) = field.get() {
                let _ = el.focus();
            }
        });
        let seed = untrack(|| naming.bound(level).map(|b| b.name)).unwrap_or_default();
        let input = editing.then(|| view! {
            <div class="fc-edrow" data-line-name=level.name()>
                <input class="fc-gin" node_ref=field type="text" autocomplete="off" spellcheck="false" maxlength=saved::NAME_MAX.to_string() placeholder="a name \u{2014} every edit after keeps it saved" aria-label="save this filter as" value=seed on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                    Some("form.submit") => {
                        ev.prevent_default();
                        naming.name(level, &event_target_value(&ev));
                        card.copying.set(None);
                    }
                    Some("form.cancel") => {
                        ev.prevent_default();
                        ev.stop_propagation();
                        card.copying.set(None);
                    }
                    _ => {}
                } />
                <button type="button" class="fc-lnk" on:click=move |_| card.copying.set(None)>"Cancel"</button>
            </div>
        });
        let taken = naming.taken(level).map(|(typed, e)| view! {
            <div class="fc-note bad" role="alert">
                {saved::taken_words(&typed, &e)}" "
                <button type="button" class="fc-bt sm pri" data-act="replace" on:click=move |_| naming.replace(level)>"Replace it"</button>
                <button type="button" class="fc-bt sm" data-act="another" on:click=move |_| naming.another(level)>"Choose another name"</button>
            </div>
        });
        let note = naming.note(level).map(|(bad, w)| view! { <div class="fc-note" class:bad=bad role="status">{w}</div> });
        Some(view! { {input}{taken}{note} })
    }
}

fn rules_view(card: Card, s: Subject, ro: Memo<bool>) -> AnyView {
    let intro = {
        let s = s.clone();
        move || match (&s, ro.get()) {
            (Subject::Line(l), _) => format!("every part of {} \u{2014} a change applies at once", line_words(*l).to_lowercase()),
            (_, true) => "read only \u{2014} turn on on this board to use and edit it".to_string(),
            (_, false) => "a change inside a saved filter edits it".to_string(),
        }
    };
    let body = {
        let s = s.clone();
        move || {
            let ro = ro.get();
            let p = card.resolved(&s);
            match &p.shape {
                Shape::Named { inner: Inner::Parts(top), .. } => group_view(card, &s, top, ro, true),
                Shape::Named { inner, name, .. } => inner_note(inner, name).into_any(),
                Shape::Group { .. } => group_view(card, &s, &p, ro, true),
                Shape::Rule(_) => ().into_any(),
            }
        }
    };
    let foot = {
        let s = s.clone();
        move || {
            let ro = ro.get();
            let p = card.resolved(&s);
            let top = match &p.shape {
                Shape::Named { inner: Inner::Parts(t), .. } => Some((**t).clone()),
                Shape::Group { .. } => Some(p.clone()),
                _ => None,
            };
            let adds = (!ro).then_some(top).flatten().map(|t| {
                let conj = match &t.shape {
                    Shape::Group { conj, .. } => *conj,
                    _ => Conj::All,
                };
                let (o1, o2) = (t.owner.clone(), t.owner.clone());
                view! {
                    <button type="button" class="fc-bt" data-fc="add-rule" on:click=move |ev| card.open_dd(o1.clone(), Vec::new(), Target::Add { group: false }, conj, false, What::Field, ev.as_ref())>
                        <span class="fc-ic">"+"</span>"Add rule"
                    </button>
                    <button type="button" class="fc-bt fc-ln" data-fc="add-group" on:click=move |ev| card.open_dd(o2.clone(), Vec::new(), Target::Add { group: true }, conj, false, What::Field, ev.as_ref())>
                        <span class="fc-ic sq">"+"</span>"Add inner group"
                    </button>
                }
            });
            adds.into_any()
        }
    };
    let text = {
        let s = s.clone();
        move || {
            // FILTER-55: a level's own filter is also edited as text (what
            // the menu's drawer did, FILTER-43): Apply, or Enter.
            if let (Subject::Line(l), true) = (&s, card.text.get()) {
                return Some(text_edit(card, *l).into_any());
            }
            card.text.get().then(|| {
                let t = parts::to_filter(&parts::inlined(&card.top(&s), &|_| None), &Display::default());
                let t = match &s {
                    Subject::Saved(n) => card.owner_node(n).map(|n| parts::to_filter(&n, &Display::default()).format()).unwrap_or_else(|| t.format()),
                    _ => t.format(),
                };
                let words = marked(&t).into_iter().map(|(m, w)| if m { view! { <b>{w}</b> }.into_any() } else { view! { <span>{w}</span> }.into_any() }).collect_view();
                view! { <div class="fc-txt" data-fc-text="">{words}</div> }.into_any()
            })
        }
    };
    let drafts = {
        let s = s.clone();
        move || (!ro.get()).then(|| drafts_view(card, s.clone()))
    };
    let line = match &s {
        Subject::Line(l) => Some(line_name_view(card, *l)),
        _ => None,
    };
    let local = match &s {
        Subject::Line(l) => Some(local_view(card, *l)),
        _ => None,
    };
    let note = move || card.note.get().map(|(bad, w)| view! {
        <div class="fc-note" class:bad=bad role=if bad { "alert" } else { "status" }>
            {w}
            <button type="button" class="p-x" aria-label="dismiss" on:click=move |_| card.note.set(None)>"\u{d7}"</button>
        </div>
    });
    view! {
        <div class="fc-sec fc-rules">
            <div class="fc-sech">"Rules "<span class="fc-sm">{intro}</span></div>
            <div class="fc-body">{body}</div>
            {local}
            <div class="fc-foot">
                {foot}
                <span class="fc-sp"></span>
                <button type="button" class="fc-lnk" data-fc="as-text" aria-expanded=move || card.text.get().to_string() on:click=move |_| card.text.update(|t| *t = !*t)>
                    {move || if card.text.get() { "\u{25be} as text" } else { "\u{25b8} as text" }}
                </button>
            </div>
            {text}
            {note}
            {line}
            {drafts}
        </div>
    }
    .into_any()
}

/// FILTER-55: a level's own filter as its text, to edit — Apply (or Enter)
/// makes it the filter; text the language does not read is refused, said
/// under it, and the filter stays as it was.
fn text_edit(card: Card, level: Level) -> impl IntoView {
    let lens = card.lens;
    let shown = Memo::new(move |_| lens.filter(level).with(Filter::format));
    let typed = StoredValue::new(String::new());
    Effect::new(move |_| typed.set_value(shown.get()));
    let why = RwSignal::new(None::<String>);
    let apply = move || match lens.apply_text(level, &typed.get_value()) {
        Ok(()) => why.set(None),
        Err(e) => why.set(Some(e.to_string())),
    };
    view! {
        <div class="fc-txt edit" data-fc-text="">
            <textarea class="fc-tin" rows="2" spellcheck="false" aria-label="the filter as text" prop:value=move || shown.get() on:input=move |ev| typed.set_value(event_target_value(&ev)) on:keydown=move |ev: web_sys::KeyboardEvent| {
                if bindings::hit(&ev, Context::Field) == Some("form.submit") {
                    ev.prevent_default();
                    apply();
                }
            }></textarea>
            <button type="button" class="fc-bt sm pri" data-fc="text-apply" on:click=move |_| apply()>"Apply"</button>
            {move || why.get().map(|w| view! { <div class="fc-note bad" role="alert">{w}</div> })}
        </div>
    }
}

/// FILTER-55: a surface's own rules (the wave board's Wave, Readiness,
/// Off-board blockers and Title; the inputs' State and Removed) — the
/// language has no words for them, so they stand beside the rules, and-ed
/// with all of them: each a row as a rule is (its field, its operator, its
/// values, each a dropdown; a bin).
fn local_view(card: Card, level: Level) -> impl IntoView {
    let lens = card.lens;
    move || {
        let local = lens.kept(level).map(|k| k.local).unwrap_or_default();
        let rows: Vec<_> = local.iter().map(|(c, cl)| (c, cl.clone())).collect();
        (!rows.is_empty()).then(|| {
            let owner = line_owner(level);
            let rows = rows
                .into_iter()
                .map(|(c, cl)| {
                    let not = cl.op == Op::Not;
                    let values = cl.values.iter().map(|v| view! { <span class="fc-tok">{c.value_label(v)}</span> }).collect_view();
                    let (o1, o2, o3) = (owner.clone(), owner.clone(), owner.clone());
                    let neg = (!c.is_flag()).then(|| view! {
                        <button type="button" class="fc-sel" class:neg=not data-fc="neg" title="is, or is not" on:click=move |ev| card.open_dd(o2.clone(), Vec::new(), Target::At, Conj::All, not, What::LocalOp(c), ev.as_ref())>
                            {crate::filter::op_label(c, cl.op, cl.values.len())}
                        </button>
                    });
                    view! {
                        <div class="fc-rule local" class:bare=c.is_flag() data-local=c.key() data-rule=format!("{} {}", c.key(), cl.values.join(","))>
                            <span class="fc-grip" aria-hidden="true">"\u{283f}"</span>
                            <button type="button" class="fc-sel" data-fc="field" title="change the field" on:click=move |ev| card.open_dd(o1.clone(), Vec::new(), Target::Local(c), Conj::All, not, What::Field, ev.as_ref())>{c.label()}</button>
                            {neg}
                            <button type="button" class="fc-del" data-fc="bin" aria-label=format!("remove the {} rule", c.label()) title="remove" on:click=move |_| {
                                card.dd.set(None);
                                lens.edit(level, |k| k.remove(c));
                            }>
                                <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5M7 7v4M9 7v4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
                            </button>
                            <div class="fc-vw">
                                <button type="button" class="fc-val" data-fc="value" title="change the value" on:click=move |ev| card.open_dd(o3.clone(), Vec::new(), Target::At, Conj::All, not, What::Local(c), ev.as_ref())>{values}</button>
                            </div>
                        </div>
                    }
                })
                .collect_view();
            view! {
                <div class="fc-local" data-fc-local=level.name()>
                    <div class="fc-sm">{format!("and {} rules \u{2014} beside these, and-ed with all of them", crate::saved::surface_words(level))}</div>
                    {rows}
                </div>
            }
        })
    }
}

fn inner_note(inner: &Inner, name: &str) -> impl IntoView {
    let w = match inner {
        Inner::Loading => "\u{2026}".to_string(),
        Inner::Missing => format!("no saved filter is named \u{201c}{name}\u{201d}"),
        Inner::Cycle(chain) => format!("refused: {chain}"),
        Inner::TooDeep => "refused: saved filters nested too deep".to_string(),
        Inner::Bad(why) => format!("its text does not read: {why}"),
        Inner::Parts(_) => String::new(),
    };
    view! { <span class="fc-inote" class:bad=!matches!(inner, Inner::Loading)>{w}</span> }
}


fn count_pill(card: Card, s: &Subject, n: &Node) -> impl IntoView {
    let text = card.wire(s, n, None);
    let global = within_home(s) && card.lens.filter(Level::Global).with(|g| !g.groups.is_empty());
    move || {
        let n = card.count(text.clone());
        let title = format!("{} tickets match this part alone{}", n.map_or("\u{2026}".into(), |n| n.to_string()), if global { ", within the global filter" } else { "" });
        view! { <span class="fc-pc" class:z=n == Some(0) title=title>{n.map_or_else(|| "\u{2026}".to_string(), |n| n.to_string())}</span> }
    }
}

fn node_at(card: Card, p: &Part) -> Option<Node> {
    if p.owner.is_empty() {
        return None;
    }
    card.owner_node(&p.owner).and_then(|t| parts::get(&t, &p.path).cloned())
}

fn group_view(card: Card, s: &Subject, g: &Part, ro: bool, top: bool) -> AnyView {
    let Shape::Group { conj, not, items } = &g.shape else { return ().into_any() };
    let (conj, not) = (*conj, *not);
    let rows = items.iter().map(|i| part_view(card, s, i, ro)).collect_view();
    let (owner, path) = (g.owner.clone(), g.path.clone());
    // FILTER-55: a part being added is a ghost row until its first choice.
    let ghost = card.dd.with(|d| d.as_ref().is_some_and(|d| matches!(d.target, Target::Add { .. }) && d.owner == owner && d.path == path)).then(|| {
        let picking = card.dd.with(|d| d.as_ref().map(|d| d.what));
        let field = match picking {
            Some(What::Values(c)) => field_of(c).0.to_string(),
            Some(What::Local(c)) => c.label().to_string(),
            _ => "Choose a field".to_string(),
        };
        let (o, p) = (owner.clone(), path.clone());
        view! {
            <div class="fc-rule ghost" data-fc-ghost="">
                <span class="fc-grip" aria-hidden="true">"\u{283f}"</span>
                <button type="button" class="fc-sel pick" data-fc="ghost-field" on:click=move |ev| {
                    let (target, not) = card.dd.with_untracked(|d| d.as_ref().map_or((Target::Add { group: false }, false), |d| (d.target.clone(), d.not)));
                    card.open_dd(o.clone(), p.clone(), target, conj, not, What::Field, ev.as_ref());
                }>{field}</button>
            </div>
        }
    });
    let word = if conj == Conj::All { "And" } else { "Or" };
    let pill = if items.is_empty() {
        ().into_any()
    } else if ro {
        view! { <span class="fc-pill" class:any=conj == Conj::Any>{not.then_some("not ")}{word}</span> }.into_any()
    } else {
        let (o, p) = (owner.clone(), path.clone());
        let other = if conj == Conj::All { "or" } else { "and" };
        view! {
            <button type="button" class="fc-pill" class:any=conj == Conj::Any data-fc="flip" title=format!("every part joins by {} \u{2014} press for {other}", word.to_lowercase()) on:click=move |_| {
                let p2 = p.clone();
                card.edit(&o, p.clone(), |t| parts::flip(t, &p2));
            }>
                {not.then_some("not ")}{word}" \u{2304}"
            </button>
        }
        .into_any()
    };
    let adds = (!ro && !top).then(|| {
        let (o1, p1, o2, p2) = (owner.clone(), path.clone(), owner.clone(), path.clone());
        view! {
            <div class="fc-gadd">
                <button type="button" class="fc-lnk" data-fc="add-condition" on:click=move |ev| card.open_dd(o1.clone(), p1.clone(), Target::Add { group: false }, conj, false, What::Field, ev.as_ref())>"+ Add condition"</button>
                <button type="button" class="fc-lnk" data-fc="add-subgroup" on:click=move |ev| card.open_dd(o2.clone(), p2.clone(), Target::Add { group: true }, conj, false, What::Field, ev.as_ref())>"+ Add group"</button>
            </div>
        }
    });
    view! {
        <div class="fc-g" class:any=conj == Conj::Any class:solo=items.len() < 2>
            <div class="fc-rail">{pill}</div>
            <div class="fc-gb">
                {rows}
                {ghost}
                {(items.is_empty() && ghost_none(card, &g.owner, &g.path)).then(|| view! { <span class="fc-sm">"no parts yet"</span> })}
                {adds}
            </div>
        </div>
    }
    .into_any()
}

fn ghost_none(card: Card, owner: &str, path: &[usize]) -> bool {
    card.dd.with(|d| !d.as_ref().is_some_and(|d| matches!(d.target, Target::Add { .. }) && d.owner == owner && d.path == path))
}

fn is_last(card: Card, p: &Part) -> bool {
    card.last.with_untracked(|l| l.as_ref().is_some_and(|(o, at)| *o == p.owner && *at == p.path))
}

fn bin(card: Card, p: &Part, words: String) -> impl IntoView {
    let (o, at) = (p.owner.clone(), p.path.clone());
    view! {
        <button type="button" class="fc-del" data-fc="bin" aria-label=format!("remove {words}") title="remove" on:click=move |_| {
            let at2 = at.clone();
            card.edit(&o, Vec::new(), |t| parts::remove(t, &at2));
        }>
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5M7 7v4M9 7v4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
        </button>
    }
}

/// What a rule's time line leads with.
fn lead(a: &At) -> &'static str {
    match a.of {
        TimeOf::HasBeen => "has been so",
        _ => "became so",
    }
}

/// Whether the time menu is open on this part — and the press that opens
/// or closes it.
fn tm_key(p: &Part) -> (String, Vec<usize>) {
    (p.owner.clone(), p.path.clone())
}

fn tm_toggle(card: Card, key: (String, Vec<usize>), ev: &web_sys::Event) {
    card.tm_at.set_value(rect_of(ev));
    card.tm.update(|t| *t = if t.as_ref() == Some(&key) { None } else { Some(key) });
}

/// Where the time menu hangs over the page — under its button, or above it
/// when there is more room there; on a phone it is a sheet (the stylesheet's).
fn tm_place(at: (f64, f64, f64), phone: bool) -> String {
    if phone {
        return String::new();
    }
    let win = dom::window();
    let vw = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1280.0);
    let vh = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let left = at.0.clamp(8.0, (vw - 280.0).max(8.0));
    let below = vh - at.2 - 12.0;
    let above = at.1 - 12.0;
    if below >= 320.0 || below >= above {
        format!("left:{left:.0}px;top:{:.0}px;max-height:{:.0}px", at.2 + 4.0, below.min(420.0))
    } else {
        format!("left:{left:.0}px;bottom:{:.0}px;max-height:{:.0}px", vh - at.1 + 4.0, above.min(420.0))
    }
}

/// The small ◷ at a rule's value's end: add a time.
fn time_add(card: Card, s: &Subject, p: &Part, r: &parts::Rule) -> impl IntoView {
    let key = tm_key(p);
    let k2 = key.clone();
    let open = move || card.tm.with(|t| t.as_ref() == Some(&k2));
    let menu = time_menu(card, s.clone(), key.clone(), r.field.clone(), None);
    view! {
        <span class="fc-tw">
            <button type="button" class="fc-tadd" data-fc="time-add" aria-expanded=move || open().to_string() aria-label="add a time" title="add a time: when it became so, or how long it has been" on:click=move |ev| tm_toggle(card, key.clone(), &ev)>"\u{25f7}"</button>
            {menu}
        </span>
    }
}

/// A set time: `◷ within 3d ⌄`, its menu under it.
fn time_button(card: Card, s: &Subject, p: &Part, r: &parts::Rule, a: &At) -> impl IntoView {
    let key = tm_key(p);
    let k2 = key.clone();
    let open = move || card.tm.with(|t| t.as_ref() == Some(&k2));
    let menu = time_menu(card, s.clone(), key.clone(), r.field.clone(), Some(a.words.clone()));
    view! {
        <span class="fc-tw">
            <button type="button" class="fc-tsel on" data-fc="time" aria-expanded=move || open().to_string() title="change the time" on:click=move |ev| tm_toggle(card, key.clone(), &ev)>"\u{25f7} "{a.words.clone()}" \u{2304}"</button>
            {menu}
        </span>
    }
}

/// The time menu: when it became so (a date field: its window), and for a
/// status how long it has been so — each with the count it would leave;
/// then a date of the reader's own.
fn time_menu(card: Card, s: Subject, key: (String, Vec<usize>), field: String, current: Option<String>) -> impl IntoView {
    move || {
        if card.tm.with(|t| t.as_ref() != Some(&key)) {
            return None;
        }
        let date = matches!(field.as_str(), "Created" | "Changed");
        let today = crate::calendar::today();
        let (became, lengths) = time_choices(date, field == "Status", today);
        let (owner, path) = key.clone();
        let item = |words: String, t: Option<Time>| {
            let on = current.as_deref() == Some(words.as_str()) || (current.is_none() && t.is_none());
            let wire = untrack(|| card.would_time(&s, &owner, &path, t.clone()).unwrap_or_else(|| card.wire(&s, &card.top(&s), None)));
            let n = move || card.count(wire.clone()).map_or("\u{2026}".to_string(), |n| n.to_string());
            let (o, at) = (owner.clone(), path.clone());
            view! {
                <button type="button" role="menuitemradio" class="fc-tmi" aria-checked=on.to_string() data-tm=words.clone() on:click=move |_| card.set_time(&o, at.clone(), t.clone())>
                    <span>{words.clone()}</span>
                    <span class="fc-pkn">"\u{2192} "{n}</span>
                </button>
            }
        };
        let head = if date { format!("{field}\u{2026}") } else { "Became so\u{2026}".to_string() };
        let rows = became.into_iter().map(|(w, t)| item(w, t)).collect_view();
        let held = (!lengths.is_empty()).then(|| {
            let rows = lengths.into_iter().map(|(w, t)| item(w, Some(t))).collect_view();
            view! { <div class="fc-tmh">"Has been so\u{2026}"</div>{rows} }
        });
        let max = format!("{:04}-{:02}-{:02}", today.y, today.m, today.d);
        let (o, at) = (owner.clone(), path.clone());
        let own = view! {
            <div class="fc-tcus">
                <select aria-label="after or before" on:change=move |ev| card.tm_own.update_value(|v| v.0 = event_target_value(&ev) == "after")>
                    <option value="after" selected=move || card.tm_own.get_value().0>"after"</option>
                    <option value="before" selected=move || !card.tm_own.get_value().0>"before"</option>
                </select>
                <input type="date" aria-label="the date" max=max on:input=move |ev| card.tm_own.update_value(|v| v.1 = event_target_value(&ev)) />
                <button type="button" class="fc-bt sm" data-fc="time-own" on:click=move |_| {
                    let (after, day) = card.tm_own.get_value();
                    let w = crate::when::window_of(&format!("{} {day}", if after { "after" } else { "before" }));
                    if let Some(w) = w {
                        card.set_time(&o, at.clone(), Some(Time::Became(w)));
                    }
                }>"Apply"</button>
            </div>
        };
        let style = tm_place(card.tm_at.get_value(), card.lens.phone_untracked());
        Some(view! {
            <div class="fc-tm" role="menu" style=style>
                <div class="fc-tmh">{head}</div>
                {rows}
                {own}
                {held}
            </div>
        })
    }
}

fn part_view(card: Card, s: &Subject, p: &Part, ro: bool) -> AnyView {
    let node = node_at(card, p);
    let count = node.as_ref().map(|n| count_pill(card, s, n).into_any()).unwrap_or_else(|| ().into_any());
    let new = is_last(card, p);
    match &p.shape {
        Shape::Rule(r) => {
            let key = format!("{}/{:?}", p.owner, p.path);
            let open = card.more.with(|m| m.contains(&key));
            let (toks, more) = shown(&r.values, open);
            let toks = toks.into_iter().map(|v| view! { <span class="fc-tok">{v}</span> }).collect_view();
            let more = more.map(|w| {
                let key = key.clone();
                view! {
                    <button type="button" class="fc-more" aria-expanded=open.to_string() on:click=move |_| card.more.update(|m| {
                        if !m.remove(&key) {
                            m.insert(key.clone());
                        }
                    })>{w}</button>
                }
            });
            let ro = ro || r.time;
            let date = r.at.as_ref().is_some_and(|a| a.of == TimeOf::Date);
            if ro {
                let val = match &r.at {
                    Some(a) if date => view! { <span class="fc-tsel on">"\u{25f7} "{a.words.clone()}</span> }.into_any(),
                    _ => view! { <span class="fc-val">{toks}</span>{more} }.into_any(),
                };
                let line = r.at.clone().filter(|_| !date).map(|a| {
                    view! { <div class="fc-trow"><span class="fc-tl">{lead(&a)}</span><span class="fc-tsel on">"\u{25f7} "{a.words}</span></div> }
                });
                return view! {
                    <div class="fc-rule ro" class:time=r.time class:bare=date && r.op.is_empty() data-rule=r.text.clone()>
                        <span class="fc-sel">{r.field.clone()}</span>
                        {(!r.op.is_empty()).then(|| view! { <span class="fc-sel" class:neg=r.not>{r.op.clone()}</span> })}
                        {count}
                        <div class="fc-vw">{val}</div>
                        {line}
                        {r.time.then(|| view! { <span class="fc-sm fc-tnote">"a time the card does not change \u{2014} as text"</span> })}
                    </div>
                }
                .into_any();
            }
            let cat = PCat::of_field(&r.field);
            let (o1, p1, o2, p2, o3, p3) = (p.owner.clone(), p.path.clone(), p.owner.clone(), p.path.clone(), p.owner.clone(), p.path.clone());
            let not = r.not;
            let neg = (!date || r.not).then(|| {
                view! {
                    <button type="button" class="fc-sel" class:neg=r.not data-fc="neg" title="is, or is not" on:click=move |ev| card.open_dd(o2.clone(), p2.clone(), Target::At, Conj::All, not, What::Op, ev.as_ref())>{if r.op.is_empty() { "is".to_string() } else { r.op.clone() }}</button>
                }
            });
            let val = match &r.at {
                Some(a) if date => time_button(card, s, p, r, a).into_any(),
                _ => {
                    let add = (r.timeable && r.at.is_none()).then(|| time_add(card, s, p, r));
                    view! {
                        <button type="button" class="fc-val" data-fc="value" title="change the value" on:click=move |ev| card.open_dd(o3.clone(), p3.clone(), Target::At, Conj::All, not, if LISTED.contains(&cat) { What::Values(cat) } else { What::Field }, ev.as_ref())>{toks}</button>
                        {more}
                        {add}
                    }
                    .into_any()
                }
            };
            let line = r.at.as_ref().filter(|_| !date).map(|a| {
                let (o, at) = (p.owner.clone(), p.path.clone());
                view! {
                    <div class="fc-trow">
                        <span class="fc-tl">{lead(a)}</span>
                        {time_button(card, s, p, r, a)}
                        <button type="button" class="fc-tx" data-fc="time-off" aria-label="take the time off" title="take the time off" on:click=move |_| card.set_time(&o, at.clone(), None)>"\u{d7}"</button>
                    </div>
                }
            });
            view! {
                <div class="fc-rule" class:new=new class:bare=date && !r.not data-rule=r.text.clone()>
                    <span class="fc-grip" aria-hidden="true">"\u{283f}"</span>
                    <button type="button" class="fc-sel" data-fc="field" title="change the field" on:click=move |ev| card.open_dd(o1.clone(), p1.clone(), Target::At, Conj::All, not, What::Field, ev.as_ref())>{r.field.clone()}</button>
                    {neg}
                    {count}
                    {bin(card, p, r.text.clone())}
                    <div class="fc-vw">{val}</div>
                    {line}
                </div>
            }
            .into_any()
        }
        Shape::Named { name, not, inner } => {
            let key = format!("{}/{:?}/{}", p.owner, p.path, slug(name));
            let open = card.unfolded.with(|u| u.contains(&key));
            let n_parts = match inner {
                Inner::Parts(q) => Some(q.rules()),
                _ => None,
            };
            let unf = {
                let key = key.clone();
                let words = if open { "\u{25be} fold".to_string() } else { n_parts.map_or("\u{25b8} open".to_string(), |n| format!("\u{25b8} {n} part{}", if n == 1 { "" } else { "s" })) };
                view! {
                    <button type="button" class="fc-unf" data-fc="unfold" aria-expanded=open.to_string() on:click=move |_| card.unfolded.update(|u| {
                        if !u.remove(&key) {
                            u.insert(key.clone());
                        }
                    })>{words}</button>
                }
            };
            let edited = card.edited(name).then(|| view! { <i class="fc-ed">"edited"</i> });
            let val = view! { <span class="fc-tok sv">"\u{2317} "{name.clone()}</span>{edited} };
            let op = if *not { "does not match" } else { "matches" };
            let refused = (!matches!(inner, Inner::Parts(_))).then(|| inner_note(inner, name));
            let row = if ro {
                view! {
                    <div class="fc-rule ro named" data-named=name.clone()>
                        <span class="fc-sel">"Saved filter"</span>
                        <span class="fc-sel fix" class:neg=*not>{op}</span>
                        {count}
                        <div class="fc-vw"><span class="fc-val">{val}</span>{unf}{refused}</div>
                    </div>
                }
                .into_any()
            } else {
                let (o1, p1, o2, p2, o3, p3) = (p.owner.clone(), p.path.clone(), p.owner.clone(), p.path.clone(), p.owner.clone(), p.path.clone());
                let not = *not;
                view! {
                    <div class="fc-rule named" class:new=new data-named=name.clone()>
                        <span class="fc-grip" aria-hidden="true">"\u{283f}"</span>
                        <button type="button" class="fc-sel" data-fc="field" title="change the field" on:click=move |ev| card.open_dd(o1.clone(), p1.clone(), Target::At, Conj::All, not, What::Field, ev.as_ref())>"Saved filter"</button>
                        <button type="button" class="fc-sel" class:neg=not data-fc="neg" title="matches, or does not match" on:click=move |ev| card.open_dd(o2.clone(), p2.clone(), Target::At, Conj::All, not, What::Op, ev.as_ref())>{op}</button>
                        {count}
                        {bin(card, p, format!("saved {name}"))}
                        <div class="fc-vw">
                            <button type="button" class="fc-val" data-fc="value" title="pick another saved filter" on:click=move |ev| card.open_dd(o3.clone(), p3.clone(), Target::At, Conj::All, not, What::Field, ev.as_ref())>{val}</button>
                            {unf}
                            {refused}
                        </div>
                    </div>
                }
                .into_any()
            };
            let boxed = match (open, inner) {
                (true, Inner::Parts(q)) => {
                    let by = card.meta.with(|m| m.get(&slug(name)).map(|(b, _)| who_words(b)));
                    card.read_meta(name);
                    let edited = card.edited(name).then(|| view! { <i class="fc-ed">"edited"</i> });
                    Some(view! {
                        <div class="fc-sbox" data-sbox=name.clone()>
                            <div class="fc-sbh">
                                <b>"\u{2317} "{name.clone()}</b>
                                {edited}
                                <span>{format!("saved filter{}{}", by.map(|b| format!(" \u{00b7} {b}")).unwrap_or_default(), if ro { "" } else { " \u{00b7} a change here edits it" })}</span>
                            </div>
                            {group_view(card, s, q, ro, false)}
                        </div>
                    })
                }
                _ => None,
            };
            view! { {row}{boxed} }.into_any()
        }
        Shape::Group { .. } => {
            let naming = card.naming.with(|n| n.as_ref().is_some_and(|(o, at)| *o == p.owner && *at == p.path));
            let head = if ro {
                ().into_any()
            } else if naming {
                let (o, at) = (p.owner.clone(), p.path.clone());
                let field = NodeRef::<leptos::html::Input>::new();
                let seed = card.typed.get_value();
                let (o2, at2) = (o.clone(), at.clone());
                Effect::new(move |_| {
                    if let Some(el) = field.get() {
                        let _ = el.focus();
                    }
                });
                view! {
                    <div class="fc-gh">
                        <input
                            class="fc-gin"
                            node_ref=field
                            type="text"
                            autocomplete="off"
                            spellcheck="false"
                            maxlength=saved::NAME_MAX.to_string()
                            placeholder="its name \u{2014} it becomes a saved filter"
                            aria-label="the new saved filter's name"
                            value=seed
                            on:input=move |ev| card.typed.set_value(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                                Some("form.submit") => {
                                    ev.prevent_default();
                                    card.name_group(o.clone(), at.clone(), event_target_value(&ev));
                                }
                                Some("form.cancel") => {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    card.naming.set(None);
                                }
                                _ => {}
                            }
                        />
                        <button type="button" class="fc-bt pri sm" data-fc="name-ok" disabled=move || card.busy.get() on:click=move |_| card.name_group(o2.clone(), at2.clone(), card.typed.get_value())>"Save as filter"</button>
                        <button type="button" class="fc-lnk" data-fc="name-x" on:click=move |_| card.naming.set(None)>"Cancel"</button>
                    </div>
                }
                .into_any()
            } else {
                let (o, at) = (p.owner.clone(), p.path.clone());
                view! {
                    <div class="fc-gh">
                        <button type="button" class="fc-lnk sm" data-fc="name-group" title="naming it saves it as a filter of its own, shown as a saved filter here" on:click=move |_| {
                            card.typed.set_value(String::new());
                            card.note.set(None);
                            card.naming.set(Some((o.clone(), at.clone())));
                        }>"+ Name this group"</button>
                        {count}
                    </div>
                }
                .into_any()
            };
            view! { <div class="fc-nest" class:new=new>{head}{group_view(card, s, p, ro, false)}</div> }.into_any()
        }
    }
}

fn drafts_view(card: Card, s: Subject) -> impl IntoView {
    move || {
        card.drafts
            .get()
            .into_iter()
            .map(|(name, n)| {
                let now = card.wire(&s, &Node::Leaf(Predicate::is(Field::Saved(name.clone()))), None);
                let was = {
                    let saved = Node::Leaf(Predicate::is(Field::Saved(name.clone())));
                    let own = parts::to_filter(&saved, &Display::default());
                    let eff = if within_home(&s) { card.lens.filter(Level::Global).with(|g| g.and(&own)) } else { own };
                    crate::counts::wire_text(&card.lens.org().with(|o| crate::orgs::within(o.as_deref(), &eff)))
                };
                let _ = n;
                let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
                let copying = card.copying.with(|c| c.as_ref().is_some_and(|c| slug(c) == slug(&name)));
                let words = move || {
                    let a = card.count(now.clone()).map_or("\u{2026}".into(), |n| n.to_string());
                    let b = card.count(was.clone()).map_or("\u{2026}".into(), |n| n.to_string());
                    format!(" \u{2014} {a} tickets, saved: {b}. Only this card counts the change until you save.")
                };
                let act = if copying {
                    let field = NodeRef::<leptos::html::Input>::new();
                    Effect::new(move |_| {
                        if let Some(el) = field.get() {
                            let _ = el.focus();
                        }
                    });
                    view! {
                        <input class="fc-gin" node_ref=field type="text" placeholder="the copy's name" aria-label="save as" on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                            Some("form.submit") => {
                                ev.prevent_default();
                                card.save_as(n1.clone(), event_target_value(&ev));
                            }
                            Some("form.cancel") => {
                                ev.prevent_default();
                                ev.stop_propagation();
                                card.copying.set(None);
                            }
                            _ => {}
                        } />
                        <button type="button" class="fc-lnk" on:click=move |_| card.copying.set(None)>"Cancel"</button>
                    }
                    .into_any()
                } else {
                    view! {
                        <button type="button" class="fc-bt" data-fc="undo" on:click=move |_| card.undo(&n2)>"Undo"</button>
                        <button type="button" class="fc-bt" data-fc="save-as" on:click=move |_| card.copying.set(Some(n3.clone()))>"Save as\u{2026}"</button>
                        <button type="button" class="fc-bt pri" data-fc="save-back" disabled=move || card.busy.get() on:click=move |_| card.save_back(n4.clone())>"Save back"</button>
                    }
                    .into_any()
                };
                view! {
                    <div class="fc-edrow" data-edited=name.clone()>
                        <span><b>"\u{2317} "{name.clone()}</b>" "<i>"edited"</i>{words}</span>
                        <span class="fc-edb">{act}</span>
                    </div>
                }
            })
            .collect_view()
    }
}

// ---------------------------------------------------------------------
// The nav under it

fn nav_section(card: Card, s: Subject) -> AnyView {
    let text = {
        let s = s.clone();
        Memo::new(move |_| card.wire(&s, &card.top(&s), None))
    };
    let within = within_home(&s) && card.lens.filter(Level::Global).with_untracked(|g| !g.groups.is_empty());
    let n = move || card.count(text.get()).map_or_else(|| "\u{2026}".to_string(), |n| n.to_string());
    view! {
        <div class="fc-sec fc-nav">
            <div class="fc-sech">
                "The nav under it "
                <span class="fc-nvm">
                    <button type="button" data-flat="0" aria-pressed=move || (!card.flat.get()).to_string() on:click=move |_| card.flat.set(false)>"tree"</button>
                    <button type="button" data-flat="1" aria-pressed=move || card.flat.get().to_string() on:click=move |_| card.flat.set(true)>"flat"</button>
                </span>
                <span class="fc-tag2"><b>{n}</b>{if within { " match within \u{2302}" } else { " match" }}</span>
            </div>
            <div class="fc-body fc-nvb">
                {move || {
                    let t = text.get();
                    if card.flat.get() { flat_view(t).into_any() } else { tree_view(card, t).into_any() }
                }}
            </div>
        </div>
    }
    .into_any()
}

fn rows_of(events: Vec<TicketsEvent>) -> (Vec<NavRow>, Option<u64>) {
    let mut rows = Vec::new();
    let mut remaining = None;
    for ev in events {
        match ev {
            TicketsEvent::Node { summary, matched, below } => rows.push(NavRow { id: summary.id.to_string(), title: summary.title, kind: summary.kind.to_string(), status: summary.status.to_string(), matched, below }),
            TicketsEvent::Summary { summary } => rows.push(NavRow { id: summary.id.to_string(), title: summary.title, kind: summary.kind.to_string(), status: summary.status.to_string(), matched: true, below: 0 }),
            TicketsEvent::Page { remaining: r, .. } => remaining = Some(r),
            _ => {}
        }
    }
    (rows, remaining)
}

async fn ask_tree(text: String, epic: Option<String>, parent: Option<String>, limit: u32, only: bool) -> Option<(Vec<NavRow>, Option<u64>)> {
    let client = session::client()?;
    let (epic, parent, filter) = (crate::typed::opt(epic).ok()?, crate::typed::opt(parent).ok()?, crate::typed::typed(&text).ok()?);
    let events = client.nav().tree_all(parent, epic, filter, None, Some(limit), only.then_some(true), None, session::zone()).await.ok()?;
    Some(rows_of(events))
}

fn nav_row(r: &NavRow, d: usize, toggle: Option<(bool, Box<dyn Fn() + 'static>)>) -> impl IntoView {
    let tw = match toggle {
        Some((open, flip)) => view! {
            <button type="button" class="fc-nt" data-nav=r.id.clone() aria-expanded=open.to_string() aria-label=format!("{} {}", if open { "fold" } else { "unfold" }, r.id) on:click=move |_| flip()>
                {if open { "\u{25be}" } else { "\u{25b8}" }}
            </button>
        }
        .into_any(),
        None => view! { <span class="fc-nt"></span> }.into_any(),
    };
    view! {
        <div class="fc-nr" class:hold=!r.matched style=format!("--d:{d}") data-nrow=r.id.clone()>
            {tw}
            <span class="fc-gl" data-kind=r.kind.clone()>{crate::nav::kind_glyph(&r.kind)}</span>
            <a href=format!("#/ticket/{}", r.id) class="fc-id">{r.id.clone()}</a>
            <span class="fc-tt">{r.title.clone()}</span>
            {(r.below > 0).then(|| view! { <span class="fc-bl" title=format!("{} matches below", r.below)>{r.below}</span> })}
            <span class=format!("{} fc-st", st_class(&r.status))>{r.status.clone()}</span>
        </div>
    }
}

/// A node and, open, its children — `nav tree --parent` under the card's filter.
fn nav_node(card: Card, text: String, r: NavRow, d: usize) -> AnyView {
    let has = r.below > 0;
    let id = r.id.clone();
    let open = {
        let id = id.clone();
        Memo::new(move |_| card.folds.with(|f| f.contains(&id)) != (d == 0))
    };
    let flip = {
        let id = id.clone();
        move || {
            card.folds.update(|f| {
                if !f.remove(&id) {
                    f.insert(id.clone());
                }
            })
        }
    };
    let row = move || nav_row(&r, d, has.then(|| (open.get(), Box::new(flip.clone()) as Box<dyn Fn()>)));
    view! {
        {row}
        {move || (has && open.get()).then(|| nav_kids(card, text.clone(), id.clone(), d + 1))}
    }
    .into_any()
}

fn nav_kids(card: Card, text: String, parent: String, d: usize) -> AnyView {
    let got: RwSignal<Option<(Vec<NavRow>, Option<u64>)>> = RwSignal::new(None);
    {
        let text = text.clone();
        spawn_local(async move {
            let r = ask_tree(text, None, Some(parent), NAV_PAGE, true).await.unwrap_or_default();
            got.try_set(Some(r));
        });
    }
    view! {
        {move || match got.get() {
            None => view! { <div class="fc-pv0" style=format!("--d:{d}")>"\u{2026}"</div> }.into_any(),
            Some((rows, more)) => view! {
                {rows.into_iter().map(|r| nav_node(card, text.clone(), r, d)).collect_view()}
                {more.filter(|m| *m > 0).map(|m| view! { <div class="fc-pv0" style=format!("--d:{d}")>{format!("and {m} more")}</div> })}
            }
            .into_any(),
        }}
    }
    .into_any()
}

fn tree_view(card: Card, text: String) -> impl IntoView {
    move || {
        let tops: Vec<String> = card.groups.with(|g| g.as_ref().map(|g| g.iter().filter(|g| g.host.is_none()).map(|g| g.epic.to_string()).collect()).unwrap_or_default());
        if tops.is_empty() {
            return view! { <div class="fc-pv0">"\u{2026}"</div> }.into_any();
        }
        let got: RwSignal<Option<Vec<NavRow>>> = RwSignal::new(None);
        let text2 = text.clone();
        spawn_local(async move {
            let mut out = Vec::new();
            for e in tops {
                if let Some((rows, _)) = ask_tree(text2.clone(), Some(e), None, 1, false).await {
                    out.extend(rows.into_iter().filter(|r| r.matched || r.below > 0));
                }
            }
            got.try_set(Some(out));
        });
        let text = text.clone();
        view! {
            {move || match got.get() {
                None => view! { <div class="fc-pv0">"\u{2026}"</div> }.into_any(),
                Some(rows) if rows.is_empty() => view! { <div class="fc-pv0">"The nav would show nothing"</div> }.into_any(),
                Some(rows) => rows.into_iter().map(|r| nav_node(card, text.clone(), r, 0)).collect_view().into_any(),
            }}
        }
        .into_any()
    }
}

fn flat_view(text: String) -> impl IntoView {
    let got: RwSignal<Option<(Vec<NavRow>, Option<u64>)>> = RwSignal::new(None);
    spawn_local(async move {
        let Some(client) = session::client() else { return };
        let Ok(filter) = crate::typed::typed(&text) else { return };
        if let Ok(events) = client.list_all(None, None, None, None, Some(NAV_PAGE), Some(true), Some(filter), None, None, session::zone()).await {
            got.try_set(Some(rows_of(events)));
        }
    });
    move || match got.get() {
        None => view! { <div class="fc-pv0">"\u{2026}"</div> }.into_any(),
        Some((rows, _)) if rows.is_empty() => view! { <div class="fc-pv0">"The nav would show nothing"</div> }.into_any(),
        Some((rows, more)) => view! {
            {rows.iter().map(|r| nav_row(r, 0, None)).collect_view()}
            {more.filter(|m| *m > 0).map(|m| view! { <div class="fc-pv0">{format!("and {m} more")}</div> })}
        }
        .into_any(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_past_four_fold_into_plus_n() {
        let v: Vec<String> = ["A", "B", "C", "D"].map(String::from).to_vec();
        assert_eq!(shown(&v, false), (v.clone(), None));
        let ten: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        assert_eq!(shown(&ten, false), (ten[..3].to_vec(), Some("+7".into())));
        assert_eq!(shown(&ten, true), (ten.clone(), Some("fewer".into())));
    }

    #[test]
    fn the_switch_ands_a_saved_filter_on_and_takes_it_off() {
        let f = Filter::parse("kind build").unwrap();
        let on = with_saved(&f, "Plexus-Ready");
        assert_eq!(on.format(), "kind build and saved Plexus-Ready");
        assert!(on_board(&on, "plexus ready"), "by slug");
        assert_eq!(with_saved(&on, "Plexus-Ready"), on, "once");
        assert_eq!(without_saved(&on, "Plexus-Ready").format(), "kind build");
        // Inside an or-group, the name alone goes.
        let g = Filter::parse("saved \"Needs you\" or kind gap").unwrap();
        assert!(on_board(&g, "Needs you"));
        assert_eq!(without_saved(&g, "Needs you").format(), "kind gap");
        // Alone, the filter is empty after.
        assert!(without_saved(&Filter::parse("saved X").unwrap(), "X").groups.is_empty());
    }

    #[test]
    fn a_copy_takes_the_next_free_name() {
        let listed = [Entry::of("Plexus-Ready"), Entry::of("Plexus-Ready copy")];
        assert_eq!(copy_name(&listed, "Plexus-Ready"), "Plexus-Ready copy 2");
        assert_eq!(copy_name(&listed, "Plexus-Ready copy"), "Plexus-Ready copy 2", "a copy's copy counts on");
        assert_eq!(copy_name(&[], "Needs you"), "Needs you copy");
    }

    #[test]
    fn a_clause_picked_becomes_a_node() {
        assert_eq!(node_of("status Ready", false), Some(Node::Leaf(Filter::parse("status Ready").unwrap().groups[0].preds[0].clone())));
        match node_of("kind gap", true) {
            Some(Node::Leaf(p)) => assert!(p.not),
            n => panic!("{n:?}"),
        }
        assert_eq!(node_of("frobnicate", false), None);
        assert_eq!(label_clause("  Needs-You "), Some("label \"needs-you\"".into()));
        assert_eq!(label_clause(" "), None);
        // Every value the picker offers reads.
        for c in PCat::ALL {
            for (w, t) in values(c, &["FILTER".into()], &["Needs you".into()]) {
                assert!(node_of(&t, false).is_some(), "{c:?} {w}: {t}");
            }
        }
    }

    #[test]
    fn as_text_marks_the_joiners() {
        let m = marked("saved Plexus-Ready and (saved \"Needs you\" or not kind gap)");
        let bold: Vec<&str> = m.iter().filter(|(b, _)| *b).map(|(_, w)| w.trim()).collect();
        assert_eq!(bold, ["and", "(saved", "or", "not"].map(|w| if w == "(saved" { "" } else { w }).into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>());
        assert_eq!(m.iter().map(|(_, w)| w.as_str()).collect::<String>(), "saved Plexus-Ready and (saved \"Needs you\" or not kind gap)");
        assert!(!marked("label \"a and b\"").iter().any(|(b, _)| *b), "a quoted and is no joiner");
        assert_eq!(who_words("key:54a9677a1234"), "key:54a9677a\u{2026}");
    }

    #[test]
    fn the_time_menu_offers_what_the_rule_takes() {
        let day = crate::calendar::Day::new(2026, 10, 23);
        let (became, held) = time_choices(false, true, day);
        let words: Vec<&str> = became.iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(words[..3], ["any time", "today", "this week"]);
        assert!(became[0].1.is_none() && became[1..].iter().all(|(_, t)| matches!(t, Some(Time::Became(_)))), "each reads as a window");
        assert!(words.contains(&"after 2026-10-01") && words.contains(&"between 2026-09-01 and 2026-09-30"), "{words:?}");
        assert_eq!(held.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), ["for over 3d", "for over 1w", "for over 2w", "for under 3d"]);
        // A kind: no has been so; a date field: no any time.
        assert!(time_choices(false, false, day).1.is_empty());
        let (dates, none) = time_choices(true, false, day);
        assert!(none.is_empty() && dates[0].0 == "today");
        assert_eq!(PCat::of_field("Created"), PCat::Dates);
        assert_eq!(values(PCat::Dates, &[], &[]).iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["created this week", "changed this week"]);
    }

    #[test]
    fn a_tick_adds_or_takes_a_value_and_a_list_is_one_rule() {
        let vals: Vec<String> = vec!["Ready".into()];
        assert_eq!(ticked(Some(("Status", &vals)), PCat::Status, "Coding"), ("status", vec!["Ready".to_string(), "Coding".to_string()]));
        assert_eq!(ticked(Some(("Status", &vals)), PCat::Status, "ready"), ("status", Vec::<String>::new()), "case ignored");
        assert_eq!(ticked(Some(("Kind", &vals)), PCat::Status, "Coding"), ("status", vec!["Coding".to_string()]), "another field: replaced");
        let under: Vec<String> = vec!["FILTER".into()];
        assert_eq!(ticked(Some(("Under", &under)), PCat::Shortcode, "APP").0, "under", "under keeps its word");
        // A list reads back as one rule, negated or not.
        for not in [false, true] {
            let n = list_node("kind", &["build".into(), "gap".into()], not).unwrap();
            let r = parts::rule_at(&n).expect("one rule");
            assert_eq!((r.field.as_str(), r.not, r.values.len()), ("Kind", not, 2));
            let f = parts::to_filter(&n, &Display::default());
            assert!(Filter::parse(&f.format()).is_ok(), "{}", f.format());
        }
        assert!(list_node("in", &["FILTER".into()], false).is_some());
        assert_eq!(list_node("kind", &[], false), None);
    }

    #[test]
    fn the_line_says_a_name_and_the_button_counts_rules() {
        let one = Filter::parse("saved \"Needs you\"").unwrap();
        assert_eq!(line_name(&one, None).as_deref(), Some("Needs you"));
        assert_eq!(line_name(&Filter::parse("not saved X").unwrap(), None), None, "negated: no name");
        let more = Filter::parse("kind build and status Ready").unwrap();
        assert_eq!(line_name(&more, None), None, "rules only: nothing but the badge");
        assert_eq!(line_name(&more, Some("open builds")).as_deref(), Some("open builds"), "the name it is saved under");
        assert_eq!(rules_of(&more), 2);
        assert_eq!(rules_of(&Filter::parse("status in (Ready, Coding) and saved X").unwrap()), 2, "a list one rule, a name one");
        assert_eq!(rules_of(&Filter::default()), 0);
        for l in Level::ALL {
            assert_eq!(line_of(&line_owner(l)), Some(l), "every level's own filter is the builder's");
        }
        assert!(within_home(&Subject::Line(Level::Board)) && !within_home(&Subject::Line(Level::Global)), "⌂'s own counts are not within itself");
        assert_eq!(line_of(&line_owner(Level::Nav)), Some(Level::Nav));
        assert_eq!(key_of(&Subject::Line(Level::Board)), "line:board");
    }
}
