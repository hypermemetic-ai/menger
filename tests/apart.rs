//! FRAME-1's acceptance 2, held line by line as FRAME-2's waves land: the
//! stored-UI framework lives apart from menger. menger's tree holds no copy
//! of it, its workspace takes the framework's crates by sibling path (MFE-10's
//! way: `../plexus-ui`), and nothing in menger names the crates by their old
//! names.
//!
//! FRAME-3 (wave 1) moves `ui-compiler/` and `ui-engine/` out whole, as
//! `plexus-ui-compiler` and `plexus-ui-engine`. FRAME-4 and FRAME-5 add their
//! paths to [`GONE`] and their crates to [`SIBLINGS`].

use std::path::{Path, PathBuf};

/// menger's root: this package's manifest directory.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The paths that went, from menger's root — each a file or a directory.
const GONE: &[&str] = &[
    // FRAME-3: the compiler and the engine, now ../plexus-ui's.
    "ui-compiler",
    "ui-engine",
];

/// The framework's packages menger's graph takes, each from `../plexus-ui`.
const SIBLINGS: &[&str] = &["plexus-ui-compiler", "plexus-ui-engine"];

/// The packages the graph may no longer hold: the crates' old names (the
/// brief's "no `menger-ui-*`" holds once FRAME-5 adds `ui-service`'s here;
/// until then menger's `ui-service` keeps its name). Spelled in two halves so
/// this file does not name them itself.
fn old_packages() -> Vec<String> {
    ["-ui-compiler", "-ui-engine"].map(|tail| format!("menger{tail}")).to_vec()
}

/// The old names, as written in a manifest and as written in Rust. Spelled
/// in two halves so this file does not name them itself.
fn old_names() -> [String; 4] {
    ["-ui-compiler", "-ui-engine", "_ui_compiler", "_ui_engine"].map(|tail| format!("menger{tail}"))
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

#[test]
fn the_framework_is_a_sibling() {
    let root = root();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .expect("cargo metadata runs");
    assert!(out.status.success(), "cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let framework = root.parent().expect("menger has a parent").join("plexus-ui");
    let broken = breaks(&packages(&String::from_utf8_lossy(&out.stdout)), &framework, SIBLINGS, &old_packages());
    assert!(broken.is_empty(), "FRAME-1: the framework is not taken from ../plexus-ui:\n  {}", broken.join("\n  "));
}

/// The check checks: the parser and the rule, on a graph written here.
#[test]
fn the_metadata_check_reads_a_graph() {
    let fixture = r#"{"packages":[
        {"name":"menger","version":"0.6.2","manifest_path":"/w/menger/Cargo.toml"},
        {"name":"plexus-ui-compiler","manifest_path":"/w/plexus-ui/compiler/Cargo.toml"},
        {"name":"plexus-ui-engine","manifest_path":"/w/menger/ui-engine/Cargo.toml"},
        {"name":"menger-ui-service","manifest_path":"/w/menger/ui-service/Cargo.toml"},
        {"name":"OLD_ENGINE","manifest_path":"/w/menger/ui-engine/Cargo.toml"}
    ],"workspace_members":[]}"#;
    let old_engine = &old_packages()[1];
    let pkgs = packages(&fixture.replace("OLD_ENGINE", old_engine));
    assert_eq!(pkgs.len(), 5);
    assert_eq!(pkgs[1], ("plexus-ui-compiler".to_string(), PathBuf::from("/w/plexus-ui/compiler/Cargo.toml")));

    let framework = Path::new("/w/plexus-ui");
    let broken = breaks(&pkgs, framework, SIBLINGS, &old_packages());
    assert_eq!(broken.len(), 2, "{broken:?}");
    assert!(broken[0].starts_with("plexus-ui-engine: from /w/menger/ui-engine"), "{broken:?}");
    assert!(broken[1].starts_with(&format!("{old_engine}: an old name")), "{broken:?}");
    assert!(!broken.iter().any(|b| b.starts_with("menger-ui-service")), "ui-service stays until FRAME-5: {broken:?}");

    // A missing crate is named; a sibling of a similar name is not the framework.
    let broken = breaks(&pkgs[..1], framework, SIBLINGS, &old_packages());
    assert_eq!(broken, ["plexus-ui-compiler: not in the graph", "plexus-ui-engine: not in the graph"]);
    let near = [("plexus-ui-compiler".to_string(), PathBuf::from("/w/plexus-ui-old/compiler/Cargo.toml"))];
    assert_eq!(breaks(&near, framework, &["plexus-ui-compiler"], &old_packages()).len(), 1);

    // The graph as the rule wants it.
    let good = [
        ("menger".to_string(), PathBuf::from("/w/menger/Cargo.toml")),
        ("plexus-ui-compiler".to_string(), PathBuf::from("/w/plexus-ui/compiler/Cargo.toml")),
        ("plexus-ui-engine".to_string(), PathBuf::from("/w/plexus-ui/engine/Cargo.toml")),
    ];
    assert!(breaks(&good, framework, SIBLINGS, &old_packages()).is_empty());
}

/// The files the name check reads: every file under `scripts/`, `deploy/`
/// (but `deploy/tests/fixtures`) and `tests/`, and the top-level
/// `Cargo.toml`, `README.md`, `AGENTS.md`.
fn named_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path == skip || name.to_string_lossy().starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if path.is_dir() {
                walk(&path, skip, out);
            } else {
                out.push(path);
            }
        }
    }
    let skip = root.join("deploy/tests/fixtures");
    let mut out = Vec::new();
    for dir in ["scripts", "deploy", "tests"] {
        walk(&root.join(dir), &skip, &mut out);
    }
    out.extend(["Cargo.toml", "README.md", "AGENTS.md"].map(|f| root.join(f)));
    out
}

#[test]
fn nothing_names_the_old_crates() {
    let root = root();
    let files = named_files(&root);
    assert!(files.iter().any(|f| f.ends_with("scripts/ui-build.sh")), "the walk lost scripts/");
    assert!(files.iter().any(|f| f.ends_with("deploy/pipeline")), "the walk lost deploy/");
    assert!(!files.iter().any(|f| f.starts_with(root.join("deploy/tests/fixtures"))), "the walk read deploy/tests/fixtures");
    let mut named = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read(file) else { continue };
        let text = String::from_utf8_lossy(&text);
        for (n, line) in text.lines().enumerate() {
            for old in &old_names() {
                if line.contains(old.as_str()) {
                    named.push(format!("{}:{}: {old}", file.strip_prefix(&root).unwrap().display(), n + 1));
                }
            }
        }
    }
    assert!(named.is_empty(), "FRAME-1: {} line(s) still name the crates by their old names:\n  {}", named.len(), named.join("\n  "));
}
