//! COMP-3: the **component registry** — every component a ticket defines,
//! who owns it, and what uses it (COMP-1 decisions 2, 3; pins 6–8).
//!
//! **What a body says** ([`read`]). A body is read as the app renders it:
//! frontmatter split off as `frontend/src/md.rs` splits it (it is collapsed
//! outside the frame, never HTML), the rest through the app's own markdown
//! parser with its options. Then:
//!
//! - a **definition** is a fenced code block whose info string's first word
//!   is `component` and whose second is the tag — ` ```component m-x ` (or
//!   `~~~`, at any nesting: a fence is a fence); its **source** is the
//!   block's content exactly as the parser hands it over. The tag must be a
//!   custom element's name ([`is_component_name`]): lowercase ASCII,
//!   starting with a letter, holding a hyphen, not one of the eight names
//!   HTML reserves. A ` ```component ` block that names no such tag, and a
//!   body that defines one name twice, are [`Problem`]s — a write refuses
//!   them in-band. A block inside an indented code block or inside a fence
//!   of another language is that block's text, not a definition. COMP-8:
//!   the words after the tag are its **preview** attributes, and the
//!   source's one `<docs>` element its **docs** ([`super::component_docs`]);
//!   a malformed `<docs>` is a [`Problem`] too.
//! - a **reference** is a tag `<name` in the body's raw HTML — an HTML block
//!   or inline HTML, as the parser finds them — whose name, ASCII
//!   lowercased (HTML's own folding), is a component name; outside HTML
//!   comments. A code span, a fenced or indented code block (a definition's
//!   own included), a tag that is not one (`a<b-c` in prose) and the
//!   frontmatter are text, not HTML. A ticket references a name once
//!   however often it writes it.
//! - a component's **source** is HTML through and through, so every `<name`
//!   in it outside comments is a reference by the component — `<script>`
//!   included, where markup a script writes counts as used; its own name in
//!   its own source is not one (it would keep itself alive).
//!
//! **What is kept** ([`Registry`]): one [`Entry`] per name, each with its
//! referrers — ticket ids and component names. A name with a source is a
//! component: owned (its ticket's body defines it) or an **orphan** (no body
//! does any more; read-only, kept while something live uses it). A name
//! with no source is **waiting**: tags some body or source writes that no
//! ticket has defined — kept like SCALE-3's forward references, so a
//! definition that arrives later is born with every reference made before
//! it, and the registry is the same whatever order the bodies were written
//! in. Waiting names never leave the store: not on the wire, not in events.
//!
//! **A write** ([`Registry::plan`]) — `create` (its body) and `amend` (a new
//! body) — works out its change before it commits, under the write-serial
//! lock, from the new body in hand and the registry's own record of what the
//! ticket defined and referenced before (the ticket owns exactly the
//! components whose `owner` it is, and references exactly the names whose
//! referrers hold it — so the old body is never read). A definition of a
//! name another ticket owns refuses the write ([`Owned`]); an orphan's name
//! is adopted. Then the sweep: an orphan is **alive** while a ticket
//! references it, or a live component — owned, or an alive orphan — does;
//! every other orphan goes in the same write, and its source's references
//! with it (a cycle of orphans that only reference each other goes too).
//! A waiting name nothing uses goes. The change is stored in the write's
//! transaction ([`store`]) and taken by the cache after it commits
//! ([`Registry::take`]); `watch` hears `component_changed` for each
//! component it moved and `component_removed` for each that went, in name
//! order, after the write's other events.
//!
//! **Stored** in one table, `components` — `name`, `source` (`NULL`:
//! waiting), `owner`, `references` (the referrers, a JSON list in the wire's
//! order), COMP-8's `preview` (a JSON list of `[name, value]`) — read whole
//! at open ([`load`]); the docs are the source's, parsed as it is read. A database without the table
//! gains it on first open, durably, once ([`migrate`]), with the registry
//! built from every body in the store's order: a name two bodies define is
//! the first's (the second is logged, and refused when it next changes its
//! body); a malformed block is logged and skipped. Orphans exist only here —
//! no body says they were ever defined — so the table is the record, not a
//! cache of the bodies, and `--verify-derived` does not rebuild it. A table
//! from before COMP-8 gains `preview` on first open, durably, once — each
//! owned component's read from its owner's body, an orphan's empty
//! ([`migrate_preview`]).

