//! FILTER-6, FILTER-46: saved filters — a level's filter saved under a name
//! and chosen again later, kept in the tracker's `saved_filters` table
//! (`tickets filters`), so they follow the owner to any browser and agents
//! can read them.
//!
//! - **Where they live**: the server's table, one row per name (matched by
//!   its slug — `Open Builds` is `open builds`, FILTER-36's rule, kept once
//!   in `tickets-api`), written with `tickets.filters.save | rename |
//!   delete` and read with `list` | `get`. They are not tickets.
//! - **The menu** ([`SavedButton`]): `Saved ▾` beside each level's `Filter`
//!   button — the global one in the header, a surface's in its bar. It reads
//!   `filters.list` as it opens (nothing is asked at startup) and lists the
//!   names; a name applies its filter to the level (its pills, its text and
//!   its URL follow — a surface's own categories cleared, since a saved
//!   filter has none); `rename` and `remove` write the table; `Save as…`
//!   opens the dialog.
//! - **Save as…** ([`SaveDialog`]): a name, the canonical text it saves, and
//!   — where the level has a surface-only part (the wave board's Wave,
//!   Readiness, Off-board blockers, Title; the inputs page's State, Removed)
//!   — what is left out. A name whose slug a listed filter's name already
//!   has offers to replace that filter or to choose another name.
//! - **The writes** go through the session's client; the list follows
//!   `watch` (`filter_saved`, `filter_deleted`: another browser's save,
//!   rename or delete). Every refusal (a dead socket, the server's in-band
//!   words — a filter the language cannot read, a cycle) is shown where the
//!   action was taken.
//!
//! - FILTER-37: **naming saves** (src/naming.rs) writes through the same
//!   `save`, `rename` and `remove` — a tree given a name is saved from that
//!   moment and every edit after saves it again ([`amend_filter`]); a filter
//!   chosen here binds its level to its name (the bar shows it), and so
//!   does a Save as…, which stays as a second door.
//!
//! Everything that decides — the slug, collisions, what is left out — is a
//! pure function here, pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::filter::Filter;
use menger_tickets_api::saved_filters::SavedFilter;
use menger_tickets_api::{Ticket, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::filter::Clauses;
use crate::lens::{Kept, Lens, Level};
use crate::{dom, session};

// ---------------------------------------------------------------------
// What they are called

// FILTER-36: how a name is matched is the server's too — `saved "Name"`
// resolves by the same slug — so it is kept once, in `tickets-api`.
#[cfg(test)]
pub(crate) use menger_tickets_api::saved_filters::SLUG_MAX;
/// The longest name kept.
pub(crate) const NAME_MAX: usize = menger_tickets_api::saved_filters::NAME_MAX;

/// FILTER-27: the filters every reader is offered before saving any —
/// "to get attention in one place": what was born today, what moved today,
/// and what is stuck. Each is shown under **Suggested** in `Saved ▾` until a
/// filter of its name is listed; choosing one applies it, `keep` saves it as
/// an ordinary saved filter (renamed and changed like any). Offered by the
/// app rather than written into the tracker, so no install seeds anyone's
/// store.
pub(crate) const SUGGESTED: [(&str, &str); 4] = [
    ("New today", "created today"),
    // FILTER-52: any transition is `moved`; `changed` is the last write.
    ("Moved today", "moved today"),
    ("Stuck", "category in (ready, active, review) for over 1w"),
    // FILTER-30: open work nobody has touched in two weeks.
    ("Untouched 2w", "not changed within 2w and not category done and not kind doc"),
];

/// A name as the server matches it (FILTER-36: the rule `saved "Name"` is
/// resolved by).
pub(crate) use menger_tickets_api::saved_filters::slug;

/// A name as kept: its whitespace runs one space, trimmed, at most
/// [`NAME_MAX`] characters.
pub(crate) fn clean_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX).collect::<String>().trim_end().to_string()
}

/// A saved filter as the list shows it: its id (its name's slug — what the
/// server finds it by, whatever its spelling) and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) name: String,
}

impl Entry {
    /// The entry of the name `name`.
    pub(crate) fn of(name: &str) -> Self {
        Entry { id: slug(name), name: name.to_string() }
    }
}

#[cfg(test)]
impl Entry {
    pub(crate) fn new(id: &str, name: &str) -> Self {
        Entry { id: id.to_string(), name: name.to_string() }
    }
}

