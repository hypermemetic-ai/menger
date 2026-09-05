//! The app's entire data plane: `POST /rest/tickets/{method}` against its
//! own origin (src/shell.rs reverse-proxies `/rest` to the REST port —
//! the REST surface emits no CORS headers, so the app stays same-origin).
//!
//! `watch` streams as SSE **over POST**, which the browser `EventSource`
//! API (GET-only) cannot consume. MFE-2's pinned fallback applies: the
//! stream is read with `fetch` + `ReadableStream`, parsing SSE frames
//! (`event: data` / `data: {...}`, blank-line terminated) by hand. Same
//! observable behavior as EventSource, minus its auto-reconnect — the
//! session loop in main.rs owns reconnection instead.

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, ReadableStreamDefaultReader, RequestInit, Response};

fn describe(v: JsValue) -> String {
    v.as_string()
        .or_else(|| js_sys::Error::from(v).message().as_string())
        .unwrap_or_else(|| "opaque JS error".into())
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no window — this app is browser-only (CSR)")
}

async fn post(path: &str, body: &serde_json::Value) -> Result<Response, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(&body.to_string()));
    let headers = Headers::new().map_err(describe)?;
    headers
        .set("content-type", "application/json")
        .map_err(describe)?;
    init.set_headers(&headers);
    let resp = JsFuture::from(window().fetch_with_str_and_init(path, &init))
        .await
        .map_err(describe)?;
    let resp: Response = resp
        .dyn_into()
        .map_err(|_| "fetch yielded a non-Response")?;
    if !resp.ok() {
        return Err(format!("{path} -> HTTP {}", resp.status()));
    }
    Ok(resp)
}

/// Unary call: `POST /rest/tickets/{method}`, returns the events in the
/// response's `data` array. In-band `{"type":"error"}` events ride along
/// inside the 2xx — callers decide how to surface them.
pub async fn call(
    method: &str,
    params: &serde_json::Value,
) -> Result<Vec<serde_json::Value>, String> {
    let path = format!("/rest/tickets/{method}");
    let resp = post(&path, params).await?;
    let text = JsFuture::from(resp.text().map_err(describe)?)
        .await
        .map_err(describe)?
        .as_string()
        .unwrap_or_default();
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{path}: bad JSON: {e}"))?;
    match v.get("data").and_then(|d| d.as_array()) {
        Some(events) => Ok(events.clone()),
        None => Err(format!("{path}: response has no data array")),
    }
}

/// An open `tickets.watch` SSE stream.
pub struct Watch {
    reader: ReadableStreamDefaultReader,
    buf: Vec<u8>,
}

/// Opens the stream; resolves once response headers are in (the server
/// commits to SSE before any event exists).
pub async fn open_watch(params: &serde_json::Value) -> Result<Watch, String> {
    let resp = post("/rest/tickets/watch", params).await?;
    let body = resp.body().ok_or("watch response has no body stream")?;
    let reader: ReadableStreamDefaultReader = body
        .get_reader()
        .dyn_into()
        .map_err(|_| "watch body has no default reader")?;
    Ok(Watch {
        reader,
        buf: Vec::new(),
    })
}

impl Watch {
    /// Next event's `content` JSON (the activation event inside the
    /// PlexusStreamItem). Err means the stream died — closed, network
    /// dropped, server restarted — and the caller should reconnect.
    pub async fn next_event(&mut self) -> Result<serde_json::Value, String> {
        loop {
            // Drain complete frames (blank-line terminated) already buffered.
            while let Some(end) = find_subslice(&self.buf, b"\n\n") {
                let frame: Vec<u8> = self.buf.drain(..end + 2).collect();
                if let Some(content) = parse_frame(&frame) {
                    return Ok(content);
                }
            }
            // Need more bytes. read() resolves {done, value: Uint8Array}.
            let chunk = JsFuture::from(self.reader.read()).await.map_err(describe)?;
            let done = Reflect::get(&chunk, &"done".into())
                .ok()
                .and_then(|d| d.as_bool())
                .unwrap_or(true);
            if done {
                return Err("watch stream closed by server".into());
            }
            let value = Reflect::get(&chunk, &"value".into()).map_err(describe)?;
            let arr = Uint8Array::new(&value);
            let start = self.buf.len();
            self.buf.resize(start + arr.length() as usize, 0);
            arr.copy_to(&mut self.buf[start..]);
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// One SSE frame -> the `content` field of its `data:` line's JSON.
/// Frames without a data line (comments, keepalives) yield None.
fn parse_frame(frame: &[u8]) -> Option<serde_json::Value> {
    let text = String::from_utf8_lossy(frame);
    let payload = text
        .lines()
        .find_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))?;
    let item: serde_json::Value = serde_json::from_str(payload.trim()).ok()?;
    item.get("content").cloned()
}

/// setTimeout as a future — the reconnect backoff.
pub async fn sleep_ms(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        window()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("setTimeout");
    });
    let _ = JsFuture::from(p).await;
}
