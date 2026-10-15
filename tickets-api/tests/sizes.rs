//! MENGER-29: the size budgets. Owner, 2026-10-12: *"make sure that the
//! codebase activations aren't getting too huge."*
//!
//! A file holding an `#[plexus_macros::activation]` keeps the activation's
//! `impl` — each method its attribute, the auth check and a match — and
//! nothing else: its logic lives in the store (or a module beside it), its
//! event helpers and per-caller reach beside the file (`activation/`), its
//! tests in a module of their own. The store's `mod.rs` is held the same
//! way: a write's logic goes in its own module under `store/`.
//!
//! Every budget is the file's size when it was set, rounded up a little, so
//! growing past one is a deliberate edit to this file — or, better, a move.

use std::path::{Path, PathBuf};

/// The budgets, in lines, by path from the repository's root. A file with
/// an activation that is not named here takes [`NEW_ACTIVATION`].
const BUDGETS: &[(&str, usize)] = &[
    // MENGER-29: 4,074 lines before events, Sight and the tests moved out.
    ("tickets-api/src/activation.rs", 2300),
    ("tickets-api/src/comments_activation.rs", 450),
    ("tickets-api/src/notes_activation.rs", 330),
    ("tickets-api/src/members_activation.rs", 280),
    ("tickets-api/src/orgs_activation.rs", 240),
    ("tickets-api/src/session_activation.rs", 250),
    ("diagrams/src/activation.rs", 140),
    ("plexus-shell/toy/src/lib.rs", 60),
    // Not an activation, held all the same: the store's write path.
    ("tickets-api/src/store/mod.rs", 5900),
];

/// A new activation's budget: room for its methods, not for their logic.
const NEW_ACTIVATION: usize = 500;

/// What to do about a file over its budget.
fn advice(path: &str) -> &'static str {
    if path.ends_with("store/mod.rs") {
        "move a write's logic into a module of its own beside it (tickets-api/src/store/<topic>.rs, \
         as cascade.rs and history.rs are) and its tests into store/tests_<topic>.rs"
    } else {
        "keep the activation's impl to its methods — each its attribute, the auth check and a match: \
         move a method's logic into the store or a module beside it, event and reach helpers into \
         the activation's own directory (as tickets-api/src/activation/events.rs and sight.rs), and \
         tests into its tests module"
    }
}

/// The repository's root: this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("tickets-api has a parent").to_path_buf()
}

/// Every `.rs` file under `dir`, skipping build output, dependencies and
/// test directories (a test's own throwaway activation is not one of ours).
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !matches!(name.as_ref(), "target" | "node_modules" | "tests" | "spike" | "perf") && !name.starts_with('.') {
                sources(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// Whether `text` declares an activation (the attribute itself, not a
/// mention of it in a comment).
fn declares_activation(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with("#[plexus_macros::activation"))
}

/// The files held to a budget, and each one's budget.
fn held() -> Vec<(String, usize, usize)> {
    let root = root();
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut held = Vec::new();
    for path in files {
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&path).unwrap();
        let named = BUDGETS.iter().find(|(p, _)| *p == rel).map(|(_, b)| *b);
        let budget = match named {
            Some(b) => b,
            None if declares_activation(&text) => NEW_ACTIVATION,
            None => continue,
        };
        held.push((rel, text.lines().count(), budget));
    }
    held.sort();
    held
}

/// The refusals for `files` (path, lines, budget): one line per file over.
fn over(files: &[(String, usize, usize)]) -> Vec<String> {
    files
        .iter()
        .filter(|(_, lines, budget)| lines > budget)
        .map(|(path, lines, budget)| {
            format!("{path} is {lines} lines, over its budget of {budget} (tickets-api/tests/sizes.rs): {}", advice(path))
        })
        .collect()
}

#[test]
fn every_activation_and_the_store_stay_within_their_budgets() {
    let files = held();
    // Every budget names a file that is there: a moved file moves its budget.
    for (path, _) in BUDGETS {
        assert!(files.iter().any(|(p, ..)| p == path), "{path} has a budget but is not there: move its budget with it");
    }
    let refused = over(&files);
    assert!(refused.is_empty(), "\n{}\n", refused.join("\n"));
}

#[test]
fn the_tickets_activation_is_under_2300_lines() {
    let (_, lines, _) = held().into_iter().find(|(p, ..)| p == "tickets-api/src/activation.rs").expect("the tickets activation");
    assert!(lines < 2300, "tickets-api/src/activation.rs is {lines} lines: {}", advice("activation.rs"));
}

#[test]
fn a_file_over_its_budget_is_refused_saying_what_to_move() {
    let files = vec![
        ("tickets-api/src/activation.rs".to_string(), 2301, 2300),
        ("tickets-api/src/store/mod.rs".to_string(), 5900, 5900),
        ("tickets-api/src/store/mod.rs".to_string(), 6000, 5900),
    ];
    let refused = over(&files);
    assert_eq!(refused.len(), 2, "{refused:?}");
    assert!(refused[0].contains("2301 lines, over its budget of 2300") && refused[0].contains("activation/events.rs"));
    assert!(refused[1].contains("store/tests_<topic>.rs"));
}

#[test]
fn an_activation_is_its_attribute_not_a_mention() {
    assert!(declares_activation("#[plexus_macros::activation(namespace = \"x\")]\nimpl X {}"));
    assert!(!declares_activation("//! emitted by `#[plexus_macros::activation(client, …)]`"));
}