use crate::component_docs;
use crate::html_runs;
use crate::types::{Component, ComponentDocs};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The table: a row per name the registry keeps.
const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS components (
            name         TEXT PRIMARY KEY,
            source       TEXT,
            owner        TEXT,
            \"references\" TEXT NOT NULL,
            preview      TEXT NOT NULL DEFAULT '[]'
        ) WITHOUT ROWID;
    ";

use crate::component_docs::is_component_name;

/// One definition a body holds: the block's content, and what COMP-8 reads
/// from it and from its opening line.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Def {
    pub(crate) source: String,
    /// The attributes after the tag on the opening line.
    pub(crate) preview: Vec<(String, String)>,
    /// The source's `<docs>` — `None` when it holds none (or, in a
    /// [`read`] that also reports a [`Problem::Docs`], a malformed one).
    pub(crate) docs: Option<ComponentDocs>,
}

/// What one body says about components ([`read`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Uses {
    /// Each name the body defines, with its source.
    pub(crate) defines: BTreeMap<String, Def>,
    /// Each name the body references as a tag.
    pub(crate) references: BTreeSet<String>,
    /// THEME-3: its ` ```theme ` blocks and the themes its frontmatter names
    /// ([`super::theme_registry`]), whose change rides with this registry's.
    pub(crate) themes: super::theme_registry::BodyThemes,
    /// FILTER-36: its `filter:` line, as a saved filter's body holds one
    /// ([`super::saved`]), whose change rides with this registry's too.
    pub(crate) saved: Option<String>,
}

impl Uses {
    fn is_empty(&self) -> bool {
        self.defines.is_empty() && self.references.is_empty()
    }
}

/// A ` ```component ` block a write refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Problem {
    /// The block names no component name — `""` when it names nothing.
    Name(String),
    /// A second block defines the name (the first is the one a build keeps).
    Twice(String),
    /// COMP-8: the name's source holds a malformed `<docs>` — `problem`
    /// says what ([`component_docs::parse`]). The definition is still read,
    /// without docs, for a build that skips rather than refuses.
    Docs { name: String, problem: String },
    /// THEME-3: a ` ```theme ` block that is not a theme — `error` names
    /// its line ([`crate::theme_block`]).
    Theme { name: String, error: String },
    /// APP-32: an `<m-mockup>` with both a `ref` and a page inside it —
    /// the line and the reason ([`crate::mockups::problems`]).
    Mockup(String),
}

/// The frontmatter split off as the app splits it (`frontend/src/md.rs`,
/// `split_frontmatter`): a body whose first line is `---` with a later line
/// that is `---` alone. The rest is what is rendered.
fn rendered(body: &str) -> &str {
    let Some(after) = body.strip_prefix("---") else { return body };
    let Some(after) = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')) else {
        return body;
    };
    let mut offset = 0;
    for line in after.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return &after[offset + line.len()..];
        }
        offset += line.len();
    }
    body
}

