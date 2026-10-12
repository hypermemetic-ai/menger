# plexus-shell

A Plexus RPC app's frontend, served beside its hub — and the whole server
started as a library call.

Lifted from [menger](https://github.com/hypermemetic-ai/menger), which
consumes it.

- **The shell.** The frontend's build (a `trunk build` or `vite build`
  output) is read into a table and served at `/` and under `/app/`. A
  request path is a key into the table, never a path on disk. The
  entrypoint is stamped as it is read: `{{WS_PORT}}` (the hub's WebSocket
  port), `{{TITLE}}`, and any placeholder the app adds. The build comes from
  an `AppSource`: `Dir(path)` (read again when `index.html` changes) or
  `Embedded(table)` (compiled into the binary).
- **The embedding.** The app crate's build script writes the table; the
  app's code includes it.
- **`LocalServer`.** Any `DynamicHub` on a Plexus WebSocket, plus the shell,
  on loopback. Port 0 picks a free port. Both listeners are bound before
  anything serves. Shut it down to release the ports. `TAURI_ORIGINS` are
  the origins to admit when the page opens in a Tauri webview.

## Use

```toml
[features]
embedded-app = []

[dependencies]
plexus-shell = "0.1"

[build-dependencies]
plexus-shell = { version = "0.1", default-features = false, features = ["build"] }
```

```rust
// build.rs: embed frontend/dist when `embedded-app` is on
fn main() {
    plexus_shell::build::embed_dir_with_feature("frontend/dist", "embedded-app");
}
```

```rust
use plexus_shell::{AppSource, EmbeddedTable, LocalConfig, LocalServer, ShellConfig};

static APP: EmbeddedTable = plexus_shell::embedded_app!();

async fn run(hub: std::sync::Arc<plexus_core::plexus::DynamicHub>) -> anyhow::Result<()> {
    // the embedded build when there is one, else the checkout's dist
    let app = AppSource::embedded_or(APP, concat!(env!("CARGO_MANIFEST_DIR"), "/frontend/dist"));
    let server = LocalServer::start(hub, LocalConfig::new(ShellConfig::new("myapp", app))).await?;
    println!("{} over {}", server.shell_url(), server.ws_url());
    server.shutdown().await;
    Ok(())
}
```

To use your own listener, routes or layers, call `router(cfg, ws_port)` and
`serve_on(listener, router, shutdown)`. `ShellConfig` carries the extras:
`routes` (merged in), `pages` (stamped HTML at a route), `stamps`,
`root_fallback` (served at `/` when there is no build), `not_built`, and
`mount` (default `/app`).

## Features

| feature | gives | dependencies |
|---|---|---|
| `build` | `build::embed_dir*` for a build script | none (always compiled) |
| `serve` | `AppSource`, `ShellConfig`, `router`, `serve_on` | axum, tokio |
| `local` (default) | `LocalServer`, `LocalConfig`, `rpc_module` | + plexus-core, plexus-transport, jsonrpsee |

A worked example lives in the repository at `plexus-shell/toy/`. It is a hub
with one activation and an embedded two-file app, and its test runs it.

MIT.
