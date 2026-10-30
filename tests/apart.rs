//! FRAME-1's acceptance 2, held line by line as FRAME-2's waves land: the
//! stored-UI framework, and the shell and the login it is served with, live
//! apart from menger. menger's tree holds no copy of them, it takes each
//! from its sibling checkout (MFE-10's way), no manifest names a directory
//! that left, nothing in it reads a path that moved, and nothing names the
//! crates by their old names.
//!
//! - FRAME-3 (wave 1): the compiler and the engine, out whole to
//!   `../plexus-ui` as `plexus-ui-compiler` and `plexus-ui-engine`. menger
//!   depends on the compiler (its tests compile natively) and on the engine
//!   only through the bundle.
//! - FRAME-8 (wave 2): the bundle is the framework's tool's — `plexus-ui`
//!   (`plexus-ui-cli`), built from the sibling by `scripts/plexus-ui` and by
//!   `deploy/pipeline` — and menger's own build and sign scripts went.
//! - FRAME-4 (wave 1): `plexus-shell` and `plexus-login`, repositories of
//!   their own beside plexus-core (`../plexus-shell`, `../plexus-login`);
//!   `tether/` apart — its `plexus-login` moves with substrate's (FRAME-9).
//! - FRAME-5 (wave 2): the ui service, out to `../plexus-ui` as
//!   `plexus-ui-service`, with no menger type in it; menger registers it
//!   beside `tickets` and mounts its routes on its own shell.
//! - FRAME-7 (wave 3): menger on top — a tenant of the framework. Its own
//!   packages are `menger` and `menger-tickets-api` and nothing else; the
//!   service, the shell, the login and the compiler come from their sibling
//!   checkouts; its shell mounts the framework's routes and has none of its
//!   own; its docs say the framework is `../plexus-ui` and menger a tenant,
//!   and point a new component at the framework's tool and contract.
//!
//! One file since FRAME-5 (it folded FRAME-4's `tests/apart_shell_login.rs`).
//! - FRAME-6 (wave 4): the binary is the framework's — `plexus-ui` (its
//!   `serve` beside `build`, `sign`, `push`) is built from `../plexus-ui`
//!   and links the service; menger's tree builds no binary of that name.
//! - FRAME-11: the dialect is named — rhx (Rhai in markup): `ui/src/*.rhx`,
//!   profile `rhx/1`; no `.svelte` under `ui/` or `tests/`, every component
//!   reached from `routes.json` or `slots.json` and every name there a
//!   source, and README / AGENTS.md / deploy/README.md say Svelte only of
//!   real Svelte (`svelte/1`, FRAME-13) or of what rhx borrowed.
//! - FRAME-13: real Svelte (`svelte/1`) is the framework's tool's — it
//!   compiles a `.svelte` in Node at build; `deploy/pipeline`'s `build_v2` checks
//!   for `node` and for svelte and esbuild in `../plexus-ui/node_modules`
//!   (`npm ci` there) before it builds a UI that holds one, and names
//!   `--node-modules` to the tool.
//! - FRAME-14: the board in both profiles — each of its five components as
//!   `<name>.rhx` and as real Svelte, `<name>-sv.svelte` (one name is one
//!   source); the Svelte board drawn at `#/` (SURFACE-2; `#/sv/…` its alias, the rhx one at `#/rhx/`) in an rhx frame whose
//!   sections hold the Svelte ones, the rhx ones `slots.json`'s defaults.
//! - FRAME-25: the Svelte board rides FRAME-19's bus — no timer of its own
//!   beside a `live`, no sanitizer or markdown of its own (`markdown` from
//!   `plexus-ui/host`), the nav's children and the chips as imported
//!   siblings (`nav-flat-sv`, `nav-children-sv`, `ticket-chip-sv`, each in
//!   both profiles), a sibling reached by its importer; no keyed `{#each`
//!   straight over a store (`$rows.rows`: Svelte compares by value there).
//! - FRAME-30: the look is the components' and the theme's — `ui/index.html`
//!   holds the page's frame alone (rules on `*`, `html`, `body`, `a` and the
//!   form controls, no token defined, no component's rule) and loads
//!   `theme.css` before anything else; `ui/theme.toml` holds the tokens;
//!   every board component, in both profiles, carries its own `<style>`.
//! - FRAME-9: `tether/` takes `plexus-login` from the sibling, as
//!   plexus-substrate (which tether takes) does — one `plexus-login` in its
//!   graph, never menger's tree's.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// menger's root: this package's manifest directory, canonical (this
/// machine's `/home` is `/var/home`).
fn root() -> PathBuf {
    canonical(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// Where a sibling checkout lives: beside menger's root.
fn sibling(dir: &str) -> PathBuf {
    root().parent().expect("menger has a parent").join(dir)
}

/// A path made canonical where it exists, else as given.
fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The directories that left, from menger's root.
const GONE: &[&str] = &[
    // FRAME-3: the compiler and the engine, now ../plexus-ui's.
    "ui-compiler",
    "ui-engine",
    // FRAME-4: repositories of their own.
    "plexus-shell",
    "plexus-login",
    // FRAME-5: the ui service (its ui-sign with it), now ../plexus-ui's.
    "ui-service",
    // FRAME-8: the bundle's build and the pipeline's signer, now ../plexus-ui's
    // `plexus-ui` (build, sign, push); ui/golden/ was the build's fallback when
    // no compiler could be had, and the tool compiles in-process.
    "scripts/ui-build.sh",
    "deploy/ui-sign.mjs",
    "ui/golden",
];

/// The crates menger's cargo graph takes from a sibling checkout: the
/// package, and the sibling it must resolve from (the engine and the tool
/// are not: the tool is built from the sibling, below, and builds the engine).
const SIBLINGS: &[(&str, &str)] = &[
    ("plexus-ui-compiler", "plexus-ui"),
    ("plexus-ui-service", "plexus-ui"),
    ("plexus-shell", "plexus-shell"),
    ("plexus-login", "plexus-login"),
];

/// The packages the graph may no longer hold: the crates' old names, as a
/// manifest writes them. Spelled in two halves so this file does not name
/// them itself.
fn old_packages() -> Vec<String> {
    ["-ui-compiler", "-ui-engine", "-ui-service"].map(|tail| format!("menger{tail}")).to_vec()
}

/// The names no file may carry: the old packages, as a manifest writes
/// them and as Rust does.
fn old_names() -> Vec<String> {
    ["-ui-compiler", "-ui-engine", "-ui-service", "_ui_compiler", "_ui_engine", "_ui_service"].map(|tail| format!("menger{tail}")).to_vec()
}

// ── the directories ────────────────────────────────────────────────────────

/// Every `Cargo.toml` under `dir` but build output, git's, dependencies,
/// the directories that left (the first check names them) and `tether/`
/// (a workspace of its own; its `plexus-login` moves with substrate's).
fn manifests(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let skipped = matches!(name.as_str(), "target" | ".git" | "node_modules") || (dir == root && (name == "tether" || GONE.contains(&name.as_str())));
            if !skipped {
                manifests(root, &path, out);
            }
        } else if name == "Cargo.toml" {
            out.push(path);
        }
    }
}

/// `path` with `.` and `..` taken out by its text alone (the place it
/// names may not exist, so it cannot be canonicalized).
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// Each quoted string on `line`.
fn quoted(line: &str) -> Vec<&str> {
    line.split('"').skip(1).step_by(2).collect()
}

/// Each `members` entry and `path =` value in `text` (a manifest in
/// `dir`), as (line, the place it resolves to) — `#` comments ignored.
fn named_places(dir: &Path, text: &str) -> Vec<(usize, PathBuf)> {
    let mut out = Vec::new();
    let mut in_members = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("");
        let key = line.trim_start();
        let mut values = Vec::new();
        if in_members || key.starts_with("members") {
            let from = if in_members { line } else { line.split_once('=').map(|(_, v)| v).unwrap_or("") };
            values.extend(quoted(from));
            in_members = !from.contains(']');
        }
        for (at, _) in line.match_indices("path") {
            let before = line[..at].chars().last();
            let rest = line[at + 4..].trim_start();
            if before.map_or(true, |c| !(c.is_alphanumeric() || c == '_' || c == '-')) && rest.starts_with('=') {
                if let Some(v) = quoted(rest).first() {
                    values.push(v);
                }
            }
        }
        out.extend(values.into_iter().map(|v| (i + 1, lexical(&dir.join(v)))));
    }
    out
}

/// The places in menger's manifests that name a directory that left,
/// under the root: "file:line: names …".
fn leftovers(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    manifests(root, root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let dir = file.parent().unwrap();
        for (line, place) in named_places(dir, &text) {
            if GONE.iter().any(|d| place.starts_with(root.join(d))) {
                let rel = file.strip_prefix(root).unwrap().display();
                out.push(format!("{rel}:{line}: names {} (under menger's root)", place.display()));
            }
        }
    }
    out
}

#[test]
fn the_moved_crates_are_not_here() {
    let root = root();
    let mut here: Vec<String> = GONE.iter().filter(|p| root.join(p).exists()).map(|p| format!("{p}/ is in the tree")).collect();
    here.extend(leftovers(&root));
    assert!(here.is_empty(), "FRAME-1: {} thing(s) that left are still here:\n  {}", here.len(), here.join("\n  "));
}

#[test]
fn sizes_holds_no_file_of_theirs() {
    let path = root().join("tickets-api/tests/sizes.rs");
    let text = std::fs::read_to_string(&path).expect("tickets-api/tests/sizes.rs");
    let named: Vec<String> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| GONE.iter().any(|dir| l.contains(&format!("\"{dir}/"))))
        .map(|(i, l)| format!("tickets-api/tests/sizes.rs:{}: {}", i + 1, l.trim()))
        .collect();
    assert!(named.is_empty(), "a budget for a file that left with its crate (it goes with the directory):\n{}", named.join("\n"));
}

// ── the graph ──────────────────────────────────────────────────────────────

