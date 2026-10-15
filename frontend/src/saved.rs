//! FILTER-6: saved filters — a level's filter saved under a name and chosen
//! again later, kept in the tracker as docs, so they follow the owner to any
//! browser and agents can read them.
//!
//! - **Where they live**: one collection, `VIEW-1`, a doc titled "Saved
//!   filters" hanging under `FILTER-1` (the first save creates it), whose
//!   body lists the saved filters in order, one line each:
//!   `- <m-demand id="open-builds">open builds</m-demand>` — DEMAND-1's
//!   element, written now so the list is ready for it (`<m-demand>` is not a
//!   registered component yet: it renders as its text). Each saved filter is
//!   a doc under `VIEW-1` at the path `_MENGER/view/<slug>` (WAVE-10: `get
//!   VIEW/_MENGER/view/open-builds`), titled with its name, whose body is the
//!   filter as canonical text on one line: `filter: kind in (build) and not
//!   status in (Complete)`. The nav hides `_MENGER/` docs by default.
//! - **The menu** ([`SavedButton`]): `Saved ▾` beside each level's `Filter`
//!   button — the global one in the header, a surface's in its bar. It reads
//!   `VIEW-1` as it opens (nothing is asked at startup) and lists the names
//!   in its order; a name applies its filter to the level (its pills, its
//!   text and its URL follow — a surface's own categories cleared, since a
//!   saved filter has none); `rename` changes the doc's title and the line
//!   together; `remove` takes the line out (the doc stays, unlisted: a doc
//!   has no removal); `Save as…` opens the dialog.
//! - **Save as…** ([`SaveDialog`]): a name, the canonical text it saves, and
//!   — where the level has a surface-only part (the wave board's Wave,
//!   Readiness, Off-board blockers, Title; the inputs page's State, Removed)
//!   — what is left out. A name whose slug a listed filter's name already
//!   has offers to replace that filter or to choose another name. A new
//!   name's path is its slug, or — a listed filter (renamed since) holding
//!   that path — the first free `slug-2`, `slug-3`, …; an unlisted doc at the
//!   path (a removed filter) is taken over rather than refused.
//! - **The writes**: `create` and `amend` through the session's client, as
//!   the board annotation's (WAVE-11, src/notes.rs) — the collection first,
//!   then the doc, then the collection's line, so a failed write never
//!   leaves a line naming nothing. Every refusal (a dead socket, the
//!   server's in-band words) is shown where the action was taken.
//!
//! - FILTER-37: **naming saves** (src/naming.rs) writes through the same
//!   `save`, `rename` and `remove` — a tree given a name is saved from that
//!   moment and every edit after amends its doc ([`amend_filter`]); a filter
//!   chosen here binds its level to its name (the bar shows it), and so
//!   does a Save as…, which stays as a second door.
//!
//! Everything that decides — the slug, the list read from `VIEW-1` and
//! rewritten, the saved text and its reading, collisions and free paths,
//! what is left out — is a pure function here, pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::filter::Filter;
use menger_tickets_api::{Ticket, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::filter::{pill_words, Clauses, Pill};
use crate::lens::{Kept, Lens, Level};
use crate::bindings::{self, Context};
use crate::{dom, keys, session};

// ---------------------------------------------------------------------
// Where they live

// FILTER-36: where they live and how a name is matched are the server's
// too — `saved "Name"` resolves by the same slug — so they are kept once,
// in `tickets-api`.
pub(crate) use menger_tickets_api::saved_filters::{COLLECTION, EPIC, FILTER_LINE, PATH_PREFIX};
#[cfg(test)]
pub(crate) use menger_tickets_api::saved_filters::SLUG_MAX;
/// The collection's title.
pub(crate) const COLLECTION_TITLE: &str = "Saved filters";
/// What the collection hangs under: the filters' scope.
pub(crate) const COLLECTION_PARENT: &str = "FILTER-1";
/// The longest name kept.
pub(crate) const NAME_MAX: usize = 120;

/// FILTER-27: the filters every reader is offered before saving any —
/// "to get attention in one place": what was born today, what moved today,
/// and what is stuck. Each is shown under **Suggested** in `Saved ▾` until a
/// filter of its name is listed; choosing one applies it, `keep` saves it as
/// an ordinary saved filter (a doc under `VIEW-1`, renamed and changed like
/// any). Offered by the app rather than written into the tracker, so no
/// install seeds docs into anyone's store.
pub(crate) const SUGGESTED: [(&str, &str); 4] = [
    ("New today", "created today"),
    ("Moved today", "changed today"),
    ("Stuck", "category in (ready, active, review) for over 1w"),
    // FILTER-30: open work nobody has touched in two weeks.
    ("Untouched 2w", "not touched within 2w and not category done and not kind doc"),
];

/// The suggestions not yet listed (by their name's slug).
pub(crate) fn suggestions(listed: &[Entry]) -> Vec<(&'static str, &'static str)> {
    SUGGESTED.into_iter().filter(|(name, _)| !listed.iter().any(|e| slug(&e.name) == slug(name) || e.id == slug(name))).collect()
}

/// A name as a path's last part (FILTER-36: the server's rule, the one
/// `saved "Name"` is resolved by).
pub(crate) use menger_tickets_api::saved_filters::slug;

/// A saved filter's path in the collection's shortcode.
pub(crate) fn path_of(id: &str) -> String {
    format!("{PATH_PREFIX}{id}")
}

/// A saved filter's path address — what `get` finds it by.
pub(crate) fn address_of(id: &str) -> String {
    format!("{EPIC}/{}", path_of(id))
}

/// A name as kept: its whitespace runs one space, trimmed, at most
/// [`NAME_MAX`] characters.
pub(crate) fn clean_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX).collect::<String>().trim_end().to_string()
}

// ---------------------------------------------------------------------
// The collection's list

/// A saved filter as the collection lists it: its id (the slug its doc's
/// path ends in) and its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[cfg(test)]
impl Entry {
    pub(crate) fn new(id: &str, name: &str) -> Self {
        Entry { id: id.to_string(), name: name.to_string() }
    }
}

/// One `<m-demand id="…">…</m-demand>` element of a body, by byte offsets:
/// the element, and its text inside.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Demand {
    id: String,
    start: usize,
    inner: std::ops::Range<usize>,
    end: usize,
}

const OPEN: &str = "<m-demand";
const CLOSE: &str = "</m-demand>";

/// Every `<m-demand>` element of `body` with an `id`, in order.
fn demands(body: &str) -> Vec<Demand> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = body[from..].find(OPEN).map(|i| from + i) {
        let after = at + OPEN.len();
        from = after;
        // `<m-demands` is another element.
        if !body[after..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            continue;
        }
        let Some(gt) = body[after..].find('>').map(|i| after + i) else { break };
        let Some(close) = body[gt + 1..].find(CLOSE).map(|i| gt + 1 + i) else { break };
        let end = close + CLOSE.len();
        from = end;
        if let Some(id) = attr(&body[after..gt], "id").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            out.push(Demand { id, start: at, inner: gt + 1..close, end });
        }
    }
    out
}

