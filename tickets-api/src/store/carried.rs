//! COMPS-5: **the definitions the bodies hold** — every ```` ```component ````
//! fence in the store, for the server's backfill into plexus-ui
//! (`src/backfill.rs`). The owner, 2026-11-05: "we need to run a backfill
//! over all existing data". A body carried over from v1 was never saved on
//! v2, so plexus-ui's registry holds none of its definitions until one is
//! put there for it.
//!
//! A definition is read as the UI reads it (`ui/src/tag-components.svelte`'s
//! `definitions`): a fence (` ``` ` or `~~~`, up to three spaces in) whose
//! info string is `component <name>` — a tag's name, lower case, with a
//! hyphen — then any words (v1's preview attributes, or `html`); its source
//! is the fence's lines, joined, with a newline after. Its profile:
//! `html/1` when the word after the name is `html`; else `element/1` when
//! the source is v1's custom element (a `<template>`, or a script reading
//! `this.root`, `this.menger` or `this.attr`); else `rhx/1`.
//!
//! [`Store::definitions`] reads, in this order, each name kept at its first:
//! every ticket's body (archived ones too), in the order the store made
//! them; then each deleted ticket's body its tombstone still keeps; then v1's
//! own registry, the `components` table v1 left in the file (V2-4: kept,
//! unread) — its owned components and its orphans, a definition v1 still
//! drew while something used it. Read as the system; nothing is written.

use super::*;
use rusqlite::OptionalExtension;

/// Where a definition was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// A ticket's body.
    Body,
    /// A deleted ticket's body, kept by its tombstone.
    Deleted,
    /// v1's registry (`components`), its owner's body gone or never read.
    V1Registry,
}

impl Found {
    pub fn as_str(self) -> &'static str {
        match self {
            Found::Body => "body",
            Found::Deleted => "deleted",
            Found::V1Registry => "v1 registry",
        }
    }
}

/// One definition: its name, profile, source, and the ticket that holds it
/// (v1's registry: its owner, `None` for an orphan).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub profile: &'static str,
    pub source: String,
    pub ticket: Option<String>,
    pub found: Found,
}

/// plexus-ui's profiles, as the UI names them.
pub const PROFILE_RHX: &str = "rhx/1";
pub const PROFILE_HTML: &str = "html/1";
pub const PROFILE_ELEMENT: &str = "element/1";

/// A fence's opening: its marker, and the rest of the line.
fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let lead = line.len() - line.trim_start_matches(' ').len();
    if lead > 3 {
        return None;
    }
    let rest = &line[lead..];
    let ch = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = rest.chars().take_while(|c| *c == ch).count();
    (len >= 3).then(|| (ch, len, &rest[len..]))
}

/// Whether `line` closes a fence of `ch` at least `len` long.
fn fence_close(line: &str, ch: char, len: usize) -> bool {
    let lead = line.len() - line.trim_start_matches(' ').len();
    if lead > 3 {
        return false;
    }
    let rest = &line[lead..];
    let n = rest.chars().take_while(|c| *c == ch).count();
    n >= len && rest[n..].trim().is_empty()
}

/// A tag's name: lower case, a letter first, hyphen-joined runs of letters
/// and digits, at least one hyphen.
fn is_tag(name: &str) -> bool {
    let mut parts = name.split('-');
    let first = parts.next().unwrap_or("");
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first.starts_with(|c: char| c.is_ascii_lowercase()) && ok(first) && name.contains('-') && parts.all(ok)
}

/// A definition's head: its name and whether the word after it is `html`.
fn head(info: &str) -> Option<(String, bool)> {
    let mut words = info.split_whitespace();
    if words.next()? != "component" {
        return None;
    }
    let name = words.next()?;
    is_tag(name).then(|| (name.to_string(), words.next() == Some("html")))
}

/// Whether `source` is v1's custom element.
pub fn is_v1(source: &str) -> bool {
    let template = source.match_indices("<template").any(|(i, m)| source[i + m.len()..].starts_with(|c: char| c == '>' || c.is_whitespace()));
    let reads = ["this.root", "this.menger", "this.attr"].iter().any(|w| {
        source.match_indices(w).any(|(i, m)| {
            let before = source[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '.'));
            let after = source[i + m.len()..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            before && after
        })
    });
    template || reads
}

/// The definitions in `body`, in order: `(name, profile, source)`.
pub fn read(body: &str) -> Vec<(String, &'static str, String)> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((ch, len, info)) = fence_open(lines[i]) else {
            i += 1;
            continue;
        };
        let mut k = i + 1;
        while k < lines.len() && !fence_close(lines[k], ch, len) {
            k += 1;
        }
        if let Some((name, html)) = head(info) {
            let source = format!("{}\n", lines[i + 1..k.min(lines.len())].join("\n"));
            let profile = if html {
                PROFILE_HTML
            } else if is_v1(&source) {
                PROFILE_ELEMENT
            } else {
                PROFILE_RHX
            };
            out.push((name, profile, source));
        }
        i = k + 1;
    }
    out
}

