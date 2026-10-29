//! FRAME-1's acceptance 2, held line by line as FRAME-2's waves land: the
//! stored-UI framework lives apart from menger. menger's tree holds no copy
//! of it, it takes the framework from the sibling checkout `../plexus-ui`
//! (MFE-10's way), nothing in it reads a path that moved, and nothing names
//! the crates by their old names.
//!
//! FRAME-3 (wave 1) moves the compiler and the engine out whole, as
//! `plexus-ui-compiler` and `plexus-ui-engine`. menger depends on the
//! compiler (its tests compile natively, a dev-dependency) and on the engine
//! only through the bundle. FRAME-8: the bundle is the framework's tool's —
//! `plexus-ui` (`plexus-ui-cli`), built from the sibling by `scripts/plexus-ui`
//! and by `deploy/pipeline` — and menger's own build and sign scripts went.
//! FRAME-4 and FRAME-5 add their paths to [`GONE`] and their names to
//! [`old_packages`].

use std::path::{Path, PathBuf};

/// menger's root: this package's manifest directory.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The sibling checkout the framework lives in.
fn framework() -> PathBuf {
    root().parent().expect("menger has a parent").join("plexus-ui")
}

/// The paths that went, from menger's root — each a file or a directory.
const GONE: &[&str] = &[
    // FRAME-3: the compiler and the engine, now ../plexus-ui's.
    "ui-compiler",
    "ui-engine",
    // FRAME-8: the bundle's build and the pipeline's signer, now ../plexus-ui's
    // `plexus-ui` (build, sign, push); ui/golden/ was the build's fallback when
    // no compiler could be had, and the tool compiles in-process. (ui-service's
    // ui-sign goes with ui-service: FRAME-5 moved it to ../plexus-ui/service.)
    "scripts/ui-build.sh",
    "deploy/ui-sign.mjs",
    "ui/golden",
];

/// The framework's packages in menger's cargo graph, each from `../plexus-ui`
/// (the engine and the tool are not: the tool is built from the sibling, below,
/// and builds the engine).
const SIBLINGS: &[&str] = &["plexus-ui-compiler"];

/// The packages the graph may no longer hold, and the names no file may
/// carry: the crates' old names, as a manifest writes them and as Rust does.
/// `ui-service`'s stays until FRAME-5. Spelled in two halves so this file
/// does not name them itself.
fn old_packages() -> Vec<String> {
    ["-ui-compiler", "-ui-engine"].map(|tail| format!("menger{tail}")).to_vec()
}
fn old_names() -> Vec<String> {
    ["-ui-compiler", "-ui-engine", "_ui_compiler", "_ui_engine"].map(|tail| format!("menger{tail}")).to_vec()
}

#[test]
fn the_moved_crates_are_not_here() {
    let root = root();
    let left: Vec<&str> = GONE.iter().copied().filter(|p| root.join(p).exists()).collect();
    assert!(left.is_empty(), "FRAME-1: {} path(s) that went to ../plexus-ui are still here:\n  {}", left.len(), left.join("\n  "));
}

/// Each package of `cargo metadata --format-version 1`'s answer: its name and
/// its `manifest_path`.
fn packages(metadata: &str) -> Vec<(String, PathBuf)> {
    let v: serde_json::Value = serde_json::from_str(metadata).expect("cargo metadata answers JSON");
    v["packages"]
        .as_array()
        .expect("cargo metadata has packages")
        .iter()
        .map(|p| {
            let name = p["name"].as_str().expect("a package has a name").to_string();
            let manifest = p["manifest_path"].as_str().expect("a package has a manifest_path");
            (name, PathBuf::from(manifest))
        })
        .collect()
}