/// The value of `name` among a tag's attributes (`id="a"`, `id='a'`, `id=a`),
/// its entities read.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let k = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let key = &attrs[k..i];
        if key.is_empty() {
            i += 1;
            continue;
        }
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = None;
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let v = i + 1;
                i = v;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = Some(&attrs[v..i.min(b.len())]);
                i += 1;
            } else {
                let v = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = Some(&attrs[v..i]);
            }
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(unescape(value.unwrap_or("")));
        }
    }
    None
}

/// Text for HTML: `&`, `<`, `>` and `"` as entities.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// HTML text read: the entities [`escape`] writes, `&#39;` and numeric
/// ones; an unknown entity stays as written.
pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let semi = tail.find(';').filter(|i| *i <= 10);
        let named = semi.and_then(|i| {
            let e = &tail[1..i];
            match e {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => e
                    .strip_prefix("#x")
                    .or_else(|| e.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| e.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            }
            .map(|c| (c, i))
        });
        match named {
            Some((c, i)) => {
                out.push(c);
                rest = &tail[i + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A demand's text as a name: tags stripped, entities read, whitespace
/// collapsed.
fn text_of(raw: &str) -> String {
    let mut plain = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    clean_name(&unescape(&plain))
}

/// The saved filters `VIEW-1`'s body lists, in its order — every
/// `<m-demand id>` element, the first of an id kept; a demand with no text
/// is named by its id.
pub(crate) fn entries(body: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for d in demands(body) {
        if out.iter().any(|e| e.id == d.id) {
            continue;
        }
        let name = text_of(&body[d.inner.clone()]);
        let name = if name.is_empty() { d.id.clone() } else { name };
        out.push(Entry { id: d.id, name });
    }
    out
}

/// A saved filter's line in the collection.
pub(crate) fn demand_line(e: &Entry) -> String {
    format!("- <m-demand id=\"{}\">{}</m-demand>", escape(&e.id), escape(&e.name))
}

/// The body with `e`'s line added after the last listed one — at its end
/// when it lists none.
pub(crate) fn with_added(body: &str, e: &Entry) -> String {
    let line = demand_line(e);
    match demands(body).last() {
        Some(last) => {
            let eol = body[last.end..].find('\n').map_or(body.len(), |i| last.end + i);
            format!("{}\n{line}{}", &body[..eol], &body[eol..])
        }
        None if body.trim().is_empty() => line,
        None => format!("{}\n\n{line}", body.trim_end()),
    }
}

/// The body with `id`'s name now `name` — `None` when it does not list
/// `id`.
pub(crate) fn with_renamed(body: &str, id: &str, name: &str) -> Option<String> {
    let d = demands(body).into_iter().find(|d| d.id == id)?;
    Some(format!("{}{}{}", &body[..d.inner.start], escape(name), &body[d.inner.end..]))
}

/// The body without `id`'s line — the whole line when the element was all
/// of it (a list item), else the element alone; `None` when it does not
/// list `id`. Every element of that id goes.
pub(crate) fn with_removed(body: &str, id: &str) -> Option<String> {
    let d = demands(body).into_iter().find(|d| d.id == id)?;
    let line_start = body[..d.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = body[d.end..].find('\n').map_or(body.len(), |i| d.end + i);
    let rest = format!("{}{}", &body[line_start..d.start], &body[d.end..line_end]);
    let next = if matches!(rest.trim(), "" | "-" | "*" | "+") {
        if line_end < body.len() {
            // The last line of a list between blank lines: one blank line
            // is left, not two.
            let (before, mut after) = (&body[..line_start], &body[line_end + 1..]);
            if before.ends_with("\n\n") && after.starts_with('\n') {
                after = &after[1..];
            }
            format!("{before}{after}")
        } else {
            body[..line_start].trim_end_matches('\n').to_string()
        }
    } else {
        format!("{}{}", &body[..d.start], &body[d.end..])
    };
    Some(with_removed(&next, id).unwrap_or(next))
}

// ---------------------------------------------------------------------
// A saved filter's doc

/// A saved filter's body: its filter as canonical text, on one line.
pub(crate) fn saved_text(f: &Filter) -> String {
    format!("{FILTER_LINE} {}", f.format())
}

/// The filter a saved filter's body holds — its first `filter:` line, read
/// by the language; else what is wrong.
pub(crate) fn filter_of(body: &str) -> Result<Filter, String> {
    let text = body
        .lines()
        .find_map(|l| l.trim().strip_prefix(FILTER_LINE))
        .ok_or_else(|| format!("it holds no `{FILTER_LINE}` line"))?;
    Filter::parse(text.trim()).map_err(|e| {
        e.chunks.into_iter().map(|c| format!("`{}` — {}", c.text, c.why)).collect::<Vec<_>>().join("; ")
    })
}

// ---------------------------------------------------------------------
// Names: collisions and free paths

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

/// The path id for slug `s`: `s`, or — a listed filter holding it (one
/// renamed since it was saved) — the first of `s-2`, `s-3`, … none holds.
pub(crate) fn free_slug(listed: &[Entry], s: &str) -> String {
    let held = |c: &str| listed.iter().any(|e| e.id == c);
    if !held(s) {
        return s.to_string();
    }
    (2..).map(|n| format!("{s}-{n}")).find(|c| !held(c)).expect("a free number")
}

/// What a Save as… writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// A new line, and the doc at its id's path (created, or — unlisted
    /// there already — taken over).
    New(Entry),
    /// The listed filter of that id replaced: its doc's filter and title,
    /// its line's name.
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
        None => Ok(Plan::New(Entry { id: free_slug(listed, &s), name })),
    }
}

/// Rename `id` to `raw`: the entry as it becomes — or a refusal: no name,
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
    Ok(Entry { id: id.to_string(), name })
}

/// The collision prompt's words: the name typed is already saved — under
/// another spelling, which it says.
pub(crate) fn taken_words(typed: &str, e: &Entry) -> String {
    let spelled = (typed != e.name).then(|| format!(", as \u{201c}{}\u{201d}", e.name)).unwrap_or_default();
    format!("\u{201c}{typed}\u{201d} is already saved{spelled} — replace that filter, or choose another name?")
}

// ---------------------------------------------------------------------
// What a level saves

/// What a level's surface-only part is, in its words — each local clause
/// as its pill reads (`Wave is 1`): what Save as… leaves out.
pub(crate) fn left_out(k: &Kept) -> Vec<String> {
    k.local
        .iter()
        .map(|(cat, clause)| {
            let (c, v) = pill_words(&Pill::Clause { cat, clause: clause.clone() });
            format!("{c} {v}")
        })
        .collect()
}

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

/// What a write's events say: the ticket — or the server's refusal.
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

/// `get { id }` — `None` when nothing is there.
pub(crate) async fn get_doc(id: &str) -> Result<Option<Ticket>, String> {
    get(&connected()?, id).await
}

/// `get { id }` — `None` when nothing is there.
async fn get(client: &session::Client, id: &str) -> Result<Option<Ticket>, String> {
    let events = client.get_all(crate::typed::typed(id)?).await.map_err(|e| session::describe(&e))?;
    if events.iter().any(|e| matches!(e, TicketsEvent::NotFound { .. })) {
        return Ok(None);
    }
    ticket_of(events).map(Some)
}

/// `amend { id, title, body }` — the ticket as it is after.
async fn amend(client: &session::Client, id: &str, title: Option<String>, body: Option<String>) -> Result<Ticket, String> {
    let (ticket, title, body) = (crate::typed::typed(id)?, crate::typed::opt(title)?, body.map(|b| crate::typed::typed(&b)).transpose()?);
    let events = client.amend_all(ticket, title, body).await.map_err(|e| session::describe(&e))?;
    ticket_of(events).map_err(|m| format!("amend {id}: {m}"))
}

/// `create { epic, title, kind: doc, body, parent, path }`.
async fn create_doc(client: &session::Client, title: &str, body: String, parent: &str, path: Option<String>) -> Result<Ticket, String> {
    use crate::typed::{opt, typed};
    let events = client
        .create_all(typed(EPIC)?, typed(title)?, Some(menger_tickets_api::Kind::Doc), None, None, Some(typed(&body)?), Some(typed(parent)?), opt(path)?, None)
        .await
        .map_err(|e| session::describe(&e))?;
    ticket_of(events).map_err(|m| format!("create: {m}"))
}

fn connected() -> Result<session::Client, String> {
    session::client().ok_or_else(|| "not connected yet".to_string())
}

/// The collection's list, read now — empty while there is no collection.
pub(crate) async fn read_list() -> Result<Vec<Entry>, String> {
    let client = connected()?;
    Ok(get(&client, COLLECTION).await?.map(|t| entries(&t.body)).unwrap_or_default())
}

/// A failed Save as… or rename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Refused(Refusal),
    Failed(String),
}

/// Save as… `name` with `filter`: the collection first (created, the first
/// time, under [`COLLECTION_PARENT`]), then the doc, then the line —
/// the entry saved and the list after.
pub(crate) async fn save(filter: Filter, name: String, replace: bool) -> Result<(Entry, Vec<Entry>), Failure> {
    let failed = Failure::Failed;
    let client = connected().map_err(failed)?;
    let held = get(&client, COLLECTION).await.map_err(failed)?;
    let listed = held.as_ref().map(|t| entries(&t.body)).unwrap_or_default();
    let plan = plan_save(&listed, &name, replace).map_err(Failure::Refused)?;
    let view = match held {
        Some(t) => t,
        None => {
            let t = create_doc(&client, COLLECTION_TITLE, String::new(), COLLECTION_PARENT, None).await.map_err(failed)?;
            if t.id != COLLECTION {
                return Err(Failure::Failed(format!("the collection was created as {}, not {COLLECTION}", t.id)));
            }
            t
        }
    };
    let entry = plan.entry().clone();
    let body = saved_text(&filter);
    match get(&client, &address_of(&entry.id)).await.map_err(failed)? {
        Some(doc) => {
            amend(&client, &doc.id, Some(entry.name.clone()), Some(body)).await.map_err(failed)?;
        }
        None => {
            create_doc(&client, &entry.name, body, &view.id, Some(path_of(&entry.id))).await.map_err(failed)?;
        }
    }
    let next = match &plan {
        Plan::New(e) => with_added(&view.body, e),
        Plan::Replace(e) => with_renamed(&view.body, &e.id, &e.name).unwrap_or_else(|| view.body.clone()),
    };
    let after = if next != view.body { amend(&client, &view.id, None, Some(next)).await.map_err(failed)?.body } else { view.body };
    // MENGER-43: Needs you's count follows a save of its filter.
    crate::needs::saved_changed(&entry.id, &filter);
    Ok((entry, entries(&after)))
}

/// Rename `id` to `name`: the doc's title, then the line — the list after.
pub(crate) async fn rename(id: String, name: String) -> Result<Vec<Entry>, Failure> {
    let failed = Failure::Failed;
    let client = connected().map_err(failed)?;
    let view = get(&client, COLLECTION).await.map_err(failed)?.ok_or(Failure::Refused(Refusal::Unlisted))?;
    let entry = plan_rename(&entries(&view.body), &id, &name).map_err(Failure::Refused)?;
    let address = address_of(&id);
    let doc = get(&client, &address).await.map_err(failed)?.ok_or_else(|| Failure::Failed(format!("{address}: not found")))?;
    if doc.title != entry.name {
        amend(&client, &doc.id, Some(entry.name.clone()), None).await.map_err(failed)?;
    }
    let next = with_renamed(&view.body, &id, &entry.name).unwrap_or_else(|| view.body.clone());
    let after = if next != view.body { amend(&client, &view.id, None, Some(next)).await.map_err(failed)?.body } else { view.body };
    Ok(entries(&after))
}

/// Take `id`'s line out of the collection (its doc stays) — the list after.
pub(crate) async fn remove(id: String) -> Result<Vec<Entry>, String> {
    let client = connected()?;
    let Some(view) = get(&client, COLLECTION).await? else { return Ok(Vec::new()) };
    let after = match with_removed(&view.body, &id) {
        Some(next) => amend(&client, &view.id, None, Some(next)).await?.body,
        None => view.body,
    };
    Ok(entries(&after))
}

/// FILTER-37: the saved filter `id`'s doc amended to hold `filter` — what
/// naming saves writes after every edit of a named tree.
pub(crate) async fn amend_filter(id: &str, filter: &Filter) -> Result<(), String> {
    let client = connected()?;
    let address = address_of(id);
    let doc = get(&client, &address).await?.ok_or_else(|| format!("{address}: not found"))?;
    let body = saved_text(filter);
    if filter_of(&doc.body).ok().as_ref() != Some(filter) {
        amend(&client, &doc.id, None, Some(body)).await?;
    }
    crate::needs::saved_changed(id, filter);
    Ok(())
}

/// MENGER-12: the filter `id`'s doc holds, or `None` while nothing is saved
/// under it — Needs you reads its saved filter this way (MENGER-43: on every
/// `g n` too), and falls back to its default.
pub(crate) async fn load_if_saved(id: &str) -> Result<Option<Filter>, String> {
    let client = connected()?;
    let address = address_of(id);
    match get(&client, &address).await? {
        None => Ok(None),
        Some(doc) => filter_of(&doc.body).map(Some).map_err(|why| format!("{address}: {why}")),
    }
}

/// MENGER-12: Save as… `name` with `filter`, replacing what that name holds —
/// the entry saved (FILTER-27's `keep`).
pub(crate) async fn save_as(filter: Filter, name: &str) -> Result<Entry, String> {
    save(filter, name.to_string(), true).await.map(|(e, _)| e).map_err(|f| match f {
        Failure::Refused(r) => r.words(),
        Failure::Failed(why) => why,
    })
}

/// The filter `id`'s doc holds.
async fn load(id: String) -> Result<Filter, String> {
    let client = connected()?;
    let address = address_of(&id);
    let doc = get(&client, &address).await?.ok_or_else(|| format!("{address}: not found"))?;
    filter_of(&doc.body).map_err(|why| format!("{address}: {why}"))
}

// ---------------------------------------------------------------------
// The state

/// The collection's list, as the app last read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Listing {
    Unread,
    Reading,
    Read(Vec<Entry>),
    Failed(String),
}

