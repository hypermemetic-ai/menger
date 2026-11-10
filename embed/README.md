# embed/

What the `menger` binary carries (DIST-19). `build.rs` looks here for
`ui.tar` — the UI's bundle, as `plexus-ui build` writes it — and, finding
it, includes its bytes; at start menger makes that bundle live where nobody
else has chosen one (`src/embedded_ui.rs`).

`ui.tar` is a build product and is never committed. `scripts/embed-ui` puts
it here, before a `cargo publish -p menger --allow-dirty` or a build meant
to be handed to someone; a checkout without it builds a menger that carries no UI.