/// What `body` defines and references, and any block a write refuses — see
/// the module doc for the rule. A body with no `<` and no `component` holds
/// neither, and is not parsed.
pub(crate) fn read(body: &str) -> (Uses, Vec<Problem>) {
    let mut uses = Uses { saved: crate::saved_filters::filter_line(body).map(str::to_string), ..Uses::default() };
    let mut problems = Vec::new();
    match super::theme_registry::read(body) {
        Ok(themes) => uses.themes = themes,
        Err((name, e)) => problems.push(Problem::Theme { name, error: e.to_string() }),
    }
    if !body.contains('<') && !body.contains("component") {
        return (uses, problems);
    }
    // The app's options (md.rs): what it renders as a table, a footnote or a
    // task list is what is read as one here.
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    // The block being read: its tag as written, its preview, and its source
    // so far.
    let mut block: Option<(String, Vec<(String, String)>, String)> = None;
    // An HTML block's lines, gathered so a comment across them is one.
    let mut html: Option<String> = None;
    // COMP-11: an element opened in an HTML block runs to its close, blank
    // lines included, as the frame renders it (`html_runs`).
    // APP-32: a page inside an `<m-mockup>` is the page's markup, not the
    // body's — read blanked, as a fence's content is code; one with a `ref`
    // too is refused, by line.
    for problem in crate::mockups::problems(body) {
        problems.push(Problem::Mockup(problem));
    }
    let blanked = crate::mockups::blank(rendered(body));
    let read = html_runs::read(&blanked, opts);
    for event in Parser::new_ext(read.source(), opts) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                if let Some((name, preview)) = component_docs::definition(&info) {
                    block = Some((name.to_string(), preview, String::new()));
                }
            }
            Event::Text(text) => {
                if let Some((_, _, source)) = &mut block {
                    source.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((name, preview, source)) = block.take() {
                    if !is_component_name(&name) {
                        problems.push(Problem::Name(name));
                    } else if let std::collections::btree_map::Entry::Vacant(free) = uses.defines.entry(name.clone()) {
                        let docs = component_docs::parse(&source).unwrap_or_else(|problem| {
                            problems.push(Problem::Docs { name, problem });
                            None
                        });
                        free.insert(Def { source, preview, docs });
                    } else {
                        problems.push(Problem::Twice(name));
                    }
                }
            }
            Event::Start(Tag::HtmlBlock) => html = Some(String::new()),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(lines) = html.take() {
                    tags(&lines, &mut uses.references);
                }
            }
            Event::Html(text) => match &mut html {
                Some(lines) => lines.push_str(&read.restore(&text)),
                None => tags(&text, &mut uses.references),
            },
            Event::InlineHtml(text) => tags(&text, &mut uses.references),
            _ => {}
        }
    }
    (uses, problems)
}

/// Every component name `html` opens a tag with — `<` then a name, which
/// ends at whitespace, `/` or `>` — ASCII lowercased, outside comments.
fn tags(html: &str, out: &mut BTreeSet<String>) {
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map_or(bytes.len(), |j| i + 4 + j + 3);
            continue;
        }
        let start = i + 1;
        if !bytes.get(start).is_some_and(u8::is_ascii_alphabetic) {
            i = start;
            continue;
        }
        let end = html[start..]
            .find(['\t', '\n', '\x0c', '\r', ' ', '/', '>'])
            .map_or(bytes.len(), |j| start + j);
        let name = html[start..end].to_ascii_lowercase();
        if is_component_name(&name) {
            out.insert(name);
        }
        i = end;
    }
}

/// The names `name`'s `source` references — every tag in it but its own.
fn source_uses(name: &str, source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    tags(source, &mut out);
    out.remove(name);
    out
}

/// Whether a referrer is a ticket (`EPIC-N`, upper case) rather than a
/// component (whose name starts with a lowercase letter).
fn is_ticket(referrer: &str) -> bool {
    !referrer.starts_with(|c: char| c.is_ascii_lowercase())
}

/// The referrers as the wire lists them: tickets in the store's order, then
/// components by name.
fn ordered(referrers: &BTreeSet<String>) -> Vec<String> {
    let (mut tickets, components): (Vec<&String>, Vec<&String>) = referrers.iter().partition(|r| is_ticket(r));
    tickets.sort_by_cached_key(|id| super::db::by_epic_seq(id));
    tickets.into_iter().chain(components).cloned().collect()
}

/// One name the registry keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The definition's content — `None` while the name is only waiting.
    pub(crate) source: Option<String>,
    /// The ticket that defines it — `None` for an orphan (or a waiting name).
    pub(crate) owner: Option<String>,
    /// Each ticket and component that references it.
    pub(crate) referrers: BTreeSet<String>,
    /// COMP-8: the definition's preview attributes (empty while waiting).
    pub(crate) preview: Vec<(String, String)>,
    /// COMP-8: the source's docs, parsed — derived, never stored.
    pub(crate) docs: Option<ComponentDocs>,
}

impl Entry {
    fn waiting() -> Self {
        Self {
            source: None,
            owner: None,
            referrers: BTreeSet::new(),
            preview: Vec::new(),
            docs: None,
        }
    }

    fn is_component(&self) -> bool {
        self.source.is_some()
    }

