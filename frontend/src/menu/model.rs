//! ACT-4: **what the one menu holds** — a pure function of what it opened
//! on ([`Facts`]): one ticket or a selection, the chip right-clicked, the
//! shortcode a nav row stands for, the reader's grants, and whether it opened
//! in the nav. The view (src/menu/mod.rs) draws [`entries`] in order and the
//! submenus' [`options`]; choosing one is a [`Change`], which [`call`] makes
//! into exactly one write (src/menu/write.rs sends it).
//!
//! The groups, each present only when it holds something, a rule between:
//!
//! ```text
//! the chip's own chooser, flat (a status, kind, severity or label chip)     (ACT-10)
//! ─────────
//! Open · Open in new tab · Wave board for P · Waves to X · Wave board · Copy code     (one ticket)
//! Copy codes                                                                         (a selection)
//! ─────────
//! Set status ▸ · Set kind ▸ · Set severity ▸ · Labels ▸ · Move under… · Blocked by…
//! Unblock ▸ (when one waits on something) · Rename… · Link commits…                   (one ticket)
//! ─────────
//! the chip's filter items (a status, kind, severity or label chip) · the ticket's
//! (FILTER-22) — or, over a selection, its shortcodes' and `under` each
//! ─────────
//! Filter everywhere: … (FILTER-25) — the same clauses, the global filter     (off it)
//! ─────────
//! Nav ▸                                                                              (in the nav)
//! ─────────
//! Delete… / Delete N…                                                               (admin)
//! ```
//!
//! - **A selection** ([`target_ids`]): a right-click on a selected row acts on
//!   the whole selection when it holds more than one; on any other row, on
//!   that row alone (the selection kept).
//! - **The grants** ([`Grants`]): the updates are absent — not disabled — for
//!   a reader who cannot write; Delete only for an admin (anyone, with login
//!   off: the server admits every call then).
//! - **ACT-10: a value chip answers with its values.** Right-clicked on a
//!   status, kind, severity or label chip, by a reader who may write, the
//!   menu's first group is that chip's chooser ([`chooser`]) — the submenu's
//!   own options, marks and writes, drawn flat ([`Entry::Choice`]) — and its
//!   `Set … ▸` is not among the updates. Off a value chip, nothing moves.
//! - **A submenu's marks** ([`options`]): one ticket marks its value; a
//!   selection marks each value with how many already hold it.
//! - **Filter everywhere** (FILTER-25): where the filter items build a
//!   surface's own filter (the nav, the board, the wave board), a group of
//!   **Filter everywhere: …** follows them — each of their clauses, the
//!   global filter. Where they build the global filter already (the ticket
//!   page), nothing doubles; on a framed chip (whose items build the global
//!   filter) the Filter items are said **Filter everywhere: …**.

use menger_tickets_api::labels::NEEDS_YOU;
use menger_tickets_api::{Kind, Severity, Status, TicketSummary};

use super::{wave_items, WaveItem};
use crate::filter::ScTree;
use crate::tags::{self, Tag};

/// What the reader may do, as far as the app knows it — the server decides
/// every call; this keeps a reader from being offered what it would refuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Grants {
    /// Every update (editor or admin; anyone with login off).
    pub(crate) write: bool,
    /// Delete (admin; anyone with login off).
    pub(crate) delete: bool,
}

/// What the menu opened on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Facts<'a> {
    /// The tickets it acts on: one, or the selection ([`target_ids`]).
    pub(crate) ids: &'a [String],
    /// Their rows, as far as they are held — a row not here yet marks nothing.
    pub(crate) rows: &'a [TicketSummary],
    /// The shortcode a nav row stands for (its root's row) — its Wave board.
    pub(crate) shortcode: Option<&'a str>,
    /// The chip right-clicked inside the element, if one was.
    pub(crate) chip: Option<&'a Tag>,
    pub(crate) grants: Grants,
    /// Opened in the nav: Nav ▸.
    pub(crate) nav: bool,
    /// The shortcodes' tree, for a shortcode's filter clause.
    pub(crate) tree: Option<&'a ScTree>,
    /// FILTER-25: the filter items build the global filter already (the
    /// ticket page, a framed chip) — no Filter everywhere group.
    pub(crate) global: bool,
    /// FILTER-25: opened on a chip in a framed body (MENGER-34).
    pub(crate) framed: bool,
}

impl Facts<'_> {
    fn one(&self) -> Option<&str> {
        match self.ids {
            [id] => Some(id),
            _ => None,
        }
    }
}

/// A submenu: its items are [`options`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Sub {
    Status,
    Kind,
    Severity,
    Labels,
    Unblock,
    Nav,
}

impl Sub {
    /// `data-sub`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Sub::Status => "status",
            Sub::Kind => "kind",
            Sub::Severity => "severity",
            Sub::Labels => "labels",
            Sub::Unblock => "unblock",
            Sub::Nav => "nav",
        }
    }

    pub(crate) fn of_name(name: &str) -> Option<Sub> {
        [Sub::Status, Sub::Kind, Sub::Severity, Sub::Labels, Sub::Unblock, Sub::Nav].into_iter().find(|s| s.name() == name)
    }

    pub(crate) fn words(self) -> &'static str {
        match self {
            Sub::Status => "Set status",
            Sub::Kind => "Set kind",
            Sub::Severity => "Set severity",
            Sub::Labels => "Labels",
            Sub::Unblock => "Unblock",
            Sub::Nav => "Nav",
        }
    }
}

/// An item that asks for more inside the menu: a ticket, a title, commits,
/// a label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// A ticket picker: every one filed under it.
    MoveUnder,
    /// A ticket picker: every one waits on it.
    BlockedBy,
    /// The title, in place (one ticket).
    Rename,
    /// Commit hashes (one ticket).
    Commits,
    /// A label to add (from Labels ▸).
    AddLabel,
}

impl Form {
    /// `data-form`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Form::MoveUnder => "move-under",
            Form::BlockedBy => "blocked-by",
            Form::Rename => "rename",
            Form::Commits => "commits",
            Form::AddLabel => "add-label",
        }
    }

    pub(crate) fn words(self) -> &'static str {
        match self {
            Form::MoveUnder => "Move under…",
            Form::BlockedBy => "Blocked by…",
            Form::Rename => "Rename…",
            Form::Commits => "Link commits…",
            Form::AddLabel => "Add a label…",
        }
    }

    /// Whether it picks a ticket (the palette's find).
    pub(crate) fn picks(self) -> bool {
        matches!(self, Form::MoveUnder | Form::BlockedBy)
    }
}