/// Each package of `cargo metadata --format-version 1`'s answer: its name
/// and its manifest's directory (canonical where it exists). A package
/// with no manifest path is skipped.
fn packages(metadata: &Value) -> Vec<(String, PathBuf)> {
    metadata["packages"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .filter_map(|p| {
                    let name = p["name"].as_str()?;
                    let dir = Path::new(p["manifest_path"].as_str()?).parent()?;
                    Some((name.to_string(), canonical(dir)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `cargo metadata` of menger's workspace, dependencies included (a
/// sibling path dependency is not a member), locked and offline.
fn metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline", "--manifest-path"])
        .arg(root().join("Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    assert!(out.status.success(), "cargo metadata --locked --offline failed:\n{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("cargo metadata answers JSON")
}

/// What the graph `pkgs` breaks of the rule: every crate of `siblings`
/// present, each from under its sibling directory in `parent` (canonical),
/// and no package named in `old`.
fn breaks(pkgs: &[(String, PathBuf)], parent: &Path, siblings: &[(&str, &str)], old: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (want, dir) in siblings {
        let checkout = canonical(&parent.join(dir));
        let found: Vec<&PathBuf> = pkgs.iter().filter(|(n, _)| n == want).map(|(_, d)| d).collect();
        if found.is_empty() {
            out.push(format!("{want}: not in the graph"));
        }
        for d in found {
            if !d.starts_with(&checkout) {
                out.push(format!("{want}: from {}, not from {}", d.display(), checkout.display()));
            }
        }
    }
    for (name, d) in pkgs {
        if old.contains(name) {
            out.push(format!("{name}: an old name, still in the graph ({})", d.display()));
        }
    }
    out
}

/// The line of `script` that builds the tool from the sibling: cargo with
/// `--manifest-path` into `../plexus-ui/` and `-p plexus-ui-cli`.
fn tool_build_line(script: &str) -> Option<(usize, &str)> {
    script
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("--manifest-path") && l.contains("../plexus-ui") && l.contains("-p plexus-ui-cli"))
        .map(|(n, l)| (n + 1, l))
}

#[test]
fn the_framework_is_a_sibling() {
    let root = root();
    let parent = root.parent().expect("menger has a parent").to_path_buf();
    for dir in ["plexus-ui", "plexus-shell", "plexus-login"] {
        assert!(sibling(dir).join("Cargo.toml").is_file(), "FRAME-1: no checkout at {} (clone {dir} beside menger)", sibling(dir).display());
    }
    let broken = breaks(&packages(&metadata()), &parent, SIBLINGS, &old_packages());
    assert!(broken.is_empty(), "FRAME-1: the framework, the shell and the login are not taken from their sibling checkouts:\n  {}", broken.join("\n  "));

    for file in ["scripts/plexus-ui", "deploy/pipeline"] {
        let script = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let cargo_lines: Vec<String> =
            script.lines().enumerate().filter(|(_, l)| l.contains("cargo build")).map(|(n, l)| format!("{file}:{}: {}", n + 1, l.trim())).collect();
        assert!(
            tool_build_line(&script).is_some(),
            "FRAME-8: {file} does not build the tool from ../plexus-ui (`--manifest-path …/../plexus-ui/… -p plexus-ui-cli`); its cargo lines:\n  {}",
            cargo_lines.join("\n  ")
        );
    }
}

/// The lines of `text` that build `ui-sign` (cargo, `--bin ui-sign`): the
/// binary moved with the service, so each must build it from the sibling —
/// `--manifest-path` into `../plexus-ui` — as the engine is built.
fn ui_sign_builds(text: &str) -> Vec<(usize, &str, bool)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.contains("cargo") && l.contains("ui-sign") && l.contains("--bin"))
        .map(|(n, l)| (n + 1, l, l.contains("--manifest-path") && l.contains("plexus-ui/Cargo.toml") && l.contains("plexus-ui-service")))
        .collect()
}

#[test]
fn ui_sign_is_built_from_the_sibling() {
    let root = root();
    let files = readers(&root);
    let mut builders = Vec::new();
    let mut wrong = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        for (n, line, ok) in ui_sign_builds(&text) {
            let at = format!("{}:{n}: {}", file.strip_prefix(&root).unwrap().display(), line.trim());
            if !ok {
                wrong.push(at.clone());
            }
            builders.push(at);
        }
    }
    // FRAME-8: nothing in menger builds it today (the tool's `sign` signs);
    // a line that comes to build it builds it from the sibling.
    let _ = &builders;
    assert!(wrong.is_empty(), "FRAME-5: ui-sign is plexus-ui-service's; these build it otherwise than with --manifest-path into ../plexus-ui:\n  {}", wrong.join("\n  "));
}

// ── the names and the paths ────────────────────────────────────────────────

/// Every file under `dir`, but build output, dependencies, dot-directories
/// and the paths in `skip`.
fn walk(dir: &Path, skip: &[PathBuf], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if skip.contains(&path) || name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            walk(&path, skip, out);
        } else {
            out.push(path);
        }
    }
}

/// What the name check reads: the whole tree, but `deploy/tests/fixtures`,
/// `tether/` (a workspace of its own), the build's `dist/` (output, ignored
/// by git) and this file.
fn whole_tree(root: &Path) -> Vec<PathBuf> {
    let skip = ["deploy/tests/fixtures", "tether", "dist", "tests/apart.rs"].map(|p| root.join(p));
    let mut out = Vec::new();
    walk(root, &skip, &mut out);
    out
}

/// What the path check reads: `tests/`, `scripts/` and `deploy/`, but
/// `deploy/tests/fixtures` and this file.
fn readers(root: &Path) -> Vec<PathBuf> {
    let skip = ["deploy/tests/fixtures", "tests/apart.rs"].map(|p| root.join(p));
    let mut out = Vec::new();
    for dir in ["tests", "scripts", "deploy"] {
        walk(&root.join(dir), &skip, &mut out);
    }
    out
}

/// Each line of each file that `hit` finds something in, as `path:line: what`.
fn lines_where(root: &Path, files: &[PathBuf], hit: impl Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut found = Vec::new();
    for file in files {
        let Ok(bytes) = std::fs::read(file) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            for what in hit(line) {
                found.push(format!("{}:{}: {what}", file.strip_prefix(root).unwrap().display(), n + 1));
            }
        }
    }
    found
}

/// The moved directories `line` names as paths: `ui-compiler/`,
/// `ui-engine/` or `ui-service/` at the start or after a character that is
/// not `-`, `_` or a word character (`(^|[^-\w])ui-(compiler|engine|service)/`).
fn moved_paths(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for dir in ["ui-compiler/", "ui-engine/", "ui-service/"] {
        for (at, _) in line.match_indices(dir) {
            let before = line[..at].chars().next_back();
            if !before.is_some_and(|c| c == '-' || c == '_' || c.is_alphanumeric()) {
                out.push(dir.to_string());
            }
        }
    }
    out
}

#[test]
fn nothing_names_the_old_crates() {
    let root = root();
    let files = whole_tree(&root);
    for want in ["scripts/plexus-ui", "deploy/pipeline", "tests/support/mod.rs", "Cargo.toml", "Cargo.lock", "README.md", "AGENTS.md", "src/builder.rs"] {
        assert!(files.contains(&root.join(want)), "the walk lost {want}");
    }
    let old = old_names();
    let named = lines_where(&root, &files, |line| old.iter().filter(|o| line.contains(o.as_str())).cloned().collect());
    assert!(named.is_empty(), "FRAME-1: {} line(s) still name the crates by their old names:\n  {}", named.len(), named.join("\n  "));
}

#[test]
fn nothing_reads_the_moved_paths() {
    let root = root();
    let files = readers(&root);
    assert!(files.contains(&root.join("tests/support/mod.rs")), "the walk lost tests/");
    assert!(!files.iter().any(|f| f.starts_with(root.join("deploy/tests/fixtures"))), "the walk read deploy/tests/fixtures");
    let read = lines_where(&root, &files, moved_paths);
    assert!(read.is_empty(), "FRAME-1: {} line(s) still read a path that moved to ../plexus-ui:\n  {}", read.len(), read.join("\n  "));
}

// ── menger, a tenant (FRAME-7) ─────────────────────────────────────────────

/// menger's own packages: the workspace's members, and nothing else of the
/// graph under its root.
const OWN: &[&str] = &["menger", "menger-tickets-api"];

/// What the graph `pkgs` (with `members`, the workspace's member names)
/// breaks of FRAME-7's first row: the members are exactly [`OWN`], no
/// package but those lives under `root`, and no package is named
/// `menger-ui-…` wherever it lives.
fn tenant_breaks(pkgs: &[(String, PathBuf)], members: &[String], root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut have: Vec<&str> = members.iter().map(String::as_str).collect();
    have.sort();
    if have != OWN {
        out.push(format!("the workspace's members are {have:?}, not {OWN:?}"));
    }
    for (name, dir) in pkgs {
        if dir.starts_with(root) && !OWN.contains(&name.as_str()) {
            out.push(format!("{name}: a package of menger's own ({})", dir.display()));
        }
        if name.starts_with(&format!("menger{}", "-ui")) {
            out.push(format!("{name}: a ui crate under menger's name ({})", dir.display()));
        }
    }
    out
}

/// The names of the workspace's members in `cargo metadata`'s answer.
fn member_names(metadata: &Value) -> Vec<String> {
    let ids: Vec<&str> = metadata["workspace_members"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    metadata["packages"]
        .as_array()
        .map(|ps| ps.iter().filter(|p| p["id"].as_str().is_some_and(|id| ids.contains(&id))).filter_map(|p| p["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[test]
fn menger_links_the_framework_and_nothing_of_its_own() {
    let root = root();
    let parent = root.parent().expect("menger has a parent").to_path_buf();
    let metadata = metadata();
    let pkgs = packages(&metadata);
    // The framework's service, the shell and the login, and the compiler
    // (a dev-dependency: the ui tests compile natively), each from its
    // sibling; the engine is in no graph of menger's — the tool builds it
    // from its own checkout (`the_framework_is_a_sibling` holds the tool).
    let mut broken = breaks(&pkgs, &parent, SIBLINGS, &old_packages());
    broken.extend(tenant_breaks(&pkgs, &member_names(&metadata), &root));
    assert!(broken.is_empty(), "FRAME-7: menger links the framework and holds nothing of it:\n  {}", broken.join("\n  "));
}

/// The lines of `src` that are code, not comment: (line number, text).
fn code_lines(src: &str) -> Vec<(usize, &str)> {
    src.lines().enumerate().filter(|(_, l)| !l.trim_start().starts_with("//")).map(|(n, l)| (n + 1, l)).collect()
}

#[test]
fn the_shell_mounts_the_frameworks_routes() {
    let src = std::fs::read_to_string(root().join("src/shell.rs")).expect("src/shell.rs");
    let code = code_lines(&src);
    let own: Vec<String> =
        code.iter().filter(|(_, l)| l.contains("fn ui_app") || l.contains("struct UiServe")).map(|(n, l)| format!("src/shell.rs:{n}: {}", l.trim())).collect();
    assert!(own.is_empty(), "FRAME-7: the shell serves the ui with routes of its own (the framework's UiRoutes does):\n  {}", own.join("\n  "));
    let built = code.iter().find(|(_, l)| l.contains("UiRoutes::new("));
    let mounted = code.iter().find(|(_, l)| l.contains("routes.mount("));
    assert!(
        built.is_some() && mounted.is_some(),
        "FRAME-7: src/shell.rs does not mount the framework's routes (a code line with `UiRoutes::new(`: {built:?}; one with `routes.mount(`: {mounted:?})"
    );
}

/// The text of `doc`'s section titled `title` (a `#`-heading of any level,
/// its text exactly `title`): from the line after the heading to the next
/// heading of the same or a higher level. Headings inside fenced code are
/// not headings.
fn section(doc: &str, title: &str) -> Option<String> {
    let mut fenced = false;
    let mut open: Option<usize> = None;
    let mut out = Vec::new();
    for line in doc.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        let heading = !fenced && level > 0 && line[level..].starts_with(' ');
        if heading {
            if let Some(at) = open {
                if level <= at {
                    break;
                }
            } else if line[level..].trim() == title {
                open = Some(level);
                continue;
            }
        }
        if open.is_some() {
            out.push(line);
        }
    }
    open.map(|_| out.join("\n"))
}

/// Where a `golden/` in `line` is named as menger's: not as a test
/// directory of the framework's (`tests/golden/`). `ui/golden/` went with
/// FRAME-8.
fn golden_as_ours(line: &str) -> bool {
    line.match_indices("golden/").any(|(at, _)| !line[..at].ends_with("tests/"))
}

/// What a doc's sections break of FRAME-7's last row: each of `wants` —
/// (file, section, the words it must hold) — present and holding them.
fn doc_breaks(docs: &[(&str, &str)], wants: &[(&str, &str, &[&str])]) -> Vec<String> {
    let mut out = Vec::new();
    for (file, title, words) in wants {
        let Some((_, text)) = docs.iter().find(|(f, _)| f == file) else {
            out.push(format!("{file}: not read"));
            continue;
        };
        match section(text, title) {
            None => out.push(format!("{file}: no section \"{title}\"")),
            Some(body) => {
                for w in words.iter().filter(|w| !body.contains(*w)) {
                    out.push(format!("{file} \"{title}\": does not say `{w}`"));
                }
            }
        }
    }
    out
}

/// The bullet of `doc` that begins with `start` (a line), joined into one
/// line: to the next line that begins a bullet or a heading, or a blank one.
fn bullet(doc: &str, start: &str) -> Option<String> {
    let mut lines = doc.lines().skip_while(|l| !l.starts_with(start));
    let first = lines.next()?;
    let rest = lines.take_while(|l| !(l.trim().is_empty() || l.starts_with("- ") || l.starts_with('#')));
    Some(std::iter::once(first).chain(rest).map(str::trim).collect::<Vec<_>>().join(" "))
}

/// The sentences of `text` (split at `. ` and `;`) that take something by
/// path and name `plexus-ui-engine` among it.
fn engine_by_path(text: &str) -> Vec<String> {
    text.split(". ").flat_map(|s| s.split(';')).filter(|s| s.contains("by path") && s.contains("plexus-ui-engine")).map(|s| s.trim().to_string()).collect()
}

#[test]
fn the_docs_say_menger_is_a_tenant() {
    let root = root();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    let (readme, agents) = (read("README.md"), read("AGENTS.md"));
    let docs = [("README.md", readme.as_str()), ("AGENTS.md", agents.as_str())];
    let mut broken = doc_breaks(
        &docs,
        &[
            // README's "The services" is its "What v2 is".
            ("README.md", "The services", &["../plexus-ui", "tenant"]),
            ("AGENTS.md", "What v2 is", &["../plexus-ui", "tenant"]),
            // The framework's tool and its contract, kept with its code.
            ("AGENTS.md", "Adding a component", &["plexus-ui build", "plexus-ui push", "../plexus-ui/CONTRACT.md"]),
        ],
    );
    // No path that left, named as menger's — the same walk as
    // `nothing_reads_the_moved_paths`, over the docs, and `ui/golden/`.
    let files = ["README.md", "AGENTS.md", "deploy/README.md"].map(|f| root.join(f));
    broken.extend(lines_where(&root, &files, moved_paths));
    broken.extend(lines_where(&root, &files, |l| if golden_as_ours(l) { vec!["golden/ (ui/golden went, FRAME-8)".into()] } else { vec![] }));
    // The members bullet: the engine built by the tool, never taken by path.
    match bullet(&agents, "- **Workspace members**") {
        None => broken.push("AGENTS.md: no \"Workspace members\" bullet".into()),
        Some(b) => broken.extend(engine_by_path(&b).into_iter().map(|s| format!("AGENTS.md \"Workspace members\": takes the engine by path: {s}"))),
    }
    assert!(broken.is_empty(), "FRAME-7: the docs do not yet say menger is the framework's tenant:\n  {}", broken.join("\n  "));
}

/// The names of the files directly in `dir` (none when it is missing).
fn names_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir).map(|e| e.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    out.sort();
    out
}

#[test]
fn no_script_of_mengers_duplicates_the_frameworks() {
    let root = root();
    let theirs = names_in(&sibling("plexus-ui").join("scripts"));
    assert!(theirs.iter().any(|n| n == "ui-compiler-wasm.sh"), "FRAME-7: ../plexus-ui/scripts holds no ui-compiler-wasm.sh: {theirs:?}");
    let twins: Vec<String> = names_in(&root.join("scripts")).into_iter().filter(|n| theirs.contains(n)).map(|n| format!("scripts/{n} (../plexus-ui/scripts/{n} is the framework's)")).collect();
    assert!(twins.is_empty(), "FRAME-7: menger keeps a copy of the framework's scripts:\n  {}", twins.join("\n  "));
    let pipeline = std::fs::read_to_string(root.join("deploy/pipeline")).expect("deploy/pipeline");
    let calls: Vec<(usize, &str)> = code_lines(&pipeline).into_iter().filter(|(_, l)| !l.trim_start().starts_with('#') && l.contains("ui-compiler-wasm.sh")).collect();
    assert!(
        !calls.is_empty() && calls.iter().all(|(_, l)| l.contains("../plexus-ui/scripts/ui-compiler-wasm.sh")),
        "FRAME-7: deploy/pipeline does not build the compiler module with ../plexus-ui/scripts/ui-compiler-wasm.sh; its lines naming the script: {calls:?}"
    );
}

#[test]
fn the_engines_test_lives_with_the_engine() {
    let root = root();
    assert!(sibling("plexus-ui").join("tests/engine_browser.mjs").is_file(), "FRAME-7: ../plexus-ui/tests/engine_browser.mjs is missing — the engine's own smoke");
    assert!(!root.join("tests/engine_browser.mjs").exists(), "FRAME-7: tests/engine_browser.mjs is the engine's test in menger; it lives with the engine (../plexus-ui's npm test)");
}

// ── the checks check ──────────────────────────────────────────────────────

/// The parsers and the rules, on what they must and must not see.
#[test]
fn the_checks_read_what_they_are_given() {
    // The metadata reader: name and manifest directory; no manifest, skipped.
    let parent = Path::new("/w");
    let [old_compiler, old_engine, old_service] = [0, 1, 2].map(|i| old_packages()[i].clone());
    let doc = serde_json::json!({ "packages": [
        { "name": "menger", "manifest_path": "/w/menger/Cargo.toml" },
        { "name": "plexus-ui-compiler", "manifest_path": "/w/plexus-ui/compiler/Cargo.toml" },
        { "name": "plexus-ui-service", "manifest_path": "/w/plexus-ui/service/Cargo.toml" },
        { "name": "plexus-shell", "manifest_path": "/w/plexus-shell/Cargo.toml" },
        { "name": "plexus-login", "manifest_path": "/w/plexus-login/Cargo.toml" },
        { "name": old_service, "manifest_path": "/w/menger/ui-service/Cargo.toml" },
        { "name": old_engine, "manifest_path": "/w/menger/ui-engine/Cargo.toml" },
        { "name": "nameless" },
    ]});
    let pkgs = packages(&doc);
    assert_eq!(pkgs.len(), 7, "a package with no manifest path is skipped: {pkgs:?}");
    assert_eq!(pkgs[1], ("plexus-ui-compiler".to_string(), PathBuf::from("/w/plexus-ui/compiler")));

    // Every sibling present and where it should be; an old name named.
    assert!(breaks(&pkgs[..5], parent, SIBLINGS, &old_packages()).is_empty(), "the graph after FRAME-5");
    assert_eq!(
        breaks(&pkgs, parent, SIBLINGS, &old_packages()),
        [
            format!("{old_service}: an old name, still in the graph (/w/menger/ui-service)"),
            format!("{old_engine}: an old name, still in the graph (/w/menger/ui-engine)"),
        ]
    );
    assert!(!old_packages().contains(&"plexus-ui-service".to_string()) && old_packages().contains(&old_compiler));

    // Missing is named; a near sibling, a copy in menger and the registry's are not the sibling.
    assert_eq!(breaks(&pkgs[..1], parent, &SIBLINGS[1..2], &[]), ["plexus-ui-service: not in the graph"]);
    for elsewhere in ["/w/plexus-ui-old/service", "/w/menger/service", "/w/menger/plexus-shell", "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/plexus-ui-service-0.1.0"] {
        let one = [("plexus-ui-service".to_string(), PathBuf::from(elsewhere)), ("plexus-shell".to_string(), PathBuf::from(elsewhere))];
        assert_eq!(breaks(&one, parent, &[SIBLINGS[1], SIBLINGS[2]], &[]).len(), 2, "{elsewhere} taken for a sibling");
    }

    // The manifest reader: members over lines, `path =` values, comments ignored.
    let text = "[workspace]\n# members = [\"commented\"]\nmembers = [\".\", \"a\",\n  \"ui-service\"]\n\
                [dependencies]\nx = { path = \"plexus-login\", version = \"1\" }\n\
                y = { path = \"../plexus-ui/service\" }\nz = { git = \"g\", subpath = \"q\" }\n";
    let got = named_places(Path::new("/m"), text);
    let want: Vec<(usize, PathBuf)> = vec![(3, "/m".into()), (3, "/m/a".into()), (4, "/m/ui-service".into()), (6, "/m/plexus-login".into()), (7, "/plexus-ui/service".into())];
    assert_eq!(got, want);
    // And on the real tree it reads menger's own members.
    let root = root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(named_places(&root, &text).iter().any(|(_, p)| *p == root.join("tickets-api")));
    // And the real graph: menger itself is where it is.
    assert_eq!(packages(&metadata()).iter().filter(|(n, _)| n == "menger").map(|(_, d)| d.clone()).collect::<Vec<_>>(), [root]);

    // The tool's build line.
    let script = "x\n  cargo build --quiet --manifest-path \"$root/../plexus-ui/Cargo.toml\" -p plexus-ui-cli --target-dir \"$target\"\n";
    assert_eq!(tool_build_line(script).map(|(n, _)| n), Some(2));
    assert!(tool_build_line("cargo build -p plexus-ui-cli\n").is_none());
    assert!(tool_build_line("cargo build --manifest-path ../plexus-ui/Cargo.toml -p plexus-ui-compiler\n").is_none());

    // The builders of ui-sign.
    let mjs = "  run(\"cargo\", [\"build\", \"--manifest-path\", path.join(REPO, \"../plexus-ui/Cargo.toml\"), \"-p\", \"plexus-ui-service\", \"--bin\", \"ui-sign\"]);\nx\n  run(\"cargo\", [\"build\", \"-p\", \"OLD\", \"--bin\", \"ui-sign\"]);\nnode deploy/ui-sign.mjs\n";
    let found: Vec<(usize, bool)> = ui_sign_builds(mjs).into_iter().map(|(n, _, ok)| (n, ok)).collect();
    assert_eq!(found, [(1, true), (3, false)]);

    // The moved paths: as a path, never inside a longer name.
    assert_eq!(moved_paths("cat ui-compiler/tests/golden/x.rhx"), ["ui-compiler/"]);
    assert_eq!(moved_paths("\"$root/ui-engine/Cargo.toml\""), ["ui-engine/"]);
    assert_eq!(moved_paths("| `ui` | the UI's versions | `ui-service/` |"), ["ui-service/"]);
    assert_eq!(moved_paths("#   engine.js  (ui-engine/, abi engine/1)"), ["ui-engine/"]);
    assert!(moved_paths("../plexus-ui/compiler/ and dist/ui-compiler.wasm and ui-service.db").is_empty());
    assert!(moved_paths("plexus-ui-compiler/ build-ui-compiler/ my_ui-engine/ plexus-ui-service/").is_empty());

    // FRAME-7: the tenant's graph — members exactly menger's two, nothing
    // else under its root, no ui crate under menger's name anywhere.
    let m = Path::new("/w/menger");
    let own = |n: &str| (n.to_string(), PathBuf::from(format!("/w/menger/{}", if n == "menger" { "" } else { "tickets-api" })));
    let members = [String::from("menger-tickets-api"), String::from("menger")];
    assert!(tenant_breaks(&[own("menger"), own("menger-tickets-api"), pkgs[2].clone()], &members, m).is_empty());
    let extra = [own("menger"), ("menger-ui-kit".to_string(), PathBuf::from("/w/elsewhere/kit")), ("plexus-ui-service".to_string(), PathBuf::from("/w/menger/vendor/s"))];
    assert_eq!(
        tenant_breaks(&extra, &members[..1], m),
        [
            "the workspace's members are [\"menger-tickets-api\"], not [\"menger\", \"menger-tickets-api\"]".to_string(),
            "menger-ui-kit: a ui crate under menger's name (/w/elsewhere/kit)".to_string(),
            "plexus-ui-service: a package of menger's own (/w/menger/vendor/s)".to_string(),
        ]
    );
    let meta = serde_json::json!({ "workspace_members": ["a 1", "b 1"], "packages": [
        { "id": "a 1", "name": "menger" }, { "id": "b 1", "name": "menger-tickets-api" }, { "id": "c 1", "name": "plexus-shell" } ]});
    assert_eq!(member_names(&meta), ["menger", "menger-tickets-api"]);

    // The shell's code lines: a comment naming the routes is not a mount.
    let shell = "    // UiRoutes::new(store, port) and routes.mount(app) went\nfn ui_app() {}\n  let r = UiRoutes::new(s, p);\n  app = routes.mount(app);\n";
    assert_eq!(code_lines(shell).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [2, 3, 4]);

    // The sections: by exact title, to the next heading of its level or
    // higher, a deeper one kept, a `#` inside a fence no heading.
    let doc = "# T\nintro\n## What v2 is\nA ../plexus-ui tenant.\n```sh\n# not a heading\n```\n### deeper\nkept\n## Next\nnot\n";
    assert_eq!(section(doc, "What v2 is").as_deref(), Some("A ../plexus-ui tenant.\n```sh\n# not a heading\n```\n### deeper\nkept"));
    assert_eq!(section(doc, "What v2"), None);
    assert_eq!(section("#What v2 is\nx\n", "What v2 is"), None);
    let docs = [("R", doc)];
    assert!(doc_breaks(&docs, &[("R", "What v2 is", &["../plexus-ui", "tenant"])]).is_empty());
    assert_eq!(
        doc_breaks(&docs, &[("R", "Next", &["tenant"]), ("R", "Gone", &[]), ("A", "What v2 is", &[])]),
        ["R \"Next\": does not say `tenant`", "R: no section \"Gone\"", "A: not read"]
    );

    // The bullet and the engine taken by path.
    let agents = "x\n- **Workspace members** (`Cargo.toml`): `.`;\n  each taken by path: `plexus-ui-compiler`,\n  `plexus-ui-engine`. The engine built by the tool.\n- next\n";
    let b = bullet(agents, "- **Workspace members**").unwrap();
    assert_eq!(b, "- **Workspace members** (`Cargo.toml`): `.`; each taken by path: `plexus-ui-compiler`, `plexus-ui-engine`. The engine built by the tool.");
    assert_eq!(engine_by_path(&b), ["each taken by path: `plexus-ui-compiler`, `plexus-ui-engine`"]);
    assert!(engine_by_path("taken by path: `plexus-ui-compiler`. `plexus-ui-engine` is built by the tool").is_empty());
    assert_eq!(bullet("a\n", "- **Workspace members**"), None);

    // golden/: menger's when not a framework's tests/golden/.
    assert!(golden_as_ours("`src/*.rhx`, `golden/` — compiled by"));
    assert!(golden_as_ours("ui/golden/ and compiler/tests/golden/"));
    assert!(!golden_as_ours("`../plexus-ui`'s (`compiler/tests/golden/`)"));
}

// ── tether (FRAME-9) ───────────────────────────────────────────────────────

/// The `path` of dependency `dep` in `text` (a manifest in `dir`): each line
/// whose key is `dep` (`dep = { path = … }`), as (line, the place it names).
fn dep_paths(dir: &Path, text: &str, dep: &str) -> Vec<(usize, PathBuf)> {
    let lines: Vec<&str> = text.lines().collect();
    named_places(dir, text)
        .into_iter()
        .filter(|(n, _)| {
            let key = lines[n - 1].trim_start();
            key.strip_prefix(dep).is_some_and(|rest| rest.trim_start().starts_with('='))
        })
        .collect()
}

/// `cargo metadata` of tether's workspace (its own, beside menger's),
/// locked and offline: the packages, or what cargo said.
fn tether_metadata(root: &Path) -> Result<Value, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline", "--manifest-path"])
        .arg(root.join("tether/Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join(" / "));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata answered no JSON: {e}"))
}

/// What tether breaks of FRAME-9's rule: its manifest's `plexus-login`
/// path is the sibling `login` (canonical), and its graph `pkgs` holds
/// exactly one `plexus-login`, from there.
fn tether_breaks(paths: &[(usize, PathBuf)], pkgs: Result<&[(String, PathBuf)], &str>, login: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if paths.is_empty() {
        out.push("tether/Cargo.toml: no `plexus-login = { path = … }`".to_string());
    }
    for (n, place) in paths {
        if canonical(place) != login {
            out.push(format!("tether/Cargo.toml:{n}: plexus-login from {}, not the sibling {}", place.display(), login.display()));
        }
    }
    match pkgs {
        Err(e) => out.push(format!("cd tether && cargo metadata --locked --offline failed: {e}")),
        Ok(pkgs) => {
            let found: Vec<&PathBuf> = pkgs.iter().filter(|(n, _)| n == "plexus-login").map(|(_, d)| d).collect();
            if found.len() != 1 {
                out.push(format!("tether's graph holds {} plexus-login(s), not one: {found:?}", found.len()));
            }
            for d in found {
                if d != login {
                    out.push(format!("tether's graph takes plexus-login from {}, not the sibling {}", d.display(), login.display()));
                }
            }
        }
    }
    out
}

#[test]
fn tether_takes_the_sibling_login() {
    let root = root();
    let login = canonical(&sibling("plexus-login"));
    assert!(login.join("Cargo.toml").is_file(), "FRAME-9: no checkout at {} (clone plexus-login beside menger)", login.display());
    let text = std::fs::read_to_string(root.join("tether/Cargo.toml")).expect("tether/Cargo.toml");
    let paths = dep_paths(&root.join("tether"), &text, "plexus-login");
    let meta = tether_metadata(&root);
    let pkgs = meta.as_ref().map(packages);
    let broken = tether_breaks(&paths, pkgs.as_deref().map_err(|e| e.as_str()), &login);
    assert!(broken.is_empty(), "FRAME-9: tether does not take the sibling plexus-login, alone:\n  {}", broken.join("\n  "));
}

/// tether's check on what it must and must not see.
#[test]
fn tether_check_reads_what_it_is_given() {
    let text = "# plexus-login = { path = \"../commented\" }\nplexus-login-extra = { path = \"../x\" }\n\
                plexus-login = { path = \"../../plexus-login\", version = \"0.2\" }\nother = { path = \"../plexus-login\" }\n";
    let paths = dep_paths(Path::new("/w/menger/tether"), text, "plexus-login");
    assert_eq!(paths, [(3, PathBuf::from("/w/plexus-login"))]);

    let login = Path::new("/w/plexus-login");
    let one = [("plexus-login".to_string(), login.to_path_buf()), ("menger-tether".to_string(), "/w/menger/tether".into())];
    assert!(tether_breaks(&paths, Ok(&one), login).is_empty(), "the sibling, once");
    // menger's tree's copy, in the manifest and in the graph; two copies; none; no path; cargo refusing.
    let old = [(47, PathBuf::from("/w/menger/plexus-login"))];
    let menger = [("plexus-login".to_string(), PathBuf::from("/w/menger/plexus-login"))];
    assert_eq!(tether_breaks(&old, Ok(&menger), login).len(), 2);
    let two = [one[0].clone(), menger[0].clone()];
    assert_eq!(tether_breaks(&paths, Ok(&two), login).len(), 2, "two copies: counted and the stray named");
    assert_eq!(tether_breaks(&paths, Ok(&one[1..]), login).len(), 1, "none in the graph");
    assert_eq!(tether_breaks(&[], Ok(&one), login).len(), 1, "no path in the manifest");
    let registry = [("plexus-login".to_string(), PathBuf::from("/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/plexus-login-0.2.2"))];
    assert_eq!(tether_breaks(&paths, Ok(&registry), login).len(), 1, "the registry's is not the sibling");
    assert_eq!(tether_breaks(&paths, Err("no such file"), login).len(), 1);
}

// ── FRAME-6: the binary ────────────────────────────────────────────────────

/// `cargo metadata` of the sibling `../plexus-ui`'s workspace, its members
/// alone (each with the dependencies its manifest declares), offline.
fn framework_metadata() -> Result<Value, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline", "--manifest-path"])
        .arg(sibling("plexus-ui").join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata of ../plexus-ui failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata answers JSON: {e}"))
}

/// The packages of `metadata` with a bin target named `bin`: each one's
/// name, manifest directory and the dependencies it declares.
fn with_bin(metadata: &Value, bin: &str) -> Vec<(String, PathBuf, Vec<String>)> {
    metadata["packages"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .filter(|p| p["targets"].as_array().is_some_and(|ts| ts.iter().any(|t| t["name"] == bin && t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "bin")))))
                .filter_map(|p| {
                    let dir = canonical(Path::new(p["manifest_path"].as_str()?).parent()?);
                    let deps = p["dependencies"].as_array().map(|ds| ds.iter().filter(|d| d["kind"].is_null()).filter_map(|d| d["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
                    Some((p["name"].as_str()?.to_string(), dir, deps))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What breaks FRAME-6's row: menger's graph (`ours`) builds a binary named
/// `plexus-ui` from under `root`; the framework's (`theirs`) builds none
/// from under `framework`, more than one, or one whose package does not
/// link `plexus-ui-service` (the tool alone, no server).
fn binary_breaks(ours: &Value, theirs: Result<&Value, &str>, root: &Path, framework: &Path) -> Vec<String> {
    let mut out: Vec<String> = with_bin(ours, "plexus-ui")
        .into_iter()
        .filter(|(_, d, _)| d.starts_with(root))
        .map(|(n, d, _)| format!("menger's tree builds a binary named plexus-ui: {n} ({})", d.display()))
        .collect();
    match theirs {
        Err(e) => out.push(e.to_string()),
        Ok(m) => {
            let bins: Vec<_> = with_bin(m, "plexus-ui").into_iter().filter(|(_, d, _)| d.starts_with(framework)).collect();
            match bins.as_slice() {
                [] => out.push(format!("{} builds no binary named plexus-ui", framework.display())),
                [(name, _, deps)] if !deps.iter().any(|d| d == "plexus-ui-service") => {
                    out.push(format!("{name}'s plexus-ui does not link plexus-ui-service: the tool, not the server (`plexus-ui serve`)"))
                }
                [_] => {}
                many => out.push(format!("{} binaries named plexus-ui: {:?}", many.len(), many.iter().map(|(n, _, _)| n).collect::<Vec<_>>())),
            }
        }
    }
    out
}

#[test]
fn the_binary_is_the_frameworks() {
    // The check first, on graphs it is handed.
    let pkg = |name: &str, dir: &str, bin: &str, deps: &[&str]| {
        serde_json::json!({ "name": name, "manifest_path": format!("{dir}/Cargo.toml"),
            "targets": [{ "name": bin, "kind": ["bin"] }],
            "dependencies": deps.iter().map(|d| serde_json::json!({ "name": d, "kind": null })).collect::<Vec<_>>() })
    };
    let graph = |ps: Vec<Value>| serde_json::json!({ "packages": ps });
    let (r, f) = (Path::new("/w/menger"), Path::new("/w/plexus-ui"));
    let server = graph(vec![pkg("plexus-ui-cli", "/w/plexus-ui/cli", "plexus-ui", &["plexus-ui-service", "clap"])]);
    let mine = graph(vec![pkg("menger", "/w/menger", "menger", &[])]);
    assert!(binary_breaks(&mine, Ok(&server), r, f).is_empty(), "the framework's, serving");
    let tool = graph(vec![pkg("plexus-ui-cli", "/w/plexus-ui/cli", "plexus-ui", &["clap"])]);
    assert_eq!(binary_breaks(&mine, Ok(&tool), r, f).len(), 1, "the tool alone");
    let dev_only = graph(vec![serde_json::json!({ "name": "plexus-ui-cli", "manifest_path": "/w/plexus-ui/cli/Cargo.toml",
        "targets": [{ "name": "plexus-ui", "kind": ["bin"] }], "dependencies": [{ "name": "plexus-ui-service", "kind": "dev" }] })]);
    assert_eq!(binary_breaks(&mine, Ok(&dev_only), r, f).len(), 1, "a dev-dependency serves nothing");
    let ours = graph(vec![pkg("menger", "/w/menger", "plexus-ui", &["plexus-ui-service"])]);
    assert_eq!(binary_breaks(&ours, Ok(&server), r, f).len(), 1, "menger building one");
    assert_eq!(binary_breaks(&mine, Ok(&graph(vec![])), r, f).len(), 1, "none built");
    let lib_only = graph(vec![serde_json::json!({ "name": "x", "manifest_path": "/w/plexus-ui/x/Cargo.toml", "targets": [{ "name": "plexus-ui", "kind": ["lib"] }], "dependencies": [] })]);
    assert_eq!(binary_breaks(&mine, Ok(&lib_only), r, f).len(), 1, "a library of that name is no binary");
    assert_eq!(binary_breaks(&mine, Err("no checkout"), r, f).len(), 1);

    // Then the real graphs.
    let theirs = framework_metadata();
    let broken = binary_breaks(&metadata(), theirs.as_ref().map_err(String::as_str), &root(), &canonical(&sibling("plexus-ui")));
    assert!(broken.is_empty(), "FRAME-6: the plexus-ui binary is the framework's, and serves:\n  {}", broken.join("\n  "));
}

/// The lines of `script` that run the tool against a service — `"$tool"`
/// or `"$UI_TOOL"` with `push`, `versions`, `preview` or `rollback`, or
/// `build` with `--contract-from` — and whether each names `--service`.
fn tool_calls(script: &str) -> Vec<(usize, &str, bool)> {
    script
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("\"$tool\"") || l.contains("\"$UI_TOOL\""))
        .filter(|(_, l)| [" push ", " versions ", " preview ", " rollback "].iter().any(|c| l.contains(c)) || (l.contains(" build ") && l.contains("--contract-from")))
        .map(|(n, l)| (n + 1, l.trim(), l.contains("--service ")))
        .collect()
}

/// FRAME-6: the tool assumes no service (its `--service` has no default and
/// `--contract-method` is gone), so every call of menger's to it names
/// `menger` itself, and nothing of menger's names the old flag.
#[test]
fn menger_names_its_service_to_the_tool() {
    assert_eq!(tool_calls("  out=$(\"$tool\" push --url \"$ws\" --service menger x)\n  \"$tool\" versions --url u\n  \"$UI_TOOL\" build ui --contract H\n"), [(1, "out=$(\"$tool\" push --url \"$ws\" --service menger x)", true), (2, "\"$tool\" versions --url u", false)]);

    let pipeline = std::fs::read_to_string(root().join("deploy/pipeline")).expect("deploy/pipeline");
    let calls = tool_calls(&pipeline);
    assert!(calls.iter().any(|(_, l, _)| l.contains(" push ")), "deploy/pipeline pushes with the tool");
    let bare: Vec<String> = calls.iter().filter(|(_, _, named)| !named).map(|(n, l, _)| format!("deploy/pipeline:{n}: {l}")).collect();
    assert!(bare.is_empty(), "FRAME-6: the tool assumes no service; these name none:\n  {}", bare.join("\n  "));

    let old = lines_where(&root(), &readers(&root()), |l| if l.contains("--contract-method") { vec![l.trim().to_string()] } else { vec![] });
    assert!(old.is_empty(), "FRAME-6: --contract-method is gone from the tool:\n  {}", old.join("\n  "));
}

/// The `.svelte` files under `dirs` of `root`, relative, sorted.
fn svelte_files(root: &Path, dirs: &[&str]) -> Vec<String> {
    let mut files = Vec::new();
    for d in dirs {
        walk(&root.join(d), &[], &mut files);
    }
    let mut out: Vec<String> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "svelte")).map(|p| p.strip_prefix(root).unwrap_or(p).display().to_string()).collect();
    out.sort();
    out
}

/// Whether `text` is a real Svelte component as `svelte/1` takes one: its
/// module script declares the methods it uses (`<script module> export
/// const uses = [...]`), which the tool requires.
fn real_svelte(text: &str) -> bool {
    text.contains("<script module>") && text.contains("export const uses")
}

/// Whether `src` draws the component `name` as a tag — `<name` followed by
/// a space, `/` or `>`, so `<b-rows-sv` is not `<b-rows`.
fn draws_tag(src: &str, name: &str) -> bool {
    let open = format!("<{name}");
    src.match_indices(&open).any(|(at, _)| src[at + open.len()..].starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>'))
}

/// The siblings a svelte/1 source imports (FRAME-19: `import X from
/// "./<name>.svelte"`, `src/<name>.svelte` beside it) and the components it
/// places by their bound marks (FRAME-29: `import X from "plexus-ui:<name>"`),
/// by name.
fn svelte_imports(src: &str) -> Vec<String> {
    src.lines()
        .filter(|l| l.trim_start().starts_with("import "))
        .filter_map(|l| l.rsplit_once(" from ").map(|(_, r)| r))
        .filter_map(|r| {
            let r = r.trim().trim_end_matches(';').trim_matches(|c| c == '"' || c == '\'');
            r.strip_prefix("./").and_then(|r| r.strip_suffix(".svelte")).or_else(|| r.strip_prefix("plexus-ui:")).map(str::to_string)
        })
        .collect()
}

/// What FRAME-11 finds wrong with a UI — its sources (a file name in
/// `ui/src`, `<name>.rhx` or `<name>.svelte`, and its text), `routes.json`
/// and `slots.json` as text: a name the routes or slots give that is no
/// component, a name with two sources (FRAME-14: one name is one source),
/// and a component nothing reaches (a route, a slot's layout or default, or
/// a `<name` tag in a reached rhx source, or a sibling a reached svelte/1
/// source imports, `import X from "./<name>.svelte"` — FRAME-19, FRAME-25 —
/// or places by its bound mark, `import X from "plexus-ui:<name>"` — FRAME-29).
fn ui_breaks(sources: &[(String, String)], routes: &str, slots: &str) -> Vec<String> {
    let mut out = Vec::new();
    let routes: Value = serde_json::from_str(routes).unwrap_or_else(|e| panic!("routes.json: {e}"));
    let slots: Value = serde_json::from_str(slots).unwrap_or_else(|e| panic!("slots.json: {e}"));
    let components: Vec<(String, &str, &str)> = sources
        .iter()
        .filter_map(|(f, text)| f.strip_suffix(".rhx").or_else(|| f.strip_suffix(".svelte")).map(|n| (n.to_string(), f.as_str(), text.as_str())))
        .collect();
    let mut named: Vec<(String, &str)> = routes.as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str().map(|n| (n.to_string(), "routes.json"))).collect();
    for (layout, sections) in slots.as_object().into_iter().flatten() {
        named.push((layout.clone(), "slots.json"));
        named.extend(sections.as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str().map(|n| (n.to_string(), "slots.json"))));
    }
    let mut seen = Vec::new();
    named.retain(|(n, _)| if seen.contains(n) { false } else { seen.push(n.clone()); true });
    let have = |n: &str| components.iter().any(|(c, _, _)| c == n);
    for (n, file) in &named {
        if !have(n) {
            out.push(format!("{file} names {n}, and ui/src holds no {n}.rhx or {n}.svelte"));
        }
    }
    let mut twice: Vec<&str> = Vec::new();
    for (c, _, _) in &components {
        if !twice.contains(&c.as_str()) && components.iter().filter(|(d, _, _)| d == c).count() > 1 {
            twice.push(c);
            out.push(format!("ui/src holds {c}.rhx and {c}.svelte: one name is one source"));
        }
    }
    // Reached: what is named, then each tag of a reached rhx source and each
    // sibling a reached svelte/1 source imports, to a fixpoint.
    let mut reached: Vec<String> = named.into_iter().map(|(n, _)| n).collect();
    let mut at = 0;
    while at < reached.len() {
        let here = reached[at].clone();
        for (_, f, src) in components.iter().filter(|(c, _, _)| *c == here) {
            for (c, _, _) in &components {
                let uses = if f.ends_with(".rhx") { draws_tag(src, c) } else { svelte_imports(src).iter().any(|i| i == c) };
                if !reached.contains(c) && uses {
                    reached.push(c.clone());
                }
            }
        }
        at += 1;
    }
    for (c, f, _) in &components {
        if !reached.contains(c) {
            out.push(format!("ui/src/{f}: no route, slot or reached component uses it"));
        }
    }
    out
}

/// The words a doc may say Svelte with: real Svelte — the profile
/// `svelte/1`, FRAME-13 that brings it, "real Svelte" — and what rhx
/// borrowed ("Svelte's template subset").
const SVELTE_ALLOWED: [&str; 4] = ["svelte/1", "FRAME-13", "real Svelte", "Svelte's template subset"];

/// A doc's units of prose, each with its first line: each sentence of a
/// paragraph (a table row, a list item, a heading and a quote begin their
/// own) and each line of a fenced block, its fence lines too.
fn prose_units(text: &str) -> Vec<(usize, String)> {
    fn flush(para: &mut Option<(usize, String)>, out: &mut Vec<(usize, String)>) {
        if let Some((line, p)) = para.take() {
            let mut rest = p.as_str();
            while !rest.is_empty() {
                let end = [". ", "? ", "! "].iter().filter_map(|t| rest.find(t)).min().map_or(rest.len(), |n| n + 1);
                out.push((line, rest[..end].trim().to_string()));
                rest = &rest[end..];
            }
        }
    }
    let (mut out, mut para, mut fenced) = (Vec::new(), None::<(usize, String)>, false);
    for (i, l) in text.lines().enumerate() {
        let t = l.trim_start();
        if t.starts_with("```") {
            flush(&mut para, &mut out);
            out.push((i + 1, t.to_string()));
            fenced = !fenced;
            continue;
        }
        if fenced {
            out.push((i + 1, l.to_string()));
            continue;
        }
        if t.is_empty() || ["|", "- ", "* ", "#", ">"].iter().any(|s| t.starts_with(s)) {
            flush(&mut para, &mut out);
        }
        if t.is_empty() {
            continue;
        }
        match &mut para {
            Some((_, p)) => {
                p.push(' ');
                p.push_str(t);
            }
            None => para = Some((i + 1, t.to_string())),
        }
    }
    flush(&mut para, &mut out);
    out
}

/// Each unit of `text` that says Svelte of what is not Svelte: one that
/// names none of [`SVELTE_ALLOWED`], or calls the dialect by an old name
/// (Svelte-like, a Svelte subset, a `svelte` code fence) whatever else it says.
fn svelte_said(text: &str) -> Vec<(usize, String)> {
    prose_units(text)
        .into_iter()
        .filter(|(_, s)| {
            let lower = s.to_lowercase();
            lower.contains("svelte")
                && (["svelte-like", "svelte subset", "subset of svelte", "```svelte"].iter().any(|o| lower.contains(o)) || !SVELTE_ALLOWED.iter().any(|a| s.contains(a)))
        })
        .collect()
}

/// FRAME-11: the dialect is rhx — `ui/src/<name>.rhx`, profile `rhx/1` — and
/// nothing of menger's calls it Svelte (the owner, 2026-10-29: "Don't put
/// .svelte if it's not .svelte"); a `.svelte` is real Svelte's (FRAME-13).
#[test]
fn the_dialect_is_rhx() {
    // The checks, on what they are given first.
    let given = |c: &[(&str, &str)]| c.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    let routes = r##"{ "#/": "a-layout", "#/x": "x-page" }"##;
    let slots = r#"{ "a-layout": { "main": "b-rows" } }"#;
    assert!(ui_breaks(&given(&[("a-layout.rhx", "<slot name=\"main\"/>"), ("b-rows.rhx", "<c-chip id={x}/>"), ("c-chip.rhx", "<p/>"), ("x-page.rhx", "<p/>")]), routes, slots).is_empty());
    assert_eq!(
        ui_breaks(&given(&[("a-layout.rhx", "<p/>"), ("b-rows.rhx", "<p/>"), ("d-lost.rhx", "<p/>")]), routes, slots),
        ["routes.json names x-page, and ui/src holds no x-page.rhx or x-page.svelte", "ui/src/d-lost.rhx: no route, slot or reached component uses it"]
    );
    // FRAME-14: a name may be real Svelte's; one name is one source; a
    // svelte/1 module reaches nothing; `<b-rows-sv` is not `<b-rows`.
    let sv = "<script module>\n  export const uses = [];\n</script>\n<p/>\n";
    assert!(ui_breaks(&given(&[("a-layout.rhx", "<p/>"), ("b-rows.svelte", sv), ("x-page.rhx", "<p/>")]), routes, slots).is_empty());
    assert_eq!(
        ui_breaks(&given(&[("a-layout.rhx", "<b-rows-sv/>"), ("b-rows.rhx", "<p/>"), ("b-rows.svelte", sv), ("b-rows-sv.svelte", sv), ("c-chip.rhx", "<p/>"), ("x-page.svelte", "<c-chip id={x}/>")]), routes, slots),
        ["ui/src holds b-rows.rhx and b-rows.svelte: one name is one source", "ui/src/c-chip.rhx: no route, slot or reached component uses it"]
    );
    // FRAME-25: a sibling a reached svelte/1 source imports is reached.
    let importer = "<script module>\n  export const uses = [];\n</script>\n<script>\n  import Chip from \"./c-chip.svelte\";\n  import { live } from \"plexus-ui/host\";\n</script>\n<Chip/>\n";
    assert_eq!(svelte_imports(importer), ["c-chip"]);
    assert!(ui_breaks(&given(&[("a-layout.rhx", "<p/>"), ("b-rows.svelte", importer), ("c-chip.svelte", sv), ("x-page.rhx", "<p/>")]), routes, slots).is_empty());
    assert!(real_svelte(sv) && !real_svelte("<script>\n  prop(\"id\");\n</script>\n<p/>\n"));
    assert_eq!(svelte_said("rhx: Svelte's template subset with Rhai folds.\n\nA `.svelte` file is real Svelte (`svelte/1`, FRAME-13) or refused.\n"), []);
    assert_eq!(
        svelte_said("Components are a Svelte-like subset. Files are\n`ui/src/*.svelte`.\n\n```svelte\n<p/>\n```\n- the Svelte subset (real Svelte aside)\n"),
        [(1, "Components are a Svelte-like subset.".to_string()), (1, "Files are `ui/src/*.svelte`.".to_string()), (4, "```svelte".to_string()), (7, "- the Svelte subset (real Svelte aside)".to_string())]
    );

    let root = root();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    // A `.svelte` is real Svelte (svelte/1) in ui/src, and nowhere else under ui/ or tests/.
    let mut broken: Vec<String> = svelte_files(&root, &["ui", "tests"])
        .into_iter()
        .filter(|f| Path::new(f).parent() != Some(Path::new("ui/src")) || !real_svelte(&read(f)))
        .map(|f| format!("{f}: a source named .svelte that is not real Svelte (svelte/1, in ui/src: <script module> export const uses = [...])"))
        .collect();

    let src = root.join("ui/src");
    let sources: Vec<(String, String)> = names_in(&src)
        .into_iter()
        .filter(|f| f.ends_with(".rhx") || f.ends_with(".svelte"))
        .map(|f| {
            let text = std::fs::read_to_string(src.join(&f)).unwrap();
            (f, text)
        })
        .collect();
    broken.extend(ui_breaks(&sources, &read("ui/routes.json"), &read("ui/slots.json")));

    for doc in ["README.md", "AGENTS.md", "deploy/README.md"] {
        broken.extend(svelte_said(&read(doc)).into_iter().map(|(n, s)| format!("{doc}:{n}: {s}")));
    }
    let (readme, agents) = (read("README.md"), read("AGENTS.md"));
    broken.extend(doc_breaks(&[("README.md", readme.as_str()), ("AGENTS.md", agents.as_str())], &[("AGENTS.md", "Adding a component", &["rhx", "ui/src/*.rhx", "ui/src/<name>.rhx:LINE"])]));
    if !readme.contains("rhx/1") && !readme.contains(".rhx") {
        broken.push("README.md: names no rhx (rhx/1, <name>.rhx)".into());
    }
    assert!(broken.is_empty(), "FRAME-11: the dialect is rhx, and Svelte is said only of real Svelte:\n  {}", broken.join("\n  "));
}

/// FRAME-14: the board's five components, each in both profiles — rhx's
/// `<name>.rhx` and real Svelte's `<name>-sv.svelte` (one name is one
/// source, so the Svelte one is named apart; `-sv` the one suffix).
const BOARD: [&str; 5] = ["board-layout", "board-rows", "nav-tree", "ticket-page", "jump-palette"];

/// FRAME-25: the components the board's draw inside another — the nav's
/// Flat and an open shortcode's children, a ticket named on a page — each in
/// both profiles too: rhx's drawn by tag, the Svelte one imported by its
/// parent (`./<name>-sv.svelte`) and a component in its own right. Neither
/// needs a route or a slot: a sibling no section defaults to is reached by
/// its importer.
const BOARD_SIBLINGS: [&str; 3] = ["nav-flat", "nav-children", "ticket-chip"];

/// FRAME-14: the frame the Svelte board is drawn in — an rhx layout, since a
/// section is the host's and only an rhx template marks one (`<slot name>`;
/// a svelte/1 module mounts no section, plexus-ui@40aa390). Its sections
/// hold the Svelte components: `top` the app's top (app-top, SURFACE-2), `bar`
/// the Svelte half of `board-layout` (the top bar and its filter line), then
/// the nav, the board's keymap (keys-help), the board, the bulk bar under it
/// (bulk-bar, SURFACE-5) and the palette.
const SV_FRAME: &str = "board-frame";

/// What FRAME-14 finds missing from the board's two profiles, given the
/// sources in `ui/src` (file name → text), `routes.json` and `slots.json`:
/// a component without its `.rhx` or its real `-sv.svelte`; a route of
/// either board that is not there; a section default that is not the rhx
/// one on the rhx board, or not the Svelte one in the frame; a frame that
/// marks not all seven of its sections.
fn board_breaks(sources: &[(String, String)], routes: &str, slots: &str) -> Vec<String> {
    let mut out = Vec::new();
    let text = |f: &str| sources.iter().find(|(n, _)| n == f).map(|(_, t)| t.as_str());
    for n in BOARD.iter().chain(BOARD_SIBLINGS.iter()) {
        if text(&format!("{n}.rhx")).is_none() {
            out.push(format!("ui/src/{n}.rhx is missing (the rhx {n})"));
        }
        match text(&format!("{n}-sv.svelte")) {
            None => out.push(format!("ui/src/{n}-sv.svelte is missing (the Svelte {n})")),
            Some(t) if !real_svelte(t) => out.push(format!("ui/src/{n}-sv.svelte is not real Svelte (svelte/1: <script module> export const uses = [...])")),
            Some(_) => {}
        }
    }
    let routes: Value = serde_json::from_str(routes).unwrap_or_else(|e| panic!("routes.json: {e}"));
    // SURFACE-2: the Svelte board is the default (owner, 2026-10-30), the rhx
    // one the comparison at #/rhx/, and #/sv/ the Svelte board's alias.
    for (route, want) in [
        ("#/", SV_FRAME),
        ("#/f/:filter", SV_FRAME),
        ("#/ticket/:id", "ticket-page-sv"),
        ("#/rhx/", "board-layout"),
        ("#/rhx/f/:filter", "board-layout"),
        ("#/rhx/ticket/:id", "ticket-page"),
        ("#/sv/", SV_FRAME),
        ("#/sv/f/:filter", SV_FRAME),
        ("#/sv/ticket/:id", "ticket-page-sv"),
    ] {
        let have = routes.get(route).and_then(Value::as_str);
        if have != Some(want) {
            out.push(format!("routes.json: {route} draws {}, not {want}", have.unwrap_or("nothing")));
        }
    }
    let slots: Value = serde_json::from_str(slots).unwrap_or_else(|e| panic!("slots.json: {e}"));
    let rhx = [("nav", "nav-tree"), ("main", "board-rows"), ("palette", "jump-palette")];
    let sv = [("top", "app-top"), ("bar", "board-layout-sv"), ("nav", "nav-tree-sv"), ("keys", "keys-help"), ("main", "board-rows-sv"), ("bulk", "bulk-bar"), ("palette", "jump-palette-sv")];
    for (layout, want) in [("board-layout", &rhx[..]), (SV_FRAME, &sv[..])] {
        for (slot, component) in want {
            let have = slots.get(layout).and_then(|l| l.get(slot)).and_then(Value::as_str);
            if have != Some(component) {
                out.push(format!("slots.json: {layout}'s {slot} defaults to {}, not {component}", have.unwrap_or("nothing")));
            }
        }
    }
    match text(&format!("{SV_FRAME}.rhx")) {
        None => out.push(format!("ui/src/{SV_FRAME}.rhx is missing (the rhx frame the Svelte board is drawn in)")),
        Some(t) => {
            // The frame only places the sections: no state, no live, no call, no
            // component of its own — so the Svelte board's numbers are Svelte's.
            for word in ["live(", "call(", "state(", "fn "] {
                if t.contains(word) {
                    out.push(format!("ui/src/{SV_FRAME}.rhx holds `{word}`: the frame only places its sections"));
                }
            }
            for tag in t.split('<').skip(1).filter_map(|r| r.split(|c: char| c.is_whitespace() || c == '/' || c == '>').next()).filter(|n| n.contains('-')) {
                out.push(format!("ui/src/{SV_FRAME}.rhx draws <{tag}>: the frame only places its sections"));
            }
            for (slot, _) in sv {
                if !t.contains(&format!("<slot name=\"{slot}\"")) {
                    out.push(format!("ui/src/{SV_FRAME}.rhx marks no section {slot} (<slot name=\"{slot}\" …/>)"));
                }
            }
        }
    }
    out
}

/// FRAME-14: menger's board in real Svelte beside the rhx one — each of the
/// five as `<name>.rhx` and `<name>-sv.svelte`, the Svelte board at `#/`
/// (SURFACE-2; `#/sv/…` its alias), the rhx one at `#/rhx/`, the rhx ones the defaults
/// (`slots.json`); a right-click puts a Svelte one in a section of the rhx
/// board (tests/pages_browser.mjs). FRAME-13's `svelte_is_not_in_menger_yet`
/// went with it: its time has come.
#[test]
fn each_board_component_has_both_profiles() {
    // The check, on what it is given first.
    let sv = "<script module>\n  export const uses = [];\n</script>\n<p/>\n";
    let mut good: Vec<(String, String)> = BOARD.iter().chain(BOARD_SIBLINGS.iter()).flat_map(|n| [(format!("{n}.rhx"), "<p/>".to_string()), (format!("{n}-sv.svelte"), sv.to_string())]).collect();
    good.push((format!("{SV_FRAME}.rhx"), r#"<slot name="top" filter={filter}/><slot name="bar"/><slot name="nav"/><slot name="keys"/><slot name="main"/><slot name="bulk"/><slot name="palette"/>"#.into()));
    let routes = r##"{ "#/": "board-frame", "#/f/:filter": "board-frame", "#/ticket/:id": "ticket-page-sv", "#/rhx/": "board-layout", "#/rhx/f/:filter": "board-layout", "#/rhx/ticket/:id": "ticket-page", "#/sv/": "board-frame", "#/sv/f/:filter": "board-frame", "#/sv/ticket/:id": "ticket-page-sv" }"##;
    let slots = r#"{ "board-layout": { "nav": "nav-tree", "main": "board-rows", "palette": "jump-palette" }, "board-frame": { "top": "app-top", "bar": "board-layout-sv", "nav": "nav-tree-sv", "keys": "keys-help", "main": "board-rows-sv", "bulk": "bulk-bar", "palette": "jump-palette-sv" } }"#;
    assert_eq!(board_breaks(&good, routes, slots), Vec::<String>::new());
    let chipless: Vec<(String, String)> = good.iter().filter(|(f, _)| f != "ticket-chip-sv.svelte").cloned().collect();
    assert_eq!(board_breaks(&chipless, routes, slots), ["ui/src/ticket-chip-sv.svelte is missing (the Svelte ticket-chip)"]);
    let mut bad = good.clone();
    bad.retain(|(f, _)| f != "nav-tree-sv.svelte" && f != "board-rows.rhx");
    bad.iter_mut().filter(|(f, _)| f == "jump-palette-sv.svelte").for_each(|(_, t)| *t = "<script>\n</script>\n<p/>\n".into());
    let swapped = r#"{ "board-layout": { "nav": "nav-tree", "main": "board-rows-sv", "palette": "jump-palette" }, "board-frame": { "top": "app-top", "bar": "board-layout-sv", "nav": "nav-tree-sv", "keys": "keys-help", "main": "board-rows-sv", "bulk": "bulk-bar", "palette": "jump-palette-sv" } }"#;
    assert_eq!(
        board_breaks(&bad, r##"{ "#/": "board-frame", "#/f/:filter": "board-frame", "#/ticket/:id": "ticket-page-sv", "#/rhx/": "board-layout", "#/rhx/f/:filter": "board-layout", "#/rhx/ticket/:id": "ticket-page", "#/sv/": "board-frame", "#/sv/ticket/:id": "ticket-page" }"##, swapped),
        [
            "ui/src/board-rows.rhx is missing (the rhx board-rows)",
            "ui/src/nav-tree-sv.svelte is missing (the Svelte nav-tree)",
            "ui/src/jump-palette-sv.svelte is not real Svelte (svelte/1: <script module> export const uses = [...])",
            "routes.json: #/sv/f/:filter draws nothing, not board-frame",
            "routes.json: #/sv/ticket/:id draws ticket-page, not ticket-page-sv",
            "slots.json: board-layout's main defaults to board-rows-sv, not board-rows",
        ]
    );
    let frameless: Vec<(String, String)> = good.iter().filter(|(f, _)| f != "board-frame.rhx").cloned().chain([("board-frame.rhx".to_string(), r#"<slot name="nav"/>"#.to_string())]).collect();
    assert_eq!(board_breaks(&frameless, routes, slots).len(), 6, "a frame that marks one section of seven");
    let busy: Vec<(String, String)> = good
        .iter()
        .filter(|(f, _)| f != "board-frame.rhx")
        .cloned()
        .chain([("board-frame.rhx".to_string(), "<script>\n  prop(\"filter\");\n  let n = live(#{ snapshot: \"tickets.nav.counts\", key: \"shortcode\" });\n</script>\n<div class=\"board-layout\"><filter-line filter={filter}/><slot name=\"top\"/><slot name=\"bar\"/><slot name=\"nav\"/><slot name=\"keys\"/><slot name=\"main\"/><slot name=\"bulk\"/><slot name=\"palette\"/></div>".to_string())])
        .collect();
    assert_eq!(
        board_breaks(&busy, routes, slots),
        ["ui/src/board-frame.rhx holds `live(`: the frame only places its sections", "ui/src/board-frame.rhx draws <filter-line>: the frame only places its sections"]
    );

    // Then menger's.
    let root = root();
    let src = root.join("ui/src");
    let sources: Vec<(String, String)> = names_in(&src).into_iter().filter(|f| f.ends_with(".rhx") || f.ends_with(".svelte")).map(|f| (f.clone(), std::fs::read_to_string(src.join(&f)).unwrap())).collect();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    let broken = board_breaks(&sources, &read("ui/routes.json"), &read("ui/slots.json"));
    assert!(broken.is_empty(), "FRAME-14: the board in both profiles, rhx the default:\n  {}", broken.join("\n  "));
}

/// A svelte/1 source's code: its text without comments — `<!-- … -->`,
/// `/* … */`, and `//` to the line's end where it starts a line or follows a
/// space (so `"https://…"` stays) — so a comment may say what went. An
/// unclosed `<!--` or `/*` hides nothing: the rest is kept as code.
fn svelte_code(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(at) = [rest.find("<!--"), rest.find("/*")].into_iter().flatten().min() {
        out.push_str(&rest[..at]);
        let close = if rest[at..].starts_with("<!--") { "-->" } else { "*/" };
        match rest[at..].find(close) {
            Some(e) => rest = &rest[at + e + close.len()..],
            None => {
                out.push_str(&rest[at..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.lines()
        .map(|l| match l.match_indices("//").find(|(i, _)| *i == 0 || l[..*i].ends_with(char::is_whitespace)) {
            Some((i, _)) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The marks of a sanitizer or a markdown renderer of a component's own, or
/// of HTML set around the bus: the copied allow-list parsed the body inert
/// (`DOMParser`), listed `"script"` among what it dropped, and rendered with
/// a `markdown` of its own; HTML set by hand (`innerHTML`, its `bind:`,
/// `outerHTML`, `insertAdjacentHTML`, `createContextualFragment`). A
/// `<svelte:element this={…} {...attrs}>` (tags and attributes from data) is
/// [`spreads_element`]'s; a dynamic tag alone is fine.
const OWN_SANITIZER: [&str; 10] =
    ["DOMParser", "parseFromString", "\"script\"", "sanitiz", "function markdown", "const markdown", "innerHTML", "outerHTML", "insertAdjacentHTML", "createContextualFragment"];

/// What keeps time by hand (beside a `live(`, the registry's `debounce_ms`
/// is the way; a focus or a scroll is Svelte's `tick()` or an action).
const OWN_TIMERS: [&str; 6] = ["setTimeout(", "setInterval(", "requestAnimationFrame(", "queueMicrotask(", "Date.now(", "performance.now("];

/// Whether `code` draws a `<svelte:element …>` that spreads attributes
/// (`{...`) — elements and attributes taken from data, the copied
/// allow-list's way of drawing a body.
fn spreads_element(code: &str) -> bool {
    code.match_indices("<svelte:element").any(|(at, _)| code[at..].split('>').next().is_some_and(|tag| tag.contains("{...")))
}

/// Whether an `{#each …}` header in `code` reads a store (`$name`): Svelte
/// 5.57.2 compares the items by value there, so every row's effects re-run
/// on each change — `const list = $derived($rows.rows)` first.
fn each_over_store(code: &str) -> bool {
    code.match_indices("{#each").any(|(at, _)| {
        let head = &code[at + "{#each".len()..];
        let head = &head[..head.find(" as ").or_else(|| head.find('}')).unwrap_or(head.len())];
        head.match_indices('$').any(|(i, _)| head[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
    })
}

/// Whether `code` imports `markdown` from `plexus-ui/host` (either quote,
/// over as many lines as the import takes).
fn imports_host_markdown(code: &str) -> bool {
    let flat = code.split_whitespace().collect::<Vec<_>>().join(" ");
    ["\"plexus-ui/host\"", "'plexus-ui/host'"].iter().any(|q| {
        flat.match_indices(q).any(|(at, _)| flat[..at].rfind("import ").is_some_and(|i| flat[i..at].contains("markdown")))
    })
}

/// What one of the board's svelte/1 sources must say (each mark at least so
/// many times), must not say, and must import.
type Wants = (&'static str, &'static [(&'static str, usize)], &'static [&'static str], &'static [&'static str]);

/// FRAME-25's wants of the board's sources: the palette's two lives on
/// `when` and `debounce_ms: 80`; the rows' window moved through the handle's
/// `update` (`params: () =>`, `debounce_ms: 20`, never a collection remade
/// in a `$derived`); the ticket page composed of its parts by their marks
/// (SURFACE-3), its collections followed, not remade (a route to it is an
/// `update(props)`) — its body through `markdown`, its head's chips imported
/// and none drawn inline; the nav's Flat and children imported and none
/// drawn inline.
const BUS_WANTS: [Wants; 6] = [
    ("jump-palette-sv.svelte", &[("debounce_ms: 80", 2), ("when:", 2)], &[], &[]),
    ("board-rows-sv.svelte", &[("debounce_ms: 20", 1), ("params: () =>", 1)], &["$derived(b.live(", "$derived(live("], &[]),
    ("ticket-page-sv.svelte", &[], &["data-chip-id", "{#snippet chip", "$derived(b.live(", "$derived(live("], &["ticket-head", "ticket-body"]),
    ("ticket-body.svelte", &[("{@html markdown(", 1)], &["$derived(b.live(", "$derived(live("], &[]),
    ("ticket-head.svelte", &[], &["data-chip-id", "{#snippet chip", "$derived(b.live(", "$derived(live("], &["ticket-chip-sv"]),
    ("nav-tree-sv.svelte", &[], &["data-nav-flat-row", "data-nav-row", "\"tickets.list\""], &["nav-flat-sv", "nav-children-sv"]),
];

/// What FRAME-25 finds of the old road in menger's svelte/1 sources (file
/// name → text, `ui/src`), comments aside: a timer of its own
/// ([`OWN_TIMERS`]) in a source that calls `live(`; a sanitizer, markdown
/// or HTML of its own ([`OWN_SANITIZER`], [`spreads_element`]), an
/// `{@html …}` not of `markdown(…)`, or a `markdown(` not imported from
/// `plexus-ui/host`; a store subscribed by hand (`.subscribe(`, or a
/// stand-in store's `subscribe:`) — the face's `live` is the store; an
/// `{#each}` over a store ([`each_over_store`]); and [`BUS_WANTS`].
fn bus_breaks(sources: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (f, text) in sources.iter().filter(|(f, _)| f.ends_with(".svelte")) {
        let code = svelte_code(text);
        if code.contains("live(") {
            for t in OWN_TIMERS.iter().filter(|t| code.contains(*t)) {
                out.push(format!("ui/src/{f}: keeps time by hand (`{t}`) beside a live(: the bus's debounce_ms keeps the time (a focus or a scroll is Svelte's tick() or an action)"));
            }
        }
        for m in OWN_SANITIZER.iter().filter(|m| code.contains(*m)) {
            out.push(format!("ui/src/{f}: a sanitizer, markdown or HTML of its own (`{m}`): the body goes through markdown from plexus-ui/host"));
        }
        if spreads_element(&code) {
            out.push(format!("ui/src/{f}: a sanitizer, markdown or HTML of its own (`<svelte:element this={{…}} {{...`): the body goes through markdown from plexus-ui/host"));
        }
        for (at, _) in code.match_indices("{@html") {
            if !code[at + "{@html".len()..].trim_start().starts_with("markdown(") {
                out.push(format!("ui/src/{f}: an {{@html …}} not of markdown(…): HTML reaches the page only through the bus's sanitized render"));
            }
        }
        if code.contains("markdown(") && !imports_host_markdown(&code) {
            out.push(format!("ui/src/{f}: markdown( not imported from plexus-ui/host"));
        }
        for m in [".subscribe(", "subscribe:"].iter().filter(|m| code.contains(*m)) {
            out.push(format!("ui/src/{f}: a store subscribed or stood in by hand (`{m}`): the face's live is the store"));
        }
        if each_over_store(&code) {
            out.push(format!("ui/src/{f}: an {{#each …}} over a store ($…; compared by value: every row re-runs on each change) — `const list = $derived($rows.rows)` first"));
        }
    }
    let text = |f: &str| sources.iter().find(|(n, _)| n == f).map(|(_, t)| svelte_code(t));
    for (f, marks, never, imports) in BUS_WANTS {
        let Some(code) = text(f) else {
            out.push(format!("ui/src/{f} is missing"));
            continue;
        };
        for (m, n) in marks {
            if code.matches(m).count() < *n {
                out.push(format!("ui/src/{f}: says `{m}` {} time(s), not {n}", code.matches(m).count()));
            }
        }
        for m in never.iter().filter(|m| code.contains(*m)) {
            out.push(format!("ui/src/{f}: says `{m}`, which the bus or a sibling now does"));
        }
        let have = svelte_imports(&code);
        for i in imports {
            if !have.iter().any(|h| h == i) {
                out.push(format!("ui/src/{f}: imports no ./{i}.svelte and no plexus-ui:{i}"));
            }
        }
    }
    out
}

/// FRAME-25: the Svelte board on FRAME-19's bus — the palette held by `when`
/// and coalesced by `debounce_ms` in the host's registry, the rows' window
/// the same through the handle's `update`, the body through `markdown` from
/// `plexus-ui/host` (the copied allow-list gone: no sanitizer of menger's
/// own in `ui/src`), the nav's children and the chips imported siblings
/// and drawn nowhere else; no store subscribed by hand, no `{#each}` over a
/// store.
#[test]
fn the_svelte_board_rides_the_bus() {
    // The checks, on what they are given first.
    let given = |c: &[(&str, &str)]| c.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    assert_eq!(svelte_code("<!-- DOMParser -->a // setTimeout(\nb /* x */ \"https://c\"\n"), "a \nb  \"https://c\"");
    assert_eq!(svelte_code("a <!-- open\nDOMParser\n"), "a <!-- open\nDOMParser", "an unclosed comment hides nothing");
    assert!(spreads_element("<svelte:element this={n.tag} {...n.attrs}/>") && !spreads_element("<svelte:element this={\"h\" + level}>x</svelte:element>"));
    assert!(each_over_store("{#each ($rows).rows as r}") && each_over_store("{#each $rows.rows as r (r.id)}") && !each_over_store("{#each list as r (r.id)}{#each STATUSES as s}"));
    assert!(imports_host_markdown("import {\n  live,\n  markdown,\n} from 'plexus-ui/host';") && !imports_host_markdown("import { markdown } from \"./mine.js\";\nimport { live } from \"plexus-ui/host\";"));
    assert_eq!(svelte_imports("import fromage from \"./ticket-chip-sv.svelte\";"), ["ticket-chip-sv"]);
    assert_eq!(svelte_imports("import Head from \"plexus-ui:ticket-head\";"), ["ticket-head"]);
    let head = "<script module>\n  export const uses = [];\n</script>\n";
    let palette = format!("{head}<script>\n  // the hand-made setTimeout( went\n  import {{ live }} from \"plexus-ui/host\";\n  const found = live({{ snapshot: \"tickets.find\", params: () => ({{ query: q }}), key: \"id\", when: () => q.length > 0, debounce_ms: 80 }});\n  const searched = live({{ snapshot: \"tickets.search\", params: () => ({{ query: q }}), key: \"id\", when: () => q.length > 0, debounce_ms: 80 }});\n  const hits = $derived($found.rows);\n</script>\n{{#each hits as f (f.id)}}<a href=\"https://x\">{{f.id}}</a>{{/each}}\n");
    let rows = format!("{head}<script>\n  import {{ live }} from \"plexus-ui/host\";\n  const rows = live({{ snapshot: \"tickets.list\", params: () => ({{ at }}), key: \"id\", debounce_ms: 20 }});\n</script>\n");
    let page = format!("{head}<script>\n  import {{ live }} from \"plexus-ui/host\";\n  import Head from \"plexus-ui:ticket-head\";\n  import Body from \"plexus-ui:ticket-body\";\n</script>\n<!-- no DOMParser, no allow-list, no data-chip-id -->\n<Head id={{id}}/><Body id={{id}}/><svelte:element this={{\"h\" + n}}>x</svelte:element>\n");
    let body = format!("{head}<script>\n  import {{ live, markdown }} from \"plexus-ui/host\";\n</script>\n<div>{{@html markdown(t.body)}}</div>\n");
    let chips = format!("{head}<script>\n  import Chip from \"./ticket-chip-sv.svelte\";\n</script>\n<Chip id={{p}}/>\n");
    let nav = format!("{head}<script>\n  import NavFlat from \"./nav-flat-sv.svelte\";\n  import NavChildren from \"./nav-children-sv.svelte\";\n</script>\n");
    let good = given(&[("jump-palette-sv.svelte", &palette), ("board-rows-sv.svelte", &rows), ("ticket-page-sv.svelte", &page), ("ticket-body.svelte", &body), ("ticket-head.svelte", &chips), ("nav-tree-sv.svelte", &nav), ("x.rhx", "{#each $x as y}DOMParser")]);
    assert_eq!(bus_breaks(&good), Vec::<String>::new());
    let old_palette = format!("{head}<script>\n  import {{ bus }} from \"plexus-ui/host\";\n  const b = bus();\n  const NONE = {{ subscribe: (fn) => (fn({{ rows: [] }}), () => {{}}) }};\n  const found = $derived(asked ? b.live({{ snapshot: \"tickets.find\", params: {{ query: asked }}, key: \"id\" }}) : NONE);\n  function typed(ev) {{ clearTimeout(timer); timer = setTimeout(() => (asked = q), 80); }}\n</script>\n{{#each $found.rows as f (f.id)}}{{f.id}}{{/each}}\n");
    let old_rows = format!("{head}<script>\n  import {{ bus }} from \"plexus-ui/host\";\n  const b = bus();\n  const rows = $derived(b.live({{ snapshot: \"tickets.list\", params: {{ at: win.at }}, key: \"id\", debounce_ms: 20 }}));\n</script>\n");
    let old_page = format!("{head}<script module>\n  const WHOLE = new Set([\"script\", \"style\"]);\n  function markdown(src) {{ return src; }}\n  export function bodyNodes(t) {{ return new DOMParser().parseFromString(markdown(t), \"text/html\"); }}\n</script>\n<script>\n  import Chip from \"./ticket-chip-sv.svelte\";\n  held[c] = s.subscribe((v) => (titles[c] = v));\n</script>\n{{#snippet chip(c)}}<a data-chip-id={{c}}>{{c}}</a>{{/snippet}}\n{{#snippet nodes(l)}}<svelte:element this={{l.tag}} {{...l.attrs}}/>{{/snippet}}\n<div>{{@html t.body}}</div>\n");
    let old_nav = format!("{head}<script>\n  import NavFlat from \"./nav-flat-sv.svelte\";\n  import NavChildren from \"./nav-children-sv.svelte\";\n  const kids = live({{ snapshot: \"tickets.list\", params: {{ parent }}, key: \"id\" }});\n</script>\n<a data-nav-row={{c.id}}></a><a data-nav-flat-row={{t.id}}></a>\n");
    let bad = given(&[("jump-palette-sv.svelte", &old_palette), ("board-rows-sv.svelte", &old_rows), ("ticket-page-sv.svelte", &old_page), ("nav-tree-sv.svelte", &old_nav)]);
    let found = bus_breaks(&bad);
    let want = [
        "ui/src/board-rows-sv.svelte: says `params: () =>` 0 time(s), not 1",
        "ui/src/board-rows-sv.svelte: says `$derived(b.live(`, which the bus or a sibling now does",
        "ui/src/jump-palette-sv.svelte: keeps time by hand (`setTimeout(`) beside a live(: the bus's debounce_ms keeps the time (a focus or a scroll is Svelte's tick() or an action)",
        "ui/src/jump-palette-sv.svelte: a store subscribed or stood in by hand (`subscribe:`): the face's live is the store",
        "ui/src/jump-palette-sv.svelte: an {#each …} over a store ($…; compared by value: every row re-runs on each change) — `const list = $derived($rows.rows)` first",
        "ui/src/jump-palette-sv.svelte: says `debounce_ms: 80` 0 time(s), not 2",
        "ui/src/jump-palette-sv.svelte: says `when:` 0 time(s), not 2",
        "ui/src/nav-tree-sv.svelte: says `data-nav-flat-row`, which the bus or a sibling now does",
        "ui/src/nav-tree-sv.svelte: says `data-nav-row`, which the bus or a sibling now does",
        "ui/src/nav-tree-sv.svelte: says `\"tickets.list\"`, which the bus or a sibling now does",
        "ui/src/ticket-page-sv.svelte: a sanitizer, markdown or HTML of its own (`DOMParser`): the body goes through markdown from plexus-ui/host",
        "ui/src/ticket-page-sv.svelte: a sanitizer, markdown or HTML of its own (`parseFromString`): the body goes through markdown from plexus-ui/host",
        "ui/src/ticket-page-sv.svelte: a sanitizer, markdown or HTML of its own (`\"script\"`): the body goes through markdown from plexus-ui/host",
        "ui/src/ticket-page-sv.svelte: a sanitizer, markdown or HTML of its own (`function markdown`): the body goes through markdown from plexus-ui/host",
        "ui/src/ticket-page-sv.svelte: a sanitizer, markdown or HTML of its own (`<svelte:element this={…} {...`): the body goes through markdown from plexus-ui/host",
        "ui/src/ticket-page-sv.svelte: an {@html …} not of markdown(…): HTML reaches the page only through the bus's sanitized render",
        "ui/src/ticket-page-sv.svelte: markdown( not imported from plexus-ui/host",
        "ui/src/ticket-page-sv.svelte: a store subscribed or stood in by hand (`.subscribe(`): the face's live is the store",
        "ui/src/ticket-page-sv.svelte: imports no ./ticket-head.svelte and no plexus-ui:ticket-head",
        "ui/src/ticket-page-sv.svelte: imports no ./ticket-body.svelte and no plexus-ui:ticket-body",
        "ui/src/ticket-body.svelte is missing",
        "ui/src/ticket-head.svelte is missing",
        "ui/src/ticket-page-sv.svelte: says `data-chip-id`, which the bus or a sibling now does",
        "ui/src/ticket-page-sv.svelte: says `{#snippet chip`, which the bus or a sibling now does",
    ];
    let mut sorted = found.clone();
    sorted.sort();
    let mut wanted: Vec<String> = want.iter().map(|s| s.to_string()).collect();
    wanted.sort();
    assert_eq!(sorted, wanted, "{found:#?}");

    // Then menger's.
    let src = root().join("ui/src");
    let sources: Vec<(String, String)> = names_in(&src).into_iter().filter(|f| f.ends_with(".svelte")).map(|f| (f.clone(), std::fs::read_to_string(src.join(&f)).unwrap())).collect();
    let broken = bus_breaks(&sources);
    assert!(broken.is_empty(), "FRAME-25: the Svelte board rides the bus — no timer, sanitizer, markdown or store of its own, its children imported:\n  {}", broken.join("\n  "));
}

/// The lines of the shell function `name` in `script`, each with its number:
/// from `name() {` to the first line that is `}` alone.
fn shell_fn<'a>(script: &'a str, name: &str) -> Vec<(usize, &'a str)> {
    let open = format!("{name}() {{");
    script
        .lines()
        .enumerate()
        .skip_while(|(_, l)| !l.starts_with(&open))
        .take_while(|(_, l)| l.trim_end() != "}")
        .map(|(n, l)| (n + 1, l))
        .collect()
}

/// What a v2 build lacks to build a `svelte/1` component (FRAME-13: the tool
/// compiles it with the real Svelte compiler in Node, svelte and esbuild
/// from the framework's `package.json`; `rhx/1` needs neither), each line of
/// `build_v2` before the tool builds `$BUILD/ui`: when the UI holds a
/// `.svelte`, a check for `node` that dies saying `svelte/1`; a check that
/// `node_modules/svelte` and `node_modules/esbuild` are there that dies
/// naming `npm ci` (how they are installed: in `../plexus-ui`); and the
/// build naming `--node-modules`. SURFACE-11: the build may be a call of
/// the helper `ui_bundle` (its lines `helper`), whose own tool call names
/// `--node-modules`; the first build, the helper's or the tool's, is the one
/// the checks must come before. The checks' lines, or every reason.
fn node_check(body: &[(usize, &str)], helper: &[(usize, &str)]) -> Result<(usize, usize), String> {
    const TOOL: &str = "\"$UI_TOOL\" build \"$BUILD/ui\"";
    let code: Vec<&(usize, &str)> = body.iter().filter(|(_, l)| !l.trim_start().starts_with('#')).collect();
    let calls = |l: &str| l.contains(TOOL) || l.trim_start().starts_with("ui_bundle ") || l.contains("&& ui_bundle ") || l.contains("; ui_bundle ");
    let (build, call) = code
        .iter()
        .find(|(_, l)| calls(l))
        .map(|(n, l)| (*n, *l))
        .ok_or_else(|| "build_v2 has no \"$UI_TOOL\" build \"$BUILD/ui\" and no ui_bundle call".to_string())?;
    let line = if call.contains(TOOL) {
        call
    } else {
        helper
            .iter()
            .filter(|(_, l)| !l.trim_start().starts_with('#'))
            .find(|(_, l)| l.contains(TOOL))
            .map(|(_, l)| *l)
            .ok_or_else(|| format!("build_v2 calls ui_bundle at line {build}, and ui_bundle has no {TOOL}"))?
    };
    let mut why = Vec::new();
    let mut find = |what: &str, hit: &dyn Fn(&str) -> bool| match code.iter().find(|(_, l)| hit(l)) {
        None => {
            why.push(format!("build_v2 never {what}"));
            0
        }
        Some((n, _)) if *n > build => {
            why.push(format!("build_v2 {what} at line {n}, after the tool builds the UI at line {build}"));
            0
        }
        Some((n, _)) => *n,
    };
    let node = find(
        "checks for node when ui/src holds a .svelte, saying svelte/1 (a line with the .svelte test, command -v node, die and svelte/1)",
        &|l| (l.contains("command -v node") || l.contains("need node")) && l.contains(".svelte") && l.contains("die") && l.contains("svelte/1"),
    );
    let modules = find(
        "checks node_modules/svelte and node_modules/esbuild when ui/src holds a .svelte, naming npm ci (a line with the .svelte test, both, die and npm ci)",
        &|l| l.contains(".svelte") && l.contains("node_modules/svelte") && l.contains("node_modules/esbuild") && l.contains("die") && l.contains("npm ci"),
    );
    if !line.contains("--node-modules") {
        why.push(format!("the tool's build (called at line {build}) names no --node-modules"));
    }
    if why.is_empty() {
        Ok((node, modules))
    } else {
        Err(why.join("; "))
    }
}

/// FRAME-13: the pipeline's build host has Node when it must — `build_v2`
/// checks for `node`, and for svelte and esbuild in `../plexus-ui`'s
/// `node_modules` (installed by `npm ci` there), before the tool builds a UI
/// holding a `.svelte`, and names `--node-modules` to the tool; an rhx-only
/// UI asks nothing of Node.
#[test]
fn the_build_host_has_node() {
    // The check, on what it is given first.
    let given = |s: &'static str| s.lines().enumerate().map(|(n, l)| (n + 1, l)).collect::<Vec<_>>();
    let good = "build_v2() {\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then command -v node >/dev/null || die \"ui-build: svelte/1 needs node\"; fi\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then [[ -d $NM/node_modules/svelte && -d $NM/node_modules/esbuild ]] || die \"run npm ci in ../plexus-ui\"; fi\n  (cd \"$BUILD\" && \"$UI_TOOL\" build \"$BUILD/ui\" --node-modules \"$NM\" -o x)\n";
    assert_eq!(node_check(&given(good), &[]), Ok((2, 3)));
    let late = "build_v2() {\n  (cd \"$BUILD\" && \"$UI_TOOL\" build \"$BUILD/ui\" --node-modules x -o x)\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then command -v node >/dev/null || die \"svelte/1 needs node\"; fi\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then [[ -d node_modules/svelte && -d node_modules/esbuild ]] || die \"npm ci\"; fi\n";
    assert_eq!(
        node_check(&given(late), &[]),
        Err("build_v2 checks for node when ui/src holds a .svelte, saying svelte/1 (a line with the .svelte test, command -v node, die and svelte/1) at line 3, after the tool builds the UI at line 2; build_v2 checks node_modules/svelte and node_modules/esbuild when ui/src holds a .svelte, naming npm ci (a line with the .svelte test, both, die and npm ci) at line 4, after the tool builds the UI at line 2".to_string())
    );
    let mute = "build_v2() {\n  # command -v node .svelte die svelte/1\n  need node\n  (cd \"$BUILD\" && \"$UI_TOOL\" build \"$BUILD/ui\" -o x)\n";
    let why = node_check(&given(mute), &[]).expect_err("a comment, or a bare `need node`, is no check");
    assert!(why.contains("never checks for node") && why.contains("never checks node_modules") && why.contains("names no --node-modules"), "{why}");
    // SURFACE-11: the build through the helper — the checks before its first call, --node-modules in the helper
    let helper = "ui_bundle() { # LOG CONTRACT\n  (cd \"$BUILD\" && \"$UI_TOOL\" build \"$BUILD/ui\" --engine e --node-modules \"$NM\" -o x) >>\"$1\" 2>&1\n";
    let via = "build_v2() {\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then command -v node >/dev/null || die \"ui-build: svelte/1 needs node\"; fi\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then [[ -d $NM/node_modules/svelte && -d $NM/node_modules/esbuild ]] || die \"run npm ci in ../plexus-ui\"; fi\n  ui_bundle \"$lr\" \"\" || die x\n  ui_bundle \"$lr\" \"$CONTRACT\" || die x\n";
    assert_eq!(node_check(&given(via), &given(helper)), Ok((2, 3)));
    let via_late = "build_v2() {\n  ui_bundle \"$lr\" \"\" || die x\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then command -v node >/dev/null || die \"svelte/1 needs node\"; fi\n  if compgen -G \"$BUILD/ui/src/*.svelte\" >/dev/null; then [[ -d node_modules/svelte && -d node_modules/esbuild ]] || die \"npm ci\"; fi\n";
    let why = node_check(&given(via_late), &given(helper)).expect_err("checks after the helper's first build");
    assert!(why.contains("after the tool builds the UI at line 2"), "{why}");
    let bare = "ui_bundle() {\n  (cd \"$BUILD\" && \"$UI_TOOL\" build \"$BUILD/ui\" -o x)\n";
    let why = node_check(&given(via), &given(bare)).expect_err("a helper naming no --node-modules");
    assert!(why.contains("names no --node-modules"), "{why}");
    assert!(node_check(&given(via), &[]).expect_err("no helper").contains("ui_bundle has no"));
    let script = "x() {\n  a\n}\nbuild_v2() {\n  b\n  c\n}\ny() {\n}\n";
    assert_eq!(shell_fn(script, "build_v2"), [(4, "build_v2() {"), (5, "  b"), (6, "  c")]);

    let pipeline = std::fs::read_to_string(root().join("deploy/pipeline")).expect("deploy/pipeline");
    let body = shell_fn(&pipeline, "build_v2");
    assert!(!body.is_empty(), "deploy/pipeline has build_v2");
    let helper = shell_fn(&pipeline, "ui_bundle");
    if let Err(why) = node_check(&body, &helper) {
        panic!("FRAME-13: the build host has Node when the UI holds svelte/1 — {why}");
    }
}

/// FRAME-30: what of `index.html` is not the frame — a rule on anything but
/// the frame's elements, a token defined, no theme.css loaded first.
fn frame_breaks(index: &str) -> Vec<String> {
    const FRAME: [&str; 7] = ["*", "html", "body", "a", "button", "input", "select"];
    let mut out = Vec::new();
    let link = index.find("/app/{{UI_VERSION}}/theme.css");
    let first = index.find("<link").zip(link).is_some_and(|(l, t)| index[l..t].find('>').is_none());
    if !first {
        out.push("theme.css is not the first <link> (rel=stylesheet, /app/{{UI_VERSION}}/theme.css)".into());
    }
    let mut rest = index;
    while let Some(i) = rest.find("<style>") {
        let body = &rest[i + 7..];
        let end = body.find("</style>").unwrap_or(body.len());
        let css = &body[..end];
        let mut clean = String::new();
        let mut c = css;
        while let Some(o) = c.find("/*") {
            clean.push_str(&c[..o]);
            c = c[o..].find("*/").map_or("", |e| &c[o + e + 2..]);
        }
        clean.push_str(c);
        for rule in clean.split('}') {
            let Some((sel, decls)) = rule.split_once('{') else { continue };
            for s in sel.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !FRAME.contains(&s) {
                    out.push(format!("index.html: `{s}` is a component's rule (or no frame's): it belongs in the component's <style>"));
                }
            }
            if decls.split(';').any(|d| d.trim_start().starts_with("--")) {
                out.push(format!("index.html: `{}` defines a token: the tokens are ui/theme.toml's", sel.trim()));
            }
        }
        rest = &body[end..];
    }
    out
}

#[test]
fn the_look_is_the_components_and_the_themes() {
    let root = root();
    let index = std::fs::read_to_string(root.join("ui/index.html")).unwrap();
    let breaks = frame_breaks(&index);
    assert!(breaks.is_empty(), "ui/index.html is the frame alone:\n  {}", breaks.join("\n  "));
    // The check reads what it is given.
    let bad = "<link rel=\"stylesheet\" href=\"/app/{{UI_VERSION}}/theme.css\"><style>:root { --bg: #fff; } .board-row { height: 28px; } * { box-sizing: border-box; }</style>";
    let got = frame_breaks(bad);
    assert!(got.iter().any(|b| b.contains(".board-row")) && got.iter().any(|b| b.contains(":root")), "{got:?}");
    assert!(frame_breaks("<link rel=\"modulepreload\" href=\"x\"><link rel=\"stylesheet\" href=\"/app/{{UI_VERSION}}/theme.css\">").iter().any(|b| b.contains("first")));
    let theme = std::fs::read_to_string(root.join("ui/theme.toml")).expect("ui/theme.toml holds the tokens");
    for t in ["\"--bg\"", "\"--panel\"", "\"--ink\"", "\"--muted\"", "\"--line\"", "\"--accent\"", "\"--hover\"", "\"--warn\"", "\"--pitch\""] {
        assert!(theme.contains(t), "ui/theme.toml keeps {t}");
    }
    let mut bare = Vec::new();
    for c in ["board-layout", "board-frame", "board-rows", "nav-tree", "nav-children", "nav-flat", "jump-palette", "ticket-page", "ticket-chip", "filter-line"] {
        for f in [format!("{c}.rhx"), format!("{c}-sv.svelte")] {
            let p = root.join("ui/src").join(&f);
            if let Ok(text) = std::fs::read_to_string(&p) {
                if !text.contains("<style>") {
                    bare.push(f);
                }
            }
        }
    }
    assert!(bare.is_empty(), "every board component carries its own <style>: {bare:?}");
}