    fn component(&self, name: &str) -> Component {
        Component {
            name: name.to_string(),
            source: self.source.clone().expect("a component has a source"),
            owner: self.owner.clone(),
            references: ordered(&self.referrers),
            docs: self.docs.clone(),
            preview: self.preview.clone(),
        }
    }
}

/// A definition refused: `name` is `owner`'s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Owned {
    pub(crate) name: String,
    pub(crate) owner: String,
}

/// A write's moves: the components' and, THEME-3, the themes' beside them.
#[derive(Debug, Default, Clone)]
pub(crate) struct Moves {
    /// Each name whose entry moves, in name order — `None`: it goes.
    pub(crate) components: Vec<(String, Option<Entry>)>,
    pub(crate) themes: super::theme_registry::Moves,
    /// FILTER-36: the ticket whose body was written and the `filter:` line
    /// it holds now — `None`: no body was written.
    pub(crate) saved: Option<(String, Option<String>)>,
}

/// One write's change to the registry, worked out before it commits.
#[derive(Debug, Default, Clone)]
pub(crate) struct Plan {
    pub(crate) moves: Moves,
    /// Each component the write registered or moved, as it leaves it.
    pub(crate) changed: Vec<Component>,
    /// Each component that left the registry.
    pub(crate) removed: Vec<String>,
}

/// The registry, as the cache holds it: every name, in name order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Registry {
    names: BTreeMap<String, Entry>,
}

impl Registry {
    /// Every component — waiting names are not — in name order.
    pub(crate) fn components(&self) -> Vec<Component> {
        self.names
            .iter()
            .filter(|(_, e)| e.is_component())
            .map(|(name, e)| e.component(name))
            .collect()
    }

    /// ACCESS-12: whether `name` is a component (`None`: it names none —
    /// a ticket, then), and the ticket that owns it if one does.
    pub(super) fn owner_of(&self, name: &str) -> Option<Option<&str>> {
        self.names.get(name).filter(|e| e.is_component()).map(|e| e.owner.as_deref())
    }

    /// The ticket that owns `name`, if one does.
    fn owner(&self, name: &str) -> Option<&str> {
        self.names.get(name).and_then(|e| e.owner.as_deref())
    }

    /// What `ticket`'s write of a body saying `uses` does to the registry —
    /// see the module doc. Nothing changes here: the caller stores the plan
    /// and the cache takes it once that has committed. A definition of a
    /// name another ticket owns is refused, naming it.
    pub(crate) fn plan(&self, ticket: &str, uses: &Uses) -> Result<Plan, Owned> {
        for name in uses.defines.keys() {
            if let Some(owner) = self.owner(name).filter(|o| *o != ticket) {
                return Err(Owned {
                    name: name.clone(),
                    owner: owner.to_string(),
                });
            }
        }
        // What the ticket defined and referenced before — the registry's own
        // record of it, so the old body is never read.
        let mut defined = Vec::new();
        let mut referenced = BTreeSet::new();
        for (name, e) in &self.names {
            if e.owner.as_deref() == Some(ticket) {
                defined.push(name.clone());
            }
            if e.referrers.contains(ticket) {
                referenced.insert(name.clone());
            }
        }
        if defined.is_empty() && referenced.is_empty() && uses.is_empty() {
            return Ok(Plan::default());
        }
        let mut work = Work::over(&self.names);
        // A definition the body no longer holds: ownerless — the sweep says
        // whether anything keeps it.
        for name in defined.iter().filter(|n| !uses.defines.contains_key(*n)) {
            work.get_mut(name).expect("a component the ticket owns").owner = None;
        }
        for (name, def) in &uses.defines {
            let source = &def.source;
            let entry = work.entry(name);
            entry.owner = Some(ticket.to_string());
            entry.preview.clone_from(&def.preview);
            entry.docs.clone_from(&def.docs);
            let before = entry.source.replace(source.clone());
            if before.as_deref() == Some(source.as_str()) {
                continue;
            }
            let was = before.map(|s| source_uses(name, &s)).unwrap_or_default();
            let is = source_uses(name, source);
            for gone in was.difference(&is) {
                if let Some(target) = work.get_mut(gone) {
                    target.referrers.remove(name);
                }
            }
            for came in is.difference(&was) {
                work.entry(came).referrers.insert(name.clone());
            }
        }
        for gone in referenced.difference(&uses.references) {
            if let Some(target) = work.get_mut(gone) {
                target.referrers.remove(ticket);
            }
        }
        for came in uses.references.difference(&referenced) {
            work.entry(came).referrers.insert(ticket.to_string());
        }
        work.sweep();
        Ok(work.plan())
    }

