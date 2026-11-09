//! COMPS-5: **the backfill** — every ```` ```component ```` definition the
//! store holds registered in plexus-ui, once, by the server as it starts.
//! The owner, 2026-11-05: "we need to run a backfill over all existing
//! data". A body carried over from v1 was never saved on v2, so plexus-ui's
//! registry (`ui.components`) holds none of its definitions and every tag
//! using one drew nothing; the UI registers a definition only when its
//! ticket is saved (`ui/src/tag-components.svelte`, `authorChanged`).
//!
//! **A step of the start, not a script**: it runs in [`crate::builder`]
//! before anything is served, so test and prod each get it with the release
//! that carries it, and every later start finds nothing to do.
//!
//! **What it registers**: [`Store::definitions`] — every ticket's body
//! (archived ones too), each deleted ticket's body its tombstone keeps, then
//! v1's own registry left in the file (orphans too); a name at its first.
//! A framed one (`element/1`, v1's custom element; `html/1`) is kept with
//! plexus-ui's [`keep_framed`] — what `ui.author { own: true }` keeps,
//! signed by the server's `--ui-author-key` — in the namespace [`AUTHOR`],
//! written by the ticket that holds it (`by`: its id), so `ui.components`
//! resolves it for every ticket using its tag, drawn by plexus-ui in a frame.
//!
//! **Idempotent, and never over v2's word**: a name the live bundle holds,
//! or one someone authored on v2 (any other namespace), is left as it is; a
//! name [`AUTHOR`] keeps at the same source is unchanged; one it keeps at
//! another source (its body changed with no save in the UI) is kept again.
//! So a second start changes nothing.
//!
//! **Refused, with the reason, each logged**: an rhx definition (it is
//! compiled into the live bundle by its ticket's save, which needs the
//! compiler and a principal holding `ui.author` on `*` — not a start's to
//! do — and SV-6: a server without `--ui-allow-rhx` refuses an rhx save
//! too); a source plexus-ui refuses (its words); and every start says
//! `backfill: N registered, M kept again, K unchanged, R refused` — with no
//! `--ui-author-key`, that it was skipped.

use menger_tickets_api::store::carried::{Definition, PROFILE_RHX};
use menger_tickets_api::store::{Store, System};
use plexus_ui_service::{keep_framed, AuthorKey, FramedSource, UiStore};

/// The namespace the backfill keeps definitions in: v1's, as no principal
/// of v2 wrote them.
pub const AUTHOR: &str = "menger:v1";

/// What one definition came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Kept in [`AUTHOR`]'s namespace; nothing resolved its name before.
    Registered,
    /// Kept again: [`AUTHOR`] held it at another source.
    Updated,
    /// [`AUTHOR`] holds it at this source already.
    Unchanged,
    /// Not kept, and why.
    Refused(String),
}

/// One line of the report: the name, where it was read, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub name: String,
    pub profile: &'static str,
    pub ticket: Option<String>,
    pub found: &'static str,
    pub outcome: Outcome,
}

/// What a backfill did; `skipped` when it could sign nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub lines: Vec<Line>,
    pub skipped: Option<String>,
}

impl Report {
    pub fn count(&self, f: impl Fn(&Outcome) -> bool) -> usize {
        self.lines.iter().filter(|l| f(&l.outcome)).count()
    }
    pub fn said(&self) -> String {
        if let Some(why) = &self.skipped {
            return format!("backfill: skipped — {why}");
        }
        format!(
            "backfill: {} registered, {} kept again, {} unchanged, {} refused",
            self.count(|o| *o == Outcome::Registered),
            self.count(|o| *o == Outcome::Updated),
            self.count(|o| *o == Outcome::Unchanged),
            self.count(|o| matches!(o, Outcome::Refused(_)))
        )
    }
}

/// The backfill, whole: read, decide, keep, log.
pub async fn backfill(store: &Store, sys: &System, ui: &UiStore, key: Option<&AuthorKey>) -> anyhow::Result<Report> {
    let Some(key) = key else {
        let report = Report { lines: Vec::new(), skipped: Some("no --ui-author-key: nothing to sign the definitions with".into()) };
        tracing::warn!("{}", report.said());
        return Ok(report);
    };
    let defs = store.definitions(sys).await.map_err(|e| anyhow::anyhow!("backfill: reading the definitions: {e}"))?;
    let report = Report { lines: defs.iter().map(|d| line(ui, key, d)).collect(), skipped: None };
    for l in &report.lines {
        let at = format!("{} ({}, {} {})", l.name, l.profile, l.found, l.ticket.as_deref().unwrap_or("with no owner"));
        match &l.outcome {
            Outcome::Registered => tracing::info!("backfill: registered {at}"),
            Outcome::Updated => tracing::info!("backfill: kept again {at}: its source moved"),
            Outcome::Unchanged => tracing::debug!("backfill: unchanged {at}"),
            Outcome::Refused(why) => tracing::warn!("backfill: refused {at}: {why}"),
        }
    }
    tracing::info!("{}", report.said());
    Ok(report)
}

fn line(ui: &UiStore, key: &AuthorKey, d: &Definition) -> Line {
    Line { name: d.name.clone(), profile: d.profile, ticket: d.ticket.clone(), found: d.found.as_str(), outcome: decide(ui, key, d) }
}

fn decide(ui: &UiStore, key: &AuthorKey, d: &Definition) -> Outcome {
    if d.profile == PROFILE_RHX {
        return Outcome::Refused("rhx/1 is compiled into the live bundle by its ticket's save (the compiler, ui.author held on `*`, and SV-6: --ui-allow-rhx), never kept by a start".into());
    }
    let now = match ui.resolve_components(Some(std::slice::from_ref(&d.name))) {
        Ok(r) => r.into_iter().next(),
        Err(e) => return Outcome::Refused(format!("the ui store: {e}")),
    };
    let version = plexus_ui_service::store::hash_of(d.source.as_bytes());
    let outcome = match &now {
        Some(r) if r.from == "bundle" => return Outcome::Refused("the live bundle holds a component of this name, drawn natively".into()),
        Some(r) if r.author.as_deref() != Some(AUTHOR) => {
            return Outcome::Refused(format!("authored on v2 by {} ({}): never written over", r.by.as_deref().unwrap_or("?"), r.author.as_deref().unwrap_or("?")))
        }
        Some(r) if r.version == version => return Outcome::Unchanged,
        Some(_) => Outcome::Updated,
        None => Outcome::Registered,
    };
    let by = d.ticket.clone().unwrap_or_else(|| "v1".into());
    let source = FramedSource { name: d.name.clone(), profile: d.profile.into(), source: d.source.clone() };
    match keep_framed(ui, key, AUTHOR, &by, &[source]) {
        Ok(_) => outcome,
        Err(e) => Outcome::Refused(e),
    }
}
