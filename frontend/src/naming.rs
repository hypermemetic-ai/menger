//! FILTER-37: naming saves — the name at the top of a level's query tree is
//! its save (owner, 2026-10-15: "adding a name saves"), and what the tree
//! needs to know of saved queries: their texts, their order in `+ saved`'s
//! picker, their counts.
//!
//! - **A name binds the tree** ([`Bound`]): given a name (the field's change
//!   — Enter, or leaving it), the level's filter is saved as FILTER-6's
//!   Save as… saves it (src/saved.rs's writes: the collection, the doc
//!   `VIEW/_MENGER/view/<slug>`, its line), and the level is bound to it;
//!   **every edit after** amends the doc — 400 ms after the last change
//!   ([`DEBOUNCE_MS`]), one write at a time, and the text written last is
//!   always the tree's: a change that lands while a write is out is written
//!   after it. A name whose slug another listed filter holds offers to
//!   replace it or to choose another, as Save as… does ([`plan`]). Typing
//!   another name renames (the doc's title and the line; the path stays);
//!   emptying it unlists (the line goes, the doc stays, as `remove` does).
//!   A name given to an empty tree waits for its first clause.
//! - **What lets go of a name**: a filter applied from outside the bar — a
//!   link's URL, `Saved ▾` choosing another (which binds that one), Make
//!   global and its undo, `reset`, `g n`, the palette's filter (the lens's
//!   stamp moves, src/lens.rs). The binding is kept per viewer, per level
//!   (`menger.lens.<level>.named`), and comes back while the level holds
//!   the text it last wrote. Unnamed, a tree is the level's filter alone.
//! - **Texts** ([`Fetch`]): a saved query's text, read when it is first
//!   peeked, inlined or evaluated — the collection's entry for the name
//!   (by slug), its doc's `filter:` line; Needs you, with no doc, its
//!   default (FILTER-36); kept, and moved by `watch` (a doc amended
//!   anywhere).
//! - **The picker's order** ([`order`]): the most recently used first
//!   (kept per viewer, [`RECENT_KEY`]), then the level's own (a saved query
//!   filed under the level's shortcode — none yet: the collection does not
//!   file them so), then the collection's order; [`SHOWN`] shown, the rest
//!   by typing. Each name shown is counted — `counts { filter: saved
//!   "Name" }` — once, where the level has a counts provider.
//!
//! What decides — the plan a name makes, the picker's order and search,
//! the recency — is pure and pinned by native tests.

use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::filter::{Field, Filter, Predicate};
use menger_tickets_api::saved_filters::{default_text, filter_line, path_slug, NEEDS_YOU, PATH_PREFIX};
use menger_tickets_api::TicketsEvent;
use wasm_bindgen_futures::spawn_local;

use crate::lens::{storage_key, Lens, Level};
use crate::saved::{self, slug, Entry, Failure, Listing, Refusal, Saved};
use crate::{dom, session};

/// How long after the last change a bound tree's doc is amended.
pub(crate) const DEBOUNCE_MS: i32 = 400;
/// How many names the picker shows before "N more: type to narrow".
pub(crate) const SHOWN: usize = 8;
/// How many recently used names are kept.
pub(crate) const RECENT_MAX: usize = 12;
/// Where they are kept, per viewer.
pub(crate) const RECENT_KEY: &str = "menger.saved.recent";

/// The saved filter a level's tree is bound to: its id (its doc's path's
/// slug) and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bound {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// A saved query's text, as the app last read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fetch {
    Loading,
    Text(String),
    /// No saved filter holds the name.
    Missing,
    Failed(String),
}

/// What a name typed does, given the level's binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    Nothing,
    /// Emptied: the bound filter's line taken out.
    Unlist(String),
    /// A first name: saved (Save as…'s plan decides new or taken).
    Save(String),
    /// Another name: the bound filter renamed.
    Rename { id: String, name: String },
}

/// The plan a name typed (`raw`) makes.
pub(crate) fn plan(bound: Option<&Bound>, raw: &str) -> Plan {
    let name = saved::clean_name(raw);
    match (bound, name.is_empty()) {
        (None, true) => Plan::Nothing,
        (Some(b), true) => Plan::Unlist(b.id.clone()),
        (None, false) => Plan::Save(name),
        (Some(b), false) if b.name == name => Plan::Nothing,
        (Some(b), false) => Plan::Rename { id: b.id.clone(), name },
    }
}