    /// The cache's half of a committed [`Plan`].
    pub(crate) fn take(&mut self, moves: Vec<(String, Option<Entry>)>) {
        for (name, entry) in moves {
            match entry {
                Some(entry) => self.names.insert(name, entry),
                None => self.names.remove(&name),
            };
        }
    }

    /// How many names it keeps that are components, owned and orphaned, and
    /// how many are waiting — for a log line.
    fn census(&self) -> (usize, usize, usize) {
        let mut out = (0, 0, 0);
        for e in self.names.values() {
            match (&e.source, &e.owner) {
                (Some(_), Some(_)) => out.0 += 1,
                (Some(_), None) => out.1 += 1,
                (None, _) => out.2 += 1,
            }
        }
        out
    }
}

/// A plan in progress: the registry as it stands, and every entry the write
/// has touched over it (`None`: gone) — copied on first touch, so a write
/// copies the few names it moves and never the registry.
struct Work<'r> {
    base: &'r BTreeMap<String, Entry>,
    over: BTreeMap<String, Option<Entry>>,
}

impl<'r> Work<'r> {
    fn over(base: &'r BTreeMap<String, Entry>) -> Self {
        Self {
            base,
            over: BTreeMap::new(),
        }
    }

    fn get(&self, name: &str) -> Option<&Entry> {
        match self.over.get(name) {
            Some(touched) => touched.as_ref(),
            None => self.base.get(name),
        }
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Entry> {
        if !self.over.contains_key(name) {
            let entry = self.base.get(name)?.clone();
            self.over.insert(name.to_string(), Some(entry));
        }
        self.over.get_mut(name).and_then(Option::as_mut)
    }

    /// `name`'s entry — a waiting one where it has none.
    fn entry(&mut self, name: &str) -> &mut Entry {
        if self.get(name).is_none() {
            self.over.insert(name.to_string(), Some(Entry::waiting()));
        }
        self.get_mut(name).expect("just made")
    }

    /// Every name, as the write leaves them.
    fn names(&self) -> Vec<String> {
        let mut names: BTreeSet<&String> = self.base.keys().collect();
        names.extend(self.over.keys());
        names.into_iter().filter(|n| self.get(n).is_some()).cloned().collect()
    }

    /// Every orphan nothing live uses goes, with its source's references;
    /// then every waiting name nothing uses.
    fn sweep(&mut self) {
        let orphans: Vec<String> = self
            .names()
            .into_iter()
            .filter(|n| self.get(n).is_some_and(|e| e.is_component() && e.owner.is_none()))
            .collect();
        if !orphans.is_empty() {
            let mut alive: HashSet<&str> = HashSet::new();
            loop {
                let mut grew = false;
                for orphan in &orphans {
                    if alive.contains(orphan.as_str()) {
                        continue;
                    }
                    let entry = self.get(orphan).expect("an orphan");
                    let kept = entry.referrers.iter().any(|r| {
                        is_ticket(r)
                            || alive.contains(r.as_str())
                            || self.get(r).is_some_and(|c| c.owner.is_some())
                    });
                    if kept {
                        alive.insert(orphan);
                        grew = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            let dead: Vec<String> = orphans.iter().filter(|o| !alive.contains(o.as_str())).cloned().collect();
            for orphan in dead {
                let source = self.get(&orphan).and_then(|e| e.source.clone()).expect("an orphan");
                self.over.insert(orphan.clone(), None);
                for target in source_uses(&orphan, &source) {
                    if let Some(t) = self.get_mut(&target) {
                        t.referrers.remove(&orphan);
                    }
                }
            }
        }
        let unused: Vec<String> = self
            .over
            .iter()
            .filter(|(_, e)| e.as_ref().is_some_and(|e| !e.is_component() && e.referrers.is_empty()))
            .map(|(n, _)| n.clone())
            .collect();
        for name in unused {
            self.over.insert(name, None);
        }
    }

    /// The plan: each touched entry that differs from what stands, and what
    /// `watch` hears of it.
    fn plan(self) -> Plan {
        let mut plan = Plan::default();
        for (name, after) in self.over {
            let before = self.base.get(&name);
            if before == after.as_ref() {
                continue;
            }
            match (&after, before) {
                (Some(e), _) if e.is_component() => plan.changed.push(e.component(&name)),
                (None, Some(b)) if b.is_component() => plan.removed.push(name.clone()),
                _ => {}
            }
            plan.moves.components.push((name, after));
        }
        plan
    }
}

/// Writes a plan's moves inside the caller's transaction.
pub(crate) fn store(conn: &Connection, moves: &Moves) -> rusqlite::Result<()> {
    super::theme_registry::store(conn, &moves.themes)?;
    for (name, entry) in &moves.components {
        match entry {
            Some(e) => {
                conn.prepare_cached(
                    "INSERT INTO components (name, source, owner, \"references\", preview) VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT (name) DO UPDATE SET
                             source = excluded.source, owner = excluded.owner,
                             \"references\" = excluded.\"references\", preview = excluded.preview",
                )?
                .execute(params![
                    name,
                    e.source,
                    e.owner,
                    serde_json::to_string(&ordered(&e.referrers)).expect("serialize referrers"),
                    serde_json::to_string(&e.preview).expect("serialize preview"),
                ])?;
            }
            None => {
                conn.prepare_cached("DELETE FROM components WHERE name = ?1")?.execute([name])?;
            }
        }
    }
    Ok(())
}

/// A JSON column, read.
fn json<T: serde::de::DeserializeOwned>(r: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(r.get_ref(column)?.as_str()?)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, e.into()))
}

/// The docs of a stored source — COMP-8 parses them as a start reads it. A
/// source stored before COMP-8 that holds a malformed `<docs>` (no write
/// since would have taken it) is logged, and has none.
fn stored_docs(name: &str, source: Option<&str>) -> Option<ComponentDocs> {
    component_docs::parse(source?).unwrap_or_else(|problem| {
        tracing::warn!("COMP-8: the component '{name}' has a malformed <docs> ({problem}) — served without docs");
        None
    })
}

/// The registry as stored — what a start reads.
pub(crate) fn load(conn: &Connection) -> rusqlite::Result<Registry> {
    let mut stmt = conn.prepare("SELECT name, source, owner, \"references\", preview FROM components")?;
    let mut rows = stmt.query([])?;
    let mut names = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let name: String = r.get(0)?;
        let source: Option<String> = r.get(1)?;
        let referrers: Vec<String> = json(r, 3)?;
        let docs = stored_docs(&name, source.as_deref());
        names.insert(
            name,
            Entry {
                source,
                owner: r.get(2)?,
                referrers: referrers.into_iter().collect(),
                preview: json(r, 4)?,
                docs,
            },
        );
    }
    Ok(Registry { names })
}

/// Whether the file has the table.
fn has_table(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'components'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )
}

