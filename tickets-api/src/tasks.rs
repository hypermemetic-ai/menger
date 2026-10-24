//! TICKETS-23: a body's **tasks** — the `<m-task id>` blocks TICKETS-10's
//! `form` decision put in a ticket's body (the owner: a task is a block in
//! the body) — read as the store reads a body's inputs ([`crate::inputs`]):
//! with [`crate::fields::marked`]'s scan, raw HTML only, so an example in a
//! fence, a code span or indented code is never a task, outside comments,
//! the frontmatter split off. Wasm-safe: the app reads a task's source out
//! of the ticket's stored body with it, never out of a frame's DOM.
//!
//! ```html
//! <m-tasks>
//! <m-task id="sheet-lock" status="Ready" name="Lock the page" files="frontend/src/sheet.rs" model="sonnet">
//!
//! Lock the document while a sheet is open — **markdown** between blank lines.
//!
//! </m-task>
//! </m-tasks>
//! ```
//!
//! A task is an `<m-task>` with an `id` (not blank); its **direction** is
//! what it holds, as written, its common indent taken off (so a task
//! written inside an indented block reads as the markdown it is, not as
//! indented code) and its blank edges trimmed. A task inside a task is a
//! task of its own, found by its own id. Of two with one id, the first.

use crate::fields::{marked, Element};

/// The element a task is written with.
const TAGS: &[&str] = &["m-task"];

/// The parameters a task carries, in the order its row shows them.
pub const PARAMS: &[&str] = &["files", "section", "feature", "depends", "model", "verify"];

/// One task of a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Its id, trimmed: unique in its ticket.
    pub id: String,
    /// Its title (`name`), trimmed; `None` when blank or absent.
    pub name: Option<String>,
    /// Its status as written, trimmed (the component's default is Draft).
    pub status: Option<String>,
    /// Its parameters that are set — [`PARAMS`]' order, each trimmed.
    pub params: Vec<(String, String)>,
    /// The ticket it was before it was folded in (`from`).
    pub from: Option<String>,
    /// What it holds, as markdown: its common indent off, blank edges
    /// trimmed.
    pub direction: String,
}

/// Whether `body` may hold a task — it names `<m-task`, in any case.
pub fn mentions(body: &str) -> bool {
    let needle = b"<m-task";
    body.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// Every task `body` declares, in document order (a task inside another
/// after it); of two with one id, the first.
pub fn read(body: &str) -> Vec<Task> {
    let mut out = Vec::new();
    if mentions(body) {
        walk(&marked(body, TAGS), &mut out);
    }
    out
}

/// The task `id` (trimmed, compared exactly) in `body`, if it declares one.
pub fn find(body: &str, id: &str) -> Option<Task> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    read(body).into_iter().find(|t| t.id == id)
}

fn walk(elements: &[Element], out: &mut Vec<Task>) {
    for e in elements {
        if let Some(id) = some(e.attr("id")) {
            if !out.iter().any(|t| t.id == id) {
                out.push(Task {
                    id: id.to_string(),
                    name: some(e.attr("name")).map(str::to_string),
                    status: some(e.attr("status")).map(str::to_string),
                    params: PARAMS.iter().filter_map(|p| some(e.attr(p)).map(|v| (p.to_string(), v.to_string()))).collect(),
                    from: some(e.attr("from")).map(str::to_string),
                    direction: dedent(&e.content),
                });
            }
        }
        walk(&e.parts, out);
    }
}

fn some(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|v| !v.is_empty())
}

/// `src` with the indent every non-blank line shares taken off, and the
/// blank lines at either end dropped.
pub fn dedent(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let first = lines.iter().position(|l| !l.trim().is_empty());
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    let (Some(first), Some(last)) = (first, last) else { return String::new() };
    // The first line may share the open tag's line (`<m-task …>Text`): it
    // carries no indent of its own, so it does not count toward the rest's.
    let counted = |i: usize, l: &str| !(i == 0 && !src.starts_with(['\n', '\r'])) && !l.trim().is_empty();
    let indent = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| counted(*i, l))
        .map(|(_, l)| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    lines[first..=last]
        .iter()
        .map(|l| {
            let lead = l.len() - l.trim_start_matches([' ', '\t']).len();
            &l[lead.min(indent)..]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "---\ntitle: x\n---\nIntro.\n\n<m-tasks label=\"Subtasks\">\n\n<m-task id=\"one\" status=\"Ready\" name=\" The first \" files=\"a.rs, b.rs\" model=\"sonnet\" verify=\"cargo test\">\n\n## Its heading\n\nSome **markdown**, and:\n\n```rust\nfn main() {}\n```\n\n<m-decision id=\"pick\"><m-question>Which?</m-question><m-option name=\"A\"></m-option></m-decision>\n\n</m-task>\n\n<m-task id=\"two\" from=\"OLD-3\">Inline text.</m-task>\n\n</m-tasks>\n";

    #[test]
    fn a_task_is_found_by_its_id_with_its_direction_as_markdown() {
        let t = find(BODY, "one").expect("one");
        assert_eq!(t.name.as_deref(), Some("The first"));
        assert_eq!(t.status.as_deref(), Some("Ready"));
        assert_eq!(t.params, vec![("files".into(), "a.rs, b.rs".into()), ("model".into(), "sonnet".into()), ("verify".into(), "cargo test".into())]);
        assert!(t.direction.starts_with("## Its heading\n\nSome **markdown**"), "{}", t.direction);
        assert!(t.direction.contains("```rust\nfn main() {}\n```"));
        assert!(t.direction.ends_with("</m-decision>"), "{}", t.direction);
        let two = find(BODY, " two ").expect("two");
        assert_eq!((two.direction.as_str(), two.from.as_deref(), two.status), ("Inline text.", Some("OLD-3"), None));
        assert_eq!(read(BODY).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(find(BODY, "three"), None);
        assert_eq!(find(BODY, ""), None);
    }

    /// An example of the markup — in a fence, a code span, a comment — is
    /// never a task: the same scan the inputs are read with.
    #[test]
    fn an_example_is_never_a_task() {
        let body = "```html\n<m-task id=\"x\">no</m-task>\n```\n\n`<m-task id=\"y\">no</m-task>`\n\n<!-- <m-task id=\"z\">no</m-task> -->\n\n~~~markdown\n<m-task id=\"x\">no</m-task>\n~~~\n\n<m-task id=\"x\">yes</m-task>\n";
        assert_eq!(read(body).iter().map(|t| (t.id.as_str(), t.direction.as_str())).collect::<Vec<_>>(), [("x", "yes")]);
        assert!(read("no tasks here").is_empty());
    }

    /// A task written indented reads as its markdown, not as indented code;
    /// a task inside a task is its own; of two with one id, the first.
    #[test]
    fn indent_nesting_and_the_first_of_two() {
        let body = "<div>\n  <m-task id=\"out\">\n\n    A line.\n\n      - nested list\n\n    <m-task id=\"in\">Inner.</m-task>\n\n  </m-task>\n</div>\n\n<m-task id=\"out\">second</m-task>\n";
        let out = find(body, "out").unwrap();
        assert!(out.direction.starts_with("A line.\n\n  - nested list\n\n<m-task id=\"in\">"), "{:?}", out.direction);
        assert_eq!(find(body, "in").unwrap().direction, "Inner.");
        assert_eq!(read(body).len(), 2);
        assert_eq!(dedent("Text on the tag's line\n    more\n"), "Text on the tag's line\nmore");
    }
}