/// A path made canonical where it exists (this machine's `/home` is
/// `/var/home`), else as given.
fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// What the graph `pkgs` breaks of the rule: every crate of `siblings`
/// present, each from under `framework`, and no package named in `old`.
fn breaks(pkgs: &[(String, PathBuf)], framework: &Path, siblings: &[&str], old: &[String]) -> Vec<String> {
    let framework = canonical(framework);
    let mut out = Vec::new();
    for want in siblings {
        let found: Vec<&PathBuf> = pkgs.iter().filter(|(n, _)| n == want).map(|(_, m)| m).collect();
        if found.is_empty() {
            out.push(format!("{want}: not in the graph"));
        }
        for m in found {
            if !canonical(m).starts_with(&framework) {
                out.push(format!("{want}: from {}, not from {}", m.display(), framework.display()));
            }
        }
    }
    for (name, m) in pkgs {
        if old.contains(name) {
            out.push(format!("{name}: an old name, still in the graph ({})", m.display()));
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
    let framework = framework();
    assert!(framework.join("Cargo.toml").is_file(), "FRAME-1: no framework checkout at {} (clone plexus-ui beside menger)", framework.display());

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .current_dir(&root)
        .output()
        .expect("cargo metadata runs");
    assert!(out.status.success(), "cargo metadata --locked --offline failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let broken = breaks(&packages(&String::from_utf8_lossy(&out.stdout)), &framework, SIBLINGS, &old_packages());
    assert!(broken.is_empty(), "FRAME-1: the framework is not taken from ../plexus-ui:\n  {}", broken.join("\n  "));

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

/// The moved directories `line` names as paths: `ui-compiler/` or
/// `ui-engine/` at the start or after a character that is not `-` or a word
/// character (`(^|[^-\w])ui-(compiler|engine)/`).
fn moved_paths(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for dir in ["ui-compiler/", "ui-engine/"] {
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
    for want in ["scripts/plexus-ui", "deploy/pipeline", "tests/support/mod.rs", "Cargo.toml", "Cargo.lock", "README.md", "AGENTS.md"] {
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

/// The checks check: the parser and the rules, on what they must and must
/// not see.
#[test]
fn the_checks_read_what_they_are_given() {
    let fixture = r#"{"packages":[
        {"name":"menger","version":"0.6.2","manifest_path":"/w/menger/Cargo.toml"},
        {"name":"plexus-ui-compiler","manifest_path":"/w/plexus-ui/compiler/Cargo.toml"},
        {"name":"menger-ui-service","manifest_path":"/w/menger/ui-service/Cargo.toml"},
        {"name":"OLD_ENGINE","manifest_path":"/w/menger/ui-engine/Cargo.toml"}
    ],"workspace_members":[]}"#;
    let old_engine = &old_packages()[1];
    let pkgs = packages(&fixture.replace("OLD_ENGINE", old_engine));
    assert_eq!(pkgs.len(), 4);
    assert_eq!(pkgs[1], ("plexus-ui-compiler".to_string(), PathBuf::from("/w/plexus-ui/compiler/Cargo.toml")));

    let framework = Path::new("/w/plexus-ui");
    let broken = breaks(&pkgs, framework, SIBLINGS, &old_packages());
    assert_eq!(broken, [format!("{old_engine}: an old name, still in the graph (/w/menger/ui-engine/Cargo.toml)")]);
    assert!(breaks(&pkgs[..3], framework, SIBLINGS, &old_packages()).is_empty(), "ui-service stays until FRAME-5");

    // Missing is named; a near sibling, a copy in menger and the registry's are not the framework.
    assert_eq!(breaks(&pkgs[..1], framework, SIBLINGS, &old_packages()), ["plexus-ui-compiler: not in the graph"]);
    for elsewhere in [
        "/w/plexus-ui-old/compiler/Cargo.toml",
        "/w/menger/compiler/Cargo.toml",
        "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/plexus-ui-compiler-0.1.0/Cargo.toml",
    ] {
        let one = [("plexus-ui-compiler".to_string(), PathBuf::from(elsewhere))];
        assert_eq!(breaks(&one, framework, SIBLINGS, &old_packages()).len(), 1, "{elsewhere} taken for the framework");
    }

    // The tool's build line.
    let script = "x\n  cargo build --quiet --manifest-path \"$root/../plexus-ui/Cargo.toml\" -p plexus-ui-cli --target-dir \"$target\"\n";
    assert_eq!(tool_build_line(script).map(|(n, _)| n), Some(2));
    assert!(tool_build_line("cargo build -p plexus-ui-cli\n").is_none());
    assert!(tool_build_line("cargo build --manifest-path ../plexus-ui/Cargo.toml -p plexus-ui-compiler\n").is_none());

    // The moved paths: as a path, never inside a longer name.
    assert_eq!(moved_paths("cat ui-compiler/tests/golden/x.svelte"), ["ui-compiler/"]);
    assert_eq!(moved_paths("\"$root/ui-engine/Cargo.toml\""), ["ui-engine/"]);
    assert_eq!(moved_paths("#   engine.js  (ui-engine/, abi engine/1)"), ["ui-engine/"]);
    assert!(moved_paths("../plexus-ui/compiler/ and dist/ui-compiler.wasm").is_empty());
    assert!(moved_paths("plexus-ui-compiler/ build-ui-compiler/ my_ui-engine/").is_empty());
}
