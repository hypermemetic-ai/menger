//! FRAME-1's acceptance 2, held line by line as FRAME-2's waves land: the
//! stored-UI framework, and the shell and the login it is served with, live
//! apart from menger. menger's tree holds no copy of them, it takes each
//! from crates.io by its published version (V2-47; MFE-10's sibling
//! checkouts before it), no manifest names a directory that left, nothing in
//! it reads a path that moved, and nothing names the crates by their old
//! names.
//!
//! - V2-47 (the owner, 2026-11-05, "B"): `plexus-ui-service`,
//!   `plexus-ui-compiler` and `plexus-login` by published version, the two
//!   plexus-ui crates at one version; the tool, the engine and the compiler
//!   module built from the framework checkout `scripts/plexus-ui-src` names —
//!   the tag `plexus-ui-v<version>` of the version menger takes, never
//!   `../plexus-ui`'s working tree — so a clone of menger alone builds.
//!
//! - FRAME-3 (wave 1): the compiler and the engine, out whole to
//!   `../plexus-ui` as `plexus-ui-compiler` and `plexus-ui-engine`. menger
//!   depends on the compiler (its tests compile natively) and on the engine
//!   only through the bundle.
//! - FRAME-8 (wave 2): the bundle is the framework's tool's — `plexus-ui`
//!   (`plexus-ui-cli`), built from the sibling by `scripts/plexus-ui` and by
//!   `deploy/pipeline` — and menger's own build and sign scripts went.
//! - FRAME-4 (wave 1): `plexus-webapp` (published; PLEXUS-16) and
//!   `plexus-login` (`../plexus-login`), repositories of their own;
//!   `tether/` apart — its `plexus-login` moves with substrate's (FRAME-9).
//! - FRAME-5 (wave 2): the ui service, out to `../plexus-ui` as
//!   `plexus-ui-service`, with no menger type in it; menger registers it
//!   beside `tickets` and mounts its routes on its own shell.
//! - FRAME-7 (wave 3): menger on top — a tenant of the framework. Its own
//!   packages are `menger` and `menger-tickets-api` and nothing else; the
//!   service, the login and the compiler come from their sibling
//!   checkouts, the shell from crates.io; its shell mounts the framework's routes and has none of its
//!   own; its docs say the framework is `../plexus-ui` and menger a tenant,
//!   and point a new component at the framework's tool and contract.
//!
//! One file since FRAME-5 (it folded FRAME-4's `tests/apart_shell_login.rs`).
//! - FRAME-6 (wave 4): the binary is the framework's — `plexus-ui` (its
//!   `serve` beside `build`, `sign`, `push`) is built from `../plexus-ui`
//!   and links the service; menger's tree builds no binary of that name.
//! - FRAME-11, SV-4: the dialect is named — real Svelte (`svelte/1`):
//!   `ui/src/*.svelte`; rhx is retired (the owner, 2026-11-09: "don't
//!   remove, just port and stop using completely"), its sources kept out of
//!   the bundle in `ui/retired-rhx/` and none in `ui/src`; a `.svelte` is
//!   real Svelte and only in `ui/src`, every component reached from
//!   `routes.json` or `slots.json` and every name there a source, and
//!   README / AGENTS.md / deploy/README.md say Svelte only of real Svelte
//!   (`svelte/1`, FRAME-13) or of what rhx borrowed.
//! - FRAME-13: real Svelte (`svelte/1`) is the framework's tool's — it
//!   compiles a `.svelte` in Node at build; `deploy/pipeline`'s `build_v2` checks
//!   for `node` and for svelte and esbuild in `../plexus-ui/node_modules`
//!   (`npm ci` there) before it builds a UI that holds one, and names
//!   `--node-modules` to the tool.
//! - FRAME-14, SV-4: the board in real Svelte — each of its components as
//!   `<name>-sv.svelte` (the rhx twins retired to `ui/retired-rhx/`); the
//!   board drawn at `#/board` (SURFACE-2, EDITOR-4; `#/sv/…` and `#/rhx/…`
//!   its aliases) in a svelte/1 frame whose sections, DOM marks, hold them.
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
//! - SURFACE-10: the sheet and the tokens whole — `sheet-page.svelte` at
//!   `#/sheet` places every component of `ui/src` by its bound mark
//!   (`plexus-ui:<name>`); `ui/theme.toml` holds a pair for every status, a
//!   mark for every kind and severity, the focus ring, the skeleton, the
//!   disabled ink and the diff pair, each token once and on one spelling,
//!   no builder's `# SURFACE-n` block left; every `var(--…)` a component
//!   names is a token of the theme; every `:hover`, `:focus…`, `:disabled`,
//!   selected and checked state a component draws has its `data-sheet~=`
//!   alias; and `scripts/plexus-ui build` refuses a literal colour
//!   (`--strict-colours`).
//! - STUDIO-12, SV-4: a route draws a page — `ui/src/page-<name>.svelte`, a
//!   `<section class="page page-<name>">` placing its components by their
//!   bound marks (`plexus-ui:<name>`) and carrying the app's top as a
//!   section (`data-slot="top"`); a placed one keeps none of the page's
//!   chrome (the board link, the palette's mark).
//! - V2-19 (FRAME-9 before it): `tether/` takes `plexus-login` as
//!   plexus-substrate (which tether takes) does — by the git URL and rev
//!   substrate's manifest names (PLEXUS-26) — one `plexus-login` in its
//!   graph, never a second from a sibling path.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// menger's root: this package's manifest directory, canonical (this
/// machine's `/home` is `/var/home`).
fn root() -> PathBuf {
    canonical(Path::new(env!("CARGO_MANIFEST_DIR")))
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

/// The crates menger's cargo graph takes of the framework and the login,
/// each by its published version from crates.io (V2-47; the engine and the
/// tool are not in the graph: the tool is built from the framework's
/// checkout at the version's tag, below, and builds the engine).
const PUBLISHED: &[&str] = &["plexus-ui-compiler", "plexus-ui-service", "plexus-login"];

/// The framework's crates menger takes, which move together: one version.
const ONE_RELEASE: &[&str] = &["plexus-ui-compiler", "plexus-ui-service"];

/// crates.io, as a lock or `cargo metadata` names it.
fn from_crates_io(source: &str) -> bool {
    source == "registry+https://github.com/rust-lang/crates.io-index" || source == "sparse+https://index.crates.io/"
}

/// The plexus-ui checkout the framework is read from — what
/// `scripts/plexus-ui-src` names: the tag of the version menger takes
/// (cloned once), or `PLEXUS_UI_SRC`.
fn framework() -> PathBuf {
    let out = Command::new(root().join("scripts/plexus-ui-src")).arg(root()).output().expect("scripts/plexus-ui-src runs");
    assert!(out.status.success(), "V2-47: scripts/plexus-ui-src names no framework checkout:\n{}", String::from_utf8_lossy(&out.stderr));
    canonical(Path::new(String::from_utf8_lossy(&out.stdout).trim()))
}

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

/// Each package of `cargo metadata`'s answer: its name, version and source
/// (`None`: a path, this repository's or another's).
fn sources(metadata: &Value) -> Vec<(String, String, Option<String>)> {
    metadata["packages"]
        .as_array()
        .map(|ps| ps.iter().filter_map(|p| Some((p["name"].as_str()?.to_string(), p["version"].as_str()?.to_string(), p["source"].as_str().map(str::to_string)))).collect())
        .unwrap_or_default()
}

/// What the graph `pkgs` breaks of the rule: every crate of `wanted`
/// present, each from crates.io; the crates of `together` at one version;
/// no package named in `old`.
fn breaks(pkgs: &[(String, String, Option<String>)], wanted: &[&str], together: &[&str], old: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for want in wanted {
        let found: Vec<_> = pkgs.iter().filter(|(n, _, _)| n == want).collect();
        if found.is_empty() {
            out.push(format!("{want}: not in the graph"));
        }
        for (_, v, src) in found {
            match src {
                Some(s) if from_crates_io(s) => {}
                Some(s) => out.push(format!("{want} {v}: from {s}, not crates.io")),
                None => out.push(format!("{want} {v}: by path, not by published version")),
            }
        }
    }
    let mut versions: Vec<&str> = pkgs.iter().filter(|(n, _, _)| together.contains(&n.as_str())).map(|(_, v, _)| v.as_str()).collect();
    versions.sort();
    versions.dedup();
    if versions.len() > 1 {
        out.push(format!("{together:?} at {versions:?}: one release, one version"));
    }
    for (name, v, _) in pkgs {
        if old.contains(name) {
            out.push(format!("{name}: an old name, still in the graph ({v})"));
        }
    }
    out
}

/// The line of `script` that builds the tool from the framework's checkout:
/// cargo with `--manifest-path` and `-p plexus-ui-cli`, never into
/// `../plexus-ui` (V2-47: the checkout `scripts/plexus-ui-src` names).
fn tool_build_line(script: &str) -> Option<(usize, &str)> {
    script
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("--manifest-path") && l.contains("-p plexus-ui-cli") && !l.contains("../plexus-ui"))
        .map(|(n, l)| (n + 1, l))
}