// ---------------------------------------------------------------------
// Names: collisions

/// Why a name is not taken as asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// It has no letter or digit: no path can be made of it.
    NoName,
    /// A listed filter's name is the same name (by slug).
    Taken(Entry),
    /// The filter is no longer in the list.
    Unlisted,
}

impl Refusal {
    pub(crate) fn words(&self) -> String {
        match self {
            Refusal::NoName => "a name needs a letter or a digit".into(),
            Refusal::Taken(e) => format!("\u{201c}{}\u{201d} is already saved", e.name),
            Refusal::Unlisted => "it is no longer in the list — reopen Saved".into(),
        }
    }
}

/// The listed filter whose name is `name`, compared by slug (`Open Builds`
/// is `open builds`).
pub(crate) fn taken_by<'a>(listed: &'a [Entry], name: &str) -> Option<&'a Entry> {
    let s = slug(name);
    (!s.is_empty()).then(|| listed.iter().find(|e| slug(&e.name) == s)).flatten()
}

/// What a Save as… writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// A new filter, under its name.
    New(Entry),
    /// The listed filter of that name replaced (its spelling taken from the
    /// new name).
    Replace(Entry),
}

impl Plan {
    pub(crate) fn entry(&self) -> &Entry {
        match self {
            Plan::New(e) | Plan::Replace(e) => e,
        }
    }
}

/// Save as… `raw` over the listed filters: a new filter; or, the name
/// already saved, that filter replaced (`replace`) or a refusal naming it.
pub(crate) fn plan_save(listed: &[Entry], raw: &str, replace: bool) -> Result<Plan, Refusal> {
    let name = clean_name(raw);
    let s = slug(&name);
    if s.is_empty() {
        return Err(Refusal::NoName);
    }
    match taken_by(listed, &name) {
        Some(e) if replace => Ok(Plan::Replace(Entry { id: e.id.clone(), name })),
        Some(e) => Err(Refusal::Taken(e.clone())),
        None => Ok(Plan::New(Entry { id: s, name })),
    }
}

/// Rename `id` to `raw`: the entry as it becomes (its id the new name's slug) — or a refusal: no name,
/// `id` not listed, or another listed filter already named so.
pub(crate) fn plan_rename(listed: &[Entry], id: &str, raw: &str) -> Result<Entry, Refusal> {
    let name = clean_name(raw);
    let s = slug(&name);
    if s.is_empty() {
        return Err(Refusal::NoName);
    }
    if !listed.iter().any(|e| e.id == id) {
        return Err(Refusal::Unlisted);
    }
    if let Some(e) = listed.iter().find(|e| e.id != id && slug(&e.name) == s) {
        return Err(Refusal::Taken(e.clone()));
    }
    Ok(Entry { id: s, name })
}

/// The collision prompt's words: the name typed is already saved — under
/// another spelling, which it says.
pub(crate) fn taken_words(typed: &str, e: &Entry) -> String {
    let spelled = (typed != e.name).then(|| format!(", as \u{201c}{}\u{201d}", e.name)).unwrap_or_default();
    format!("\u{201c}{typed}\u{201d} is already saved{spelled} — replace that filter, or choose another name?")
}

// ---------------------------------------------------------------------
// What a level saves

/// Whose filter a level's is, as the dialog says it.
pub(crate) fn level_words(level: Level) -> &'static str {
    match level {
        Level::Global => "the global filter",
        Level::Nav => "the nav's filter",
        Level::Board => "the board's filter",
        Level::Waves => "the wave board's filter",
        Level::Inputs => "the inputs page's filter",
    }
}

/// Whose a level's surface-only part is.
pub(crate) fn surface_words(level: Level) -> &'static str {
    match level {
        Level::Waves => "the wave board's own",
        Level::Inputs => "the inputs page's own",
        _ => "this view's own",
    }
}

/// What a saved filter becomes at a level: its filter, nothing else (a
/// surface's own categories are not in it).
pub(crate) fn chosen(filter: Filter) -> Kept {
    Kept { filter, local: Clauses::default() }
}

// ---------------------------------------------------------------------
// The writes