/// One line of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Open(String),
    OpenNew(String),
    /// MENGER-31's wave items.
    Wave(WaveItem),
    /// WAVE-5: a shortcode's row — its wave board.
    WaveBoard(String),
    /// Copy code / Copy codes: the text copied, and the words.
    Copy { text: String, words: &'static str },
    Sub(Sub),
    /// ACT-10: a submenu's options drawn flat, its head and indent gone — a
    /// value chip's own chooser, first.
    Choice(Sub),
    Form(Form),
    /// FILTER-22's items (and a selection's).
    Filter(tags::Item),
    /// Delete… — how many.
    Delete(usize),
    Rule,
}

/// ACT-3 × ACT-4: the tickets a right-click on `id` acts on — the selection,
/// when `id` is in it and it holds more than one; otherwise `id` alone (the
/// selection kept).
pub(crate) fn target_ids(id: &str, selected: &[String]) -> Vec<String> {
    if selected.len() > 1 && selected.iter().any(|s| s == id) {
        selected.to_vec()
    } else {
        vec![id.to_string()]
    }
}

/// The menu's header: one ticket's id, or "N tickets".
pub(crate) fn heading(ids: &[String]) -> String {
    match ids {
        [id] => id.clone(),
        many => format!("{} tickets", many.len()),
    }
}

/// ACT-10: the chooser a value chip puts first, flat — its submenu, when the
/// reader may write and the menu would offer it at all (a status chooser
/// needs a held row with a lifecycle, as Set status ▸ does).
pub(crate) fn chooser(f: &Facts) -> Option<Sub> {
    if !f.grants.write || f.ids.is_empty() {
        return None;
    }
    match f.chip? {
        Tag::Status(_) => f.rows.iter().any(|r| r.kind.has_lifecycle()).then_some(Sub::Status),
        Tag::Kind(_) => Some(Sub::Kind),
        Tag::Severity(_) => Some(Sub::Severity),
        Tag::Label(_) => Some(Sub::Labels),
        _ => None,
    }
}

/// The menu, in order (see the module doc).
pub(crate) fn entries(f: &Facts) -> Vec<Entry> {
    let one = f.one();
    let row = one.and_then(|id| f.rows.iter().find(|r| r.id == id));
    let mut groups: Vec<Vec<Entry>> = Vec::new();

    // ACT-10: a value chip's own chooser, flat, first.
    let flat = chooser(f);
    if let Some(sub) = flat {
        groups.push(vec![Entry::Choice(sub)]);
    }

    // The ticket's own actions.
    let mut acts = Vec::new();
    match one {
        Some(id) => {
            acts.push(Entry::Open(id.to_string()));
            acts.push(Entry::OpenNew(id.to_string()));
            let (parent, doc) = row.map(|r| (r.parent.as_deref(), r.kind == Kind::Doc)).unwrap_or((None, false));
            acts.extend(wave_items(id, parent).into_iter().filter(|w| !(doc && w.kind == "to")).map(Entry::Wave));
            if let Some(sc) = f.shortcode {
                acts.push(Entry::WaveBoard(sc.to_string()));
            }
            acts.push(Entry::Copy { text: id.to_string(), words: "Copy code" });
        }
        None => acts.push(Entry::Copy { text: f.ids.join("\n"), words: "Copy codes" }),
    }
    groups.push(acts);

    // Every update — absent for a reader who cannot write.
    let mut updates = Vec::new();
    if f.grants.write && !f.ids.is_empty() {
        // SDLC-11: a doc has no status to set; until a row is held, none.
        if f.rows.iter().any(|r| r.kind.has_lifecycle()) {
            updates.push(Entry::Sub(Sub::Status));
        }
        updates.extend([Entry::Sub(Sub::Kind), Entry::Sub(Sub::Severity), Entry::Sub(Sub::Labels)]);
        updates.extend([Entry::Form(Form::MoveUnder), Entry::Form(Form::BlockedBy)]);
        if f.rows.iter().any(|r| !r.blocked_by.is_empty()) {
            updates.push(Entry::Sub(Sub::Unblock));
        }
        if one.is_some() {
            updates.extend([Entry::Form(Form::Rename), Entry::Form(Form::Commits)]);
        }
    }
    // ACT-10: the chooser already drawn flat has no submenu as well.
    if let Some(sub) = flat {
        updates.retain(|e| *e != Entry::Sub(sub));
    }
    groups.push(updates);

    // The filter items: a value chip's first, then the ticket's (or the
    // selection's).
    let mut filters: Vec<Entry> = Vec::new();
    if let Some(chip @ (Tag::Status(_) | Tag::Kind(_) | Tag::Severity(_) | Tag::Label(_))) = f.chip {
        filters.extend(tags::items(chip, f.tree).into_iter().map(Entry::Filter));
    }
    match one {
        Some(id) => filters.extend(tags::items(&Tag::ticket(id), f.tree).into_iter().map(Entry::Filter)),
        None => filters.extend(tags::many_items(f.ids, f.tree).into_iter().map(Entry::Filter)),
    }
    if f.framed {
        filters = filters
            .into_iter()
            .map(|e| match e {
                Entry::Filter(it) => Entry::Filter(tags::as_everywhere(vec![it]).remove(0)),
                e => e,
            })
            .collect();
    }
    groups.push(filters);

    // FILTER-25: the same clauses, the global filter — off it.
    let mut everywhere: Vec<Entry> = Vec::new();
    if !f.global {
        if let Some(chip @ (Tag::Status(_) | Tag::Kind(_) | Tag::Severity(_) | Tag::Label(_))) = f.chip {
            everywhere.extend(tags::everywhere(chip, f.tree).into_iter().map(Entry::Filter));
        }
        match one {
            Some(id) => everywhere.extend(tags::everywhere(&Tag::ticket(id), f.tree).into_iter().map(Entry::Filter)),
            None => everywhere.extend(tags::many_everywhere(f.ids, f.tree).into_iter().map(Entry::Filter)),
        }
    }
    groups.push(everywhere);

    if f.nav {
        groups.push(vec![Entry::Sub(Sub::Nav)]);
    }
    if f.grants.delete && !f.ids.is_empty() {
        groups.push(vec![Entry::Delete(f.ids.len())]);
    }

    let mut out = Vec::new();
    for g in groups.into_iter().filter(|g| !g.is_empty()) {
        if !out.is_empty() {
            out.push(Entry::Rule);
        }
        out.extend(g);
    }
    out
}