/// The Save as… dialog's subject: whose filter, the filter it saves, and
/// what of the level is left out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Draft {
    level: Level,
    filter: Filter,
    left_out: Vec<String>,
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
    rename_draft: RwSignal<String>,
    removing: RwSignal<Option<String>>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    /// The Save as… dialog, while it is up: its subject, the name typed,
    /// the listed filter the name collides with, what went wrong, a write in
    /// flight.
    dialog: RwSignal<Option<Draft>>,
    name: RwSignal<String>,
    taken: RwSignal<Option<Entry>>,
    d_problem: RwSignal<Option<String>>,
    d_busy: RwSignal<bool>,
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

/// The app's saved filters.
pub(crate) fn use_saved() -> Saved {
    use_context::<Saved>().expect("saved filters are provided by the App")
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
            rename_draft: RwSignal::new(String::new()),
            removing: RwSignal::new(None),
            problem: RwSignal::new(None),
            busy: RwSignal::new(false),
            dialog: RwSignal::new(None),
            name: RwSignal::new(String::new()),
            taken: RwSignal::new(None),
            d_problem: RwSignal::new(None),
            d_busy: RwSignal::new(false),
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
        // Once read, the list follows `watch`: the collection's `amended`
        // (another browser's save, rename, remove) carries its body.
        session::listen(move |ev| {
            let (TicketsEvent::Amended { ticket, .. } | TicketsEvent::Created { ticket }) = ev else { return };
            if ticket.id == COLLECTION && s.list.try_with_untracked(|l| !matches!(l, Listing::Unread)) == Some(true) {
                let next = Listing::Read(entries(&ticket.body));
                if s.list.with_untracked(|l| *l != next) {
                    s.list.set(next);
                }
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

    fn open_menu(self, level: Level) {
        for l in Level::ALL {
            self.lens.pop(l).close(false);
        }
        self.renaming.set(None);
        self.removing.set(None);
        self.problem.set(None);
        self.open.set(Some(level));
        self.refresh();
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
    fn suggest(self, level: Level, name: &'static str, text: &'static str) {
        let Ok(filter) = Filter::parse(text) else { return };
        self.lens.apply(level, Some(chosen(filter)));
        self.lens.pop(level).tree_open.set(false);
        self.close_menu(true);
        self.say(level, format!("\u{201c}{name}\u{201d} applied"));
    }

    /// FILTER-27: a suggestion kept — saved as an ordinary saved filter.
    fn keep(self, level: Level, name: &'static str, text: &'static str) {
        let Ok(filter) = Filter::parse(text) else { return };
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.problem.set(None);
        spawn_local(async move {
            let got = save_as(filter, name).await;
            self.busy.try_set(false);
            match got {
                Ok(_) => {
                    self.refresh();
                    self.say(level, format!("\u{201c}{name}\u{201d} kept among the saved filters"));
                }
                Err(why) => {
                    self.problem.try_set(Some(format!("not kept: {why}")));
                }
            }
        });
    }

    /// A saved filter chosen: its filter becomes the level's.
    fn choose(self, level: Level, e: Entry) {
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
                    self.lens.pop(level).tree_open.set(false);
                    self.close_menu(true);
                    self.say(level, format!("\u{201c}{}\u{201d} applied", e.name));
                }
                Err(why) => {
                    self.problem.try_set(Some(format!("not applied: {why}")));
                }
            }
        });
    }

    fn start_rename(self, e: &Entry) {
        self.removing.set(None);
        self.problem.set(None);
        self.rename_draft.set(e.name.clone());
        self.renaming.set(Some(e.id.clone()));
        request_animation_frame(|| {
            if let Some(el) = dom::window()
                .document()
                .and_then(|d| d.query_selector(".smenu input.sname-in").ok().flatten())
                .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            {
                let _ = el.focus();
                el.select();
            }
        });
    }

    fn finish_rename(self, level: Level, id: String) {
        if self.busy.get_untracked() {
            return;
        }
        let name = self.rename_draft.get_untracked();
        self.busy.set(true);
        self.problem.set(None);
        spawn_local(async move {
            let got = rename(id, name).await;
            self.busy.try_set(false);
            match got {
                Ok(list) => {
                    self.list.try_set(Listing::Read(list));
                    self.renaming.try_set(None);
                    let first = format!("{} [role=menuitem]", saved_sel(level));
                    request_animation_frame(move || focus(&first));
                }
                Err(Failure::Refused(r)) => {
                    self.problem.try_set(Some(format!("not renamed: {}", r.words())));
                }
                Err(Failure::Failed(why)) => {
                    self.problem.try_set(Some(format!("not renamed: {why}")));
                }
            }
        });
    }

    fn finish_remove(self, level: Level, id: String) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.problem.set(None);
        spawn_local(async move {
            let got = remove(id).await;
            self.busy.try_set(false);
            match got {
                Ok(list) => {
                    self.list.try_set(Listing::Read(list));
                    self.removing.try_set(None);
                    let first = format!("{} [role=menuitem]", saved_sel(level));
                    request_animation_frame(move || focus(&first));
                }
                Err(why) => {
                    self.problem.try_set(Some(format!("not removed: {why}")));
                }
            }
        });
    }

    /// Save as… for `level`: the dialog, over what the level keeps now.
    fn open_dialog(self, level: Level) {
        let Some(k) = untrack(|| self.lens.kept(level)) else { return };
        self.close_menu(false);
        focus(&format!("{} button.sbtn", saved_sel(level)));
        keys::remember_focus();
        self.name.set(String::new());
        self.taken.set(None);
        self.d_problem.set(None);
        self.d_busy.set(false);
        self.dialog.set(Some(Draft { level, left_out: left_out(&k), filter: k.filter }));
    }

    fn close_dialog(self) {
        if self.dialog.get_untracked().is_none() {
            return;
        }
        self.dialog.set(None);
        keys::restore_focus();
    }

    fn submit(self, replace: bool) {
        let Some(d) = self.dialog.get_untracked() else { return };
        if self.d_busy.get_untracked() {
            return;
        }
        self.d_busy.set(true);
        self.d_problem.set(None);
        let name = self.name.get_untracked();
        spawn_local(async move {
            let got = save(d.filter.clone(), name, replace).await;
            self.d_busy.try_set(false);
            match got {
                Ok((entry, list)) => {
                    self.list.try_set(Listing::Read(list));
                    // FILTER-37: saved under a name, the level wears it.
                    if let Some(n) = crate::naming::hook() {
                        if untrack(|| self.lens.filter(d.level).get()) == d.filter {
                            n.bind(d.level, crate::naming::Bound { id: entry.id.clone(), name: entry.name.clone() }, d.filter.format());
                        }
                    }
                    self.close_dialog();
                    self.say(d.level, format!("saved as \u{201c}{}\u{201d}", entry.name));
                }
                Err(Failure::Refused(Refusal::Taken(e))) => {
                    self.taken.try_set(Some(e));
                }
                Err(Failure::Refused(r)) => {
                    self.d_problem.try_set(Some(r.words()));
                }
                Err(Failure::Failed(why)) => {
                    self.d_problem.try_set(Some(format!("not saved: {why}")));
                }
            }
        });
    }

    /// Choose another name: the collision put away, the name selected.
    fn rename_instead(self) {
        self.taken.set(None);
        request_animation_frame(|| {
            if let Some(el) = dom::window()
                .document()
                .and_then(|d| d.get_element_by_id("save-as-name"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            {
                let _ = el.focus();
                el.select();
            }
        });
    }
}

// ---------------------------------------------------------------------
// The menu

/// The focusable rows of the open menu, and which has focus.
fn menu_items(level: Level) -> (Vec<web_sys::HtmlElement>, Option<usize>) {
    let Some(doc) = dom::window().document() else { return (Vec::new(), None) };
    let Ok(list) = doc.query_selector_all(&format!("{} .smenu [role=menuitem]:not([disabled])", saved_sel(level))) else {
        return (Vec::new(), None);
    };
    let items: Vec<web_sys::HtmlElement> = (0..list.length()).filter_map(|i| list.get(i).and_then(|n| n.dyn_into().ok())).collect();
    let at = doc.active_element().and_then(|a| items.iter().position(|el| el.is_same_node(Some(&a))));
    (items, at)
}

/// `Saved ▾` beside a level's `Filter` button, and its menu while open.
#[component]
pub(crate) fn SavedButton(level: Level) -> impl IntoView {
    let s = use_saved();
    let is_open = Memo::new(move |_| s.open.get() == Some(level));
    let toggle = move |_| {
        if s.open.get_untracked() == Some(level) {
            s.close_menu(true);
        } else {
            s.open_menu(level);
        }
    };
    let on_key = move |ev: web_sys::KeyboardEvent| {
        if !is_open.get_untracked() {
            return;
        }
        let id = bindings::hit(&ev, Context::Menu);
        // A rename's field keeps its own keys but Escape.
        let in_field = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.tag_name() == "INPUT");
        match id {
            Some("dialog.close") => {
                ev.prevent_default();
                ev.stop_propagation();
                if s.renaming.get_untracked().is_some() || s.removing.get_untracked().is_some() {
                    s.renaming.set(None);
                    s.removing.set(None);
                    s.problem.set(None);
                    let first = format!("{} [role=menuitem]", saved_sel(level));
                    request_animation_frame(move || focus(&first));
                } else {
                    s.close_menu(true);
                }
            }
            Some("list.next" | "list.prev") if !in_field => {
                ev.prevent_default();
                let (items, at) = menu_items(level);
                if let Some(n) = crate::menu::step(at, items.len(), id) {
                    let _ = items[n].focus();
                }
            }
            _ => {}
        }
    };
    // Focus leaving the menu for elsewhere (Tab past it) closes it; focus
    // dropped by a row redrawn under it (`relatedTarget` none) does not.
    let on_focusout = move |ev: web_sys::FocusEvent| {
        let gone = ev
            .related_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.closest(&saved_sel(level)).ok().flatten().is_none());
        if gone && is_open.get_untracked() {
            s.close_menu(false);
        }
    };
    let flash = move || s.flash.get().filter(|(l, _)| *l == level).map(|(_, w)| view! { <span class="sflash" role="status">{w}</span> });
    view! {
        <span class="saved" data-saved=level.name() on:keydown=on_key on:focusout=on_focusout>
            <button
                class="fbtn sbtn"
                title=format!("saved filters — choose one for {}, or save it under a name", level_words(level))
                aria-haspopup="menu"
                aria-expanded=move || is_open.get().to_string()
                on:click=toggle
            >
                "Saved"
                <span class="caret" aria-hidden="true">"▾"</span>
            </button>
            {move || is_open.get().then(|| menu_view(s, level))}
            {flash}
        </span>
    }
}