/// The names `+ saved` offers: the collection's, in its order, and Needs
/// you always (it resolves without a doc).
pub(crate) fn names(listed: &[Entry]) -> Vec<String> {
    let mut out: Vec<String> = listed.iter().map(|e| e.name.clone()).collect();
    if !out.iter().any(|n| slug(n) == slug(NEEDS_YOU.0)) {
        out.push(NEEDS_YOU.0.to_string());
    }
    out
}

/// A name in the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pick {
    pub(crate) name: String,
    pub(crate) recent: bool,
    pub(crate) own: bool,
}

/// Whether `name` answers `query`: what is typed is in the name (case
/// folded), or the query's slug in the name's (`plexus ready` finds
/// Plexus-Ready).
fn answers(name: &str, query: &str) -> bool {
    name.to_lowercase().contains(&query.to_lowercase()) || (!slug(query).is_empty() && slug(name).contains(&slug(query)))
}

/// The picker's names for `query`: the recently used first (most recent
/// first), then the level's own, then the rest in the collection's order —
/// each once, by slug; those `query` answers.
pub(crate) fn order(names: &[String], recent: &[String], own: &[String], query: &str) -> Vec<Pick> {
    let known = |n: &String| names.iter().find(|x| slug(x) == slug(n)).cloned();
    let rec: Vec<String> = recent.iter().filter_map(known).collect();
    let mine: Vec<String> = own.iter().filter_map(known).collect();
    let mut out: Vec<Pick> = Vec::new();
    for n in rec.iter().chain(&mine).chain(names) {
        if out.iter().any(|p| slug(&p.name) == slug(n)) || !answers(n, query.trim()) {
            continue;
        }
        let is = |list: &[String]| list.iter().any(|x| slug(x) == slug(n));
        out.push(Pick { name: n.clone(), recent: is(&rec), own: is(&mine) });
    }
    out
}

/// The first [`SHOWN`], and how many more there are.
pub(crate) fn shown(mut all: Vec<Pick>) -> (Vec<Pick>, usize) {
    let more = all.len().saturating_sub(SHOWN);
    all.truncate(SHOWN);
    (all, more)
}

/// The recency with `name` used now: first, once, at most [`RECENT_MAX`].
pub(crate) fn with_recent(recent: &[String], name: &str) -> Vec<String> {
    std::iter::once(name.to_string()).chain(recent.iter().filter(|r| slug(r) != slug(name)).cloned()).take(RECENT_MAX).collect()
}

/// The listed filter a name means: its name's slug, else its id.
pub(crate) fn entry_for<'a>(listed: &'a [Entry], name: &str) -> Option<&'a Entry> {
    let s = slug(name);
    listed.iter().find(|e| slug(&e.name) == s).or_else(|| listed.iter().find(|e| e.id == s))
}

/// `saved "Name"` as a filter's text.
pub(crate) fn saved_text_of(name: &str) -> String {
    Predicate::is(Field::Saved(name.to_string())).format()
}

/// A binding as kept: `id`, `name`, the text last written — one per line.
fn kept_line(b: &Bound, text: &str) -> String {
    format!("{}\n{}\n{text}", b.id, b.name)
}

fn read_kept(v: &str) -> Option<(Bound, String)> {
    let mut it = v.splitn(3, '\n');
    let (id, name, text) = (it.next()?, it.next()?, it.next().unwrap_or(""));
    (!id.is_empty() && !name.is_empty()).then(|| (Bound { id: id.into(), name: name.into() }, text.into()))
}

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn load(key: &str) -> Option<String> {
    storage().and_then(|s| s.get_item(key).ok().flatten())
}

fn store(key: &str, v: Option<&str>) {
    if let Some(s) = storage() {
        let _ = match v {
            Some(v) => s.set_item(key, v),
            None => s.remove_item(key),
        };
    }
}

