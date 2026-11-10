//! DIST-19: **the UI the binary carries.** When the package holds a bundle —
//! `embed/ui.tar` (what `scripts/embed-ui` puts there, and what the published
//! crate ships), or the file `MENGER_UI_BUNDLE` names — its path is handed to
//! the crate (`MENGER_EMBEDDED_UI`, under `cfg(menger_embedded_ui)`) and
//! `src/embedded_ui.rs` includes its bytes. With neither, nothing is carried
//! and the build is what it was: no Node, no wasm toolchain, no bundle.

use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(menger_embedded_ui)");
    println!("cargo:rerun-if-env-changed=MENGER_UI_BUNDLE");
    // The directory is the checkout's (embed/README.md), so cargo has a path
    // that exists to watch whether or not a bundle is in it.
    println!("cargo:rerun-if-changed=embed");

    let named = std::env::var_os("MENGER_UI_BUNDLE").filter(|v| !v.is_empty()).map(PathBuf::from);
    let bundle = match named {
        // A file that is named is carried, or the build says why not.
        Some(path) => path,
        None => {
            let beside = PathBuf::from("embed/ui.tar");
            if !beside.is_file() {
                return;
            }
            beside
        }
    };
    let bundle = bundle
        .canonicalize()
        .unwrap_or_else(|e| panic!("MENGER_UI_BUNDLE {}: {e} — name a bundle `plexus-ui build` wrote, or unset it", bundle.display()));
    let len = std::fs::metadata(&bundle).map(|m| m.len()).unwrap_or(0);
    assert!(len > 0, "{} is empty: no bundle to carry", bundle.display());
    println!("cargo:rerun-if-changed={}", bundle.display());
    println!("cargo:rustc-env=MENGER_EMBEDDED_UI={}", bundle.display());
    println!("cargo:rustc-cfg=menger_embedded_ui");
}
