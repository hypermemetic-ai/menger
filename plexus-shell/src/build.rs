//! The build-script half: an app's build directory compiled into its
//! binary (menger's DIST-3 `build.rs`, lifted).
//!
//! `include_bytes!` resolves in the crate that compiles it, so the table
//! is written by the **app's** build script into the app's `OUT_DIR`, and
//! included by the app's code with [`crate::embedded_app!`]:
//!
//! ```text
//! // Cargo.toml
//! [build-dependencies]
//! plexus-shell = { version = "0.1", default-features = false, features = ["build"] }
//!
//! // build.rs
//! fn main() {
//!     plexus_shell::build::embed_dir_with_feature("frontend/dist", "embedded-app");
//! }
//! ```
//!
//! The walk is the shell's own: regular files only — a symlink is not
//! followed — keyed by their '/'-joined path relative to the directory, and
//! a directory with no entrypoint (`index.html`) is no build: nothing is
//! embedded, with a cargo warning. Every function here always writes the
//! file, so [`crate::embedded_app!`] always has a table to include.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Embeds `dir` (relative to the crate's `CARGO_MANIFEST_DIR`, or absolute)
/// and returns how many files it embedded. Asks cargo to rerun when
/// anything under it changes, or — when it is missing — on every build, so
/// a frontend build after the first compile is seen.
pub fn embed_dir(dir: impl AsRef<Path>) -> usize {
    let dist = manifest_dir().join(dir.as_ref());
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = Vec::new();
    if dist.join("index.html").is_file() {
        walk(&dist, "", &mut files);
        files.sort();
    } else {
        println!(
            "cargo:warning=plexus-shell: no app build at {} (no index.html); embedding none — build the frontend first",
            dist.display()
        );
    }
    write(&files);
    files.len()
}

/// [`embed_dir`] when the crate's `feature` is on (`embedded-app` →
/// `CARGO_FEATURE_EMBEDDED_APP`), else [`embed_nothing`]. Returns how many
/// files it embedded.
pub fn embed_dir_with_feature(dir: impl AsRef<Path>, feature: &str) -> usize {
    println!("cargo:rerun-if-changed=build.rs");
    let var = format!("CARGO_FEATURE_{}", feature.to_ascii_uppercase().replace('-', "_"));
    if std::env::var_os(var).is_some() {
        embed_dir(dir)
    } else {
        embed_nothing();
        0
    }
}

/// Writes an empty table: the app has no embedded build.
pub fn embed_nothing() {
    write(&[]);
}

/// The table's source: an [`crate::EmbeddedTable`] expression over
/// `include_bytes!` of each file's absolute path.
pub fn table_source(files: &[(String, PathBuf)]) -> String {
    let mut src = String::from("{\n    const TABLE: &[(&str, &[u8])] = &[\n");
    for (key, path) in files {
        writeln!(src, "        ({key:?}, include_bytes!({:?})),", path.display().to_string()).unwrap();
    }
    src.push_str("    ];\n    TABLE\n}\n");
    src
}

/// The regular files under `dir`, keyed by their '/'-joined relative path;
/// a symlink is not followed.
pub fn walk(dir: &Path, prefix: &str, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let (Ok(kind), Some(name)) = (entry.file_type(), entry.file_name().to_str().map(str::to_string)) else {
            continue;
        };
        let key = format!("{prefix}{name}");
        if kind.is_dir() {
            walk(&entry.path(), &format!("{key}/"), files);
        } else if kind.is_file() {
            files.push((key, entry.path()));
        }
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR for a build script"))
}

fn write(files: &[(String, PathBuf)]) {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR for a build script")).join(crate::EMBEDDED_FILE);
    std::fs::write(&out, table_source(files)).unwrap_or_else(|e| panic!("plexus-shell: cannot write {}: {e}", out.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walk_takes_regular_files_by_relative_key() {
        let root = tempfile::tempdir().unwrap();
        let dist = root.path().join("dist");
        std::fs::create_dir_all(dist.join("vendor")).unwrap();
        std::fs::write(dist.join("index.html"), "i").unwrap();
        std::fs::write(dist.join("vendor/m.js"), "m").unwrap();
        std::fs::write(root.path().join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(root.path().join("secret"), dist.join("link")).unwrap();
        let mut files = Vec::new();
        walk(&dist, "", &mut files);
        files.sort();
        let keys: Vec<_> = files.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["index.html", "vendor/m.js"], "a symlink is not followed");
        let src = table_source(&files);
        assert!(src.contains("(\"vendor/m.js\", include_bytes!("), "{src}");
        assert!(table_source(&[]).contains("const TABLE: &[(&str, &[u8])] = &[\n    ];"));
    }
}