#[test]
fn the_framework_is_published() {
    let root = root();
    let broken = breaks(&sources(&metadata()), PUBLISHED, ONE_RELEASE, &old_packages());
    assert!(broken.is_empty(), "V2-47: the framework and the login are not taken by published version:\n  {}", broken.join("\n  "));

    for file in ["scripts/plexus-ui", "deploy/pipeline"] {
        let script = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let cargo_lines: Vec<String> =
            script.lines().enumerate().filter(|(_, l)| l.contains("cargo build")).map(|(n, l)| format!("{file}:{}: {}", n + 1, l.trim())).collect();
        assert!(
            tool_build_line(&script).is_some() && script.contains("plexus-ui-src"),
            "V2-47: {file} does not build the tool from the checkout scripts/plexus-ui-src names (`--manifest-path … -p plexus-ui-cli`, no ../plexus-ui); its cargo lines:\n  {}",
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
/// V2-24: the media service is menger's own crate, as the tickets service is.
/// V2-29: and the docs service. V2-31: and the quota service.
const OWN: &[&str] = &["menger", "menger-docs", "menger-media", "menger-quota", "menger-tickets-api"];

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
    let metadata = metadata();
    let pkgs = packages(&metadata);
    // The framework's service, the login, and the compiler (a
    // dev-dependency: the ui tests compile natively), each by published
    // version; the engine is in no graph of menger's — the tool builds it
    // from the framework's checkout (`the_framework_is_published` holds the tool).
    let mut broken = breaks(&sources(&metadata), PUBLISHED, ONE_RELEASE, &old_packages());
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
            ("README.md", "The services", &["plexus-ui", "tenant", "by published version"]),
            ("AGENTS.md", "What v2 is", &["plexus-ui", "tenant", "by published version"]),
            // The framework's tool and its contract, kept with its code.
            ("AGENTS.md", "Adding a component", &["plexus-ui build", "plexus-ui push", "CONTRACT.md"]),
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
    let theirs = names_in(&framework().join("scripts"));
    assert!(theirs.iter().any(|n| n == "ui-compiler-wasm.sh"), "FRAME-7: the framework's scripts/ holds no ui-compiler-wasm.sh: {theirs:?}");
    let twins: Vec<String> = names_in(&root.join("scripts")).into_iter().filter(|n| theirs.contains(n)).map(|n| format!("scripts/{n} (the framework's scripts/{n} is its own)")).collect();
    assert!(twins.is_empty(), "FRAME-7: menger keeps a copy of the framework's scripts:\n  {}", twins.join("\n  "));
    let pipeline = std::fs::read_to_string(root.join("deploy/pipeline")).expect("deploy/pipeline");
    let calls: Vec<(usize, &str)> = code_lines(&pipeline).into_iter().filter(|(_, l)| !l.trim_start().starts_with('#') && l.contains("ui-compiler-wasm.sh")).collect();
    assert!(
        calls.iter().any(|(_, l)| l.contains("$PUI/scripts/ui-compiler-wasm.sh")) && calls.iter().all(|(_, l)| l.contains("scripts/ui-compiler-wasm.sh") && !l.contains("../plexus-ui")),
        "FRAME-7, V2-47: deploy/pipeline does not build the compiler module with the framework checkout's ($PUI) scripts/ui-compiler-wasm.sh; its lines naming the script: {calls:?}"
    );
}

#[test]
fn the_engines_test_lives_with_the_engine() {
    let root = root();
    assert!(framework().join("tests/engine_browser.mjs").is_file(), "FRAME-7: the framework's tests/engine_browser.mjs is missing — the engine's own smoke");
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
        { "name": "plexus-login", "manifest_path": "/w/plexus-login/Cargo.toml" },
        { "name": old_service, "manifest_path": "/w/menger/ui-service/Cargo.toml" },
        { "name": old_engine, "manifest_path": "/w/menger/ui-engine/Cargo.toml" },
        { "name": "nameless" },
    ]});
    let pkgs = packages(&doc);
    assert_eq!(pkgs.len(), 6, "a package with no manifest path is skipped: {pkgs:?}");
    assert_eq!(pkgs[1], ("plexus-ui-compiler".to_string(), PathBuf::from("/w/plexus-ui/compiler")));

    // V2-47: every crate present and from crates.io, the framework's two at
    // one version; an old name named.
    let io = Some("registry+https://github.com/rust-lang/crates.io-index".to_string());
    let src = |n: &str, v: &str, s: Option<String>| (n.to_string(), v.to_string(), s);
    let good = [src("menger", "0.6.2", None), src("plexus-ui-compiler", "0.1.0", io.clone()), src("plexus-ui-service", "0.1.0", io.clone()), src("plexus-login", "0.2.3", io.clone())];
    assert!(breaks(&good, PUBLISHED, ONE_RELEASE, &old_packages()).is_empty(), "the graph after V2-47");
    let sparse = [src("plexus-login", "0.2.3", Some("sparse+https://index.crates.io/".into()))];
    assert!(breaks(&sparse, &["plexus-login"], &[], &[]).is_empty(), "the sparse index is crates.io");
    let mut old = good.to_vec();
    old.push(src(&old_service, "0.6.2", None));
    assert_eq!(breaks(&old, PUBLISHED, ONE_RELEASE, &old_packages()), [format!("{old_service}: an old name, still in the graph (0.6.2)")]);
    assert!(!old_packages().contains(&"plexus-ui-service".to_string()) && old_packages().contains(&old_compiler) && old_packages().contains(&old_engine));

    // Missing is named; a path (a sibling's or menger's) and a git revision are not a published version;
    // the two framework crates at two versions are two releases.
    assert_eq!(breaks(&good[..1], &["plexus-ui-service"], &[], &[]), ["plexus-ui-service: not in the graph"]);
    let path = [src("plexus-ui-service", "0.1.0", None), src("plexus-login", "0.2.2", None)];
    assert_eq!(
        breaks(&path, &["plexus-ui-service", "plexus-login"], &[], &[]),
        ["plexus-ui-service 0.1.0: by path, not by published version", "plexus-login 0.2.2: by path, not by published version"]
    );
    let git = [src("plexus-login", "0.2.3", Some("git+https://github.com/hypermemetic-ai/plexus-login?rev=23b9e2c8#23b9e2c8".into()))];
    assert_eq!(breaks(&git, &["plexus-login"], &[], &[]).len(), 1, "a git revision taken for a published version");
    let split = [src("plexus-ui-compiler", "0.1.0", io.clone()), src("plexus-ui-service", "0.1.1", io.clone())];
    assert_eq!(breaks(&split, ONE_RELEASE, ONE_RELEASE, &[]), ["[\"plexus-ui-compiler\", \"plexus-ui-service\"] at [\"0.1.0\", \"0.1.1\"]: one release, one version"]);
    // And the real graph's sources: menger by path, plexus-core from crates.io.
    let real = sources(&metadata());
    assert!(real.iter().any(|(n, _, s)| n == "menger" && s.is_none()) && real.iter().any(|(n, _, s)| n == "plexus-core" && s.as_deref().is_some_and(from_crates_io)));
    let _ = (parent, &pkgs);

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

    // The tool's build line: from the checkout plexus-ui-src names, never ../plexus-ui.
    let script = "x\n  cargo build --quiet --manifest-path \"$src/Cargo.toml\" -p plexus-ui-cli --target-dir \"$target\"\n";
    assert_eq!(tool_build_line(script).map(|(n, _)| n), Some(2));
    assert!(tool_build_line("cargo build -p plexus-ui-cli\n").is_none());
    assert!(tool_build_line("cargo build --manifest-path \"$root/../plexus-ui/Cargo.toml\" -p plexus-ui-cli\n").is_none(), "the sibling's working tree");
    assert!(tool_build_line("cargo build --manifest-path $PUI/Cargo.toml -p plexus-ui-compiler\n").is_none());

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

    // FRAME-7: the tenant's graph — members exactly menger's own (V2-24: three; V2-29: four; V2-31: five), nothing
    // else under its root, no ui crate under menger's name anywhere.
    let m = Path::new("/w/menger");
    let own = |n: &str| (n.to_string(), PathBuf::from(format!("/w/menger/{}", match n { "menger" => "", "menger-media" => "media", "menger-docs" => "docs", "menger-quota" => "quota", _ => "tickets-api" })));
    let members = [String::from("menger-tickets-api"), String::from("menger"), String::from("menger-media"), String::from("menger-docs"), String::from("menger-quota")];
    assert!(tenant_breaks(&[own("menger"), own("menger-tickets-api"), own("menger-media"), own("menger-docs"), own("menger-quota"), pkgs[2].clone()], &members, m).is_empty());
    let extra = [own("menger"), ("menger-ui-kit".to_string(), PathBuf::from("/w/elsewhere/kit")), ("plexus-ui-service".to_string(), PathBuf::from("/w/menger/vendor/s"))];
    assert_eq!(
        tenant_breaks(&extra, &members[..1], m),
        [
            "the workspace's members are [\"menger-tickets-api\"], not [\"menger\", \"menger-docs\", \"menger-media\", \"menger-quota\", \"menger-tickets-api\"]".to_string(),
            "menger-ui-kit: a ui crate under menger's name (/w/elsewhere/kit)".to_string(),
            "plexus-ui-service: a package of menger's own (/w/menger/vendor/s)".to_string(),
        ]
    );
    let meta = serde_json::json!({ "workspace_members": ["a 1", "b 1"], "packages": [
        { "id": "a 1", "name": "menger" }, { "id": "b 1", "name": "menger-tickets-api" }, { "id": "c 1", "name": "plexus-webapp" } ]});
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