/// Naming saves for the app's lifetime — every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct Naming {
    lens: Lens,
    saved: Saved,
    bound: [RwSignal<Option<Bound>>; 5],
    /// A name given while the tree was empty: saved with its first clause.
    pending: [RwSignal<Option<String>>; 5],
    /// A name another listed filter holds: the name typed, and that filter.
    taken: [RwSignal<Option<(String, Entry)>>; 5],
    /// What naming last did, or why it did not (`true`: a problem).
    note: [RwSignal<Option<(bool, String)>>; 5],
    busy: [RwSignal<bool>; 5],
    written: [StoredValue<Option<String>>; 5],
    turn: [StoredValue<u32>; 5],
    writing: [StoredValue<bool>; 5],
    texts: RwSignal<HashMap<String, Fetch>>,
    counts: RwSignal<HashMap<String, Option<usize>>>,
    recent: RwSignal<Vec<String>>,
}

thread_local! {
    static HOOK: std::cell::Cell<Option<Naming>> = const { std::cell::Cell::new(None) };
}

/// The app's naming (`None` before the App made it).
pub(crate) fn hook() -> Option<Naming> {
    HOOK.with(std::cell::Cell::get)
}

/// The app's naming.
pub(crate) fn use_naming() -> Naming {
    use_context::<Naming>().expect("naming is provided by the App")
}

impl Naming {
    pub(crate) fn new(lens: Lens, saved: Saved) -> Self {
        let recent = load(RECENT_KEY).map(|v| v.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()).unwrap_or_default();
        let n = Naming {
            lens,
            saved,
            bound: std::array::from_fn(|_| RwSignal::new(None)),
            pending: std::array::from_fn(|_| RwSignal::new(None)),
            taken: std::array::from_fn(|_| RwSignal::new(None)),
            note: std::array::from_fn(|_| RwSignal::new(None)),
            busy: std::array::from_fn(|_| RwSignal::new(false)),
            written: std::array::from_fn(|_| StoredValue::new(None)),
            turn: std::array::from_fn(|_| StoredValue::new(0)),
            writing: std::array::from_fn(|_| StoredValue::new(false)),
            texts: RwSignal::new(HashMap::new()),
            counts: RwSignal::new(HashMap::new()),
            recent: RwSignal::new(recent),
        };
        for level in Level::ALL {
            let seen = StoredValue::new(0u32);
            Effect::new(move |_| {
                let stamp = lens.stamp(level);
                let text = lens.filter(level).with(Filter::format);
                let moved = seen.get_value() != stamp;
                seen.set_value(stamp);
                let i = level.index();
                if n.bound[i].get_untracked().is_none() {
                    match n.pending[i].get_untracked() {
                        Some(name) if !text.is_empty() && !moved => {
                            n.pending[i].set(None);
                            n.save(level, name, false);
                        }
                        _ => n.rebind(level, &text),
                    }
                    return;
                }
                if n.written[i].with_value(|w| w.as_deref() == Some(text.as_str())) {
                    return;
                }
                if moved {
                    n.unbind(level);
                    n.rebind(level, &text);
                    return;
                }
                n.schedule(level);
            });
        }
        // A saved query's doc changed anywhere: its text follows.
        session::listen(move |ev| {
            let (TicketsEvent::Amended { ticket, .. } | TicketsEvent::Created { ticket } | TicketsEvent::Ticket { ticket }) = ev else { return };
            let Some(path) = ticket.path.as_deref().filter(|p| p.starts_with(PATH_PREFIX)) else { return };
            let Some(text) = filter_line(&ticket.body).map(str::to_string) else { return };
            let keys = [slug(&ticket.title), path_slug(path).unwrap_or_default().to_string()];
            n.texts.try_update(|m| {
                for k in keys {
                    if m.contains_key(&k) {
                        m.insert(k, Fetch::Text(text.clone()));
                    }
                }
            });
        });
        provide_context(n);
        HOOK.with(|h| h.set(Some(n)));
        n
    }

    fn key(self, level: Level) -> String {
        format!("{}.named", storage_key(level, untrack(|| self.lens.route_sc(level)).as_deref()))
    }

    /// The saved filter `level` is bound to. Tracked.
    pub(crate) fn bound(self, level: Level) -> Option<Bound> {
        self.bound[level.index()].get()
    }

    /// The name a taken prompt, a problem or a step says. Tracked.
    pub(crate) fn note(self, level: Level) -> Option<(bool, String)> {
        self.note[level.index()].get()
    }

