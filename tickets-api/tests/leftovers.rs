//! V2-9: the teardown's leftovers go. V2-3 took the frontend and V2-4 the
//! other services, but `scripts/` still held the browser checks of v1's
//! pages, `diagrams/` a service whose tests read the app's sources,
//! `spike/` a spike's artifact and `perf/chat-probe.mjs` a probe of an
//! activation that went. Only what serves v2 is in the tree; the rest is at
//! v0.0.1. This file is that check, over the tree (acceptance 1).

use std::path::{Path, PathBuf};

/// The repository's root: this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("tickets-api has a parent").to_path_buf()
}

/// The paths that go, from the repository's root — each a file or a
/// directory. Grouped as the ticket names them.
const GOES: &[&str] = &[
    // Browser checks of the Leptos app's pages and its recordings: the
    // board, the filter card, the jump, a reconnect, the live app in a
    // ticket, the window's scroll, the safe area, the first paint, #/pair,
    // #/components, the ask recorded on the app's screen.
    "scripts/ask-recorded.mjs",
    "scripts/components-check.mjs",
    "scripts/components-check.sh",
    "scripts/desktop-scroll-check.mjs",
    "scripts/desktop-scroll-check.sh",
    "scripts/filter-card-check.mjs",
    "scripts/filter-card-check.sh",
    "scripts/first-paint.mjs",
    "scripts/live-check.mjs",
    "scripts/live-check.sh",
    "scripts/pair-check.mjs",
    "scripts/pair-check.sh",
    "scripts/reconnect-check.mjs",
    "scripts/safe-area-check.mjs",
    // A frame or a mockup: a task's sheet (and its fixture), mockups on a phone.
    "scripts/task-sheet-check.mjs",
    "scripts/task-sheet-check.sh",
    "scripts/fixtures/task-components.md",
    "scripts/mock-preview-check.mjs",
    // A chat: the anchor, the cards, the frames, a reply, the strip, a
    // table, a chip's tap, the composer's inset, a HEIC (and its fixture),
    // the round ↓, the keyboard (and its shim), a message's tap, the
    // keyboard's tap, an image marked up.
    "scripts/chat-anchor-check.mjs",
    "scripts/chat-anchor-check.sh",
    "scripts/chat-card-check.mjs",
    "scripts/chat-frames-check.mjs",
    "scripts/chat-reply-check.mjs",
    "scripts/chat-reply-check.sh",
    "scripts/chat-strip-check.mjs",
    "scripts/chat-strip-scroll-check.mjs",
    "scripts/chat-strip-scroll-check.sh",
    "scripts/chat-table-check.mjs",
    "scripts/chip-tap-check.mjs",
    "scripts/composer-inset-check.mjs",
    "scripts/heic-check.mjs",
    "scripts/fixtures/tiny.heic",
    "scripts/jump-check.mjs",
    "scripts/jump-check.sh",
    "scripts/kb-check.mjs",
    "scripts/kb-shim.js",
    "scripts/message-tap-check.mjs",
    "scripts/tap-keyboard-check.mjs",
    "scripts/markup-check.mjs",
    "scripts/markup-check.sh",
    // A theme: the sweep.
    "scripts/theme-sweep.mjs",
    // A screenshot of the app.
    "scripts/shot.mjs",
    // The deploy page: ask, → test, the stage pills, pinch zoom, → prod.
    "scripts/deploy-ask-check.mjs",
    "scripts/deploy-ask-check.sh",
    "scripts/deploy-button-check.mjs",
    "scripts/deploy-filter-check.mjs",
    "scripts/deploy-touch-check.mjs",
    "scripts/prod-button-check.mjs",
    "scripts/prod-button-check.sh",
    // The chat bridge.
    "scripts/chat-bridge",
    // The landing's media, made by running the app: its capture and its
    // stills, and the landing inside the app's mockup view.
    "scripts/landing-media.sh",
    "scripts/landing-media.mjs",
    "scripts/landing-mock.py",
    // DIST-3's publish: the crate with v1's app built by trunk inside it.
    "scripts/publish.sh",
    // The app's home-screen icons: drawn from web/favicon.svg into
    // frontend/icons/, both gone with v1 (the landing's icon is committed).
    "scripts/icons.sh",
    // ORG-5's one-off move of the tracker's ids: done, and nothing a
    // throwaway holds for it to move.
    "scripts/org-5-move.sh",
    // The diagrams service — with its launcher and unit — and the ACCESS-6 spike.
    "diagrams",
    "deploy/launch/run-diagrams",
    "deploy/systemd/menger-diagrams@.service",
    "spike",
    // The probe of the chats activation, and the magnitude ladder (it
    // drives the app and builds it).
    "perf/chat-probe.mjs",
    "perf/magnitude.mjs",
    // FILTER-15's byte comparison of `find`, `ready_set` and `inputs`: v2
    // has no inputs, and the probe waits on them for ever.
    "perf/reads-identical.mjs",
];