// ── tether (FRAME-9, V2-19) ─────────────────────────────────────────────────

/// A `git = "…"` dependency's URL and `rev` in `text` (a manifest): each
/// line whose key is `dep`, as (line, url, rev) — `None` for either it
/// does not say.
fn git_deps(text: &str, dep: &str) -> Vec<(usize, Option<String>, Option<String>)> {
    let value = |line: &str, key: &str| -> Option<String> {
        let at = line.find(&format!("{key} ="))?;
        quoted(&line[at..]).first().map(|s| s.to_string())
    };
    text.lines()
        .enumerate()
        .filter(|(_, raw)| {
            let key = raw.split('#').next().unwrap_or("").trim_start();
            key.strip_prefix(dep).is_some_and(|rest| rest.trim_start().starts_with('='))
        })
        .map(|(i, raw)| {
            let line = raw.split('#').next().unwrap_or("");
            (i + 1, value(line, "git"), value(line, "rev"))
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

/// Each package named `name` in `cargo metadata`'s answer: its `source`
/// (`None` for a path dependency).
fn sources_of(metadata: &Value, name: &str) -> Vec<Option<String>> {
    metadata["packages"]
        .as_array()
        .map(|ps| ps.iter().filter(|p| p["name"].as_str() == Some(name)).map(|p| p["source"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// What tether breaks of the rule: its manifest's one `plexus-login` is
/// `git = URL, rev = REV` — the URL and rev substrate names — and its
/// graph `found` holds exactly one `plexus-login`, from that git source.
fn tether_breaks(deps: &[(usize, Option<String>, Option<String>)], found: Result<&[Option<String>], &str>, url: &str, rev: &str) -> Vec<String> {
    let mut out = Vec::new();
    if deps.is_empty() {
        out.push("tether/Cargo.toml: no `plexus-login = { git = …, rev = … }`".to_string());
    }
    for (n, git, at) in deps {
        if git.as_deref() != Some(url) || at.as_deref() != Some(rev) {
            out.push(format!("tether/Cargo.toml:{n}: plexus-login from {git:?} at {at:?}, not substrate's {url} at {rev}"));
        }
    }
    let wanted = format!("git+{url}?rev={rev}#{rev}");
    match found {
        Err(e) => out.push(format!("cd tether && cargo metadata --locked --offline failed: {e}")),
        Ok(found) => {
            if found.len() != 1 {
                out.push(format!("tether's graph holds {} plexus-login(s), not one: {found:?}", found.len()));
            }
            for s in found {
                if s.as_deref() != Some(wanted.as_str()) {
                    out.push(format!("tether's graph takes plexus-login from {s:?}, not {wanted}"));
                }
            }
        }
    }
    out
}

/// V2-19 (FRAME-9 before it): `tether/` takes `plexus-login` as
/// plexus-substrate — which tether imports — takes it: by the one git URL
/// and rev substrate's manifest names (PLEXUS-26: substrate is on 0.7 by
/// published versions, plexus-login by `git`, unpublished on that line), so
/// its graph holds one `plexus-login`, never a second from a sibling path.
/// Substrate's manifest is read where tether takes substrate from (V2-44,
/// PLEXUS-13: its public repository at a revision, so the checkout cargo
/// resolved — no sibling needed).
#[test]
fn tether_takes_the_login_substrate_names() {
    let root = root();
    let meta = tether_metadata(&root);
    let substrate = meta
        .as_ref()
        .ok()
        .and_then(|m| m["packages"].as_array()?.iter().find(|p| p["name"] == "plexus-substrate")?["manifest_path"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| panic!("PLEXUS-13: tether's graph holds no plexus-substrate: {:?}", meta.as_ref().err()));
    let substrate_text = std::fs::read_to_string(&substrate).unwrap_or_else(|e| panic!("PLEXUS-26: plexus-substrate's manifest ({}): {e}", substrate.display()));
    let named = git_deps(&substrate_text, "plexus-login");
    let [(_, Some(url), Some(rev))] = named.as_slice() else { panic!("plexus-substrate/Cargo.toml names plexus-login by git and rev once: {named:?}") };
    let text = std::fs::read_to_string(root.join("tether/Cargo.toml")).expect("tether/Cargo.toml");
    let deps = git_deps(&text, "plexus-login");
    let found = meta.as_ref().map(|m| sources_of(m, "plexus-login"));
    let broken = tether_breaks(&deps, found.as_deref().map_err(|e| e.as_str()), url, rev);
    assert!(broken.is_empty(), "V2-19: tether does not take plexus-login as substrate does, alone:\n  {}", broken.join("\n  "));
}

#[test]
fn tether_check_reads_what_it_is_given() {
    let url = "https://github.com/hypermemetic-ai/plexus-login";
    let rev = "23b9e2c8ba710e78a296091775823760eeab078a";
    let text = format!(
        "# plexus-login = {{ git = \"x\" }}\nplexus-login-extra = {{ git = \"y\" }}\nplexus-login = {{ git = \"{url}\", rev = \"{rev}\" }}\nother = {{ path = \"../plexus-login\" }}\n"
    );
    let deps = git_deps(&text, "plexus-login");
    assert_eq!(deps, [(3, Some(url.to_string()), Some(rev.to_string()))]);
    assert_eq!(git_deps("plexus-login = { path = \"../../plexus-login\" }\n", "plexus-login"), [(1, None, None)], "a path line says neither");

    let wanted = Some(format!("git+{url}?rev={rev}#{rev}"));
    let one = [wanted.clone()];
    assert!(tether_breaks(&deps, Ok(&one[..]), url, rev).is_empty(), "substrate's, once");
    // A sibling path in the manifest and in the graph; two copies; none; no line; another rev; the registry's; cargo refusing.
    let path = [(47, None, None)];
    let sibling: [Option<String>; 1] = [None];
    assert_eq!(tether_breaks(&path, Ok(&sibling[..]), url, rev).len(), 2);
    let two = [wanted.clone(), None];
    assert_eq!(tether_breaks(&deps, Ok(&two[..]), url, rev).len(), 2, "two copies: counted and the stray named");
    let none: [Option<String>; 0] = [];
    assert_eq!(tether_breaks(&deps, Ok(&none[..]), url, rev).len(), 1, "none in the graph");
    assert_eq!(tether_breaks(&[], Ok(&one[..]), url, rev).len(), 1, "no line in the manifest");
    let other_rev = [(3, Some(url.to_string()), Some("abc123".to_string()))];
    assert_eq!(tether_breaks(&other_rev, Ok(&one[..]), url, rev).len(), 1, "another rev");
    let registry = [Some("registry+https://github.com/rust-lang/crates.io-index".to_string())];
    assert_eq!(tether_breaks(&deps, Ok(&registry[..]), url, rev).len(), 1, "the registry's is not substrate's");
    assert_eq!(tether_breaks(&deps, Err("no such file"), url, rev).len(), 1);
}

// ── FRAME-6: the binary ────────────────────────────────────────────────────

/// `cargo metadata` of the framework checkout's workspace (`framework()`),
/// its members alone (each with the dependencies its manifest declares), offline.
fn framework_metadata() -> Result<Value, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline", "--manifest-path"])
        .arg(framework().join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata of the framework's checkout failed:\n{}", String::from_utf8_lossy(&out.stderr)));
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
    let broken = binary_breaks(&metadata(), theirs.as_ref().map_err(String::as_str), &root(), &framework());
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

/// SV-4: where each retired rhx component went, when its port is not
/// `<name>.svelte` or its twin `<name>-sv.svelte`: board-layout's own part is
/// board-layout-sv in board-frame's `bar`, the rhx ticket page's is page-ticket.
const RETIRED_TO: [(&str, &str); 2] = [("board-layout", "board-layout-sv"), ("ticket-page", "page-ticket")];

/// What SV-4 finds of rhx in the UI, given the names in `ui/src` and in
/// `ui/retired-rhx`: an rhx source left in `ui/src`, and a retired one with
/// no port in `ui/src` (`<name>.svelte`, `<name>-sv.svelte` or
/// [`RETIRED_TO`]'s).
fn retired_breaks(src: &[String], retired: &[String]) -> Vec<String> {
    let mut out: Vec<String> = src.iter().filter(|f| f.ends_with(".rhx")).map(|f| format!("ui/src/{f}: rhx is retired (SV-4) — its port is real Svelte, the source kept in ui/retired-rhx/")).collect();
    for n in retired.iter().filter_map(|f| f.strip_suffix(".rhx")) {
        let to = RETIRED_TO.iter().find(|(from, _)| *from == n).map(|(_, to)| to.to_string());
        let ports = [Some(format!("{n}.svelte")), Some(format!("{n}-sv.svelte")), to.map(|t| format!("{t}.svelte"))];
        if !ports.iter().flatten().any(|p| src.contains(p)) {
            out.push(format!("ui/retired-rhx/{n}.rhx: no port in ui/src ({n}.svelte or {n}-sv.svelte)"));
        }
    }
    out
}

/// FRAME-11, SV-4: the dialect is real Svelte — `ui/src/<name>.svelte`,
/// profile `svelte/1` (FRAME-13) — and rhx is retired: its sources kept in
/// `ui/retired-rhx/`, out of the bundle, each with its port in `ui/src`;
/// nothing of menger's calls anything else Svelte (the owner, 2026-10-29:
/// "Don't put .svelte if it's not .svelte").
#[test]
fn the_dialect_is_svelte() {
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
    let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(retired_breaks(&names(&["a-page.svelte", "b-rows-sv.svelte", "page-ticket.svelte"]), &names(&["a-page.rhx", "b-rows.rhx", "ticket-page.rhx"])).is_empty());
    assert_eq!(
        retired_breaks(&names(&["a-page.rhx", "a-page.svelte"]), &names(&["c-lost.rhx"])),
        ["ui/src/a-page.rhx: rhx is retired (SV-4) — its port is real Svelte, the source kept in ui/retired-rhx/", "ui/retired-rhx/c-lost.rhx: no port in ui/src (c-lost.svelte or c-lost-sv.svelte)"]
    );
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
    let retired = names_in(&root.join("ui/retired-rhx"));
    if !retired.iter().any(|f| f.ends_with(".rhx")) {
        broken.push("ui/retired-rhx/ holds no .rhx: the retired sources are kept, not deleted (SV-4)".into());
    }
    broken.extend(retired_breaks(&names_in(&src), &retired));

    for doc in ["README.md", "AGENTS.md", "deploy/README.md"] {
        broken.extend(svelte_said(&read(doc)).into_iter().map(|(n, s)| format!("{doc}:{n}: {s}")));
    }
    let (readme, agents) = (read("README.md"), read("AGENTS.md"));
    broken.extend(doc_breaks(&[("README.md", readme.as_str()), ("AGENTS.md", agents.as_str())], &[("AGENTS.md", "Adding a component", &["svelte/1", "ui/src/*.svelte", "ui/src/<name>.svelte:LINE", "ui/retired-rhx"])]));
    if !readme.contains("ui/retired-rhx") {
        broken.push("README.md: names no ui/retired-rhx (where rhx went, SV-4)".into());
    }
    assert!(broken.is_empty(), "FRAME-11, SV-4: the dialect is real Svelte, rhx retired, and Svelte is said only of real Svelte:\n  {}", broken.join("\n  "));
}

/// FRAME-14, SV-4: the board's components in real Svelte, `<name>-sv.svelte`
/// (one name was one source while rhx's `<name>.rhx` stood beside it; the
/// rhx ones are retired to `ui/retired-rhx/`). STUDIO-12: the ticket page
/// is `page-ticket`, placing the ticket's parts.
const BOARD: [&str; 4] = ["board-layout", "board-rows", "nav-tree", "jump-palette"];

/// FRAME-25: the components the board's draw inside another — the nav's
/// Flat and an open shortcode's children, a ticket named on a page — each
/// imported by its parent (`./<name>-sv.svelte`) and a component in its own
/// right. Neither needs a route or a slot: a sibling no section defaults to
/// is reached by its importer.
const BOARD_SIBLINGS: [&str; 3] = ["nav-flat", "nav-children", "ticket-chip"];

/// FRAME-14, SV-4: the frame the board is drawn in — a svelte/1 layout
/// (CONTRACT.md "A layout in any profile": its sections DOM marks,
/// `<div data-slot="…">`, which the host fills). Its sections hold the
/// board: `top` the app's top (app-top, SURFACE-2), `bar` board-layout-sv
/// (the top bar and its filter line), then the nav, the facets (nav-facets)
/// and the saved filters (filters-saved, SURFACE-6) under it, the board's
/// keymap (keys-help), the board, the bulk bar under it (bulk-bar,
/// SURFACE-5) and the palette.
const SV_FRAME: &str = "board-frame";

/// The frame's sections and their defaults.
const FRAME_SLOTS: [(&str, &str); 9] = [
    ("top", "app-top"),
    ("bar", "board-layout-sv"),
    ("nav", "nav-tree-sv"),
    ("facets", "nav-facets"),
    ("saved", "filters-saved"),
    ("keys", "keys-help"),
    ("main", "board-rows-sv"),
    ("bulk", "bulk-bar"),
    ("palette", "jump-palette-sv"),
];

/// What FRAME-14 and SV-4 find missing from the board, given the sources in
/// `ui/src` (file name → text), `routes.json` and `slots.json`: a component
/// without its real `-sv.svelte`; a route of the board that is not there (the
/// `#/rhx/…` ones the board's aliases now); a section default that is not
/// the Svelte one in the frame; a frame that is not svelte/1, marks not all
/// nine of its sections, or does more than place them.
fn board_breaks(sources: &[(String, String)], routes: &str, slots: &str) -> Vec<String> {
    let mut out = Vec::new();
    let text = |f: &str| sources.iter().find(|(n, _)| n == f).map(|(_, t)| t.as_str());
    for n in BOARD.iter().chain(BOARD_SIBLINGS.iter()) {
        match text(&format!("{n}-sv.svelte")) {
            None => out.push(format!("ui/src/{n}-sv.svelte is missing (the Svelte {n})")),
            Some(t) if !real_svelte(t) => out.push(format!("ui/src/{n}-sv.svelte is not real Svelte (svelte/1: <script module> export const uses = [...])")),
            Some(_) => {}
        }
    }
    let routes: Value = serde_json::from_str(routes).unwrap_or_else(|e| panic!("routes.json: {e}"));
    // SURFACE-2: the Svelte board is the board (owner, 2026-10-30); #/sv/
    // its alias, and SV-4: #/rhx/ too, where the rhx one stood.
    for (route, want) in [
        ("#/board", SV_FRAME),
        ("#/f/:filter", SV_FRAME),
        ("#/ticket/:id", "page-ticket"),
        ("#/rhx/", SV_FRAME),
        ("#/rhx/f/:filter", SV_FRAME),
        ("#/rhx/ticket/:id", "page-ticket"),
        ("#/sv/", SV_FRAME),
        ("#/sv/f/:filter", SV_FRAME),
        ("#/sv/ticket/:id", "page-ticket"),
    ] {
        let have = routes.get(route).and_then(Value::as_str);
        if have != Some(want) {
            out.push(format!("routes.json: {route} draws {}, not {want}", have.unwrap_or("nothing")));
        }
    }
    let slots: Value = serde_json::from_str(slots).unwrap_or_else(|e| panic!("slots.json: {e}"));
    for (slot, component) in FRAME_SLOTS {
        let have = slots.get(SV_FRAME).and_then(|l| l.get(slot)).and_then(Value::as_str);
        if have != Some(component) {
            out.push(format!("slots.json: {SV_FRAME}'s {slot} defaults to {}, not {component}", have.unwrap_or("nothing")));
        }
    }
    match text(&format!("{SV_FRAME}.svelte")) {
        None => out.push(format!("ui/src/{SV_FRAME}.svelte is missing (the frame the board is drawn in)")),
        Some(t) => {
            if !real_svelte(t) {
                out.push(format!("ui/src/{SV_FRAME}.svelte is not real Svelte (svelte/1: <script module> export const uses = [...])"));
            }
            // The frame only places the sections: no state, no live, no call, no
            // component of its own — so the board's numbers are its sections'.
            let code = svelte_code(t);
            for word in ["live(", "call(", "$state(", "bus("] {
                if code.contains(word) {
                    out.push(format!("ui/src/{SV_FRAME}.svelte holds `{word}`: the frame only places its sections"));
                }
            }
            for i in svelte_imports(&code) {
                out.push(format!("ui/src/{SV_FRAME}.svelte imports {i}: the frame only places its sections"));
            }
            for (slot, _) in FRAME_SLOTS {
                if !code.contains(&format!("data-slot=\"{slot}\"")) {
                    out.push(format!("ui/src/{SV_FRAME}.svelte marks no section {slot} (<div data-slot=\"{slot}\" …>)"));
                }
            }
        }
    }
    out
}

/// FRAME-14, SV-4: menger's board in real Svelte — each component as
/// `<name>-sv.svelte`, the frame a svelte/1 layout, the board at `#/board`
/// (SURFACE-2, EDITOR-4; `#/sv/…` and `#/rhx/…` its aliases).
#[test]
fn the_board_is_svelte() {
    // The check, on what it is given first.
    let sv = "<script module>\n  export const uses = [];\n</script>\n<p/>\n";
    let frame = format!("{sv}{}", FRAME_SLOTS.iter().map(|(s, _)| format!("<div data-slot=\"{s}\"></div>")).collect::<String>());
    let mut good: Vec<(String, String)> = BOARD.iter().chain(BOARD_SIBLINGS.iter()).map(|n| (format!("{n}-sv.svelte"), sv.to_string())).collect();
    good.push((format!("{SV_FRAME}.svelte"), frame.clone()));
    let routes = r##"{ "#/board": "board-frame", "#/f/:filter": "board-frame", "#/ticket/:id": "page-ticket", "#/rhx/": "board-frame", "#/rhx/f/:filter": "board-frame", "#/rhx/ticket/:id": "page-ticket", "#/sv/": "board-frame", "#/sv/f/:filter": "board-frame", "#/sv/ticket/:id": "page-ticket" }"##;
    let slots = r#"{ "board-frame": { "top": "app-top", "bar": "board-layout-sv", "nav": "nav-tree-sv", "facets": "nav-facets", "saved": "filters-saved", "keys": "keys-help", "main": "board-rows-sv", "bulk": "bulk-bar", "palette": "jump-palette-sv" } }"#;
    assert_eq!(board_breaks(&good, routes, slots), Vec::<String>::new());
    let chipless: Vec<(String, String)> = good.iter().filter(|(f, _)| f != "ticket-chip-sv.svelte").cloned().collect();
    assert_eq!(board_breaks(&chipless, routes, slots), ["ui/src/ticket-chip-sv.svelte is missing (the Svelte ticket-chip)"]);
    let mut bad = good.clone();
    bad.retain(|(f, _)| f != "nav-tree-sv.svelte");
    bad.iter_mut().filter(|(f, _)| f == "jump-palette-sv.svelte").for_each(|(_, t)| *t = "<script>\n</script>\n<p/>\n".into());
    let swapped = r#"{ "board-frame": { "top": "app-top", "bar": "board-layout-sv", "nav": "nav-tree-sv", "facets": "nav-facets", "saved": "filters-saved", "keys": "keys-help", "main": "board-rows", "bulk": "bulk-bar", "palette": "jump-palette-sv" } }"#;
    assert_eq!(
        board_breaks(&bad, r##"{ "#/board": "board-frame", "#/f/:filter": "board-frame", "#/ticket/:id": "page-ticket", "#/rhx/": "board-layout", "#/rhx/f/:filter": "board-frame", "#/rhx/ticket/:id": "page-ticket", "#/sv/": "board-frame", "#/sv/ticket/:id": "ticket-page" }"##, swapped),
        [
            "ui/src/nav-tree-sv.svelte is missing (the Svelte nav-tree)",
            "ui/src/jump-palette-sv.svelte is not real Svelte (svelte/1: <script module> export const uses = [...])",
            "routes.json: #/rhx/ draws board-layout, not board-frame",
            "routes.json: #/sv/f/:filter draws nothing, not board-frame",
            "routes.json: #/sv/ticket/:id draws ticket-page, not page-ticket",
            "slots.json: board-frame's main defaults to board-rows, not board-rows-sv",
        ]
    );
    let frameless: Vec<(String, String)> = good.iter().filter(|(f, _)| f != "board-frame.svelte").cloned().chain([("board-frame.svelte".to_string(), format!("{sv}<div data-slot=\"nav\"></div>"))]).collect();
    assert_eq!(board_breaks(&frameless, routes, slots).len(), 8, "a frame that marks one section of nine");
    let busy: Vec<(String, String)> = good
        .iter()
        .filter(|(f, _)| f != "board-frame.svelte")
        .cloned()
        .chain([("board-frame.svelte".to_string(), frame.replace("<p/>", "<script>\n  import { live } from \"plexus-ui/host\";\n  import Line from \"plexus-ui:filter-line-sv\";\n  const n = live({ snapshot: \"tickets.nav.counts\", key: \"shortcode\" });\n</script>\n<Line/>"))])
        .collect();
    assert_eq!(
        board_breaks(&busy, routes, slots),
        ["ui/src/board-frame.svelte holds `live(`: the frame only places its sections", "ui/src/board-frame.svelte imports filter-line-sv: the frame only places its sections"]
    );

    // Then menger's.
    let root = root();
    let src = root.join("ui/src");
    let sources: Vec<(String, String)> = names_in(&src).into_iter().filter(|f| f.ends_with(".rhx") || f.ends_with(".svelte")).map(|f| (f.clone(), std::fs::read_to_string(src.join(&f)).unwrap())).collect();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    let broken = board_breaks(&sources, &read("ui/routes.json"), &read("ui/slots.json"));
    assert!(broken.is_empty(), "FRAME-14, SV-4: the board in real Svelte:\n  {}", broken.join("\n  "));
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

/// EDITOR-3: a component whose timer is a write's pause, not a read's
/// debounce — the editor sends `tickets.amend` 600 ms after the last key,
/// which no `live` spec can keep (the registry debounces what is asked,
/// never what is written). Named here, reviewed here; every other timer
/// beside a `live(` is still the bus's.
const WRITE_PAUSE: [&str; 1] = ["editor-body.svelte"];

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
/// `update(props)`) — its body through `markdown` in ticket-view, its head's chips imported
/// and none drawn inline; the nav's Flat and children imported and none
/// drawn inline.
const BUS_WANTS: [Wants; 6] = [
    ("jump-palette-sv.svelte", &[("debounce_ms: 80", 2), ("when:", 2)], &[], &[]),
    ("board-rows-sv.svelte", &[("debounce_ms: 20", 1), ("params: () =>", 1)], &["$derived(b.live(", "$derived(live("], &[]),
    // SURFACE-16: the rendering is ticket-view's, from props alone; ticket-body keeps the live spec and places it.
    ("ticket-body.svelte", &[], &["markdown(", "$derived(b.live(", "$derived(live("], &["ticket-view"]),
    ("ticket-view.svelte", &[("{@html markdown(", 1), ("export const uses = [];", 1)], &["live(", "bus(", "call("], &[]),
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
        if code.contains("live(") && !WRITE_PAUSE.contains(&f.as_str()) {
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
    let body = format!("{head}<script>\n  import {{ live }} from \"plexus-ui/host\";\n  import View from \"./ticket-view.svelte\";\n</script>\n<View body={{t.body}}/>\n");
    let view = format!("{head}<script>\n  import {{ markdown }} from \"plexus-ui/host\";\n</script>\n<div>{{@html markdown(body)}}</div>\n");
    let chips = format!("{head}<script>\n  import Chip from \"./ticket-chip-sv.svelte\";\n</script>\n<Chip id={{p}}/>\n");
    let nav = format!("{head}<script>\n  import NavFlat from \"./nav-flat-sv.svelte\";\n  import NavChildren from \"./nav-children-sv.svelte\";\n</script>\n");
    let good = given(&[("jump-palette-sv.svelte", &palette), ("board-rows-sv.svelte", &rows), ("ticket-page-sv.svelte", &page), ("ticket-body.svelte", &body), ("ticket-view.svelte", &view), ("ticket-head.svelte", &chips), ("nav-tree-sv.svelte", &nav), ("x.rhx", "{#each $x as y}DOMParser")]);
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
        "ui/src/ticket-body.svelte is missing",
        "ui/src/ticket-view.svelte is missing",
        "ui/src/ticket-head.svelte is missing",
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
    for c in ["board-layout", "board-frame", "board-rows", "nav-tree", "nav-children", "nav-flat", "jump-palette", "page-ticket", "ticket-chip", "filter-line"] {
        for f in [format!("{c}.svelte"), format!("{c}-sv.svelte")] {
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

/// SURFACE-10: what the sheet lacks — each component of `ui/src` (its name
/// from `<name>.svelte`, the sheet aside) the sheet does not
/// import as `plexus-ui:<name>`, and the route that is not `#/sheet`'s.
/// STUDIO-12: `#/sheet` draws `page-sheet`, the page that places the
/// sheet — the one component the sheet does not place (it would hold itself).
fn sheet_breaks(names: &[String], sheet: &str, routes: &str) -> Vec<String> {
    let mut out = Vec::new();
    for n in names.iter().filter(|n| *n != "sheet-page" && *n != "page-sheet") {
        if !sheet.contains(&format!("\"plexus-ui:{n}\"")) {
            out.push(format!("ui/src/sheet-page.svelte places no {n} (import … from \"plexus-ui:{n}\")"));
        }
    }
    let routes: Value = serde_json::from_str(routes).unwrap_or_else(|e| panic!("routes.json: {e}"));
    if routes.get("#/sheet").and_then(Value::as_str) != Some("page-sheet") {
        out.push("ui/routes.json: #/sheet is not page-sheet".into());
    }
    out
}

/// The names of `ui/src`'s components, sorted.
fn component_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("ui/src"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|f| f.strip_suffix(".rhx").or_else(|| f.strip_suffix(".svelte")).map(str::to_string))
        .collect();
    names.sort();
    names
}

#[test]
fn every_component_is_on_the_sheet() {
    // The check reads what it is given.
    let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let sheet = "<script>\n  import Rows from \"plexus-ui:board-rows\";\n</script>\n";
    assert_eq!(sheet_breaks(&names(&["board-rows", "sheet-page"]), sheet, r##"{"#/sheet":"page-sheet"}"##), Vec::<String>::new());
    assert_eq!(
        sheet_breaks(&names(&["board-rows", "nav-tree"]), sheet, "{}"),
        ["ui/src/sheet-page.svelte places no nav-tree (import … from \"plexus-ui:nav-tree\")", "ui/routes.json: #/sheet is not page-sheet"]
    );
    // Then the real sheet: every component and every layout.
    let root = root();
    let sheet = std::fs::read_to_string(root.join("ui/src/sheet-page.svelte")).expect("ui/src/sheet-page.svelte: the sheet (SURFACE-10)");
    let routes = std::fs::read_to_string(root.join("ui/routes.json")).unwrap();
    let broken = sheet_breaks(&component_names(&root), &sheet, &routes);
    assert!(broken.is_empty(), "SURFACE-10: a component missing from the sheet:\n  {}", broken.join("\n  "));
}

/// The statuses, kinds and severities the tokens paint (README "Surface").
const STATUSES: [&str; 11] = ["draft", "pending", "ready", "coding", "in-review", "complete", "blocked", "superseded", "canceled", "epic", "doc"];
const KINDS: [&str; 6] = ["scope", "build", "execution", "gap", "spike", "doc"];
const SEVERITIES: [&str; 4] = ["critical", "high", "medium", "low"];

/// The tokens `theme` (theme.toml's text) defines, with each line's number.
fn theme_tokens(theme: &str) -> Vec<(usize, String, String)> {
    theme
        .lines()
        .enumerate()
        .filter_map(|(n, l)| {
            let l = l.trim_start();
            let rest = l.strip_prefix('"')?;
            let (name, value) = rest.split_once('"')?;
            name.starts_with("--").then(|| (n + 1, name.to_string(), value.trim_start().trim_start_matches('=').trim().to_string()))
        })
        .collect()
}

/// SURFACE-10: what keeps `theme` from being whole — a token APP-114's sheet
/// wants that it lacks, a token twice, a builder's `# SURFACE-n` block not
/// folded in, a colour not on `#rrggbb` / `#rrggbbaa`.
fn theme_breaks(theme: &str) -> Vec<String> {
    let mut out = Vec::new();
    let tokens = theme_tokens(theme);
    let mut want: Vec<String> = Vec::new();
    for s in STATUSES {
        want.push(format!("--st-{s}-bg"));
        want.push(format!("--st-{s}-fg"));
    }
    want.extend(KINDS.iter().map(|k| format!("--kind-{k}")));
    want.extend(SEVERITIES.iter().map(|k| format!("--sev-{k}")));
    want.extend(["--focus", "--skeleton", "--disabled", "--row-sel", "--sel", "--hover", "--diff-add-bg", "--diff-add-fg", "--diff-del-bg", "--diff-del-fg"].map(String::from));
    for w in &want {
        if !tokens.iter().any(|(_, n, _)| n == w) {
            out.push(format!("ui/theme.toml lacks {w}"));
        }
    }
    for (i, (n, name, _)) in tokens.iter().enumerate() {
        if tokens[..i].iter().any(|(_, m, _)| m == name) {
            out.push(format!("ui/theme.toml:{n}: {name} twice"));
        }
    }
    for (n, l) in theme.lines().enumerate() {
        if l.contains("# SURFACE-") {
            out.push(format!("ui/theme.toml:{}: a builder's block, not folded in: {}", n + 1, l.trim()));
        }
    }
    for (n, name, value) in &tokens {
        for (at, _) in value.match_indices('#') {
            let hex = value[at + 1..].chars().take_while(char::is_ascii_hexdigit).count();
            if hex != 6 && hex != 8 {
                out.push(format!("ui/theme.toml:{n}: {name} spells a colour short ({}): #rrggbb or #rrggbbaa", &value[at..at + 1 + hex]));
            }
        }
    }
    out
}

/// The `<style>` of a component's source, comments aside.
fn style_of(src: &str) -> String {
    let Some(at) = src.find("<style>") else { return String::new() };
    let body = &src[at + 7..];
    let css = &body[..body.find("</style>").unwrap_or(body.len())];
    let mut clean = String::new();
    let mut c = css;
    while let Some(o) = c.find("/*") {
        clean.push_str(&c[..o]);
        c = c[o..].find("*/").map_or("", |e| &c[o + e + 2..]);
    }
    clean.push_str(c);
    clean
}

/// SURFACE-10: each `var(--name` of a component's sheet that the theme does
/// not define (nor the sheet itself, as `--name:`).
fn token_breaks(sources: &[(String, String)], theme: &str) -> Vec<String> {
    let defined: Vec<String> = theme_tokens(theme).into_iter().map(|(_, n, _)| n).collect();
    let mut out = Vec::new();
    for (f, src) in sources {
        let css = style_of(src);
        let mut seen: Vec<&str> = Vec::new();
        for (at, _) in css.match_indices("var(--") {
            let name = &css[at + 4..];
            let name = &name[..name.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(name.len())];
            if !defined.iter().any(|d| d == name) && !css.contains(&format!("{name}:")) && !seen.contains(&name) {
                seen.push(name);
                out.push(format!("ui/src/{f}: var({name}) — no such token in ui/theme.toml"));
            }
        }
    }
    out
}

/// SURFACE-10: each state a component's sheet draws on a pseudo-class or a
/// mark — `:hover`, `:focus-visible` / `:focus`, `:disabled`, a selected or a
/// checked row — whose sheet never names its alias `[data-sheet~=…]`
/// (APP-114: the sheet shows a state held, without the pointer).
fn alias_breaks(sources: &[(String, String)]) -> Vec<String> {
    const STATES: [(&str, &[&str]); 5] = [
        ("hover", &[":hover"]),
        ("focus", &[":focus"]),
        ("disabled", &[":disabled"]),
        ("selected", &["[data-selected", "[aria-selected"]),
        ("checked", &[":checked", "[aria-pressed", "[aria-checked", "[data-checked"]),
    ];
    let mut out = Vec::new();
    for (f, src) in sources {
        let css = style_of(src);
        for (state, marks) in STATES {
            if marks.iter().any(|m| css.contains(m)) && !css.contains(&format!("[data-sheet~={state}]")) {
                out.push(format!("ui/src/{f}: draws {state} ({}) and no [data-sheet~={state}]", marks.iter().filter(|m| css.contains(*m)).copied().collect::<Vec<_>>().join(", ")));
            }
        }
    }
    out
}

/// Whether `script` (scripts/plexus-ui) hands `build` the strict lint.
fn builds_strict(script: &str) -> bool {
    script.lines().any(|l| !l.trim_start().starts_with('#') && l.contains("build") && l.contains("--strict-colours"))
}

#[test]
fn the_tokens_are_whole() {
    // The checks read what they are given.
    let ok = {
        let mut t = String::from("[colour.status]\n");
        for s in STATUSES {
            t += &format!("\"--st-{s}-bg\" = ['#ffffff', '#000000', '{s}']\n\"--st-{s}-fg\" = ['#000000', '#ffffff', '{s}']\n");
        }
        for k in KINDS {
            t += &format!("\"--kind-{k}\" = ['#123456', '#654321', '{k}']\n");
        }
        for k in SEVERITIES {
            t += &format!("\"--sev-{k}\" = ['#123456', '#654321', '{k}']\n");
        }
        for k in ["focus", "skeleton", "disabled", "row-sel", "sel", "hover", "diff-add-bg", "diff-add-fg", "diff-del-bg", "diff-del-fg"] {
            t += &format!("\"--{k}\" = ['#123456', '#1d1c1a24', '{k}']\n");
        }
        t
    };
    assert_eq!(theme_breaks(&ok), Vec::<String>::new());
    let bad = format!("{ok}# SURFACE-4\n\"--focus\" = ['#a23', '#ffffff', 'again']\n");
    let got = theme_breaks(&bad);
    assert!(got.iter().any(|b| b.contains("--focus twice")) && got.iter().any(|b| b.contains("# SURFACE-4")) && got.iter().any(|b| b.contains("#a23")), "{got:?}");
    assert!(theme_breaks("").iter().any(|b| b.contains("--st-epic-bg")));
    let given = |c: &[(&str, &str)]| c.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    assert_eq!(token_breaks(&given(&[("a.svelte", "<style>.a { color: var(--ink); --w: 2px; width: var(--w); background: var(--nope, var(--bg)) }</style>")]), "\"--ink\" = ['#000000', 'x']\n\"--bg\" = ['#000000', 'x']\n"), ["ui/src/a.svelte: var(--nope) — no such token in ui/theme.toml"]);
    assert_eq!(
        alias_breaks(&given(&[("a.svelte", "<style>.r:hover, .r:global([data-sheet~=hover]) { } .r[data-selected] { }</style>"), ("b.rhx", "<p/>")])),
        ["ui/src/a.svelte: draws selected ([data-selected) and no [data-sheet~=selected]"]
    );
    assert!(builds_strict("if [[ ${1:-} == build ]]; then set -- build --strict-colours \"${@:2}\"; fi") && !builds_strict("# build --strict-colours\nexec plexus-ui \"$@\""));

    // Then the real ones.
    let root = root();
    let theme = std::fs::read_to_string(root.join("ui/theme.toml")).unwrap();
    let broken = theme_breaks(&theme);
    assert!(broken.is_empty(), "SURFACE-10: ui/theme.toml is not whole:\n  {}", broken.join("\n  "));
    let sources: Vec<(String, String)> = component_names(&root)
        .into_iter()
        .flat_map(|n| [format!("{n}.rhx"), format!("{n}.svelte")])
        .filter_map(|f| std::fs::read_to_string(root.join("ui/src").join(&f)).ok().map(|t| (f, t)))
        .collect();
    let broken = token_breaks(&sources, &theme);
    assert!(broken.is_empty(), "SURFACE-10: a component paints with a token the theme lacks:\n  {}", broken.join("\n  "));
    let broken = alias_breaks(&sources);
    assert!(broken.is_empty(), "SURFACE-10: a state the sheet cannot hold:\n  {}", broken.join("\n  "));
    let script = std::fs::read_to_string(root.join("scripts/plexus-ui")).unwrap();
    assert!(builds_strict(&script), "SURFACE-10: scripts/plexus-ui builds with --strict-colours (a literal colour is refused, not warned)");
}

/// STUDIO-12, SV-4: a route draws a page — a component of components the
/// studio edits in the browser — placing the components by their bound
/// marks. Each page (its name holds a hyphen, as a component's must:
/// `page-<name>`, `ui/src/page-<name>.svelte`), the routes that draw it and
/// the components it places (`import Head from "plexus-ui:ticket-head"`, a
/// bound mark the host fills). The first ten are the app's pages (EDITOR-4:
/// `page-editor` at `#/`, V2-29: a doc at `#/doc/:doc`; the board at
/// `#/board`) and carry the app's top (`<div data-slot="top">`, app-top by
/// `slots.json`) and the palette its jump button opens (`<div
/// data-slot="palette">`, jump-palette-sv); the rest draw one component
/// alone for its browser cases. V2-25: `page-epics` (`#/epics`) and
/// `page-me` (`#/me`) went with the `epics` activation and the owner's call
/// on `session`'s page.
const PAGES: [(&str, &[&str], &[&str]); 16] = [
    ("page-editor", &["#/", "#/edit/:id", "#/doc/:doc"], &["editor-screen"]),
    ("page-ticket", &["#/ticket/:id", "#/sv/ticket/:id"], &["ticket-head", "ticket-body", "ticket-edit", "ticket-relations", "ticket-actions", "ticket-commits", "ticket-history", "ticket-revisions"]),
    ("page-new", &["#/new", "#/new/:parent"], &["ticket-create"]),
    ("page-search", &["#/search", "#/search/:q", "#/search/:q/f/:filter"], &["search-page"]),
    ("page-ready", &["#/ready", "#/ready/:filter"], &["work-ready"]),
    ("page-waves", &["#/waves", "#/waves/in/:epics", "#/waves/:id", "#/waves/to/:to"], &["work-waves"]),
    ("page-filters", &["#/filters", "#/filters/:filter"], &["filters-saved"]),
    ("page-orgs", &["#/orgs"], &["orgs-list"]),
    ("page-releases", &["#/releases"], &["ui-versions"]),
    ("page-sheet", &["#/sheet", "#/sheet/:id"], &["sheet-page"]),
    ("page-bulk", &["#/bulk", "#/bulk/:ids"], &["bulk-bar"]),
    ("page-blockers", &["#/blockers/:id"], &["work-blockers"]),
    ("page-edit", &["#/form/:id"], &["ticket-edit"]),
    ("page-actions", &["#/actions/:id"], &["ticket-actions"]),
    ("page-facets", &["#/facets", "#/facets/:filter"], &["nav-facets"]),
    ("page-counts", &["#/counts/:filter"], &["nav-counts"]),
];

/// The app's pages: [`PAGES`]' first nine, each with the top.
const APP_PAGES: usize = 10;

/// The components a svelte/1 source places by their bound marks
/// (FRAME-29: `import X from "plexus-ui:<name>"`), by name.
fn bound_imports(src: &str) -> Vec<String> {
    src.lines()
        .filter(|l| l.trim_start().starts_with("import "))
        .filter_map(|l| l.rsplit_once(" from ").map(|(_, r)| r))
        .filter_map(|r| r.trim().trim_end_matches(';').trim_matches(|c| c == '"' || c == '\'').strip_prefix("plexus-ui:").map(str::to_string))
        .collect()
}

/// What STUDIO-12 and SV-4 find wrong, given the sources in `ui/src` (file
/// name → text), `routes.json` and `slots.json`: a route's page that places
/// no component (no bound mark, no section); a page of [`PAGES`] missing,
/// not real Svelte, not routed as the table says, not a `<section
/// class="page page-…">`, not placing one of its components by its bound
/// mark, or placing one that is not svelte/1; an app page without its top
/// and palette (`data-slot="top"`, app-top by default; `data-slot="palette"`,
/// jump-palette-sv); and a placed component that kept the page's chrome —
/// the board link (`data-back`, app-top's now) or the palette's mark
/// (`plexus-ui:jump-palette…`, the page's `palette` section now).
fn page_breaks(sources: &[(String, String)], routes: &str, slots: &str) -> Vec<String> {
    let mut out = Vec::new();
    let text = |f: &str| sources.iter().find(|(n, _)| n == f).map(|(_, t)| t.as_str());
    let routes: Value = serde_json::from_str(routes).unwrap_or_else(|e| panic!("routes.json: {e}"));
    let slots: Value = serde_json::from_str(slots).unwrap_or_else(|e| panic!("slots.json: {e}"));
    for (route, c) in routes.as_object().into_iter().flatten().filter_map(|(r, v)| v.as_str().map(|c| (r, c))) {
        if let Some(t) = text(&format!("{c}.svelte")) {
            let code = svelte_code(t);
            if !code.contains("data-slot=") && bound_imports(&code).is_empty() {
                out.push(format!("ui/src/{c}.svelte, {route}'s page, places no component: a page is a component of components"));
            }
        }
    }
    for (i, (page, at, places)) in PAGES.iter().enumerate() {
        let Some(t) = text(&format!("{page}.svelte")) else {
            out.push(format!("ui/src/{page}.svelte is missing (the page of {})", at.join(", ")));
            continue;
        };
        let code = svelte_code(t);
        if !real_svelte(t) {
            out.push(format!("ui/src/{page}.svelte is not real Svelte (svelte/1: <script module> export const uses = [...])"));
        }
        for route in *at {
            let have = routes.get(route).and_then(Value::as_str);
            if have != Some(page) {
                out.push(format!("routes.json: {route} draws {}, not {page}", have.unwrap_or("nothing")));
            }
        }
        if !code.contains(&format!("class=\"page {page}\"")) {
            out.push(format!("ui/src/{page}.svelte: no <section class=\"page {page}\">"));
        }
        let placed = bound_imports(&code);
        for c in *places {
            if !placed.iter().any(|p| p == c) {
                out.push(format!("ui/src/{page}.svelte places no {c} (import … from \"plexus-ui:{c}\")"));
            }
            match text(&format!("{c}.svelte")) {
                Some(s) if real_svelte(s) => {
                    // The sheet places every component, the palettes among them: a tile, not its chrome.
                    for chrome in ["data-back", "\"plexus-ui:jump-palette", "data-action=\"palette.open\""].iter().filter(|_| *c != "sheet-page") {
                        if s.contains(chrome) {
                            out.push(format!("ui/src/{c}.svelte keeps the page's chrome (`{chrome}`): it is {page}'s"));
                        }
                    }
                }
                _ => out.push(format!("ui/src/{page}.svelte places {c}, which is no svelte/1 component")),
            }
        }
        if i < APP_PAGES {
            for (slot, want) in [("top", "app-top"), ("palette", "jump-palette-sv")] {
                if !code.contains(&format!("data-slot=\"{slot}\"")) {
                    out.push(format!("ui/src/{page}.svelte marks no section {slot} (<div data-slot=\"{slot}\" …>)"));
                }
                let have = slots.get(page).and_then(|l| l.get(slot)).and_then(Value::as_str);
                if have != Some(want) {
                    out.push(format!("slots.json: {page}'s {slot} defaults to {}, not {want}", have.unwrap_or("nothing")));
                }
            }
        }
    }
    out
}

#[test]
fn every_route_is_a_page() {
    // The check reads what it is given: a page per route of the table, each
    // placing its components; then a page placing nothing, a missing one, a
    // component kept chrome.
    let head = "<script module>\n  export const uses = [];\n</script>\n";
    let sv = |extra: &str| format!("{head}<p{extra}/>\n");
    let mut good: Vec<(String, String)> = Vec::new();
    let mut routes = serde_json::Map::new();
    let mut slots = serde_json::Map::new();
    for (i, (page, at, places)) in PAGES.iter().enumerate() {
        let top = if i < APP_PAGES { "<div data-slot=\"top\"></div><div data-slot=\"palette\"></div>" } else { "" };
        let imports: String = places.iter().enumerate().map(|(n, c)| format!("  import C{n} from \"plexus-ui:{c}\";\n")).collect();
        let marks: String = (0..places.len()).map(|n| format!("<C{n} {{id}}/>")).collect();
        good.push((format!("{page}.svelte"), format!("{head}<script>\n{imports}</script>\n<section class=\"page {page}\">{top}{marks}</section>\n")));
        for c in *places {
            if !good.iter().any(|(f, _)| f == &format!("{c}.svelte")) {
                good.push((format!("{c}.svelte"), sv("")));
            }
        }
        for r in *at {
            routes.insert(r.to_string(), Value::from(*page));
        }
        if i < APP_PAGES {
            slots.insert(page.to_string(), serde_json::json!({ "top": "app-top", "palette": "jump-palette-sv" }));
        }
    }
    good.push(("board-frame.svelte".into(), format!("{head}<div data-slot=\"top\"></div>")));
    routes.insert("#/board".into(), Value::from("board-frame")); // EDITOR-4: the board; `#/` is page-editor's, from the table
    routes.insert("#/rhx/ticket/:id".into(), Value::from("page-ticket"));
    let (r, s) = (Value::Object(routes.clone()).to_string(), Value::Object(slots.clone()).to_string());
    assert_eq!(page_breaks(&good, &r, &s), Vec::<String>::new());

    let mut bad = good.clone();
    bad.retain(|(f, _)| f != "page-orgs.svelte");
    for (f, t) in bad.iter_mut() {
        match f.as_str() {
            "page-ticket.svelte" => *t = t.replace("  import C5 from \"plexus-ui:ticket-commits\";\n", "").replace("<div data-slot=\"top\"></div>", ""),
            "page-releases.svelte" => *t = t.replace("class=\"page page-releases\"", "class=\"releases\""),
            "search-page.svelte" => *t = sv(" data-back").replace("[]", "[]; // \"plexus-ui:jump-palette-sv\""),
            "page-counts.svelte" => *t = format!("{head}<section class=\"page page-counts\"><p/></section>"),
            _ => {}
        }
    }
    bad.push(("ticket-page-sv.svelte".into(), sv("")));
    let mut routes2 = routes.clone();
    routes2.insert("#/sv/ticket/:id".into(), Value::from("ticket-page-sv"));
    let mut slots2 = slots.clone();
    slots2.insert("page-ready".into(), serde_json::json!({ "top": "board-layout-sv" }));
    slots2.insert("page-releases".into(), serde_json::json!({ "top": "app-top" }));
    let mut got = page_breaks(&bad, &Value::Object(routes2).to_string(), &Value::Object(slots2).to_string());
    got.sort();
    let mut want = vec![
        "ui/src/ticket-page-sv.svelte, #/sv/ticket/:id's page, places no component: a page is a component of components",
        "routes.json: #/sv/ticket/:id draws ticket-page-sv, not page-ticket",
        "ui/src/page-counts.svelte, #/counts/:filter's page, places no component: a page is a component of components",
        "ui/src/page-counts.svelte places no nav-counts (import … from \"plexus-ui:nav-counts\")",
        "ui/src/page-releases.svelte: no <section class=\"page page-releases\">",
        "ui/src/page-orgs.svelte is missing (the page of #/orgs)",
        "ui/src/page-ticket.svelte places no ticket-commits (import … from \"plexus-ui:ticket-commits\")",
        "ui/src/page-ticket.svelte marks no section top (<div data-slot=\"top\" …>)",
        "slots.json: page-ready's top defaults to board-layout-sv, not app-top",
        "slots.json: page-ready's palette defaults to nothing, not jump-palette-sv",
        "slots.json: page-releases's palette defaults to nothing, not jump-palette-sv",
        "ui/src/search-page.svelte keeps the page's chrome (`data-back`): it is page-search's",
        "ui/src/search-page.svelte keeps the page's chrome (`\"plexus-ui:jump-palette`): it is page-search's",
    ];
    want.sort();
    assert_eq!(got, want);

    // Then menger's own.
    let root = root();
    let src = root.join("ui/src");
    let sources: Vec<(String, String)> = names_in(&src).into_iter().filter(|f| f.ends_with(".svelte")).map(|f| (f.clone(), std::fs::read_to_string(src.join(&f)).unwrap())).collect();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    let broken = page_breaks(&sources, &read("ui/routes.json"), &read("ui/slots.json"));
    assert!(broken.is_empty(), "STUDIO-12, SV-4: every route is a page placing its components:\n  {}", broken.join("\n  "));
}
