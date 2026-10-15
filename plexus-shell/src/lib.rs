//! plexus-shell — a Plexus RPC app's frontend, served beside its hub.
//!
//! Lifted from menger (DIST-3, SCAFFOLD-5). Three pieces:
//!
//! - **The shell** (`serve`): the app's build — a `trunk build` or `vite
//!   build` output — read into a table and served at `/` and under `/app/`.
//!   A request path is a key into the table, never a path on disk, so
//!   there is no path to sanitise (menger's ACCESS-33). The entrypoint
//!   (`index.html`) is stamped as it is read: `{{WS_PORT}}` with the hub's
//!   WebSocket port, `{{TITLE}}` with the tab's title, and any placeholder
//!   the app adds ([`ShellConfig::stamps`]). The table comes from an
//!   [`AppSource`]: a directory on disk (read again when its entrypoint
//!   changes, so a rebuild needs no restart) or a table compiled into the
//!   binary. The app adds its own routes ([`ShellConfig::routes`]) and
//!   stamped pages ([`ShellConfig::pages`]); [`router`] is the composed
//!   router and [`serve_on`] serves one, hardened. Every page it serves
//!   goes out under a Content-Security-Policy ([`ShellConfig::csp`],
//!   [`DEFAULT_CSP`]): its inline scripts by hash, the wasm, its hub's
//!   socket, nothing that frames it (menger's ACCESS-28).
//! - **The embedding** ([`build`], always compiled, no dependencies): the
//!   app crate's build script calls
//!   [`build::embed_dir`]`("frontend/dist")`, which writes the table into
//!   `OUT_DIR`; the crate's code takes it with [`embedded_app!`] and hands
//!   it to [`AppSource::Embedded`]. The table is the app's: `include_bytes!`
//!   resolves in the crate that expands it, and nothing here reaches for a
//!   global.
//! - **The local server** (`local`, default): [`LocalServer::start`] runs a
//!   hub's WebSocket and the shell on loopback — ports 0 for free ones,
//!   bound before anything serves — and hands back the ports, the URLs and
//!   a shutdown. A Tauri webview's origins are [`TAURI_ORIGINS`].
//!
//! ```text
//! // build.rs — embed frontend/dist when the `embedded-app` feature is on
//! fn main() { plexus_shell::build::embed_dir_with_feature("frontend/dist", "embedded-app"); }
//!
//! // src/lib.rs
//! static APP: plexus_shell::EmbeddedTable = plexus_shell::embedded_app!();
//! let shell = plexus_shell::ShellConfig::new("myapp", plexus_shell::AppSource::Embedded(APP));
//! let server = plexus_shell::LocalServer::start(hub, plexus_shell::LocalConfig::new(shell)).await?;
//! println!("{} over {}", server.shell_url(), server.ws_url());
//! ```

pub mod build;

#[cfg(feature = "serve")]
mod app;
#[cfg(feature = "serve")]
mod csp;
#[cfg(feature = "serve")]
mod shell;
#[cfg(feature = "local")]
mod local;

#[cfg(feature = "serve")]
pub use app::{has_entrypoint, mime_of, stamp, AppSource};
#[cfg(feature = "serve")]
pub use csp::{hub_sources, page_policy, script_hashes, DEFAULT_CSP};
#[cfg(feature = "serve")]
pub use shell::{harden, not_built_page, router, serve_on, ShellConfig};
#[cfg(feature = "local")]
pub use local::{log_activations, loopback_origins, rpc_module, LocalConfig, LocalServer};

/// An app's build compiled into its binary: `(path relative to the build
/// directory, bytes)`, as [`build::embed_dir`] writes it and
/// [`embedded_app!`] includes it. Empty when nothing was embedded.
pub type EmbeddedTable = &'static [(&'static str, &'static [u8])];

/// The origins a Tauri 2 webview loads its page from (custom protocol on
/// macOS/Linux, `http(s)://tauri.localhost` on Windows and Android) — for
/// [`LocalConfig::extra_origins`] when the shell's page is opened in one.
pub const TAURI_ORIGINS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];

/// The file [`build::embed_dir`] writes into `OUT_DIR`, which
/// [`embedded_app!`] includes.
pub const EMBEDDED_FILE: &str = "plexus_shell_app.rs";

/// The app's build as the crate's build script embedded it — an
/// [`EmbeddedTable`] expression. Expands in the calling crate, so its
/// `OUT_DIR` is the app's: the build script must have called one of
/// [`build`]'s `embed_*` functions (each always writes the file, an empty
/// table when there is nothing to embed).
///
/// ```text
/// static APP: plexus_shell::EmbeddedTable = plexus_shell::embedded_app!();
/// ```
#[macro_export]
macro_rules! embedded_app {
    () => {
        include!(concat!(env!("OUT_DIR"), "/plexus_shell_app.rs"))
    };
}