impl Store {
    /// COMPS-5: every definition the store holds, each name at its first
    /// (the module's order). Read as the system, off the runtime.
    pub async fn definitions(&self, _sys: &System) -> Result<Vec<Definition>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || -> Result<Vec<Definition>, StoreError> {
            let conn = db.lock().expect("db lock poisoned");
            let mut out: Vec<Definition> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut take = |out: &mut Vec<Definition>, ticket: Option<String>, found: Found, defs: Vec<(String, &'static str, String)>| {
                for (name, profile, source) in defs {
                    if seen.insert(name.clone()) {
                        out.push(Definition { name, profile, source, ticket: ticket.clone(), found });
                    }
                }
            };
            {
                let mut stmt = conn.prepare("SELECT id, body FROM tickets WHERE instr(body, 'component') > 0 ORDER BY rowid")?;
                let mut rows = stmt.query([])?;
                while let Some(r) = rows.next()? {
                    let (id, body): (String, String) = (r.get(0)?, r.get(1)?);
                    take(&mut out, Some(id), Found::Body, read(&body));
                }
            }
            let has = |table: &str| -> rusqlite::Result<bool> {
                Ok(conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |_| Ok(())).optional()?.is_some())
            };
            if has("tombstones")? {
                let mut stmt = conn.prepare("SELECT id, ticket FROM tombstones WHERE ticket IS NOT NULL ORDER BY at_unix")?;
                let mut rows = stmt.query([])?;
                while let Some(r) = rows.next()? {
                    let (id, kept): (String, String) = (r.get(0)?, r.get(1)?);
                    let body = serde_json::from_str::<serde_json::Value>(&kept).ok().and_then(|t| t["body"].as_str().map(str::to_string)).unwrap_or_default();
                    take(&mut out, Some(id), Found::Deleted, read(&body));
                }
            }
            if has("components")? {
                let mut stmt = conn.prepare("SELECT name, source, owner FROM components WHERE source IS NOT NULL ORDER BY name")?;
                let mut rows = stmt.query([])?;
                while let Some(r) = rows.next()? {
                    let (name, source, owner): (String, String, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?);
                    if !is_tag(&name) {
                        continue;
                    }
                    // v1's registry kept the block's content; the UI's source ends with a newline.
                    let source = if source.ends_with('\n') { source } else { format!("{source}\n") };
                    let profile = if is_v1(&source) { PROFILE_ELEMENT } else { PROFILE_HTML };
                    take(&mut out, owner, Found::V1Registry, vec![(name, profile, source)]);
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| StoreError::Db(format!("read task failed: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILM: &str = "<template><b class=\"t\"></b></template>\n<style>:host{display:block}</style>\n<script>\nthis.root.querySelector('.t').textContent = 'x';\n</script>";
    const HELLO: &str = "<script>\n  prop(\"words\");\n</script>\n<style>.x-hello { color: var(--ink); }</style>\n<p class=\"x-hello\">{words}</p>";

    #[test]
    fn a_fence_is_read_by_its_head_and_its_content() {
        let body = format!(
            "Intro.\n\n```component x-film data-field=film\n{FILM}\n```\n\n```component x-hello\n{HELLO}\n```\n\n~~~component x-card html\n<div id=\"card\"></div>\n~~~\n\n````md\n```component x-inner\n<template></template>\n```\n````\n\n```component Nope\nx\n```\n\n```js\nthis.root\n```\n"
        );
        let got = read(&body);
        let names: Vec<(&str, &str)> = got.iter().map(|(n, p, _)| (n.as_str(), *p)).collect();
        assert_eq!(names, vec![("x-film", PROFILE_ELEMENT), ("x-hello", PROFILE_RHX), ("x-card", PROFILE_HTML)], "{got:?}");
        assert_eq!(got[0].2, format!("{FILM}\n"), "the fence's lines, a newline after: as the UI reads it");
    }

    #[test]
    fn v1_is_a_template_or_a_script_reading_v1s_element() {
        assert!(is_v1("<template>\n<p></p></template>"));
        assert!(is_v1("<p></p><script>this.menger.on('theme', f)</script>"));
        assert!(is_v1("<script>const a = this.attr('x');</script>"));
        assert!(!is_v1(HELLO), "rhx's own script and style say nothing");
        assert!(!is_v1("<templates></templates><script>that.root</script>"));
    }
}