/// The Delete item's words.
pub(crate) fn delete_words(n: usize) -> String {
    if n == 1 {
        "Delete…".into()
    } else {
        format!("Delete {n}…")
    }
}

/// A submenu item's mark: one ticket's value, or how many of a selection
/// hold it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    None,
    Current,
    Count(usize),
}

impl Mark {
    fn of(holding: usize, total: usize) -> Mark {
        match (holding, total) {
            (0, _) => Mark::None,
            (_, 1) => Mark::Current,
            (n, _) => Mark::Count(n),
        }
    }

    /// What the mark column shows.
    pub(crate) fn text(self) -> String {
        match self {
            Mark::None => String::new(),
            Mark::Current => "✓".into(),
            Mark::Count(n) => n.to_string(),
        }
    }
}

/// One change the menu makes — to every ticket it acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Status(Status),
    Kind(Kind),
    /// `None` clears it.
    Severity(Option<Severity>),
    Parent(String),
    Block(String),
    Unblock(String),
    Label(String),
    Unlabel(String),
    /// One ticket's.
    Title(String),
    /// One ticket's.
    Commits(Vec<String>),
}

impl Change {
    /// How the error banner names it.
    pub(crate) fn words(&self) -> String {
        match self {
            Change::Status(s) => format!("set status {s}"),
            Change::Kind(k) => format!("set kind {k}"),
            Change::Severity(Some(s)) => format!("set severity {s}"),
            Change::Severity(None) => "clear severity".into(),
            Change::Parent(p) => format!("move under {p}"),
            Change::Block(b) => format!("block by {b}"),
            Change::Unblock(b) => format!("unblock {b}"),
            Change::Label(l) => format!("label {l}"),
            Change::Unlabel(l) => format!("unlabel {l}"),
            Change::Title(_) => "rename".into(),
            Change::Commits(_) => "link commits".into(),
        }
    }
}

/// One item of a submenu: its value (`data-value`), its words, its mark, and
/// the change it makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Opt {
    pub(crate) value: String,
    pub(crate) words: String,
    pub(crate) mark: Mark,
    pub(crate) change: Change,
}

/// A submenu's items over the tickets `ids` (their held `rows` mark them).
/// Nav ▸ has none here — its items are src/shape.rs's.
pub(crate) fn options(sub: Sub, ids: &[String], rows: &[TicketSummary]) -> Vec<Opt> {
    let total = ids.len();
    let holding = |f: &dyn Fn(&TicketSummary) -> bool| rows.iter().filter(|r| f(r)).count();
    let opt = |value: String, words: String, n: usize, change: Change| Opt { value, words, mark: Mark::of(n, total), change };
    match sub {
        Sub::Status => Status::LIFECYCLE
            .into_iter()
            .map(|s| opt(s.as_str().into(), s.as_str().into(), holding(&|r| r.status == s), Change::Status(s)))
            .collect(),
        Sub::Kind => Kind::ALL
            .into_iter()
            .map(|k| opt(k.as_str().into(), k.as_str().into(), holding(&|r| r.kind == k), Change::Kind(k)))
            .collect(),
        Sub::Severity => [Some(Severity::Critical), Some(Severity::High), Some(Severity::Medium), Some(Severity::Low), None]
            .into_iter()
            .map(|s| {
                let name = s.map_or("none", |s| s.as_str());
                opt(name.into(), name.into(), holding(&|r| r.severity == s), Change::Severity(s))
            })
            .collect(),
        Sub::Labels => {
            let mut labels: Vec<String> = Vec::new();
            for r in rows {
                for l in &r.labels {
                    if !labels.iter().any(|h| l == h) {
                        labels.push(l.to_string());
                    }
                }
            }
            if !labels.iter().any(|l| l == NEEDS_YOU) {
                labels.push(NEEDS_YOU.into());
            }
            let mut out = Vec::new();
            for l in labels {
                let n = holding(&|r| r.labels.iter().any(|h| *h == l));
                if n == 0 {
                    out.push(opt(l.clone(), l.clone(), 0, Change::Label(l)));
                } else if n >= total {
                    // Held by every one: taking it off.
                    out.push(opt(l.clone(), l.clone(), n, Change::Unlabel(l)));
                } else {
                    out.push(opt(l.clone(), format!("{l} — add to all"), n, Change::Label(l.clone())));
                    out.push(Opt { value: l.clone(), words: format!("{l} — remove from all"), mark: Mark::None, change: Change::Unlabel(l) });
                }
            }
            out
        }
        Sub::Unblock => {
            let mut blockers: Vec<String> = Vec::new();
            for r in rows {
                for b in &r.blocked_by {
                    if !blockers.iter().any(|h| b == h) {
                        blockers.push(b.to_string());
                    }
                }
            }
            blockers
                .into_iter()
                .map(|b| {
                    let n = holding(&|r| r.blocked_by.iter().any(|h| *h == b));
                    opt(b.clone(), b.clone(), n, Change::Unblock(b))
                })
                .collect()
        }
        Sub::Nav => Vec::new(),
    }
}

/// An update's fields, as `update` and `update_many` take them (the wire's
/// spellings).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Args {
    pub(crate) title: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) parent: Option<String>,
    pub(crate) severity: Option<String>,
    pub(crate) block: Option<Vec<String>>,
    pub(crate) unblock: Option<Vec<String>>,
    pub(crate) label: Option<Vec<String>>,
    pub(crate) unlabel: Option<Vec<String>>,
}

/// The one write a change makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    /// One ticket's status: `set_status`, as the board's select — MENGER-13's
    /// restore prompt on leaving Canceled rides it.
    SetStatus(String, Status),
    /// One ticket: `update`.
    Update(String, Args),
    /// A selection: `update_many { ids }` (ACT-2).
    Many(Vec<String>, Args),
    /// One ticket's labels: `label { id, add, remove }`.
    Label { id: String, add: Vec<String>, remove: Vec<String> },
    /// One ticket's commits: `commits.link { id, commits }` (ACCESS-15).
    Commits(String, Vec<String>),
}

