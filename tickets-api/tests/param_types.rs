//! ACCESS-8: no bare string parameter. The owner, 2026-10-12: *"we need to
//! make sure that our activations use strong types for all the parameters.
//! if we don't have the types we construct them. no more raw string passing
//! in."*
//!
//! The line is held by the compiler: every `#[plexus_macros::activation]`
//! in menger is declared with `typed_params` (MACRO-12), and a method
//! parameter typed `String`, `&str`, `Option<String>`, `Option<&str>` or
//! `Vec<String>` is then a compile error on the parameter itself —
//!
//! ```text
//! error: `get.id`: a bare string parameter; give it a type (typed_params)
//! ```
//!
//! This test holds the flag: it reads every file that declares an
//! activation — the workspace, `diagrams/` and the shell's toy included —
//! and fails on an attribute without `typed_params`, naming the file. (It
//! replaces the hand-rolled guard ACCESS-8's body first described, which
//! read the signatures; the macro reads them better.) A parameter's type
//! comes from `menger_tickets_api::vocab`, the vocabulary's enums, or a
//! type made for it there — never the flag taken off.

use std::path::{Path, PathBuf};

/// The repository's root: this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("tickets-api has a parent").to_path_buf()
}

/// Every `.rs` file under `dir`, but build output, dependencies, spikes and
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

/// Each activation attribute in `text` — from `#[plexus_macros::activation`
/// to the line that closes it — and whether it names `typed_params`.
fn attributes(text: &str) -> Vec<(usize, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("#[plexus_macros::activation") {
            continue;
        }
        let mut depth = 0i32;
        let mut typed = false;
        for l in &lines[i..] {
            typed |= l.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == "typed_params");
            depth += l.matches('(').count() as i32 - l.matches(')').count() as i32;
            if depth <= 0 && l.contains(']') {
                break;
            }
        }
        out.push((i + 1, typed));
    }
    out
}

#[test]
fn every_activation_is_declared_with_typed_params() {
    let root = root();
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut seen = 0;
    let mut missing = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        for (line, typed) in attributes(&text) {
            seen += 1;
            if !typed {
                let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                missing.push(format!("{rel}:{line}: an activation without `typed_params` — every parameter takes a type (ACCESS-8, MACRO-12)"));
            }
        }
    }
    // tickets, comments, notes, members, orgs, session, themes, deployments,
    // diagrams' mermaid and the toy's echo.
    assert!(seen >= 10, "found only {seen} activations: the walk lost some");
    assert!(missing.is_empty(), "\n{}\n", missing.join("\n"));
}

#[test]
fn an_attribute_without_the_flag_is_found() {
    let without = "#[plexus_macros::activation(\n    namespace = \"x\",\n    client\n)]\nimpl X {}";
    let with = "#[plexus_macros::activation(namespace = \"x\", typed_params, client)]\nimpl X {}";
    assert_eq!(attributes(without), [(1, false)]);
    assert_eq!(attributes(with), [(1, true)]);
    assert!(attributes("//! `#[plexus_macros::activation(client, …)]`").is_empty());
}