/// What a ticket read's events say: the ticket — or the server's refusal.
fn ticket_of(events: Vec<TicketsEvent>) -> Result<Ticket, String> {
    for ev in events {
        match ev {
            TicketsEvent::Amended { ticket, .. } | TicketsEvent::Created { ticket } | TicketsEvent::Ticket { ticket } => return Ok(ticket),
            TicketsEvent::NotFound { id } => return Err(format!("{id}: not found")),
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Err("no answer".into())
}

/// A ticket by id — `None` when nothing is there.
pub(crate) async fn get_doc(id: &str) -> Result<Option<Ticket>, String> {
    let client = connected()?;
    let events = client.get_all(crate::typed::typed(id)?).await.map_err(|e| session::describe(&e))?;
    if events.iter().any(|e| matches!(e, TicketsEvent::NotFound { .. })) {
        return Ok(None);
    }
    ticket_of(events).map(Some)
}

fn connected() -> Result<session::Client, String> {
    session::client().ok_or_else(|| "not connected yet".to_string())
}

/// The filters a `tickets.filters` answer holds — or the server's refusal.
fn filters_of(events: Vec<TicketsEvent>) -> Result<Vec<SavedFilter>, String> {
    let mut out = Vec::new();
    for ev in events {
        match ev {
            TicketsEvent::FilterSaved { filter, .. } => out.push(filter),
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(out)
}

/// What a server says of a name it has no filter for.
const NONE_NAMED: &str = "no saved filter named";

/// `filters.get { name }` — `None` when no filter is saved under it. Needs
/// you's default is no saved filter (the server answers it, saved at 0).
pub(crate) async fn fetch_row(name: &str) -> Result<Option<SavedFilter>, String> {
    let client = connected()?;
    let events = client.filters().get_all(crate::typed::typed(name)?).await.map_err(|e| session::describe(&e))?;
    match filters_of(events) {
        Ok(rows) => Ok(rows.into_iter().next()),
        Err(why) if why.contains(NONE_NAMED) => Ok(None),
        Err(why) => Err(why),
    }
}

/// The saved filters, read now (Needs you's default left out until it is
/// saved).
pub(crate) async fn read_list() -> Result<Vec<Entry>, String> {
    let events = connected()?.filters().list_all().await.map_err(|e| session::describe(&e))?;
    Ok(filters_of(events)?.into_iter().filter(|f| f.saved_at != 0).map(|f| Entry::of(&f.name)).collect())
}

/// A failed Save as… or rename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Refused(Refusal),
    Failed(String),
}

/// `filters.save { name, filter }` — the row saved.
async fn put(client: &session::Client, name: &str, filter: &Filter) -> Result<SavedFilter, String> {
    use crate::typed::typed;
    let events = client.filters().save_all(typed(name)?, typed(&filter.format())?).await.map_err(|e| session::describe(&e))?;
    filters_of(events)?.into_iter().next().ok_or_else(|| "no answer".to_string())
}

/// Save as… `name` with `filter`: the entry saved and the list after.
pub(crate) async fn save(filter: Filter, name: String, replace: bool) -> Result<(Entry, Vec<Entry>), Failure> {
    let failed = Failure::Failed;
    let client = connected().map_err(failed)?;
    let listed = read_list().await.map_err(failed)?;
    let plan = plan_save(&listed, &name, replace).map_err(Failure::Refused)?;
    let entry = plan.entry().clone();
    put(&client, &entry.name, &filter).await.map_err(failed)?;
    // MENGER-43: Needs you's count follows a save of its filter.
    crate::needs::saved_changed(&entry.id, &filter);
    Ok((entry, read_list().await.map_err(failed)?))
}

/// Rename `id` to `name` — the list after.
pub(crate) async fn rename(id: String, name: String) -> Result<Vec<Entry>, Failure> {
    use crate::typed::typed;
    let failed = Failure::Failed;
    let client = connected().map_err(failed)?;
    let entry = plan_rename(&read_list().await.map_err(failed)?, &id, &name).map_err(Failure::Refused)?;
    let (was, to) = (typed(&id).map_err(failed)?, typed(&entry.name).map_err(failed)?);
    let events = client.filters().rename_all(was, to).await.map_err(|e| failed(session::describe(&e)))?;
    filters_of(events).map_err(failed)?;
    read_list().await.map_err(failed)
}

/// Delete the filter `id` — the list after.
pub(crate) async fn remove(id: String) -> Result<Vec<Entry>, String> {
    let events = connected()?.filters().delete_all(crate::typed::typed(&id)?).await.map_err(|e| session::describe(&e))?;
    if let Some(TicketsEvent::Error { message }) = events.into_iter().find(|e| matches!(e, TicketsEvent::Error { .. })) {
        return Err(message);
    }
    read_list().await
}

/// FILTER-37: the saved filter `id` saved again as `filter` — what naming
/// saves writes after every edit of a named tree.
pub(crate) async fn amend_filter(id: &str, filter: &Filter) -> Result<(), String> {
    let client = connected()?;
    let row = fetch_row(id).await?.ok_or_else(|| format!("{NONE_NAMED} \"{id}\""))?;
    if Filter::parse(&row.filter).ok().as_ref() != Some(filter) {
        put(&client, &row.name, filter).await?;
    }
    crate::needs::saved_changed(id, filter);
    Ok(())
}

/// MENGER-12: the filter saved under `id`, or `None` while nothing is saved
/// under it — Needs you reads its saved filter this way (MENGER-43: on every
/// `g n` too), and falls back to its default.
pub(crate) async fn load_if_saved(id: &str) -> Result<Option<Filter>, String> {
    match fetch_row(id).await? {
        Some(row) if row.saved_at != 0 => Filter::parse(&row.filter).map(Some).map_err(|e| format!("{id}: {e}")),
        _ => Ok(None),
    }
}

/// The filter saved under `id`.
async fn load(id: String) -> Result<Filter, String> {
    let row = fetch_row(&id).await?.ok_or_else(|| format!("{NONE_NAMED} \"{id}\""))?;
    Filter::parse(&row.filter).map_err(|e| format!("{id}: {e}"))
}

// ---------------------------------------------------------------------
// The state

/// The saved filters, as the app last read them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Listing {
    Unread,
    Reading,
    Read(Vec<Entry>),
    Failed(String),
}


/// Saved filters for the app's lifetime — every handle `Copy`; [`use_saved`]
/// finds it.
#[derive(Clone, Copy)]
pub(crate) struct Saved {
    lens: Lens,
    list: RwSignal<Listing>,
    /// A read of the collection is in flight.
    reading: RwSignal<bool>,
    /// The level whose Saved menu is open.
    open: RwSignal<Option<Level>>,
    /// In the open menu: the entry being renamed and its draft, the entry
    /// whose removal waits for a yes, what went wrong, a write in flight.
    renaming: RwSignal<Option<String>>,
    removing: RwSignal<Option<String>>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    /// The Save as… dialog, while it is up: its subject, the name typed,
    /// the listed filter the name collides with, what went wrong, a write in
    /// flight.
    /// What was just done, beside a level's Saved button, for a moment.
    flash: RwSignal<Option<(Level, String)>>,
    flashes: StoredValue<u32>,
}

thread_local! {
    /// MENGER-43: the app's saved filters, for `g n` (a key, outside any
    /// view).
    static HOOK: std::cell::Cell<Option<Saved>> = const { std::cell::Cell::new(None) };
}

/// The app's saved filters, from anywhere (`None` before the App made them).
pub(crate) fn hook() -> Option<Saved> {
    HOOK.with(std::cell::Cell::get)
}

fn saved_sel(level: Level) -> String {
    format!("[data-saved=\"{}\"]", level.name())
}

fn focus(sel: &str) {
    if let Some(el) = dom::window()
        .document()
        .and_then(|d| d.query_selector(sel).ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = el.focus();
    }
}

impl Saved {
    /// Saved filters, provided to every view under the caller; a mousedown
    /// outside an open menu closes it.
    pub(crate) fn new(lens: Lens) -> Self {
        let s = Saved {
            lens,
            list: RwSignal::new(Listing::Unread),
            reading: RwSignal::new(false),
            open: RwSignal::new(None),
            renaming: RwSignal::new(None),
            removing: RwSignal::new(None),
            problem: RwSignal::new(None),
            busy: RwSignal::new(false),
            flash: RwSignal::new(None),
            flashes: StoredValue::new(0),
        };
        window_event_listener(leptos::ev::mousedown, move |ev: web_sys::MouseEvent| {
            let Some(level) = s.open.get_untracked() else { return };
            let inside = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|el| el.closest(&saved_sel(level)).ok().flatten().is_some());
            if !inside {
                s.close_menu(false);
            }
        });
        // Once read, the list follows `watch`: a filter saved, renamed or
        // deleted (another browser's) is read again.
        session::listen(move |ev| {
            if matches!(ev, TicketsEvent::FilterSaved { .. } | TicketsEvent::FilterDeleted { .. }) && s.list.try_with_untracked(|l| matches!(l, Listing::Read(_))) == Some(true) {
                s.refresh();
            }
        });
        provide_context(s);
        HOOK.with(|h| h.set(Some(s)));
        s
    }

    /// FILTER-37: the collection's list as last read.
    pub(crate) fn listing(self) -> Listing {
        self.list.get_untracked()
    }

    /// FILTER-37: the collection's list as a write answered it.
    pub(crate) fn set_list(self, list: Vec<Entry>) {
        self.list.try_set(Listing::Read(list));
    }

    /// FILTER-37: the list's names (tracked) — read once when unread.
    pub(crate) fn listed(self) -> Vec<Entry> {
        if self.list.with_untracked(|l| matches!(l, Listing::Unread)) && !self.reading.get_untracked() {
            self.refresh();
        }
        self.list.with(|l| match l {
            Listing::Read(list) => list.clone(),
            _ => Vec::new(),
        })
    }

    /// FILTER-53: the list's names, untracked (read once when unread).
    pub(crate) fn listed_untracked(self) -> Vec<Entry> {
        untrack(|| self.listed())
    }

    /// The collection read again; the list shown meanwhile is kept.
    pub(crate) fn refresh(self) {
        if !matches!(self.list.get_untracked(), Listing::Read(_)) {
            self.list.set(Listing::Reading);
        }
        self.reading.set(true);
        spawn_local(async move {
            let next = match read_list().await {
                Ok(list) => Listing::Read(list),
                Err(why) => Listing::Failed(why),
            };
            if self.list.try_with_untracked(|l| *l != next) == Some(true) {
                self.list.try_set(next);
            }
            self.reading.try_set(false);
        });
    }

    /// The menu closed — focus back on its button when a key closed it.
    fn close_menu(self, restore: bool) {
        let Some(level) = self.open.get_untracked() else { return };
        self.open.set(None);
        self.renaming.set(None);
        self.removing.set(None);
        self.problem.set(None);
        if restore {
            focus(&format!("{} button.sbtn", saved_sel(level)));
        }
    }

    /// FILTER-41: what is flashed at `level` now, beside its line. Tracked.
    pub(crate) fn flash_for(self, level: Level) -> Option<String> {
        self.flash.get().filter(|(l, _)| *l == level).map(|(_, w)| w)
    }

    /// Words flashed beside `level`'s `Saved ▾` — MENGER-43's `g n` says
    /// what it applied there too.
    pub(crate) fn say(self, level: Level, words: String) {
        let n = self.flashes.get_value() + 1;
        self.flashes.set_value(n);
        self.flash.set(Some((level, words)));
        spawn_local(async move {
            dom::sleep_ms(4000).await;
            if self.flashes.try_get_value() == Some(n) {
                self.flash.try_set(None);
            }
        });
    }

    /// FILTER-27: a suggestion chosen — its filter becomes the level's.
    pub(crate) fn suggest(self, level: Level, name: &'static str, text: &'static str) {
        let Ok(filter) = Filter::parse(text) else { return };
        self.lens.apply(level, Some(chosen(filter)));
        self.close_menu(true);
        self.say(level, format!("\u{201c}{name}\u{201d} applied"));
    }

    /// A saved filter chosen: its filter becomes the level's.
    pub(crate) fn choose(self, level: Level, e: Entry) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.problem.set(None);
        spawn_local(async move {
            let got = load(e.id.clone()).await;
            self.busy.try_set(false);
            match got {
                Ok(filter) => {
                    // FILTER-37: chosen, the level is that filter, by name.
                    if let Some(n) = crate::naming::hook() {
                        n.bind(level, crate::naming::Bound { id: e.id.clone(), name: e.name.clone() }, filter.format());
                    }
                    self.lens.apply(level, Some(chosen(filter)));
                                self.close_menu(true);
                    self.say(level, format!("\u{201c}{}\u{201d} applied", e.name));
                }
                Err(why) => {
                    self.problem.try_set(Some(format!("not applied: {why}")));
                }
            }
        });
    }

}