    pub(crate) fn taken(self, level: Level) -> Option<(String, Entry)> {
        self.taken[level.index()].get()
    }

    pub(crate) fn busy(self, level: Level) -> bool {
        self.busy[level.index()].get()
    }

    /// A name waiting for the tree's first clause. Tracked.
    pub(crate) fn pending(self, level: Level) -> Option<String> {
        self.pending[level.index()].get()
    }

    pub(crate) fn dismiss(self, level: Level) {
        self.note[level.index()].set(None);
    }

    /// `level` bound to `b`, which holds `text` now.
    pub(crate) fn bind(self, level: Level, b: Bound, text: String) {
        let i = level.index();
        store(&self.key(level), Some(&kept_line(&b, &text)));
        self.written[i].set_value(Some(text));
        self.taken[i].set(None);
        self.pending[i].set(None);
        self.touch(&b.name);
        self.bound[i].set(Some(b));
    }

    /// `level` unnamed: its tree is its filter alone.
    pub(crate) fn unbind(self, level: Level) {
        let i = level.index();
        store(&self.key(level), None);
        self.written[i].set_value(None);
        self.taken[i].set(None);
        self.pending[i].set(None);
        if self.bound[i].with_untracked(Option::is_some) {
            self.bound[i].set(None);
        }
    }

    /// The binding kept for `level`, while it holds the text last written.
    fn rebind(self, level: Level, text: &str) {
        let Some((b, kept)) = load(&self.key(level)).as_deref().and_then(read_kept) else { return };
        if kept == text && !text.is_empty() {
            let i = level.index();
            self.written[i].set_value(Some(kept));
            self.bound[i].set(Some(b));
        }
    }

    /// The name field's change: [`plan`]'s writes.
    pub(crate) fn name(self, level: Level, raw: &str) {
        let i = level.index();
        if self.busy[i].get_untracked() {
            return;
        }
        self.taken[i].set(None);
        match plan(self.bound[i].get_untracked().as_ref(), raw) {
            Plan::Nothing => {
                if saved::clean_name(raw).is_empty() {
                    self.pending[i].set(None);
                }
            }
            Plan::Save(name) => {
                if untrack(|| self.lens.filter(level).with(Filter::is_empty)) {
                    self.pending[i].set(Some(name.clone()));
                    self.note[i].set(Some((false, format!("\u{201c}{name}\u{201d} is saved with the tree's first clause"))));
                } else {
                    self.save(level, name, false);
                }
            }
            Plan::Unlist(id) => {
                let was = self.bound[i].get_untracked().map(|b| b.name).unwrap_or_default();
                self.busy[i].set(true);
                spawn_local(async move {
                    let got = saved::remove(id).await;
                    self.busy[i].try_set(false);
                    match got {
                        Ok(list) => {
                            self.saved.set_list(list);
                            self.unbind(level);
                            self.note[i].try_set(Some((false, format!("\u{201c}{was}\u{201d} is unlisted — its doc stays"))));
                        }
                        Err(why) => {
                            self.note[i].try_set(Some((true, format!("not unlisted: {why}"))));
                        }
                    }
                });
            }
            Plan::Rename { id, name } => {
                self.busy[i].set(true);
                spawn_local(async move {
                    let got = saved::rename(id.clone(), name.clone()).await;
                    self.busy[i].try_set(false);
                    match got {
                        Ok(list) => {
                            self.saved.set_list(list);
                            let text = self.written[i].get_value().unwrap_or_default();
                            self.bind(level, Bound { id, name: name.clone() }, text);
                            self.note[i].try_set(Some((false, format!("renamed \u{201c}{name}\u{201d}"))));
                        }
                        Err(Failure::Refused(Refusal::Taken(e))) => {
                            self.taken[i].try_set(Some((name, e)));
                        }
                        Err(Failure::Refused(r)) => {
                            self.note[i].try_set(Some((true, format!("not renamed: {}", r.words()))));
                        }
                        Err(Failure::Failed(why)) => {
                            self.note[i].try_set(Some((true, format!("not renamed: {why}"))));
                        }
                    }
                });
            }
        }
    }

