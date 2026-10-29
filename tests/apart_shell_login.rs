//! FRAME-4: `plexus-shell` and `plexus-login` are repositories of their own
//! beside plexus-core, and menger takes them by sibling path (owner,
//! 2026-10-29: "it ships with menger, which I really don't want. I want
//! this to be its own plexus service.").
//!
//! Held here: neither directory is in menger's tree, cargo resolves each
//! crate from the sibling checkout (`../plexus-shell`, `../plexus-login`),
//! and `tickets-api/tests/sizes.rs` holds no file of theirs to a budget.
//! FRAME-3's `tests/apart.rs` holds the compiler and the engine the same
//! way; FRAME-5 folds the two files into one.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The two crates that left, by package name and sibling directory.
const LIFTED: &[(&str, &str)] = &[("plexus-shell", "plexus-shell"), ("plexus-login", "plexus-login")];

/// menger's root (this package's manifest directory), canonical.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().expect("menger's root")
}

/// Where a sibling checkout lives: beside menger's root, whether or not it
/// is there yet (so not canonicalized itself — its parent is).
fn sibling(dir: &str) -> PathBuf {
    root().parent().expect("menger's root has a parent").join(dir)
}

/// Each package `cargo metadata` names, as (name, manifest directory).
/// The directory is the manifest's parent, canonicalized when it exists.
fn packages(metadata: &Value) -> Vec<(String, PathBuf)> {
    metadata["packages"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .filter_map(|p| {
                    let name = p["name"].as_str()?;
                    let manifest = Path::new(p["manifest_path"].as_str()?);
                    let dir = manifest.parent()?;
                    Some((name.to_string(), dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The directories `name` resolves from (more than one would be two
/// packages of one name — cargo refuses that, but say it if it happens).
fn dirs_of(pkgs: &[(String, PathBuf)], name: &str) -> Vec<PathBuf> {
    pkgs.iter().filter(|(n, _)| n == name).map(|(_, d)| d.clone()).collect()
}

/// `cargo metadata` of menger's workspace, dependencies included (a
/// sibling path dependency is not a member, so `--no-deps` would not list it).
fn metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
        .arg(root().join("Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    assert!(out.status.success(), "cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("cargo metadata answers JSON")
}

#[test]
fn the_lifted_crates_are_not_here() {
    let root = root();
    let here: Vec<String> = LIFTED
        .iter()
        .map(|(_, dir)| root.join(dir))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        here.is_empty(),
        "still in menger's tree (FRAME-4: each is a repository of its own beside plexus-core): {}",
        here.join(", ")
    );
}

#[test]
fn the_shell_and_the_login_are_siblings() {
    let pkgs = packages(&metadata());
    let mut wrong = Vec::new();
    for (name, dir) in LIFTED {
        let want = sibling(dir);
        let got = dirs_of(&pkgs, name);
        if got != [want.clone()] {
            let got: Vec<String> = got.iter().map(|d| d.display().to_string()).collect();
            wrong.push(format!("{name}: resolved from [{}], wanted {} (the sibling checkout)", got.join(", "), want.display()));
        }
    }
    assert!(wrong.is_empty(), "\n{}\n", wrong.join("\n"));
}

#[test]
fn sizes_holds_no_file_of_theirs() {
    let path = root().join("tickets-api/tests/sizes.rs");
    let text = std::fs::read_to_string(&path).expect("tickets-api/tests/sizes.rs");
    let named: Vec<String> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| LIFTED.iter().any(|(_, dir)| l.contains(&format!("\"{dir}/"))))
        .map(|(i, l)| format!("tickets-api/tests/sizes.rs:{}: {}", i + 1, l.trim()))
        .collect();
    assert!(named.is_empty(), "a budget for a file that left with its crate (it goes with the directory):\n{}", named.join("\n"));
}

#[test]
fn the_metadata_reader_finds_a_package_where_its_manifest_is() {
    let root = root();
    let side = sibling("plexus-shell");
    let inside = root.join("plexus-login");
    let doc = serde_json::json!({
        "packages": [
            { "name": "plexus-shell", "manifest_path": side.join("Cargo.toml") },
            { "name": "plexus-login", "manifest_path": inside.join("Cargo.toml") },
            { "name": "menger", "manifest_path": root.join("Cargo.toml") },
            { "name": "nameless" },
        ]
    });
    let pkgs = packages(&doc);
    assert_eq!(pkgs.len(), 3, "a package with no manifest path is skipped: {pkgs:?}");
    assert_eq!(dirs_of(&pkgs, "plexus-shell"), [side.clone()]);
    assert_eq!(dirs_of(&pkgs, "menger"), [root.clone()]);
    // In the tree is not beside it: the sibling test would refuse this one.
    assert_ne!(dirs_of(&pkgs, "plexus-login"), [sibling("plexus-login")]);
    assert!(dirs_of(&pkgs, "plexus-ui").is_empty());
    // And the reader reads the real thing: menger itself is where it is.
    assert_eq!(dirs_of(&packages(&metadata()), "menger"), [root]);
}
