//! DIST-3: the app's build, embedded.
//!
//! With the `embedded-app` feature, `frontend/dist` (the Leptos app's
//! `trunk build`) is compiled into the binary as a table of
//! `(path relative to dist, bytes)`, which `src/shell.rs` takes with
//! `plexus_shell::embedded_app!()` — so a menger with no checkout beside it
//! serves the app. Without the feature, or with no build in dist, the table
//! is empty and the shell reads dist from disk as it always has.
//!
//! SCAFFOLD-5: the walk and the table are plexus-shell's (its `build`
//! feature): regular files only — a symlink is not followed — keyed by
//! their '/'-joined path, and a dist with no entrypoint (`index.html`) is
//! no build.

fn main() {
    plexus_shell::build::embed_dir_with_feature("frontend/dist", "embedded-app");
}