/// One-shot, durable migration of a database written before COMP-3 — and
/// a new one's first open. If the file has no `components` table, this
/// makes it and fills it with the registry built from every body, in the
/// store's order, in one transaction committed before the cache loads: a
/// SIGKILL before the commit leaves no table, and the next open builds
/// again; after it, every open reads ([`load`]). A body is parsed only when
/// it holds a `<` or `component` (SQLite answers which); the rest say
/// nothing. A malformed block is logged and skipped; a name a later body
/// defines again is the earlier body's, the later one logged. No ticket is
/// written. Returns whether the table was made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    if has_table(conn)? {
        migrate_preview(conn)?;
        return Ok(false);
    }
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    tx.execute_batch(TABLE)?;
    let mut registry = Registry::default();
    let mut read_bodies = 0usize;
    {
        let mut stmt = tx.prepare(
            "SELECT id, body FROM tickets
                 WHERE instr(body, '<') > 0 OR instr(body, 'component') > 0
                 ORDER BY epic, seq",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            read_bodies += 1;
            let id = r.get_ref(0)?.as_str()?.to_string();
            let (mut uses, problems) = read(r.get_ref(1)?.as_str()?);
            for problem in problems {
                match problem {
                    Problem::Name(name) => tracing::warn!(
                        "COMP-3 migration: {id} has a ```component block naming '{name}', which is not a \
                         component name — not registered"
                    ),
                    Problem::Twice(name) => tracing::warn!(
                        "COMP-3 migration: {id} defines '{name}' twice — the first block is its source"
                    ),
                    Problem::Docs { name, problem } => tracing::warn!(
                        "COMP-3 migration: {id}'s '{name}' has a malformed <docs> ({problem}) — registered \
                         without docs"
                    ),
                    // THEME-3's own migration reads the theme blocks.
                    Problem::Theme { .. } => {}
                    // APP-32: a stored body is drawn as it is; only a write refuses it.
                    Problem::Mockup(_) => {}
                }
            }
            uses.defines.retain(|name, _| match registry.owner(name) {
                Some(owner) if owner != id => {
                    tracing::warn!(
                        "COMP-3 migration: {id} defines '{name}', which {owner} (earlier in the store's order) \
                         defines — {owner}'s; {id}'s next body change is refused while it still defines it"
                    );
                    false
                }
                _ => true,
            });
            let plan = registry.plan(&id, &uses).expect("every name another ticket owns was dropped");
            registry.take(plan.moves.components);
        }
    }
    let rows: Vec<(String, Option<Entry>)> = registry.names.iter().map(|(n, e)| (n.clone(), Some(e.clone()))).collect();
    store(&tx, &Moves { components: rows, ..Moves::default() })?;
    let tickets: u64 = tx.query_row("SELECT count(*) FROM tickets", [], |r| r.get(0))?;
    tx.commit()?;
    let (owned, orphans, waiting) = registry.census();
    tracing::info!(
        "COMP-3: added the components table — {owned} component(s), {orphans} orphan(s), {waiting} name(s) \
         referenced before any ticket defines them — built from the bodies of {tickets} ticket(s) ({read_bodies} \
         parsed) in {:.1} ms, durably",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(true)
}