/// `change` to every one of `ids` as one write — `None` for what a
/// selection does not take (a title, commits) or for no tickets.
pub(crate) fn call(ids: &[String], change: &Change) -> Option<Call> {
    let one = match ids {
        [] => return None,
        [id] => Some(id.clone()),
        _ => None,
    };
    let mut a = Args::default();
    match (change, &one) {
        (Change::Status(s), Some(id)) => return Some(Call::SetStatus(id.clone(), *s)),
        (Change::Label(l), Some(id)) => return Some(Call::Label { id: id.clone(), add: vec![l.clone()], remove: Vec::new() }),
        (Change::Unlabel(l), Some(id)) => return Some(Call::Label { id: id.clone(), add: Vec::new(), remove: vec![l.clone()] }),
        (Change::Commits(c), Some(id)) => return Some(Call::Commits(id.clone(), c.clone())),
        (Change::Title(t), Some(_)) => a.title = Some(t.clone()),
        (Change::Title(_) | Change::Commits(_), None) => return None,
        (Change::Status(s), None) => a.status = Some(s.as_str().into()),
        (Change::Label(l), None) => a.label = Some(vec![l.clone()]),
        (Change::Unlabel(l), None) => a.unlabel = Some(vec![l.clone()]),
        (Change::Kind(k), _) => a.kind = Some(k.as_str().into()),
        (Change::Severity(s), _) => a.severity = Some(s.map_or("none", |s| s.as_str()).into()),
        (Change::Parent(p), _) => a.parent = Some(p.clone()),
        (Change::Block(b), _) => a.block = Some(vec![b.clone()]),
        (Change::Unblock(b), _) => a.unblock = Some(vec![b.clone()]),
    }
    Some(match one {
        Some(id) => Call::Update(id, a),
        None => Call::Many(ids.to_vec(), a),
    })
}

/// ACT-10: where `]` (`up`) or `[` moves the tickets `ids` — the next (or
/// previous) status on the worker line ([`Status::LINE`]) from the one
/// status they all hold — or, in the reader's words, why nothing is written:
/// a row not held, a doc, a selection of mixed statuses, a status off the
/// line, the line's end.
pub(crate) fn step_status(ids: &[String], rows: &[TicketSummary], up: bool) -> Result<Status, String> {
    let mut held: Vec<&TicketSummary> = Vec::new();
    for id in ids {
        match rows.iter().find(|r| &r.id == id) {
            Some(r) => held.push(r),
            None => return Err(format!("{id} is not loaded yet: try again in a moment")),
        }
    }
    let Some(first) = held.first() else { return Err("no ticket to step".into()) };
    if let Some(doc) = held.iter().find(|r| !r.kind.has_lifecycle()) {
        return Err(format!("{} is a doc: it has no status to step", doc.id));
    }
    let mut statuses: Vec<Status> = Vec::new();
    for r in &held {
        if !statuses.contains(&r.status) {
            statuses.push(r.status);
        }
    }
    if statuses.len() > 1 {
        let named: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        return Err(format!("the selection holds {}: step one status at a time", named.join(", ")));
    }
    let now = first.status;
    let to = if up { now.next() } else { now.prev() };
    match (to, now.on_line()) {
        (Some(s), _) => Ok(s),
        (None, None) => Err(format!("{now} has no {} status: pick one from the menu", if up { "next" } else { "previous" })),
        (None, Some(_)) if up => Err(format!("{now} is the last status on the line")),
        (None, Some(_)) => Err(format!("{now} is the first status on the line")),
    }
}

/// What a picked ticket can be for `ids`: never one of them.
pub(crate) fn pickable(ids: &[String], candidate: &str) -> bool {
    !ids.iter().any(|id| id.eq_ignore_ascii_case(candidate))
}