    /// The level's filter saved as `name` (`replace`: over the filter that
    /// holds it), and the level bound to it.
    fn save(self, level: Level, name: String, replace: bool) {
        let i = level.index();
        let f = untrack(|| self.lens.filter(level).get());
        self.busy[i].set(true);
        spawn_local(async move {
            let got = saved::save(f.clone(), name.clone(), replace).await;
            self.busy[i].try_set(false);
            match got {
                Ok((entry, list)) => {
                    self.saved.set_list(list);
                    self.texts.try_update(|m| {
                        m.insert(slug(&entry.name), Fetch::Text(crate::counts::wire_text(&f)));
                    });
                    self.bind(level, Bound { id: entry.id, name: entry.name.clone() }, f.format());
                    self.note[i].try_set(Some((false, format!("saved as \u{201c}{}\u{201d} — every edit keeps it saved", entry.name))));
                }
                Err(Failure::Refused(Refusal::Taken(e))) => {
                    self.taken[i].try_set(Some((name, e)));
                }
                Err(Failure::Refused(r)) => {
                    self.note[i].try_set(Some((true, r.words())));
                }
                Err(Failure::Failed(why)) => {
                    self.note[i].try_set(Some((true, format!("not saved: {why}"))));
                }
            }
        });
    }

    /// The taken prompt's "replace it".
    pub(crate) fn replace(self, level: Level) {
        if let Some((name, _)) = self.taken[level.index()].get_untracked() {
            self.taken[level.index()].set(None);
            self.save(level, name, true);
        }
    }

    /// The taken prompt's "choose another name".
    pub(crate) fn another(self, level: Level) {
        self.taken[level.index()].set(None);
    }

    /// An amend of the bound doc, [`DEBOUNCE_MS`] after the last change.
    fn schedule(self, level: Level) {
        let i = level.index();
        let turn = self.turn[i].get_value().wrapping_add(1);
        self.turn[i].set_value(turn);
        spawn_local(async move {
            dom::sleep_ms(DEBOUNCE_MS).await;
            if self.turn[i].try_get_value() == Some(turn) {
                self.flush(level);
            }
        });
    }

    /// The bound doc amended to the level's filter now — one write at a
    /// time; what changed meanwhile is written after it.
    fn flush(self, level: Level) {
        let i = level.index();
        if self.writing[i].get_value() {
            return;
        }
        let Some(b) = self.bound[i].get_untracked() else { return };
        let f = untrack(|| self.lens.filter(level).get());
        let text = f.format();
        if self.written[i].with_value(|w| w.as_deref() == Some(text.as_str())) {
            return;
        }
        self.writing[i].set_value(true);
        spawn_local(async move {
            let got = saved::amend_filter(&b.id, &f).await;
            // Gone (the app torn down): nothing to follow.
            if self.writing[i].try_set_value(false).is_some() {
                return;
            }
            if self.bound[i].with_untracked(|x| x.as_ref().map(|x| &x.id) != Some(&b.id)) {
                return;
            }
            match got {
                Ok(()) => {
                    store(&self.key(level), Some(&kept_line(&b, &text)));
                    self.written[i].set_value(Some(text.clone()));
                    self.texts.try_update(|m| {
                        m.insert(slug(&b.name), Fetch::Text(crate::counts::wire_text(&f)));
                    });
                    self.note[i].try_set(None);
                    // The last one: a change that landed during the write.
                    if untrack(|| self.lens.filter(level).with(Filter::format)) != text {
                        self.flush(level);
                    }
                }
                Err(why) => {
                    self.note[i].try_set(Some((true, format!("\u{201c}{}\u{201d} not saved: {why}", b.name))));
                }
            }
        });
    }

    /// `name` used now (picked, applied): first among the recent.
    pub(crate) fn touch(self, name: &str) {
        let next = with_recent(&self.recent.get_untracked(), name);
        store(RECENT_KEY, Some(&next.join("\n")));
        self.recent.set(next);
    }

    /// The recently used names. Tracked.
    pub(crate) fn recent(self) -> Vec<String> {
        self.recent.get()
    }

    /// `name`'s text as fetched. Tracked.
    pub(crate) fn text(self, name: &str) -> Option<Fetch> {
        self.texts.with(|m| m.get(&slug(name)).cloned())
    }

