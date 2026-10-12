//! The app's build (app/: two files) embedded with `embedded-app` — the
//! line every app using plexus-shell's embedding writes.
fn main() {
    plexus_shell::build::embed_dir_with_feature("app", "embedded-app");
}
