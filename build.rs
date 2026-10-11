//! DIST-3: the app's build, embedded.
//!
//! With the `embedded-app` feature, `frontend/dist` (the Leptos app's
//! `trunk build`) is compiled into the binary as a table of
//! `(path relative to dist, bytes)` — `$OUT_DIR/embedded_app.rs`, which
//! `src/shell.rs` includes — so a menger with no checkout beside it serves
//! the app. Without the feature, or with no build in dist, the table is
//! empty and the shell reads dist from disk as it always has.
//!
//! The walk is the shell's own (ACCESS-33's `AppFiles::load`): regular
//! files only — a symlink is not followed — keyed by their '/'-joined path,
//! and a dist with no entrypoint (`index.html`) is no build.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("embedded_app.rs");
    let mut files = Vec::new();
    if std::env::var_os("CARGO_FEATURE_EMBEDDED_APP").is_some() {
        let dist = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("frontend/dist");
        // A directory: cargo rescans everything under it. Missing, it reruns
        // every build — so a `trunk build` after the first compile is seen.
        println!("cargo:rerun-if-changed={}", dist.display());
        if dist.join("index.html").is_file() {
            walk(&dist, "", &mut files);
            files.sort();
        } else {
            println!(
                "cargo:warning=embedded-app: no app build at {} (run `trunk build --release` in frontend/); \
                 embedding none — the shell serves the vanilla page",
                dist.display()
            );
        }
    }
    let mut src = String::from(
        "/// DIST-3: the app's build, embedded by build.rs (`embedded-app`): \
         (path relative to frontend/dist, bytes).\n\
         pub(crate) static EMBEDDED_APP: &[(&str, &[u8])] = &[\n",
    );
    for (key, path) in &files {
        writeln!(src, "    ({key:?}, include_bytes!({:?})),", path.display().to_string()).unwrap();
    }
    src.push_str("];\n");
    std::fs::write(&out, src).expect("write embedded_app.rs");
}

fn walk(dir: &Path, prefix: &str, files: &mut Vec<(String, PathBuf)>) {
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