/// COMP-8: one-shot, durable migration of a `components` table written
/// before COMP-8. If it has no `preview` column, this adds it — `TEXT NOT
/// NULL DEFAULT '[]'`, as [`TABLE`] makes it — and fills each owned
/// component's from its owner's body (the opening line it is defined on
/// now); an orphan's stays empty, no body saying it any more. One
/// transaction, committed before the cache loads; every later open finds the
/// column. Returns whether it was added.
pub(crate) fn migrate_preview(conn: &mut Connection) -> anyhow::Result<bool> {
    if super::db::has_column(conn, "components", "preview")? {
        return Ok(false);
    }
    let tx = conn.transaction()?;
    tx.execute("ALTER TABLE components ADD COLUMN preview TEXT NOT NULL DEFAULT '[]'", [])?;
    let mut filled = 0usize;
    {
        let mut owned = tx.prepare(
            "SELECT c.name, t.body FROM components c JOIN tickets t ON t.id = c.owner WHERE c.source IS NOT NULL",
        )?;
        let mut rows = owned.query([])?;
        let mut update = tx.prepare("UPDATE components SET preview = ?1 WHERE name = ?2")?;
        while let Some(r) = rows.next()? {
            let name: String = r.get(0)?;
            let (uses, _) = read(r.get_ref(1)?.as_str()?);
            if let Some(def) = uses.defines.get(&name).filter(|d| !d.preview.is_empty()) {
                update.execute(params![serde_json::to_string(&def.preview)?, name])?;
                filled += 1;
            }
        }
    }
    tx.commit()?;
    tracing::info!("COMP-8: added the components table's preview column — {filled} component(s) with a preview line, durably");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(body: &str) -> Vec<String> {
        read(body).0.references.into_iter().collect()
    }

    #[test]
    fn a_component_name_is_a_custom_elements() {
        for good in ["m-x", "m-wave-summary", "x-1", "a-b.c_d", "my-"] {
            assert!(is_component_name(good), "{good}");
        }
        for bad in ["", "mx", "M-x", "m-X", "1-x", "-x", "m x", "m-ü", "font-face", "annotation-xml"] {
            assert!(!is_component_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_fence_tagged_component_defines_and_its_content_is_the_source() {
        let (uses, problems) = read("intro\n\n```component m-x\n<template><b>x</b></template>\n```\n\nafter\n");
        assert!(problems.is_empty());
        assert_eq!(uses.defines["m-x"].source, "<template><b>x</b></template>\n");
        assert_eq!((uses.defines["m-x"].preview.len(), uses.defines["m-x"].docs.as_ref()), (0, None));
        assert!(uses.references.is_empty(), "a definition's own content is not a reference by the body");
        let (uses, _) = read("~~~component m-y extra words\nsrc\n~~~\n");
        assert_eq!(uses.defines["m-y"].source, "src\n", "a tilde fence");
        assert_eq!(
            uses.defines["m-y"].preview,
            vec![("extra".to_string(), String::new()), ("words".to_string(), String::new())],
            "COMP-8: words after the tag are its preview attributes"
        );
    }

    #[test]
    fn malformed_blocks_are_problems() {
        assert_eq!(read("```component Bad\nx\n```\n").1, vec![Problem::Name("Bad".into())]);
        assert_eq!(read("```component\nx\n```\n").1, vec![Problem::Name(String::new())]);
        assert_eq!(read("```component nohyphen\nx\n```\n").1, vec![Problem::Name("nohyphen".into())]);
        let (uses, problems) = read("```component m-x\none\n```\n\n```component m-x\ntwo\n```\n");
        assert_eq!(problems, vec![Problem::Twice("m-x".into())]);
        assert_eq!(uses.defines["m-x"].source, "one\n", "the first block");
    }

    #[test]
    fn references_are_html_tags_outside_code_and_comments() {
        assert_eq!(refs("a <m-x board=PLX></m-x> b"), vec!["m-x"]);
        assert_eq!(refs("<div>\n<m-x></m-x>\n<M-Y/>\n</div>\n"), vec!["m-x", "m-y"], "an HTML block, folded");
        assert!(refs("`<m-x>` in a code span").is_empty());
        assert!(refs("```html\n<m-x></m-x>\n```\n").is_empty(), "a fence");
        assert!(refs("para\n\n    <m-x></m-x>\n").is_empty(), "an indented code block");
        // COMP-11: an element runs to its close across a blank line — what
        // is indented in it is HTML, as the frame renders it.
        assert_eq!(refs("<section>\n<p>a</p>\n\n    <m-x></m-x>\n</section>\n"), vec!["m-x"]);
        assert!(refs("<!-- <m-x></m-x> -->\n").is_empty(), "a comment");
        assert!(refs("<!--\n<m-x>\n-->\n").is_empty(), "a comment across an HTML block's lines");
        assert!(refs("i<n-1 in prose is no tag").is_empty());
        assert!(refs("<div> <span> no component </span></div>").is_empty(), "no hyphen, no component");
        assert!(refs("---\ntag: <m-x>\n---\nbody").is_empty(), "frontmatter is not rendered");
        assert_eq!(refs("> quoted <m-x></m-x>"), vec!["m-x"]);
    }

    #[test]
    fn a_definition_in_another_languages_example_is_not_one() {
        let example = "````markdown\n```component m-x\n<template></template>\n```\n````\n";
        assert_eq!(read(example), (Uses::default(), vec![]));
        let indented = "Write it so:\n\n    ```component m-x\n    <template></template>\n    ```\n";
        assert_eq!(read(indented), (Uses::default(), vec![]));
    }

    #[test]
    fn a_source_references_every_tag_but_its_own() {
        let src = "<template><m-y></m-y><m-x></m-x></template><!-- <m-z> --><script>this.innerHTML = '<m-w>'</script>";
        assert_eq!(source_uses("m-x", src).into_iter().collect::<Vec<_>>(), vec!["m-w", "m-y"]);
    }

    #[test]
    fn referrers_list_tickets_in_the_stores_order_then_components() {
        let set: BTreeSet<String> = ["MNG-10", "m-a", "MNG-9", "ABC-2", "z-z"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ordered(&set), vec!["ABC-2", "MNG-9", "MNG-10", "m-a", "z-z"]);
    }
}