/// Where focus goes in the open menu: its first item — once there is one
/// to take it (the list read, Save as… enabled) — unless an item or a
/// field already has it; the menu itself until then, so the keys reach it.
fn focus_in_menu(level: Level) {
    let Some(doc) = dom::window().document() else { return };
    let menu = format!("{} .smenu", saved_sel(level));
    let on_item = doc
        .active_element()
        .is_some_and(|a| a.closest(&format!("{menu} [role=menuitem], {menu} input")).ok().flatten().is_some());
    if !on_item {
        let (items, _) = menu_items(level);
        match items.first() {
            Some(el) => {
                let _ = el.focus();
            }
            None => focus(&menu),
        }
    }
}

/// The open menu kept inside the window: hung from its button's left edge,
/// moved left by what would pass the right one (a narrow window, a button
/// near the edge).
fn fit_menu(level: Level) {
    let Some(el) = dom::window()
        .document()
        .and_then(|d| d.query_selector(&format!("{} .smenu", saved_sel(level))).ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return;
    };
    let Some(anchor) = el.parent_element() else { return };
    let vw = dom::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY);
    let (at, width) = (anchor.get_bounding_client_rect().left(), el.get_bounding_client_rect().width());
    let left = ((vw - 8.0 - width) - at).min(0.0).max(8.0 - at).min(0.0);
    let _ = el.style().set_property("left", &format!("{left}px"));
}

