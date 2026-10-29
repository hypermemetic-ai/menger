//! UI-6: **the wasm entry** — what `index.html` calls once `engine.js` has
//! loaded `engine.wasm`: `boot(version)`. It reads the stamped socket port
//! (`<meta name="menger-ws-port">`), opens the page's one WebSocket (signed
//! in by the session's cookie), reads `/app/<version>/manifest.json`,
//! fetches the route's components, mounts them into `document.body`, and
//! listens: the hash (routing), `ui.watch` (a push, `slot_changed`), the
//! socket (a reconnect on a 1 s loop). Behind the `dom` feature.

use wasm_bindgen::prelude::*;

/// Boots the engine on the page for the version `version` (its hash, as
/// `index.html` was served for it).
#[wasm_bindgen]
pub fn boot(version: String) {
    let _ = version;
    web_sys::console::error_1(&"UI-6: the engine is a stub".into());
}
