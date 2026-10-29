//! FRAME-4: `plexus-shell` and `plexus-login` are repositories of their own
//! beside plexus-core, and menger takes them by sibling path (owner,
//! 2026-10-29: "it ships with menger, which I really don't want. I want
//! this to be its own plexus service.").
//!
//! Held here: neither directory is in menger's tree and no manifest names
//! one under the root (a member or a `path`, by file and line; `tether/`
//! apart — its `plexus-login` moves with substrate's, FRAME-9), cargo resolves each
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

/// Every `Cargo.toml` under `dir` but build output, git's, dependencies,
/// the lifted directories themselves (the first check names them) and
/// `tether/` (its `plexus-login` moves with substrate's, FRAME-9).
fn manifests(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let skipped = matches!(name.as_str(), "target" | ".git" | "node_modules")
                || (dir == root && (name == "tether" || LIFTED.iter().any(|(_, d)| *d == name)));
            if !skipped {
                manifests(root, &path, out);
            }
        } else if name == "Cargo.toml" {
            out.push(path);
        }
    }
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

/// The places in menger's manifests that name a lifted directory under
/// the root: "file:line: value".
fn leftovers(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    manifests(root, root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let dir = file.parent().unwrap();
        for (line, place) in named_places(dir, &text) {
            if LIFTED.iter().any(|(_, d)| place.starts_with(root.join(d))) {
                let rel = file.strip_prefix(root).unwrap().display();
                out.push(format!("{rel}:{line}: names {} (under menger's root)", place.display()));
            }
        }
    }
    out
}

#[test]
fn the_lifted_crates_are_not_here() {
    let root = root();
    let mut here: Vec<String> = LIFTED
        .iter()
        .map(|(_, dir)| root.join(dir))
        .filter(|p| p.exists())
        .map(|p| format!("{} is in the tree", p.display()))
        .collect();
    here.extend(leftovers(&root));
    assert!(
        here.is_empty(),
        "still in menger's tree (FRAME-4: each is a repository of its own beside plexus-core):\n{}",
        here.join("\n")
    );
}

#[test]
fn the_manifest_reader_finds_members_and_paths() {
    let dir = Path::new("/m");
    let text = "[workspace]\n# members = [\"commented\"]\nmembers = [\".\", \"a\",\n  \"plexus-shell/toy\"]\n\
                [dependencies]\nx = { path = \"plexus-login\", version = \"1\" }\n\
                y = { path = \"../plexus-shell\" }\nz = { git = \"g\", subpath = \"q\" }\n";
    let got = named_places(dir, text);
    let want: Vec<(usize, PathBuf)> = vec![
        (3, "/m".into()),
        (3, "/m/a".into()),
        (4, "/m/plexus-shell/toy".into()),
        (6, "/m/plexus-login".into()),
        (7, "/plexus-shell".into()),
    ];
    assert_eq!(got, want);
    // And on the real tree it reads menger's own members.
    let root = root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(named_places(&root, &text).iter().any(|(_, p)| *p == root.join("tickets-api")));
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