fn menu_view(s: Saved, level: Level) -> impl IntoView {
    let empty = Memo::new(move |_| s.lens.kept(level).map_or(true, |k| k.filter.is_empty()));
    // Drawn, and again whenever the list changes: placed, and focused.
    Effect::new(move |_| {
        s.list.track();
        request_animation_frame(move || {
            fit_menu(level);
            focus_in_menu(level);
            // Pulled in, the menu no longer widens the page, and the row its
            // button sits in can settle elsewhere: placed again from there.
            request_animation_frame(move || fit_menu(level));
        });
    });
    // What stands in for the rows (reading, a failure, none yet), and the
    // rows — keyed, so a list read again unchanged keeps each row's DOM (and
    // the focus on it).
    let note = Memo::new(move |_| match s.list.get() {
        Listing::Unread | Listing::Reading => Some((false, "reading the saved filters…".to_string())),
        Listing::Failed(why) => Some((true, format!("the saved filters could not be read: {why}"))),
        Listing::Read(list) if list.is_empty() => Some((false, "none yet — Save as… keeps this filter under a name".to_string())),
        Listing::Read(_) => None,
    });
    let listed = Memo::new(move |_| match s.list.get() {
        Listing::Read(list) => list,
        _ => Vec::new(),
    });
    // FILTER-27: the suggestions not yet listed — once the list is read.
    let suggested = Memo::new(move |_| match s.list.get() {
        Listing::Read(list) => suggestions(&list),
        Listing::Failed(_) => SUGGESTED.to_vec(),
        _ => Vec::new(),
    });
    let rows = view! {
        {move || note.get().map(|(bad, words)| view! { <div class="fempty" class:sproblem=bad>{words}</div> })}
        <For
            each=move || listed.get()
            key=|e: &Entry| (e.id.clone(), e.name.clone())
            children=move |e: Entry| row_view(s, level, e)
        />
    };
    view! {
        <div class="fmenu smenu" role="menu" tabindex="-1" aria-label="saved filters" data-level=level.name() data-reading=move || s.reading.get().then_some("")>
            <div class="fhead">"Saved filters"</div>
            <div class="slist">{rows}</div>
            {move || (!suggested.get().is_empty()).then(|| view! {
                <div class="fhead fdiv">"Suggested"</div>
                {suggested.get().into_iter().map(|(name, text)| view! {
                    <div class="frow srow suggested" data-suggested=slug(name)>
                        <button class="schoose" role="menuitem" title=format!("{name}: {text}") on:click=move |_| s.suggest(level, name, text)>
                            {name}
                        </button>
                        <button class="sact" data-act="keep" title="keep it among the saved filters" disabled=move || s.busy.get() on:click=move |_| s.keep(level, name, text)>
                            "keep"
                        </button>
                    </div>
                }).collect_view()}
            })}
            {move || s.problem.get().map(|p| view! { <div class="fempty sproblem" role="alert">{p}</div> })}
            <div class="fhead fdiv"></div>
            <button
                class="frow ssave"
                role="menuitem"
                disabled=move || empty.get()
                title=move || if empty.get() { "nothing to save — the filter is empty".to_string() } else { format!("save {} under a name", level_words(level)) }
                on:click=move |_| s.open_dialog(level)
            >
                <span class="flabel">"Save as…"</span>
            </button>
        </div>
    }
}