/// Words that name v1's app, its frames and its services: none is left in
/// a script or a probe that stays.
const APP_WORDS: &[&str] = &["frontend/dist", "trunk", "Leptos", "mockup", "chats", "comments", "themes"];

/// Whether `word` occurs in `text` as an identifier of its own (not inside
/// a longer one: `mockups` is not `mockup`), ignoring case.
fn names(text: &str, word: &str) -> bool {
    let (text, word) = (text.to_ascii_lowercase(), word.to_ascii_lowercase());
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(&word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

/// Every file under `dir`, recursively, `node_modules` skipped.
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "node_modules") {
                files(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

#[test]
fn the_leftovers_are_gone() {
    let root = root();
    let left: Vec<&str> = GOES.iter().copied().filter(|p| root.join(p).exists()).collect();
    assert!(left.is_empty(), "V2-9: {} path(s) that go are still here:\n  {}", left.len(), left.join("\n  "));
}

#[test]
fn nothing_left_names_the_app() {
    let root = root();
    let mut checked = Vec::new();
    files(&root.join("scripts"), &mut checked);
    let perf = root.join("perf");
    for entry in std::fs::read_dir(&perf).unwrap_or_else(|e| panic!("{}: {e}", perf.display())) {
        let path = entry.unwrap().path();
        // perf/notes.md records measurements made on v1's app: what it
        // names there is history, kept as written.
        if path.file_name().is_some_and(|n| n == "notes.md") {
            continue;
        }
        if path.is_file() && path.extension().is_some_and(|x| ["mjs", "py", "sh", "md"].iter().any(|e| x == *e)) {
            checked.push(path);
        }
    }
    checked.sort();
    // It cannot pass over nothing: what stays is among what is read.
    for must in ["scripts/ui-build.sh", "perf/pages-probe.mjs"] {
        assert!(checked.contains(&root.join(must)), "V2-9: the word test does not read {must}");
    }
    let mut named = Vec::new();
    for file in &checked {
        // A binary file is skipped: a read that is not UTF-8.
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        for (n, line) in text.lines().enumerate() {
            if APP_WORDS.iter().any(|w| names(line, w)) {
                named.push(format!("{}:{}: {}", file.strip_prefix(&root).unwrap().display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        named.is_empty(),
        "V2-9: {} line(s) in scripts/ and perf/ name v1's app ({APP_WORDS:?}):\n  {}",
        named.len(),
        named.join("\n  ")
    );
}

/// `scripts/publish.sh` named it too; it goes (GOES), so it is not read here.
#[test]
fn nothing_names_diagrams() {
    let root = root();
    let named: Vec<&str> = ["deploy/pipeline", "deploy/tests/pipeline", "Cargo.toml"]
        .into_iter()
        .filter(|p| std::fs::read_to_string(root.join(p)).unwrap_or_else(|e| panic!("{p}: {e}")).contains("diagrams"))
        .collect();
    assert!(named.is_empty(), "V2-9: these still name diagrams: {named:?}");
}

#[test]
fn tether_names_no_chat_bridge_nor_its_models() {
    let text = std::fs::read_to_string(root().join("tether/src/archetypes.rs")).unwrap();
    let named: Vec<&str> = ["chat-bridge", "models.mjs"].into_iter().filter(|w| text.contains(w)).collect();
    assert!(named.is_empty(), "V2-9: tether/src/archetypes.rs still names {named:?}");
}

/// The check checks: the identifier rule, case-insensitive, on what it must
/// and must not see.
#[test]
fn a_word_is_found_only_as_an_identifier_of_its_own_in_any_case() {
    assert!(names("# Needs a built menger, frontend/dist (`trunk build --release`)", "frontend/dist"));
    assert!(names("# Needs a built menger, frontend/dist (`trunk build --release`)", "trunk"));
    assert!(names("the Leptos app", "leptos"));
    assert!(names("the LEPTOS app", "Leptos"));
    assert!(names("menger.chats.post", "chats"));
    assert!(names("```mockup widths=390", "mockup"));
    assert!(!names("mockups on a phone", "mockup"));
    assert!(!names("trunks", "trunk"));
    assert!(!names("chats_activation", "chats"));
    assert!(!names("frontend/distribution", "frontend/dist"));
    assert!(!names("nothing here", "themes"));
}

/// V2-12: `cargo test --workspace` resolves every target of every member
/// before it compiles anything, examples included — so a `path = "…"` in a
/// manifest that names a file outside the tree is a red suite before a test
/// runs (the release `20261029T173657Z-454cd56`: `tickets-api/Cargo.toml`'s
/// example in `spike/`, removed by V2-9). Every `path` value in every
/// manifest of the workspace, resolved beside that manifest, exists.
fn manifest_paths(manifest: &Path) -> Vec<(usize, PathBuf)> {
    let text = std::fs::read_to_string(manifest).unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
    manifest_paths_in(&text, manifest)
}

#[test]
fn every_manifest_path_exists() {
    assert_eq!(manifest_paths_in("x = 1\npath = \"../a/b.rs\"\n# path = \"no\"\n  path=\"c\"\n", Path::new("/m/Cargo.toml")), [(2, PathBuf::from("/m/../a/b.rs")), (4, PathBuf::from("/m/c"))]);
    let root = root();
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let mut manifests = vec![root.join("Cargo.toml")];
    for member in workspace.lines().filter(|l| l.trim_start().starts_with("members")).flat_map(|l| l.split('"').skip(1).step_by(2)) {
        manifests.push(root.join(member).join("Cargo.toml"));
    }
    let mut missing = Vec::new();
    for manifest in &manifests {
        for (line, path) in manifest_paths(manifest) {
            if !path.exists() {
                missing.push(format!("{}:{line}: {}", manifest.strip_prefix(&root).unwrap().display(), path.strip_prefix(&root).unwrap_or(&path).display()));
            }
        }
    }
    assert!(manifests.len() > 3, "the workspace's members were not read: {manifests:?}");
    assert!(missing.is_empty(), "V2-12: {} manifest path(s) that do not exist:\n  {}", missing.len(), missing.join("\n  "));
}

/// The parser on a string, for the self-check above.
fn manifest_paths_in(text: &str, manifest: &Path) -> Vec<(usize, PathBuf)> {
    let dir = manifest.parent().unwrap();
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let line = line.trim_start();
            if line.starts_with('#') {
                return None;
            }
            let rest = line.strip_prefix("path")?.trim_start().strip_prefix('=')?.trim_start();
            let value = rest.strip_prefix('"')?.split('"').next()?;
            Some((i + 1, dir.join(value)))
        })
        .collect()
}