    /// What [`crate::anyof::evaluated`] reads: a name's text, `Some(None)`
    /// for one nothing holds, `None` while not fetched. Tracked.
    pub(crate) fn lookup(self, name: &str) -> Option<Option<String>> {
        match self.text(name)? {
            Fetch::Text(t) => Some(Some(t)),
            Fetch::Missing | Fetch::Failed(_) => Some(None),
            Fetch::Loading => None,
        }
    }

    /// `name`'s text read, once (again only when it failed).
    pub(crate) fn fetch(self, name: &str) {
        let key = slug(name);
        if self.texts.with_untracked(|m| matches!(m.get(&key), Some(Fetch::Loading | Fetch::Text(_) | Fetch::Missing))) {
            return;
        }
        self.texts.update(|m| {
            m.insert(key.clone(), Fetch::Loading);
        });
        let name = name.to_string();
        spawn_local(async move {
            let got = self.read(&name).await;
            self.texts.try_update(|m| {
                m.insert(key, got);
            });
        });
    }

    async fn read(self, name: &str) -> Fetch {
        let listed = match self.saved.listing() {
            Listing::Read(l) => l,
            _ => saved::read_list().await.unwrap_or_default(),
        };
        let id = entry_for(&listed, name).map_or_else(|| slug(name), |e| e.id.clone());
        match saved::get_doc(&saved::address_of(&id)).await {
            Ok(Some(t)) => match filter_line(&t.body) {
                Some(text) => Fetch::Text(text.to_string()),
                None => Fetch::Failed("its doc holds no `filter:` line".into()),
            },
            Ok(None) => default_text(name).map_or(Fetch::Missing, |d| Fetch::Text(d.to_string())),
            Err(why) => Fetch::Failed(why),
        }
    }

    /// `name`'s text, fetched now when not known.
    pub(crate) async fn text_now(self, name: &str) -> Result<String, String> {
        if let Some(Fetch::Text(t)) = untrack(|| self.text(name)) {
            return Ok(t);
        }
        match self.read(name).await {
            Fetch::Text(t) => {
                self.texts.try_update(|m| {
                    m.insert(slug(name), Fetch::Text(t.clone()));
                });
                Ok(t)
            }
            Fetch::Missing => Err(format!("no saved filter named \u{201c}{name}\u{201d}")),
            Fetch::Failed(why) => Err(why),
            Fetch::Loading => Err("still reading".into()),
        }
    }

    /// The picker opened: its counts asked again as it shows names.
    pub(crate) fn fresh_counts(self) {
        self.counts.set(HashMap::new());
    }

    /// `name`'s count in the picker (`Some(None)`: asked). Tracked.
    pub(crate) fn count(self, name: &str) -> Option<Option<usize>> {
        self.counts.with(|m| m.get(&slug(name)).copied())
    }