fn row_view(s: Saved, level: Level, e: Entry) -> impl IntoView {
    let id = e.id.clone();
    let renaming = {
        let id = id.clone();
        Memo::new(move |_| s.renaming.with(|r| r.as_deref() == Some(id.as_str())))
    };
    let removing = {
        let id = id.clone();
        Memo::new(move |_| s.removing.with(|r| r.as_deref() == Some(id.as_str())))
    };
    move || {
        let e = e.clone();
        let id = id.clone();
        if renaming.get() {
            let done = {
                let id = id.clone();
                move || s.finish_rename(level, id.clone())
            };
            let done_key = done.clone();
            view! {
                <div class="frow srow renaming" data-saved-id=id.clone()>
                    <input
                        class="sname-in"
                        type="text"
                        maxlength=NAME_MAX.to_string()
                        aria-label=format!("rename \u{201c}{}\u{201d}", e.name)
                        prop:value=move || s.rename_draft.get()
                        on:input=move |ev| s.rename_draft.set(event_target_value(&ev))
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if bindings::is(&ev, Context::Field, "form.submit") {
                                ev.prevent_default();
                                done_key();
                            }
                        }
                    />
                    <button class="sact on" data-act="rename-ok" disabled=move || s.busy.get() on:click=move |_| done()>"rename"</button>
                    <button class="sact" data-act="rename-cancel" on:click=move |_| { s.renaming.set(None); s.problem.set(None); }>"cancel"</button>
                </div>
            }
            .into_any()
        } else if removing.get() {
            let yes = id.clone();
            view! {
                <div class="frow srow removing" data-saved-id=id.clone()>
                    <span class="flabel">{format!("remove \u{201c}{}\u{201d} from the list?", e.name)}</span>
                    <button class="sact on" role="menuitem" data-act="remove-ok" disabled=move || s.busy.get() on:click=move |_| s.finish_remove(level, yes.clone())>"remove"</button>
                    <button class="sact" role="menuitem" data-act="remove-cancel" on:click=move |_| s.removing.set(None)>"keep"</button>
                </div>
            }
            .into_any()
        } else {
            let pick = e.clone();
            let ren = e.clone();
            let rid = id.clone();
            view! {
                <div class="frow srow" data-saved-id=id.clone()>
                    <button
                        class="schoose"
                        role="menuitem"
                        title=format!("{}: {}", e.name, address_of(&e.id))
                        on:click=move |_| s.choose(level, pick.clone())
                    >
                        {e.name.clone()}
                    </button>
                    <button class="sact" data-act="rename" title="rename it" on:click=move |_| s.start_rename(&ren)>"rename"</button>
                    <button
                        class="sact"
                        data-act="remove"
                        title="take it out of the list (its doc stays)"
                        on:click=move |_| {
                            s.renaming.set(None);
                            s.problem.set(None);
                            s.removing.set(Some(rid.clone()));
                        }
                    >
                        "remove"
                    </button>
                </div>
            }
            .into_any()
        }
    }
}

// ---------------------------------------------------------------------
// The dialog

/// Save as…: mounted once beside the app's columns; up while a level's
/// Save as… is.
#[component]
pub(crate) fn SaveDialog() -> impl IntoView {
    let s = use_saved();
    move || s.dialog.get().map(|d| dialog_view(s, d))
}

