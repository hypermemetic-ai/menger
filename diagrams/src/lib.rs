//! `diagrams` — mermaid, parsed by the bundle the app serves, over Plexus
//! RPC (COMPILE-4).
//!
//! A diagram is checked today only when someone opens its ticket page, and
//! the failure is drawn there and recorded nowhere (COMPILE-1). mermaid is
//! JavaScript and menger is not, so this is its own service: a Plexus hub,
//! `diagrams`, with one activation, `mermaid`, whose answers come from a Node
//! program running the vendored `mermaid-11.17.2.min.js` in a jsdom window
//! (COMPILE-3: the browser's verdict on all 53 live diagrams). Its own
//! process, so menger never shares an address space — or a failure — with a
//! JavaScript engine, and runs the same whether or not this is up.
//!
//! | Consumer | Features | Gets |
//! |---|---|---|
//! | `menger-diagrams` (the service binary) | *(default: `server`)* | [`types`], the activation [`Mermaid`] with its real bodies, [`node`] (the child process it owns), [`blocks`] (the app's rule for what a mermaid block is), [`build_hub`] — and [`MermaidClient`] |
//! | a client — menger, an agent's tool, a browser | `default-features = false` | [`types`] and [`MermaidClient`], over wasm-safe crates only |
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use menger_diagrams::{MermaidClient, MermaidEvent};
//! use plexus_client::WsTransport;
//!
//! let transport = WsTransport::connect("ws://127.0.0.1:4458", "diagrams").await?;
//! let client = MermaidClient::new(&transport);
//! for event in client.check_all("```mermaid\ngraph LR\n  A --> B\n```\n".into()).await? {
//!     if let MermaidEvent::Diagram { index, line, result } = event {
//!         println!("diagram {index} (line {line}): {result:?}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod types;
pub use types::{MermaidEvent, Verdict};

pub mod activation;
#[cfg(feature = "server")]
pub use activation::Mermaid;
pub use activation::MermaidClient;

/// The mermaid blocks of a markdown body, by the app's rule.
#[cfg(feature = "server")]
pub mod blocks;

/// The Node program, as a child process the service owns.
#[cfg(feature = "server")]
pub mod node;

/// The hub's name: `synapse -P 4458 diagrams mermaid parse --source …`.
pub const HUB: &str = "diagrams";

/// The service's WebSocket port (inside synapse's 4440–4459 scan range,
/// beside menger's 4455–4457).
pub const DEFAULT_PORT: u16 = 4458;

/// Builds the `diagrams` hub around one Node driver. Nothing starts here:
/// the program is started by the first call.
#[cfg(feature = "server")]
pub fn build_hub(driver: node::Driver) -> std::sync::Arc<plexus_core::plexus::DynamicHub> {
    std::sync::Arc::new(plexus_core::plexus::DynamicHub::new(HUB).register(Mermaid::new(driver)))
}
