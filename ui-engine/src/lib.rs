//! UI-6: **the engine** — the host interface `engine/1` of UI-4's contract.
//!
//! The engine owns what lives across swaps: the socket, the cache, the
//! signals, the registry. A component owns only its tree and its folds; it
//! never touches the DOM, the socket or another component.
//!
//! | module | what it holds |
//! |---|---|
//! | [`signals`] | a signal runtime: [`signals::Signal`], bindings re-run only when a signal they read changes |
//! | [`tree`] | the compiled view tree (UI-4's schema), its interpreter over a [`tree::Renderer`]; the recording renderer (native tests) and, with `dom`, the DOM one |
//! | [`live`] | `live()`: a collection `{ rows, status }` folded from a snapshot and a changes stream, shared by method and params, kept 5 s past its last reader |
//! | [`folds`] | a component's handlers in Rhai — `(state, event) → state`, only `call` and `navigate` registered, `call` held to the component's `uses` |
//! | [`swap`] | the bundle's manifest, the module registry (name → hashes), mounting, hot-swap, and the sections (slots and their assignments) |
//! | [`router`] | `manifest.routes`: a hash route to a component, its `:name` segments the props |
//! | [`transport`] | the wire seam ([`transport::Wire`]): the one socket on wasm32, a fake for the tests |
//!
//! The wasm entry (`boot`) is [`entry`], behind the `dom` feature.

pub mod signals;
pub mod tree;
pub mod live;
pub mod folds;
pub mod swap;
pub mod router;
pub mod transport;

#[cfg(all(target_arch = "wasm32", feature = "dom"))]
pub mod entry;

/// The host interface this engine implements; a manifest's `abi`.
pub const ABI: &str = "engine/1";