/// The commits a typed entry names: split at commas and spaces, each once.
pub(crate) fn parse_commits(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in raw.split(|c: char| c == ',' || c.is_whitespace()).filter(|c| !c.is_empty()) {
        if !out.iter().any(|o| o == c) {
            out.push(c.to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------
// Delete…

/// One ticket a delete takes, and what points at it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Doomed {
    pub(crate) id: String,
    pub(crate) title: String,
    /// Its children (they move to its parent, or go too).
    pub(crate) children: u64,
    /// The tickets outside the delete that wait on it (they are unblocked).
    pub(crate) waiting: Vec<String>,
}

/// What Delete… asks before it writes: every ticket, and what moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeletePlan {
    pub(crate) ids: Vec<String>,
    pub(crate) doomed: Vec<Doomed>,
}

impl DeletePlan {
    /// `ids`, as their held `rows` say them (a row not held: its id alone).
    pub(crate) fn of(ids: &[String], rows: &[TicketSummary]) -> DeletePlan {
        let doomed = ids
            .iter()
            .map(|id| match rows.iter().find(|r| &r.id == id) {
                Some(r) => Doomed {
                    id: id.clone(),
                    title: r.title.clone(),
                    children: r.child_count,
                    waiting: r.unlocks.iter().filter(|u| !ids.iter().any(|i| *u == i)).map(ToString::to_string).collect(),
                },
                None => Doomed { id: id.clone(), title: String::new(), children: 0, waiting: Vec::new() },
            })
            .collect();
        DeletePlan { ids: ids.to_vec(), doomed }
    }

    /// Whether one has children — the dialog asks where they go.
    pub(crate) fn has_children(&self) -> bool {
        self.doomed.iter().any(|d| d.children > 0)
    }

    /// Whether a ticket outside waits on one — the dialog says they are
    /// unblocked.
    pub(crate) fn has_waiting(&self) -> bool {
        self.doomed.iter().any(|d| !d.waiting.is_empty())
    }

    /// The dialog's heading.
    pub(crate) fn words(&self) -> String {
        match self.ids.as_slice() {
            [id] => format!("Delete {id}?"),
            many => format!("Delete {} tickets?", many.len()),
        }
    }
}

/// What a `delete` takes for a plan: `children` (`reparent` / `delete`) when
/// one has children, `unblock` when one is waited on.
pub(crate) fn delete_args(plan: &DeletePlan, children_go_too: bool) -> (Option<String>, Option<bool>) {
    let children = plan.has_children().then(|| if children_go_too { "delete" } else { "reparent" }.to_string());
    let unblock = plan.has_waiting().then_some(true);
    (children, unblock)
}

/// The undo toast's words.
pub(crate) fn deleted_words(ids: &[String]) -> String {
    match ids {
        [id] => format!("deleted {id}"),
        many => format!("deleted {} tickets", many.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, status: Status, kind: Kind) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        TicketSummary {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            archived: None,
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(),
            seq: seq.parse().unwrap(),
            title: format!("{id} title"),
            status,
            kind,
            severity: None,
            parent: None,
            child_count: 0,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            commits: Vec::new(),
            path: None,
            placement: None,
            labels: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn names(v: &[&str]) -> Vec<menger_tickets_api::TicketName> {
        v.iter().map(|s| menger_tickets_api::TicketName::new(*s)).collect()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    const ALL: Grants = Grants { write: true, delete: true };

    fn facts<'a>(ids: &'a [String], rows: &'a [TicketSummary], grants: Grants) -> Facts<'a> {
        Facts { ids, rows, shortcode: None, chip: None, grants, nav: false, tree: None, global: true, framed: false }
    }

    /// Each entry as the menu says it.
    fn said(es: &[Entry]) -> Vec<String> {
        es.iter()
            .map(|e| match e {
                Entry::Open(_) => "Open".into(),
                Entry::OpenNew(_) => "Open in new tab".into(),
                Entry::Wave(w) => w.words.clone(),
                Entry::WaveBoard(_) => "Wave board".into(),
                Entry::Copy { words, .. } => words.to_string(),
                Entry::Sub(s) => format!("{} ▸", s.words()),
                Entry::Choice(s) => format!("[{}]", s.name()),
                Entry::Form(f) => f.words().into(),
                Entry::Filter(i) => i.words.clone(),
                Entry::Delete(n) => delete_words(*n),
                Entry::Rule => "—".into(),
            })
            .collect()
    }

    /// The ticket's example, on DEMAND-4 (one ticket, an admin): its
    /// actions, every update, the filter items, Delete — each group ruled off.
    #[test]
    fn one_ticket_holds_its_actions_every_update_and_its_filter_items() {
        let mut r = row("DEMAND-4", Status::Ready, Kind::Build);
        r.parent = Some(menger_tickets_api::TicketId::new("DEMAND-1"));
        r.blocked_by = names(&["DEMAND-2"]);
        let (i, rows) = (ids(&["DEMAND-4"]), vec![r]);
        assert_eq!(
            said(&entries(&facts(&i, &rows, ALL))),
            [
                "Open", "Open in new tab", "Wave board for DEMAND-1", "Waves to DEMAND-4", "Copy code", "—",
                "Set status ▸", "Set kind ▸", "Set severity ▸", "Labels ▸", "Move under…", "Blocked by…", "Unblock ▸", "Rename…", "Link commits…", "—",
                "Filter DEMAND", "Filter under DEMAND-4", "Add DEMAND to the filter", "Build a filter from DEMAND…", "—",
                "Delete…",
            ]
        );
        assert_eq!(heading(&i), "DEMAND-4");
    }

    /// A selection: "N tickets", Copy codes, every update but the one-ticket
    /// ones (no Open, no Rename, no Link commits), its shortcodes' filter items
    /// and `under` each, Delete N….
    #[test]
    fn a_selection_acts_in_aggregate_and_the_one_ticket_items_step_aside() {
        let i = ids(&["DEMAND-4", "DEMAND-5", "MNG-2"]);
        let rows: Vec<_> = i.iter().map(|id| row(id, Status::Ready, Kind::Build)).collect();
        let es = entries(&facts(&i, &rows, ALL));
        assert_eq!(
            said(&es),
            [
                "Copy codes", "—",
                "Set status ▸", "Set kind ▸", "Set severity ▸", "Labels ▸", "Move under…", "Blocked by…", "—",
                "Filter DEMAND, MNG", "Filter under each of 3", "Add DEMAND, MNG to the filter", "Build a filter from DEMAND, MNG…", "—",
                "Delete 3…",
            ]
        );
        assert_eq!(es[0], Entry::Copy { text: "DEMAND-4\nDEMAND-5\nMNG-2".into(), words: "Copy codes" }, "one per line");
        assert_eq!(heading(&i), "3 tickets");
    }

    /// The updates are absent — not disabled — for a reader who cannot
    /// write; Delete only for an admin.
    #[test]
    fn the_grants_decide_what_is_there() {
        let (i, rows) = (ids(&["A-1"]), vec![row("A-1", Status::Ready, Kind::Build)]);
        let reader = said(&entries(&facts(&i, &rows, Grants::default())));
        assert_eq!(reader, ["Open", "Open in new tab", "Waves to A-1", "Copy code", "—", "Filter A", "Filter under A-1", "Add A to the filter", "Build a filter from A…"]);
        let editor = said(&entries(&facts(&i, &rows, Grants { write: true, delete: false })));
        assert!(editor.contains(&"Set status ▸".to_string()) && !editor.iter().any(|w| w.starts_with("Delete")));
        let admin = said(&entries(&facts(&i, &rows, ALL)));
        assert_eq!(admin.last().map(String::as_str), Some("Delete…"));
    }

    /// A status, kind, severity or label chip puts its own filter items
    /// first in the filter group; the id chip is the ticket's own.
    #[test]
    fn a_chip_puts_its_filter_items_first() {
        let (i, rows) = (ids(&["DEMAND-4"]), vec![row("DEMAND-4", Status::Pending, Kind::Build)]);
        let chip = Tag::status(Status::Pending);
        let f = Facts { chip: Some(&chip), ..facts(&i, &rows, Grants::default()) };
        let s = said(&entries(&f));
        let at = s.iter().position(|w| w == "Filter status Pending").unwrap();
        assert_eq!(s[at - 1], "—");
        assert_eq!(&s[at..at + 4], ["Filter status Pending", "Add status Pending to the filter", "Build a filter from Pending…", "Filter DEMAND"]);
        let id_chip = Tag::ticket("DEMAND-4");
        let plain = said(&entries(&facts(&i, &rows, Grants::default())));
        assert_eq!(said(&entries(&Facts { chip: Some(&id_chip), ..facts(&i, &rows, Grants::default()) })), plain);
        for chip in [Tag::kind(Kind::Spike), Tag::severity(Severity::High), Tag::Label("needs-you".into())] {
            let s = said(&entries(&Facts { chip: Some(&chip), ..facts(&i, &rows, Grants::default()) }));
            let first = s.iter().find(|w| w.starts_with("Filter")).unwrap();
            assert!(!first.starts_with("Filter DEMAND"), "{chip:?}: {first}");
        }
    }

    /// ACT-10: a status chip, right-clicked by a reader who may write, opens
    /// the menu on the ten statuses, flat and first — the current marked,
    /// then a rule, then the menu as it was, with no Set status ▸. A kind,
    /// severity or label chip does the same with theirs (Add a label… is the
    /// view's, at the labels' end); a reader who cannot write, an id chip or
    /// no chip: the menu as before.
    #[test]
    fn a_value_chip_puts_its_own_chooser_first_flat() {
        let (i, rows) = (ids(&["DEMAND-4"]), vec![row("DEMAND-4", Status::Ready, Kind::Build)]);
        let plain = said(&entries(&facts(&i, &rows, ALL)));
        let chip = Tag::status(Status::Ready);
        let f = Facts { chip: Some(&chip), ..facts(&i, &rows, ALL) };
        let es = entries(&f);
        assert_eq!(es[0], Entry::Choice(Sub::Status));
        assert_eq!(es[1], Entry::Rule);
        let flat = options(Sub::Status, &i, &rows);
        assert_eq!(flat.iter().map(|o| o.value.as_str()).collect::<Vec<_>>(), Status::LIFECYCLE.map(|s| s.as_str()));
        assert_eq!(
            flat.iter().filter(|o| o.mark != Mark::None).map(|o| (o.value.as_str(), o.mark)).collect::<Vec<_>>(),
            [("Ready", Mark::Current)]
        );
        let s = said(&es);
        assert!(!s.contains(&"Set status ▸".to_string()), "{s:?}");
        // Then the menu as it was, less its Set status ▸.
        let rest: Vec<String> = plain.iter().filter(|w| *w != "Set status ▸").cloned().collect();
        let filters_at = s.iter().position(|w| w == "Filter status Ready").unwrap();
        assert_eq!(&s[2..filters_at], &rest[..rest.iter().position(|w| w == "Filter DEMAND").unwrap()]);
        // A kind, severity and label chip: theirs.
        for (chip, sub) in [
            (Tag::kind(Kind::Build), Sub::Kind),
            (Tag::severity(Severity::High), Sub::Severity),
            (Tag::Label("needs-you".into()), Sub::Labels),
        ] {
            let s = said(&entries(&Facts { chip: Some(&chip), ..facts(&i, &rows, ALL) }));
            assert_eq!(&s[..2], [format!("[{}]", sub.name()), "—".to_string()], "{chip:?}");
            assert!(!s.contains(&format!("{} ▸", sub.words())), "{chip:?}: {s:?}");
            assert!(s.contains(&"Set status ▸".to_string()), "the others stay: {s:?}");
        }
        // Without the write grant: as before — no chooser at all.
        let reader = Facts { chip: Some(&chip), ..facts(&i, &rows, Grants::default()) };
        assert_eq!(chooser(&reader), None);
        assert!(!entries(&reader).iter().any(|e| matches!(e, Entry::Choice(_) | Entry::Sub(Sub::Status))));
        // An id chip, or no chip: unchanged.
        let id_chip = Tag::ticket("DEMAND-4");
        assert_eq!(said(&entries(&Facts { chip: Some(&id_chip), ..facts(&i, &rows, ALL) })), plain);
        // A doc's status chip: no status chooser, flat or not.
        let (d, drows) = (ids(&["DOCS-3"]), vec![row("DOCS-3", Status::Doc, Kind::Doc)]);
        let doc_chip = Tag::status(Status::Doc);
        let s = said(&entries(&Facts { chip: Some(&doc_chip), ..facts(&d, &drows, ALL) }));
        assert!(!s.iter().any(|w| w == "[status]" || w == "Set status ▸"), "{s:?}");
        // A selection: the chooser, with its counts.
        let many = ids(&["A-1", "A-2"]);
        let mrows: Vec<_> = many.iter().map(|id| row(id, Status::Ready, Kind::Build)).collect();
        let es = entries(&Facts { chip: Some(&chip), ..facts(&many, &mrows, ALL) });
        assert_eq!(es[0], Entry::Choice(Sub::Status));
    }

    /// ACT-10: `]` and `[` step along the worker line from the one status
    /// the tickets hold; a mix, a doc, a row not held, a status off the line
    /// and the line's ends say why and write nothing.
    #[test]
    fn a_step_walks_the_worker_line_or_says_why_not() {
        let one = ids(&["A-1"]);
        let r = |s| vec![row("A-1", s, Kind::Build)];
        assert_eq!(step_status(&one, &r(Status::Ready), true), Ok(Status::Coding));
        assert_eq!(step_status(&one, &r(Status::Ready), false), Ok(Status::Pending));
        assert_eq!(step_status(&one, &r(Status::InReview), true), Ok(Status::Complete));
        assert_eq!(step_status(&one, &r(Status::Epic), true), Err("Epic has no next status: pick one from the menu".into()));
        assert_eq!(step_status(&one, &r(Status::Canceled), false), Err("Canceled has no previous status: pick one from the menu".into()));
        assert_eq!(step_status(&one, &r(Status::Complete), true), Err("Complete is the last status on the line".into()));
        assert_eq!(step_status(&one, &r(Status::Idea), false), Err("Idea is the first status on the line".into()));
        assert_eq!(step_status(&one, &[], true), Err("A-1 is not loaded yet: try again in a moment".into()));
        let doc = vec![row("A-1", Status::Doc, Kind::Doc)];
        assert_eq!(step_status(&one, &doc, true), Err("A-1 is a doc: it has no status to step".into()));
        let many = ids(&["A-1", "A-2", "A-3"]);
        let mut rows: Vec<_> = many.iter().map(|id| row(id, Status::Ready, Kind::Build)).collect();
        assert_eq!(step_status(&many, &rows, true), Ok(Status::Coding), "one status: one step for all");
        rows[2].status = Status::Coding;
        assert_eq!(step_status(&many, &rows, true), Err("the selection holds Ready, Coding: step one status at a time".into()));
        // And the write is the menu's own: set_status for one, update_many for many.
        assert_eq!(call(&one, &Change::Status(Status::Coding)), Some(Call::SetStatus("A-1".into(), Status::Coding)));
    }

    /// A shortcode's row in the nav: its root's menu, the shortcode's Wave
    /// board, and Nav ▸; a doc has no status to set and no "Waves to".
    #[test]
    fn a_shortcode_row_in_the_nav_and_a_doc() {
        let (i, rows) = (ids(&["DEMAND-1"]), vec![row("DEMAND-1", Status::Epic, Kind::Scope)]);
        let f = Facts { shortcode: Some("DEMAND"), nav: true, ..facts(&i, &rows, ALL) };
        let s = said(&entries(&f));
        assert!(s.contains(&"Wave board".to_string()));
        let nav = s.iter().position(|w| w == "Nav ▸").unwrap();
        assert_eq!((s[nav - 1].as_str(), s[nav + 1].as_str()), ("—", "—"));
        assert!(entries(&f).contains(&Entry::WaveBoard("DEMAND".into())));
        let (d, drows) = (ids(&["DOCS-3"]), vec![row("DOCS-3", Status::Doc, Kind::Doc)]);
        let s = said(&entries(&facts(&d, &drows, ALL)));
        assert!(!s.contains(&"Set status ▸".to_string()) && !s.contains(&"Waves to DOCS-3".to_string()));
        // Until the row is held: no status either.
        assert!(!said(&entries(&facts(&d, &[], ALL))).contains(&"Set status ▸".to_string()));
    }

    /// One ticket marks its value; a selection marks each value with how many
    /// hold it.
    #[test]
    fn a_submenu_marks_the_value_or_how_many_hold_it() {
        let one = ids(&["A-1"]);
        let rows = vec![row("A-1", Status::Ready, Kind::Build)];
        let marks = |sub, i: &[String], rows: &[TicketSummary]| {
            options(sub, i, rows).into_iter().filter(|o| o.mark != Mark::None).map(|o| (o.value, o.mark)).collect::<Vec<_>>()
        };
        assert_eq!(marks(Sub::Status, &one, &rows), [("Ready".to_string(), Mark::Current)]);
        assert_eq!(marks(Sub::Kind, &one, &rows), [("build".to_string(), Mark::Current)]);
        assert_eq!(marks(Sub::Severity, &one, &rows), [("none".to_string(), Mark::Current)]);
        let many = ids(&["A-1", "A-2", "A-3"]);
        let mut rows: Vec<_> = many.iter().map(|id| row(id, Status::Ready, Kind::Build)).collect();
        rows[2].status = Status::Pending;
        rows[0].severity = Some(Severity::High);
        assert_eq!(marks(Sub::Status, &many, &rows), [("Pending".to_string(), Mark::Count(1)), ("Ready".to_string(), Mark::Count(2))]);
        assert_eq!(marks(Sub::Severity, &many, &rows), [("High".to_string(), Mark::Count(1)), ("none".to_string(), Mark::Count(2))]);
        assert_eq!(Mark::Count(2).text(), "2");
        assert_eq!(Mark::Current.text(), "✓");
        // Every lifecycle status, every kind, four severities and none.
        assert_eq!(options(Sub::Status, &one, &[]).len(), 10);
        assert_eq!(options(Sub::Kind, &one, &[]).len(), 6);
        assert_eq!(options(Sub::Severity, &one, &[]).last().map(|o| o.change.clone()), Some(Change::Severity(None)));
    }

    /// Labels ▸: a label every one holds comes off; one none holds goes on;
    /// one some hold can go on all or come off all; `needs-you` is always
    /// offered. Unblock ▸ names every blocker any of them waits on.
    #[test]
    fn labels_and_unblock_over_one_or_many() {
        let one = ids(&["A-1"]);
        let mut r = row("A-1", Status::Ready, Kind::Build);
        r.labels = vec!["stale".parse().unwrap()];
        let o = options(Sub::Labels, &one, &[r.clone()]);
        assert_eq!(
            o.iter().map(|o| (o.words.as_str(), o.mark, o.change.clone())).collect::<Vec<_>>(),
            [("stale", Mark::Current, Change::Unlabel("stale".into())), ("needs-you", Mark::None, Change::Label("needs-you".into()))]
        );
        let many = ids(&["A-1", "A-2"]);
        let r2 = row("A-2", Status::Ready, Kind::Build);
        let o = options(Sub::Labels, &many, &[r.clone(), r2.clone()]);
        assert_eq!(
            o.iter().map(|o| (o.words.as_str(), o.change.clone())).collect::<Vec<_>>(),
            [
                ("stale — add to all", Change::Label("stale".into())),
                ("stale — remove from all", Change::Unlabel("stale".into())),
                ("needs-you", Change::Label("needs-you".into())),
            ]
        );
        assert_eq!(o[0].mark, Mark::Count(1));
        let mut b1 = r2.clone();
        b1.blocked_by = names(&["A-9", "B-1"]);
        let mut b2 = r.clone();
        b2.blocked_by = names(&["A-9"]);
        let o = options(Sub::Unblock, &many, &[b1, b2]);
        assert_eq!(o.iter().map(|o| (o.value.as_str(), o.mark)).collect::<Vec<_>>(), [("A-9", Mark::Count(2)), ("B-1", Mark::Count(1))]);
    }

    /// One write per change: `update` for one ticket (`set_status`, `label`
    /// and `commits.link` for theirs), `update_many` for a selection; a title
    /// and commits are one ticket's.
    #[test]
    fn a_change_is_one_write() {
        let one = ids(&["A-1"]);
        let many = ids(&["A-1", "B-2"]);
        assert_eq!(call(&one, &Change::Status(Status::Ready)), Some(Call::SetStatus("A-1".into(), Status::Ready)));
        assert_eq!(
            call(&many, &Change::Status(Status::Ready)),
            Some(Call::Many(many.clone(), Args { status: Some("Ready".into()), ..Args::default() }))
        );
        assert_eq!(call(&one, &Change::Kind(Kind::Gap)), Some(Call::Update("A-1".into(), Args { kind: Some("gap".into()), ..Args::default() })));
        assert_eq!(
            call(&many, &Change::Severity(None)),
            Some(Call::Many(many.clone(), Args { severity: Some("none".into()), ..Args::default() }))
        );
        assert_eq!(
            call(&many, &Change::Parent("C-1".into())),
            Some(Call::Many(many.clone(), Args { parent: Some("C-1".into()), ..Args::default() }))
        );
        assert_eq!(
            call(&one, &Change::Block("C-1".into())),
            Some(Call::Update("A-1".into(), Args { block: Some(ids(&["C-1"])), ..Args::default() }))
        );
        assert_eq!(
            call(&many, &Change::Unblock("C-1".into())),
            Some(Call::Many(many.clone(), Args { unblock: Some(ids(&["C-1"])), ..Args::default() }))
        );
        assert_eq!(call(&one, &Change::Label("x".into())), Some(Call::Label { id: "A-1".into(), add: ids(&["x"]), remove: vec![] }));
        assert_eq!(
            call(&many, &Change::Unlabel("x".into())),
            Some(Call::Many(many.clone(), Args { unlabel: Some(ids(&["x"])), ..Args::default() }))
        );
        assert_eq!(call(&one, &Change::Title("New".into())), Some(Call::Update("A-1".into(), Args { title: Some("New".into()), ..Args::default() })));
        assert_eq!(call(&many, &Change::Title("New".into())), None);
        assert_eq!(call(&one, &Change::Commits(ids(&["abc"]))), Some(Call::Commits("A-1".into(), ids(&["abc"]))));
        assert_eq!(call(&many, &Change::Commits(ids(&["abc"]))), None);
        assert_eq!(call(&[], &Change::Kind(Kind::Gap)), None);
    }

    /// A right-click on a selected row acts on the selection when it holds
    /// more than one; anywhere else, on that row alone.
    #[test]
    fn the_target_is_the_selection_or_the_row() {
        let sel = ids(&["A-1", "A-2", "A-3"]);
        assert_eq!(target_ids("A-2", &sel), sel);
        assert_eq!(target_ids("B-1", &sel), ids(&["B-1"]));
        assert_eq!(target_ids("A-1", &ids(&["A-1"])), ids(&["A-1"]));
        assert_eq!(target_ids("A-1", &[]), ids(&["A-1"]));
    }

    /// Delete… lists every ticket and what moves: children (reparented, or
    /// gone too) and the tickets outside that wait on one (unblocked).
    #[test]
    fn delete_says_what_moves() {
        let i = ids(&["A-1", "A-2"]);
        let mut a = row("A-1", Status::Ready, Kind::Build);
        a.child_count = 2;
        a.unlocks = names(&["A-2", "B-7"]);
        let b = row("A-2", Status::Ready, Kind::Build);
        let plan = DeletePlan::of(&i, &[a, b]);
        assert_eq!(plan.doomed[0].waiting, ids(&["B-7"]), "A-2 goes too: not waiting");
        assert!(plan.has_children() && plan.has_waiting());
        assert_eq!(delete_args(&plan, false), (Some("reparent".into()), Some(true)));
        assert_eq!(delete_args(&plan, true), (Some("delete".into()), Some(true)));
        assert_eq!(plan.words(), "Delete 2 tickets?");
        let lone = DeletePlan::of(&ids(&["C-1"]), &[]);
        assert_eq!(delete_args(&lone, false), (None, None));
        assert_eq!(lone.words(), "Delete C-1?");
        assert_eq!(deleted_words(&ids(&["C-1"])), "deleted C-1");
        assert_eq!(delete_words(3), "Delete 3…");
    }

    /// FILTER-25: off the global filter (a nav or board row) the menu offers
    /// it too, each of its clauses; on the ticket page nothing doubles; a
    /// framed chip says its Filter items as Filter everywhere.
    #[test]
    fn off_the_global_filter_the_menu_offers_it_too() {
        // On a nav or board row (the surface's own filter), the filter group,
        // then Filter everywhere for each of its clauses.
        let (i, rows) = (ids(&["DEMAND-1"]), vec![row("DEMAND-1", Status::Ready, Kind::Build)]);
        let on_nav = Facts { global: false, ..facts(&i, &rows, Grants::default()) };
        let s = said(&entries(&on_nav));
        let at = s.iter().position(|w| w == "Filter DEMAND").unwrap();
        assert_eq!(
            &s[at..],
            [
                "Filter DEMAND", "Filter under DEMAND-1", "Add DEMAND to the filter", "Build a filter from DEMAND…", "—",
                "Filter everywhere: DEMAND", "Filter everywhere: under DEMAND-1",
            ]
        );
        // A status chip: its own first, in both groups.
        let chip = Tag::status(Status::Ready);
        let s = said(&entries(&Facts { chip: Some(&chip), ..on_nav }));
        let at = s.iter().position(|w| w == "Filter everywhere: status Ready").unwrap();
        assert_eq!(&s[at..], ["Filter everywhere: status Ready", "Filter everywhere: DEMAND", "Filter everywhere: under DEMAND-1"]);
        // A selection: its shortcodes, `under` each.
        let many = ids(&["DEMAND-1", "MNG-2"]);
        let s = said(&entries(&Facts { global: false, ..facts(&many, &[], Grants::default()) }));
        assert_eq!(&s[s.len() - 2..], ["Filter everywhere: DEMAND, MNG", "Filter everywhere: under each of 2"]);
        // On the ticket page the Filter items are the global filter's: nothing
        // doubles.
        let page = said(&entries(&facts(&i, &rows, Grants::default())));
        assert!(!page.iter().any(|w| w.contains("everywhere")), "{page:?}");
        // A framed chip (the global filter, MENGER-34): its Filter items said
        // as Filter everywhere, once each.
        let framed = said(&entries(&Facts { framed: true, ..facts(&i, &rows, Grants::default()) }));
        let at = framed.iter().position(|w| w == "Filter everywhere: DEMAND").unwrap();
        assert_eq!(
            &framed[at..],
            ["Filter everywhere: DEMAND", "Filter everywhere: under DEMAND-1", "Add DEMAND to the filter", "Build a filter from DEMAND…"]
        );
        assert_eq!(framed.iter().filter(|w| w.starts_with("Filter everywhere")).count(), 2);
    }

    /// A picker never offers one of the tickets acted on; commits split as
    /// labels do.
    #[test]
    fn pickers_and_commits() {
        let i = ids(&["A-1", "A-2"]);
        assert!(!pickable(&i, "a-1") && pickable(&i, "A-3"));
        assert_eq!(parse_commits(" abc1234, def5678 abc1234\n"), ids(&["abc1234", "def5678"]));
        assert!(parse_commits("  ").is_empty());
        assert_eq!(Sub::of_name("labels"), Some(Sub::Labels));
        assert_eq!(Sub::of_name("nope"), None);
    }
}