// ---------------------------------------------------------------------
// The dialog

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suggestions_parse() {
        for (name, text) in SUGGESTED {
            let f = Filter::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(f.format(), text, "{name}: canonical");
        }
        assert_eq!(SUGGESTED.len(), 4);
    }

    /// The slug: lower case, letters and digits, one `-` per run of anything
    /// else, none at the ends — empty for a name with no letter or digit;
    /// at most 64 characters, never ending in `-`.
    #[test]
    fn a_name_becomes_a_slug() {
        assert_eq!(slug("open builds"), "open-builds");
        assert_eq!(slug("Open Builds"), "open-builds");
        assert_eq!(slug("  open   builds!! "), "open-builds");
        assert_eq!(slug("MNG: in review / mine"), "mng-in-review-mine");
        assert_eq!(slug("C++ & Rust"), "c-rust");
        assert_eq!(slug("v2.0"), "v2-0");
        assert_eq!(slug("snake_case"), "snake-case");
        assert_eq!(slug("Über Läufe"), "über-läufe", "letters of any script are letters");
        assert_eq!(slug("--"), "");
        assert_eq!(slug("   "), "");
        assert_eq!(slug("· ✓ ·"), "");
        let long = slug(&"ab ".repeat(40));
        assert!(long.chars().count() <= SLUG_MAX && !long.ends_with('-'), "{long}");
        assert_eq!(clean_name("  open \t builds  "), "open builds");
        assert_eq!(clean_name(&"x".repeat(200)).chars().count(), NAME_MAX);
        assert_eq!(Entry::of("Open Builds"), Entry::new("open-builds", "Open Builds"));
    }

    /// Name collisions: a name a listed filter has (by slug) is refused,
    /// naming it — or, asked, that filter is replaced under the new
    /// spelling; a new name is its slug; a name with no letter or digit is
    /// no name.
    #[test]
    fn a_name_already_saved_is_offered_for_replacing() {
        let listed = vec![Entry::new("open-builds", "open builds"), Entry::new("my-reviews", "my reviews")];
        assert_eq!(plan_save(&listed, "Open  Builds", false), Err(Refusal::Taken(listed[0].clone())));
        assert_eq!(plan_save(&listed, "open builds!", true), Ok(Plan::Replace(Entry::new("open-builds", "open builds!"))));
        assert_eq!(plan_save(&listed, "My Reviews", true), Ok(Plan::Replace(Entry::new("my-reviews", "My Reviews"))));
        assert_eq!(plan_save(&listed, "blocked work", false), Ok(Plan::New(Entry::new("blocked-work", "blocked work"))));
        assert_eq!(plan_save(&listed, " ?! ", false), Err(Refusal::NoName));
        assert_eq!(plan_save(&[], "open builds", false), Ok(Plan::New(Entry::new("open-builds", "open builds"))));
        assert_eq!(Refusal::Taken(listed[0].clone()).words(), "\u{201c}open builds\u{201d} is already saved");
        assert_eq!(taken_words("open builds", &listed[0]), "\u{201c}open builds\u{201d} is already saved — replace that filter, or choose another name?");
        assert_eq!(
            taken_words("Open Builds", &listed[0]),
            "\u{201c}Open Builds\u{201d} is already saved, as \u{201c}open builds\u{201d} — replace that filter, or choose another name?"
        );
        assert_eq!(taken_by(&listed, "OPEN-builds").map(|e| e.id.as_str()), Some("open-builds"));
        assert_eq!(taken_by(&listed, "--"), None);
    }

    /// A rename: to a name no other listed filter has (its own, respelled,
    /// is fine); refused for no name, an unlisted filter, or another's name.
    /// The entry's id becomes the new name's slug (the server finds a filter
    /// by it).
    #[test]
    fn a_rename_keeps_names_apart() {
        let listed = vec![Entry::new("open-builds", "open builds"), Entry::new("my-reviews", "my reviews")];
        assert_eq!(plan_rename(&listed, "open-builds", "builds in flight"), Ok(Entry::new("builds-in-flight", "builds in flight")));
        assert_eq!(plan_rename(&listed, "open-builds", "Open Builds"), Ok(Entry::new("open-builds", "Open Builds")));
        assert_eq!(plan_rename(&listed, "open-builds", "My  reviews"), Err(Refusal::Taken(listed[1].clone())));
        assert_eq!(plan_rename(&listed, "gone", "anything"), Err(Refusal::Unlisted));
        assert_eq!(plan_rename(&listed, "my-reviews", "  "), Err(Refusal::NoName));
    }
}