fn dialog_view(s: Saved, d: Draft) -> impl IntoView {
    let field = NodeRef::<leptos::html::Input>::new();
    field.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        match bindings::hit(&ev, Context::Dialog) {
            Some("dialog.close") => {
                ev.prevent_default();
                s.close_dialog();
            }
            Some(id @ ("dialog.next-field" | "dialog.prev-field")) => {
                ev.prevent_default();
                keys::trap_tab(id == "dialog.prev-field");
            }
            _ => {}
        }
        ev.stop_propagation();
    };
    let text = d.filter.format();
    let left = (!d.left_out.is_empty()).then(|| {
        let what = d.left_out.join(" · ");
        view! {
            <p class="sleft" data-left-out="">
                <strong>"Left out: "</strong>
                {what}
                {format!(" — {}, which a saved filter does not keep.", surface_words(d.level))}
            </p>
        }
    });
    view! {
        <div class="overlay" on:click=move |_| s.close_dialog()>
            <div
                id="save-as"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-labelledby="save-as-h"
                data-level=d.level.name()
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <h2 id="save-as-h">{format!("Save {}", level_words(d.level))}</h2>
                <label class="sname">
                    <span>"Name"</span>
                    <input
                        id="save-as-name"
                        node_ref=field
                        type="text"
                        autocomplete="off"
                        spellcheck="false"
                        maxlength=NAME_MAX.to_string()
                        placeholder="open builds"
                        prop:value=move || s.name.get()
                        on:input=move |ev| {
                            s.name.set(event_target_value(&ev));
                            s.taken.set(None);
                            s.d_problem.set(None);
                        }
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if bindings::is(&ev, Context::Field, "form.submit") {
                                ev.prevent_default();
                                if s.taken.get_untracked().is_none() {
                                    s.submit(false);
                                }
                            }
                        }
                    />
                </label>
                <div class="stext">
                    <span class="slab">"saves"</span>
                    <code>{saved_text(&d.filter)}</code>
                </div>
                {(text.is_empty()).then(|| view! { <p class="sleft">"The filter is empty."</p> })}
                {left}
                {move || {
                    let name = s.name.get();
                    s.taken.get().map(|e| view! {
                        <div class="staken" role="alert">
                            <p>{taken_words(&clean_name(&name), &e)}</p>
                            <div class="sacts">
                                <button class="primary" data-act="replace" disabled=move || s.d_busy.get() on:click=move |_| s.submit(true)>"Replace it"</button>
                                <button data-act="another" on:click=move |_| s.rename_instead()>"Choose another name"</button>
                            </div>
                        </div>
                    })
                }}
                {move || s.d_problem.get().map(|p| view! { <p class="sproblem" role="alert">{p}</p> })}
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| s.close_dialog()>"Cancel"</button>
                    <button
                        class="primary"
                        data-act="save"
                        disabled=move || s.d_busy.get() || s.taken.with(Option::is_some) || slug(&s.name.get()).is_empty()
                        on:click=move |_| s.submit(false)
                    >
                        {move || if s.d_busy.get() { "Saving…" } else { "Save" }}
                    </button>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{Cat, Clausal, Op};

    /// The slug: lower case, letters and digits, one `-` per run of anything
    /// else, none at the ends — empty for a name with no letter or digit;
    /// at most 64 characters, never ending in `-`.
    #[test]
    fn the_suggestions_parse_and_step_aside_once_listed() {
        for (name, text) in SUGGESTED {
            let f = Filter::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(f.format(), text, "{name}: canonical");
        }
        assert_eq!(suggestions(&[]).len(), 4);
        let listed = [Entry::new("stuck", "Stuck"), Entry::new("x", "new today"), Entry::new("untouched-2w", "Untouched 2w")];
        assert_eq!(suggestions(&listed), [("Moved today", "changed today")]);
    }

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
        assert_eq!(path_of("open-builds"), "_MENGER/view/open-builds");
        assert_eq!(address_of("open-builds"), "VIEW/_MENGER/view/open-builds");
        assert_eq!(clean_name("  open \t builds  "), "open builds");
        assert_eq!(clean_name(&"x".repeat(200)).chars().count(), NAME_MAX);
    }

    /// The list from VIEW-1: every `<m-demand id>` in order, by name — other
    /// text ignored, entities read, tags inside stripped, the first of an id
    /// kept, a demand without an id or a close skipped, a demand with no
    /// text named by its id.
    #[test]
    fn the_list_is_read_from_the_collection() {
        let body = "- <m-demand id=\"open-builds\">open builds</m-demand>\n";
        assert_eq!(entries(body), vec![Entry::new("open-builds", "open builds")]);
        let body = "Saved by the app.\n\n\
            - <m-demand id=\"open-builds\">open builds</m-demand>\n\
            - <m-demand id='mine' data-x=\"y\">my <b>reviews</b> &amp; fixes</m-demand>\n\
            * <m-demand id=bare>bare</m-demand>\n\
            - <m-demand>no id</m-demand>\n\
            - <m-demands id=\"other\">another element</m-demands>\n\
            - <m-demand id=\"open-builds\">again</m-demand>\n\
            - <m-demand id=\"empty\"></m-demand>\n";
        assert_eq!(
            entries(body),
            vec![
                Entry::new("open-builds", "open builds"),
                Entry::new("mine", "my reviews & fixes"),
                Entry::new("bare", "bare"),
                Entry::new("empty", "empty"),
            ]
        );
        assert_eq!(entries(""), vec![]);
        assert_eq!(entries("no list here"), vec![]);
        assert_eq!(entries("<m-demand id=\"x\">unclosed"), vec![], "no close, no demand");
        assert_eq!(unescape("a &lt;b&gt; &quot;c&quot; &#39;d&#39; &#x41; &bogus; & e"), "a <b> \"c\" 'd' A &bogus; & e");
    }

    /// The line written, and the list rewritten: added after the last line
    /// (at the end of a body with none), renamed in place, removed with its
    /// line — every other byte of the body kept; the name escaped, and read
    /// back as written.
    #[test]
    fn the_list_is_rewritten_line_by_line() {
        let a = Entry::new("open-builds", "open builds");
        assert_eq!(demand_line(&a), "- <m-demand id=\"open-builds\">open builds</m-demand>");
        let body = with_added("", &a);
        assert_eq!(body, "- <m-demand id=\"open-builds\">open builds</m-demand>");
        let b = Entry::new("mine", "mine <& yours>");
        let body = with_added(&body, &b);
        assert_eq!(body, "- <m-demand id=\"open-builds\">open builds</m-demand>\n- <m-demand id=\"mine\">mine &lt;&amp; yours&gt;</m-demand>");
        assert_eq!(entries(&body), vec![a.clone(), b.clone()], "read back as written");

        // Text around the list stays.
        let framed = "Intro.\n\n- <m-demand id=\"open-builds\">open builds</m-demand>\n\nOutro.\n";
        let c = Entry::new("c", "c");
        assert_eq!(
            with_added(framed, &c),
            "Intro.\n\n- <m-demand id=\"open-builds\">open builds</m-demand>\n- <m-demand id=\"c\">c</m-demand>\n\nOutro.\n"
        );
        assert_eq!(with_added("Intro.\n", &c), "Intro.\n\n- <m-demand id=\"c\">c</m-demand>");

        // Renamed: the name only, the id (the doc's path) kept.
        let renamed = with_renamed(&body, "open-builds", "builds in flight").unwrap();
        assert_eq!(entries(&renamed), vec![Entry::new("open-builds", "builds in flight"), b.clone()]);
        assert_eq!(with_renamed(&body, "nope", "x"), None);

        // Removed: its line; the last one leaves an empty list.
        let removed = with_removed(&body, "open-builds").unwrap();
        assert_eq!(removed, "- <m-demand id=\"mine\">mine &lt;&amp; yours&gt;</m-demand>");
        assert_eq!(with_removed(&removed, "mine").unwrap(), "");
        assert_eq!(with_removed(&body, "mine").unwrap(), "- <m-demand id=\"open-builds\">open builds</m-demand>");
        assert_eq!(with_removed(&body, "nope"), None);
        assert_eq!(
            with_removed(framed, "open-builds").unwrap(),
            "Intro.\n\nOutro.\n",
            "the line goes, the text around it stays"
        );
        // An element sharing its line with other words: the element alone.
        assert_eq!(with_removed("see <m-demand id=\"x\">x</m-demand> here", "x").unwrap(), "see  here");
        // A hand-written duplicate goes too.
        assert_eq!(with_removed("- <m-demand id=\"x\">x</m-demand>\n- <m-demand id=\"x\">x</m-demand>\n", "x").unwrap(), "");
    }

    /// The saved text: `filter: ` and the canonical text, read back as the
    /// same filter; a body without the line, or with one the language
    /// refuses, says why.
    #[test]
    fn the_saved_text_is_the_canonical_filter() {
        let f = Filter::parse("kind in (build) and not status in (Complete)").unwrap();
        let text = saved_text(&f);
        assert_eq!(text, format!("filter: {}", f.format()));
        assert_eq!(filter_of(&text).unwrap(), f);
        assert_eq!(filter_of(&format!("A note.\n\n  {text}\n")).unwrap(), f, "the first filter: line, wherever it is");
        let g = Filter::parse("status in (Ready, \"In Review\") and not in MNG · flat").unwrap();
        assert_eq!(filter_of(&saved_text(&g)).unwrap(), g, "quotes and the display part round trip");
        assert_eq!(filter_of("filter: ").unwrap(), Filter::default(), "the empty filter");
        assert!(filter_of("no line").unwrap_err().contains("filter:"));
        let bad = filter_of("filter: status Bogus").unwrap_err();
        assert!(bad.contains("status Bogus"), "{bad}");
    }

    /// Name collisions: a name a listed filter has (by slug) is refused,
    /// naming it — or, asked, that filter is replaced at its own path under
    /// the new spelling; a new name takes its slug, or the first free
    /// `slug-n` when a renamed filter holds it; a name with no letter or
    /// digit is no name.
    #[test]
    fn a_name_already_saved_is_offered_for_replacing() {
        let listed = vec![Entry::new("open-builds", "open builds"), Entry::new("mine", "my reviews")];
        assert_eq!(plan_save(&listed, "Open  Builds", false), Err(Refusal::Taken(listed[0].clone())));
        assert_eq!(plan_save(&listed, "open builds!", true), Ok(Plan::Replace(Entry::new("open-builds", "open builds!"))));
        assert_eq!(plan_save(&listed, "My Reviews", true), Ok(Plan::Replace(Entry::new("mine", "My Reviews"))), "its own path, not the name's slug");
        assert_eq!(plan_save(&listed, "blocked work", false), Ok(Plan::New(Entry::new("blocked-work", "blocked work"))));
        assert_eq!(plan_save(&listed, " ?! ", false), Err(Refusal::NoName));
        assert_eq!(plan_save(&[], "open builds", false), Ok(Plan::New(Entry::new("open-builds", "open builds"))));
        assert_eq!(Refusal::Taken(listed[0].clone()).words(), "\u{201c}open builds\u{201d} is already saved");
        assert_eq!(taken_words("open builds", &listed[0]), "\u{201c}open builds\u{201d} is already saved — replace that filter, or choose another name?");
        assert_eq!(
            taken_words("Open Builds", &listed[0]),
            "\u{201c}Open Builds\u{201d} is already saved, as \u{201c}open builds\u{201d} — replace that filter, or choose another name?"
        );

        // `mine` was saved as "mine" and renamed; "mine" again is a new name
        // whose path is held.
        assert_eq!(plan_save(&listed, "Mine", false), Ok(Plan::New(Entry::new("mine-2", "Mine"))));
        let more = [listed.clone(), vec![Entry::new("mine-2", "x")]].concat();
        assert_eq!(free_slug(&more, "mine"), "mine-3");
        assert_eq!(free_slug(&more, "fresh"), "fresh");
        assert_eq!(taken_by(&listed, "OPEN-builds").map(|e| e.id.as_str()), Some("open-builds"));
        assert_eq!(taken_by(&listed, "--"), None);
    }

    /// A rename: to a name no other listed filter has (its own, respelled,
    /// is fine); refused for no name, an unlisted filter, or another's name.
    #[test]
    fn a_rename_keeps_names_apart() {
        let listed = vec![Entry::new("open-builds", "open builds"), Entry::new("mine", "my reviews")];
        assert_eq!(plan_rename(&listed, "open-builds", "builds in flight"), Ok(Entry::new("open-builds", "builds in flight")));
        assert_eq!(plan_rename(&listed, "open-builds", "Open Builds"), Ok(Entry::new("open-builds", "Open Builds")));
        assert_eq!(plan_rename(&listed, "open-builds", "My  reviews"), Err(Refusal::Taken(listed[1].clone())));
        assert_eq!(plan_rename(&listed, "gone", "anything"), Err(Refusal::Unlisted));
        assert_eq!(plan_rename(&listed, "mine", "  "), Err(Refusal::NoName));
    }

    /// What Save as… leaves out: a level's surface-only clauses, as their
    /// pills read; nothing where there are none. A chosen filter carries
    /// none.
    #[test]
    fn the_surface_only_part_is_left_out() {
        let mut k = Kept { filter: Filter::parse("kind in (build)").unwrap(), local: Clauses::default() };
        assert!(left_out(&k).is_empty());
        k.set(Cat::Wave, Op::Is, ["1".to_string()]);
        k.set(Cat::Title, Op::Is, ["filter".to_string()]);
        assert_eq!(left_out(&k), vec!["Wave is wave 1".to_string(), "Title contains \u{201c}filter\u{201d}".to_string()]);
        assert_eq!(saved_text(&k.filter), "filter: kind build", "the language part alone is saved");
        let c = chosen(k.filter.clone());
        assert_eq!((c.filter, c.local), (k.filter.clone(), Clauses::default()));
        assert_eq!(level_words(Level::Waves), "the wave board's filter");
        assert_eq!(surface_words(Level::Waves), "the wave board's own");
    }
}
