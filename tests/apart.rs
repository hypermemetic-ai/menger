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
        .find(|(_, l)| l.contains("--manifest-path") && l.contains("../plexus-ui/") && l.contains("-p plexus-ui-cli"))
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

#[test]
fn the_docs_say_menger_is_a_tenant() {
    let root = root();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
    let (readme, agents) = (read("README.md"), read("AGENTS.md"));
    let docs = [("README.md", readme.as_str()), ("AGENTS.md", agents.as_str())];
    let mut broken = doc_breaks(
        &docs,
        &[
            ("README.md", "What v2 is", &["../plexus-ui", "tenant"]),
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
    assert!(broken.is_empty(), "FRAME-7: the docs do not yet say menger is the framework's tenant:\n  {}", broken.join("\n  "));
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
    assert_eq!(moved_paths("cat ui-compiler/tests/golden/x.svelte"), ["ui-compiler/"]);
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

    // golden/: menger's when not a framework's tests/golden/.
    assert!(golden_as_ours("`src/*.svelte`, `golden/` — compiled by"));
    assert!(golden_as_ours("ui/golden/ and compiler/tests/golden/"));
    assert!(!golden_as_ours("`../plexus-ui`'s (`compiler/tests/golden/`)"));
}