    /// `counts { filter: saved "Name" }`, once per picker.
    pub(crate) fn ask_count(self, name: &str) {
        let key = slug(name);
        if self.counts.with_untracked(|m| m.contains_key(&key)) {
            return;
        }
        self.counts.update(|m| {
            m.insert(key.clone(), None);
        });
        let Some(client) = session::client() else { return };
        let text = saved_text_of(name);
        spawn_local(async move {
            let Ok(filter) = crate::typed::typed(&text) else { return };
            let Ok(events) = client.counts_all(Some(filter), None, session::zone()).await else { return };
            let mut n = 0usize;
            for ev in events {
                match ev {
                    TicketsEvent::Count { held, .. } => n += held as usize,
                    TicketsEvent::Error { .. } => return,
                    _ => {}
                }
            }
            self.counts.try_update(|m| {
                m.insert(key, Some(n));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: &str, name: &str) -> Bound {
        Bound { id: id.into(), name: name.into() }
    }

    #[test]
    fn a_name_plans_its_write() {
        assert_eq!(plan(None, "  "), Plan::Nothing);
        assert_eq!(plan(None, " Mine  or Plexus "), Plan::Save("Mine or Plexus".into()));
        let bound = b("mine-or-plexus", "Mine or Plexus");
        assert_eq!(plan(Some(&bound), "Mine or Plexus"), Plan::Nothing);
        assert_eq!(plan(Some(&bound), ""), Plan::Unlist("mine-or-plexus".into()));
        assert_eq!(plan(Some(&bound), "Theirs"), Plan::Rename { id: "mine-or-plexus".into(), name: "Theirs".into() });
        // The path stays through a rename; the line and the doc are saved.rs's.
        assert_eq!(saved::plan_rename(&[Entry::new("mine-or-plexus", "Mine or Plexus")], "mine-or-plexus", "Theirs"), Ok(Entry::new("mine-or-plexus", "Theirs")));
        // A name another filter holds: Save as…'s refusal, which offers replace.
        let listed = [Entry::new("plexus-readyu", "Plexus-Ready")];
        assert_eq!(saved::plan_save(&listed, "plexus ready", false), Err(Refusal::Taken(listed[0].clone())));
        assert_eq!(saved::plan_save(&listed, "plexus ready", true), Ok(saved::Plan::Replace(Entry::new("plexus-readyu", "plexus ready"))));
        assert_eq!(saved::plan_save(&listed, "Mine or Plexus", false), Ok(saved::Plan::New(Entry::new("mine-or-plexus", "Mine or Plexus"))));
    }

    #[test]
    fn a_binding_is_kept_with_its_text() {
        let k = kept_line(&b("x", "X y"), "kind build and (ready or blocked)");
        assert_eq!(read_kept(&k), Some((b("x", "X y"), "kind build and (ready or blocked)".into())));
        assert_eq!(read_kept("\n\n"), None);
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_picker_puts_the_recent_first_and_searches() {
        let listed = [
            Entry::new("plexus-readyu", "Plexus-Ready"),
            Entry::new("plexus-coding", "Plexus-Coding"),
            Entry::new("created-active-weekly", "Created-Active-Weekly"),
            Entry::new("deployment", "Deployment"),
        ];
        let all = names(&listed);
        assert_eq!(all, strs(&["Plexus-Ready", "Plexus-Coding", "Created-Active-Weekly", "Deployment", "Needs you"]), "Needs you always");
        assert_eq!(names(&[Entry::new("needs-you", "Needs You")]), strs(&["Needs You"]), "listed once");
        let o = order(&all, &strs(&["deployment", "Gone"]), &[], "");
        assert_eq!(o.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Deployment", "Plexus-Ready", "Plexus-Coding", "Created-Active-Weekly", "Needs you"]);
        assert!(o[0].recent && !o[1].recent);
        // The level's own after the recent.
        let o = order(&all, &[], &strs(&["Plexus-Coding"]), "");
        assert_eq!(o[0], Pick { name: "Plexus-Coding".into(), recent: false, own: true });
        // Search: words, case folded; a slug.
        let names_of = |q: &str| order(&all, &[], &[], q).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names_of("ready"), strs(&["Plexus-Ready"]));
        assert_eq!(names_of("PLEXUS"), strs(&["Plexus-Ready", "Plexus-Coding"]));
        assert_eq!(names_of("plexus ready"), strs(&["Plexus-Ready"]));
        assert_eq!(names_of("needs"), strs(&["Needs you"]));
        assert!(names_of("nothing like it").is_empty());
        // Eight shown, the rest counted.
        let crowd: Vec<String> = (0..32).map(|i| format!("Query {i}")).collect();
        let (first, more) = shown(order(&crowd, &[], &[], ""));
        assert_eq!((first.len(), more), (SHOWN, 24));
        let (first, more) = shown(order(&crowd, &[], &[], "query 3"));
        assert_eq!((first.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), more), (vec!["Query 3", "Query 30", "Query 31"], 0));
    }

    #[test]
    fn the_recent_are_kept_first_and_once() {
        let r = with_recent(&strs(&["A", "B"]), "b");
        assert_eq!(r, strs(&["b", "A"]));
        let many: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        assert_eq!(with_recent(&many, "x").len(), RECENT_MAX);
        assert_eq!(entry_for(&[Entry::new("plexus-readyu", "Plexus-Ready")], "plexus ready").map(|e| e.id.as_str()), Some("plexus-readyu"));
        assert_eq!(entry_for(&[Entry::new("old-name", "New Name")], "Old Name").map(|e| e.id.as_str()), Some("old-name"), "by its path, renamed");
        assert_eq!(saved_text_of("Needs you"), "saved \"Needs you\"");
        assert_eq!(saved_text_of("Plexus-Ready"), "saved Plexus-Ready");
    }
}
