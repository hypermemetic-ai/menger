//! COMMENT-5: images in comments — attached in every box that writes one,
//! drawn in every comment that shows one.
//!
//! **Attaching** ([`Attach`], [`Attachments`]): a screenshot pasted into the
//! box, a file dropped on it, or one picked with its Image button is shown
//! at once as a thumbnail of the file with a spinner (CHAT-30; a static
//! "uploading…" under Motion none or calm), then read,
//! checked here by the server's own rule (`menger_tickets_api::media::check`:
//! PNG, JPEG, GIF or WebP by its bytes, never SVG, at most 7 MiB — a file
//! refused says why and is never sent), and kept with `media.put` for the
//! ticket the comment is on (none for a comment on the app). Its markdown,
//! `![name](media:<hash>)`, goes into the box at the cursor, and the box
//! shows it as a thumbnail — the thumbnails are the draft's own `media:`
//! images, so deleting the markdown removes one, and × removes both. One
//! that fails says why on its thumbnail, with *remove* (and *retry*, where
//! trying again can help); the box's send waits while one is on its way or
//! failed ([`waits`]), so a message never goes without its image.
//!
//! **Drawing** ([`hydrate`]): a comment's `media:` image is an `img.cimg`
//! with no `src` (src/md.rs, [`segments`]); the page asks `media.get` for
//! its bytes — once per image per page load — and sets a `blob:` URL (the
//! CSP allows `blob:` and `data:` images; nothing else is drawn). An image
//! the reader may not see is `not_found` and stays its alt. A click opens it
//! full size ([`open_full`]): fitted, scrolled and zoomed (APP-77); ×, Esc
//! or a tap on the backdrop closes it.
//!
//! COMMENT-10: **a ticket body's images** ([`hydrate_body`], [`held`]) are
//! `img.bimg`s, asked for held to the body's ticket (`media.get { hash,
//! ticket }`: only an image kept for that ticket — one kept for another, a
//! chat or the app is `not_found` there, whoever reads). The page fills an
//! unframed body's itself; a framed body's SDK asks the host
//! (`menger:media`, src/frame.rs), which answers with the bytes from the
//! same copy, held to the ticket stamped on the frame. One not found is
//! its alt and a "not available" mark ([`missing`]). A tap or click opens
//! one full screen (a frame's through `menger:zoom`, [`zoom`]).
//!
//! What decides — the segments of a comment, the draft's images, where an
//! image's markdown goes, the name it takes, which ticket an image is for —
//! is pure and pinned by native tests; the rest is the DOM.

use std::cell::RefCell;
use std::collections::HashMap;

use leptos::html::Textarea;
use leptos::prelude::*;
use menger_tickets_api::media as rules;
use menger_tickets_api::{Anchor, MediaEvent};

/// What the picker offers.
pub(crate) const ACCEPT: &str = "image/png,image/jpeg,image/gif,image/webp";

/// A stretch of a comment's text: words, or an image written in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seg {
    Text(String),
    /// `![alt](media:<hash>)` — drawn from `media.get`.
    Media { alt: String, hash: String },
    /// `![alt](data:image/…)` — drawn as it is.
    Data { alt: String, url: String },
    /// `![alt](https://…)` — not drawn: its alt, linked.
    Remote { alt: String, url: String },
}

/// Whether `url` is an image a comment draws as written.
fn is_data_image(url: &str) -> bool {
    let d = url.trim().to_ascii_lowercase();
    ["png", "jpeg", "gif", "webp"].iter().any(|t| d.starts_with(&format!("data:image/{t};")) || d.starts_with(&format!("data:image/{t},")))
}

/// One `![alt](dest)` at the start of `s`: the segment and its length.
fn image_at(s: &str) -> Option<(Seg, usize)> {
    let rest = s.strip_prefix("![")?;
    let close = rest.find(']')?;
    let alt = &rest[..close];
    if alt.contains('\n') {
        return None;
    }
    let after = rest[close + 1..].strip_prefix('(')?;
    let end = after.find(')')?;
    let dest = after[..end].trim();
    if dest.is_empty() || dest.contains(char::is_whitespace) {
        return None;
    }
    let len = 2 + close + 1 + 1 + end + 1;
    let alt = alt.to_string();
    let seg = if let Some(hash) = rules::hash_of(dest) {
        Seg::Media { alt, hash }
    } else if is_data_image(dest) {
        Seg::Data { alt, url: dest.to_string() }
    } else if dest.starts_with("https://") || dest.starts_with("http://") {
        Seg::Remote { alt, url: dest.to_string() }
    } else {
        return None;
    };
    Some((seg, len))
}

/// `text` cut into its words and the images written in it (a malformed or
/// unknown one stays words).
pub(crate) fn segments(text: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    while let Some(i) = text[at..].find("![") {
        let start = at + i;
        match image_at(&text[start..]) {
            Some((seg, len)) => {
                plain.push_str(&text[at..start]);
                if !plain.is_empty() {
                    out.push(Seg::Text(std::mem::take(&mut plain)));
                }
                out.push(seg);
                at = start + len;
            }
            None => {
                plain.push_str(&text[at..start + 2]);
                at = start + 2;
            }
        }
    }
    plain.push_str(&text[at..]);
    if !plain.is_empty() {
        out.push(Seg::Text(plain));
    }
    out
}

/// The draft's `media:` images, each once, in order: (alt, hash).
pub(crate) fn refs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for s in segments(text) {
        if let Seg::Media { alt, hash } = s {
            if !out.iter().any(|(_, h)| *h == hash) {
                out.push((alt, hash));
            }
        }
    }
    out
}

/// The name an image takes in its markdown: the file's, without its
/// extension (a pasted screenshot's generic `image.png` is "screenshot"),
/// and nothing that would end its brackets.
pub(crate) fn alt_of(file_name: &str) -> String {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let clean: String = stem.chars().map(|c| if matches!(c, '[' | ']' | '(' | ')' | '\n' | '\r') { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    match clean.as_str() {
        "" | "image" | "blob" => "screenshot".to_string(),
        _ => clean,
    }
}

/// An image's markdown.
pub(crate) fn markdown(alt: &str, hash: &str) -> String {
    format!("![{alt}](media:{hash})")
}

/// `text` with `snippet` put at `at` (a UTF-16 offset, as a textarea
/// counts) on its own — a space before it unless at a line's start, one
/// after unless a line ends there — and the offset just past it.
pub(crate) fn insert(text: &str, at_utf16: usize, snippet: &str) -> (String, usize) {
    let mut byte = text.len();
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= at_utf16 {
            byte = i;
            break;
        }
        units += c.len_utf16();
    }
    let (before, after) = text.split_at(byte);
    let lead = if before.is_empty() || before.ends_with(char::is_whitespace) { "" } else { " " };
    let trail = if after.starts_with(char::is_whitespace) { "" } else { " " };
    let put = format!("{lead}{snippet}{trail}");
    let cursor = before.encode_utf16().count() + put.encode_utf16().count();
    (format!("{before}{put}{after}"), cursor)
}

/// `text` without the image `hash` (every time it is written), and the
/// space it leaves tidied.
pub(crate) fn remove(text: &str, hash: &str) -> String {
    let mut out = String::new();
    for s in segments(text) {
        match s {
            Seg::Media { hash: h, .. } if h == hash => {
                if out.ends_with(' ') {
                    out.pop();
                }
            }
            Seg::Text(t) => out.push_str(&t),
            Seg::Media { alt, hash } => out.push_str(&markdown(&alt, &hash)),
            Seg::Data { alt, url } | Seg::Remote { alt, url } => out.push_str(&format!("![{alt}]({url})")),
        }
    }
    out
}

/// The ticket an image for a comment at `anchor` is put for: the element's,
/// else the page's (`#/ticket/X`); none on the app.
pub(crate) fn ticket_of(anchor: &Anchor) -> Option<String> {
    anchor.ticket.clone().filter(|t| !t.trim().is_empty()).or_else(|| {
        let rest = anchor.route.trim().strip_prefix("#/ticket/")?;
        let id = rest.split(['?', '#']).next().unwrap_or(rest).trim();
        (!id.is_empty()).then(|| id.to_string())
    })
}

/// Why a file is not sent — the server's rule, said before anything goes.
pub(crate) fn refusal(name: &str, declared: &str, bytes: &[u8]) -> Result<rules::Sniffed, String> {
    let declared = if declared.trim().is_empty() {
        // No type given (some drops): what its bytes say, or the bytes' refusal.
        rules::sniff(bytes).ok().flatten().map(|s| s.mime).unwrap_or("application/octet-stream")
    } else {
        declared
    };
    rules::check(declared, bytes).map_err(|p| format!("{name}: {p}"))
}

// ── the bytes, as the page holds them ───────────────────────────────────

/// What the page knows of an image.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Known {
    Pending,
    Url(String),
    /// Not there, or not the reader's.
    Missing,
}

thread_local! {
    /// Each image the page has asked for or put: its `blob:` URL once in hand.
    static KNOWN: ArcRwSignal<HashMap<String, Known>> = ArcRwSignal::new(HashMap::new());
    /// The comment images drawn before their bytes came: filled when they do.
    static WAITING: RefCell<HashMap<String, Vec<web_sys::Element>>> = RefCell::new(HashMap::new());
    /// Whether the click that opens an image full size is listened for.
    static LISTENING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn known() -> ArcRwSignal<HashMap<String, Known>> {
    KNOWN.with(Clone::clone)
}

/// The image `hash`'s URL, tracked — asked for the first time it is wanted.
pub(crate) fn url(hash: &str) -> Option<String> {
    let k = known().with(|m| m.get(hash).cloned());
    match k {
        Some(Known::Url(u)) => Some(u),
        Some(_) => None,
        None => {
            fetch(hash.to_string());
            None
        }
    }
}

/// A `blob:` URL of `bytes`, typed `mime`.
fn blob_url(bytes: &[u8], mime: &str) -> Option<String> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

/// The image `hash` is known: set it, and fill what waited for it.
fn settle(hash: &str, k: Known) {
    known().update(|m| {
        m.insert(hash.to_string(), k.clone());
    });
    let waiting = WAITING.with(|w| w.borrow_mut().remove(hash)).unwrap_or_default();
    for el in waiting {
        fill(&el, &k);
    }
}

fn fill(el: &web_sys::Element, k: &Known) {
    match k {
        Known::Url(u) => {
            let _ = el.set_attribute("src", u);
            let _ = el.set_attribute("data-state", "drawn");
        }
        Known::Missing => {
            let _ = el.set_attribute("data-state", "missing");
        }
        Known::Pending => {
            let _ = el.set_attribute("data-state", "pending");
        }
    }
}

/// `media.get { hash }`, once: its bytes as a `blob:` URL.
fn fetch(hash: String) {
    if known().with_untracked(|m| m.contains_key(&hash)) {
        return;
    }
    known().update(|m| {
        m.insert(hash.clone(), Known::Pending);
    });
    wasm_bindgen_futures::spawn_local(async move {
        let got = async {
            let client = crate::session::media_client().ok_or_else(|| "not connected".to_string())?;
            let h: menger_tickets_api::vocab::MediaHash = crate::typed::typed(&hash)?;
            let events = client.get_all(h, None).await.map_err(|e| crate::session::describe(&e))?;
            for ev in events {
                if let MediaEvent::Media { mime, data, .. } = ev {
                    use base64::Engine;
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|e| e.to_string())?;
                    return blob_url(&bytes, &mime).ok_or_else(|| "no blob".to_string());
                }
            }
            Err("not found".to_string())
        }
        .await;
        settle(&hash, got.map(Known::Url).unwrap_or(Known::Missing));
    });
}

/// COMMENT-5: every `img[data-media]` under `root` drawn: from the page's
/// copy, or asked for and filled when it comes. And the click that opens one
/// full size listened for.
pub(crate) fn hydrate(root: &web_sys::Element) {
    listen();
    let Ok(nodes) = root.query_selector_all("img[data-media]") else { return };
    for i in 0..nodes.length() {
        let Some(el) = nodes.get(i).and_then(|n| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(n).ok()) else { continue };
        let Some(hash) = el.get_attribute("data-media") else { continue };
        if el.get_attribute("src").is_some_and(|s| !s.is_empty()) {
            continue;
        }
        let k = known().with_untracked(|m| m.get(&hash).cloned());
        match k {
            Some(k @ (Known::Url(_) | Known::Missing)) => fill(&el, &k),
            _ => {
                fill(&el, &Known::Pending);
                WAITING.with(|w| w.borrow_mut().entry(hash.clone()).or_default().push(el.clone()));
                fetch(hash);
            }
        }
    }
}

/// The click on a drawn comment image (or a box's thumbnail) that opens it
/// full size — listened for once, on the document.
fn listen() {
    if LISTENING.with(|l| l.replace(true)) {
        return;
    }
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let Some(doc) = crate::dom::window().document() else { return };
    let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        // AGS-28: a plain click, as a link is followed (not a middle click).
        let click = crate::bindings::hit_mouse(&ev, crate::bindings::Context::Link) == Some("link.follow");
        if ev.default_prevented() || !click || crate::bindings::is_middle(&ev) {
            return;
        }
        let Some(img) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).filter(|el| el.matches("img.cimg, img.bimg, .cthumb img").unwrap_or(false)) else {
            return;
        };
        let Some(src) = img.get_attribute("src").filter(|s| !s.is_empty()) else { return };
        ev.prevent_default();
        ev.stop_propagation();
        open_full(&src, &img.get_attribute("alt").unwrap_or_default());
    });
    let _ = doc.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    cb.forget();
    // COMMENT-10: a tap on a touch screen gives an image no click (WebKit
    // synthesizes none for an element that is not a control), so the tap is
    // read here: one finger, lifted where it landed and soon ([`is_tap`]).
    // The lift is prevented, so no click follows to open it again.
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    opts.set_passive(true);
    let start = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        let img = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).filter(|el| el.matches("img.cimg[src], img.bimg[src], .cthumb img[src]").unwrap_or(false));
        let at = ev.touches().get(0).map(|t| (f64::from(t.client_x()), f64::from(t.client_y())));
        TAP.with(|t| *t.borrow_mut() = img.zip(at).filter(|_| ev.touches().length() == 1).map(|(el, (x, y))| (el, x, y, js_sys::Date::now())));
    });
    let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("touchstart", start.as_ref().unchecked_ref(), &opts);
    start.forget();
    let moved = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        let here = ev.touches().get(0).filter(|_| ev.touches().length() == 1).map(|t| (f64::from(t.client_x()), f64::from(t.client_y())));
        TAP.with(|t| {
            let mut t = t.borrow_mut();
            if t.as_ref().is_some_and(|(_, x0, y0, at)| here.is_none_or(|(x, y)| !is_tap(*x0, *y0, *at, x, y, *at))) {
                *t = None;
            }
        });
    });
    let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("touchmove", moved.as_ref().unchecked_ref(), &opts);
    moved.forget();
    let end = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
        let Some((img, x0, y0, at)) = TAP.with(|t| t.borrow_mut().take()) else { return };
        let lift = ev.changed_touches().get(0).map(|t| (f64::from(t.client_x()), f64::from(t.client_y())));
        let Some((x, y)) = lift else { return };
        if ev.default_prevented() || !is_tap(x0, y0, at, x, y, js_sys::Date::now()) {
            return;
        }
        let Some(src) = img.get_attribute("src").filter(|s| !s.is_empty()) else { return };
        ev.prevent_default();
        open_full(&src, &img.get_attribute("alt").unwrap_or_default());
    });
    let end_opts = web_sys::AddEventListenerOptions::new();
    end_opts.set_capture(true);
    end_opts.set_passive(false);
    let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("touchend", end.as_ref().unchecked_ref(), &end_opts);
    end.forget();
}

thread_local! {
    /// The touch on a drawn image under way: the image, where and when it
    /// landed.
    static TAP: RefCell<Option<(web_sys::Element, f64, f64, f64)>> = const { RefCell::new(None) };
}

/// Whether a touch that landed at `(x0, y0)` at `at0` and lifted at `(x,
/// y)` at `at` is a tap: within 10px and 500ms (a hold or a scroll is not).
pub(crate) fn is_tap(x0: f64, y0: f64, at0: f64, x: f64, y: f64, at: f64) -> bool {
    (x - x0).hypot(y - y0) <= 10.0 && at - at0 <= 500.0
}

/// APP-77: the width an image opens at — inside the screen, as it always
/// was, unless that draws it a sliver: a tall one drawn under half the
/// screen's width fits the width and scrolls down; a wide one under a
/// quarter of its height (a landscape photo on a phone is neither) fits the
/// height and scrolls across. Never past its own size.
pub(crate) fn fit_width(nw: f64, nh: f64, vw: f64, vh: f64) -> f64 {
    if nw <= 0.0 || nh <= 0.0 || vw <= 0.0 || vh <= 0.0 {
        return vw.max(1.0);
    }
    let pad = if vw > 4.0 * FIT_PAD && vh > 4.0 * FIT_PAD { FIT_PAD } else { 0.0 };
    let contain = nw.min(vw - 2.0 * pad).min((vh - 2.0 * pad) * nw / nh);
    if contain < vw / 2.0 && nh / nw > vh / vw {
        nw.min(vw)
    } else if contain * nh / nw < vh / 4.0 && nw / nh > vw / vh {
        nw.min(vh * nw / nh)
    } else {
        contain
    }
}

/// The room left round an image that fits inside the screen.
const FIT_PAD: f64 = 16.0;

/// The zoom's range, in multiples of the fit.
pub(crate) const ZOOM_MAX: f64 = 8.0;

/// A double-tap's zoom from the fit.
const ZOOM_TAP: f64 = 2.5;

/// A button's step.
const ZOOM_STEP: f64 = 1.5;

/// A zoom held to its range.
pub(crate) fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() { z.clamp(1.0, ZOOM_MAX) } else { 1.0 }
}

/// Where a scroller's offset goes when what it scrolls scales from `z0` to
/// `z1` about a point `at` px into its view, so the point under the
/// fingers (or the tap) stays where it is.
pub(crate) fn anchored(scroll: f64, at: f64, z0: f64, z1: f64) -> f64 {
    if z0 <= 0.0 {
        return scroll;
    }
    ((scroll + at) * z1 / z0 - at).max(0.0)
}

/// Whether a tap at `(x, y)` at `at` is the second of a double-tap after
/// one at `(x0, y0)` at `at0`: within 300ms and 30px.
pub(crate) fn is_double(x0: f64, y0: f64, at0: f64, x: f64, y: f64, at: f64) -> bool {
    (x - x0).hypot(y - y0) <= 30.0 && at - at0 <= 300.0 && at >= at0
}

/// The viewer's zoom: the fit's width, the zoom, a pinch under way (its
/// fingers' first distance and the zoom then), the last tap.
#[derive(Default)]
struct View {
    fit: f64,
    z: f64,
    pinch: Option<(f64, f64)>,
    tap: Option<(f64, f64, f64)>,
    start: Option<(f64, f64, f64)>,
}

/// The image at `src`, full size, over the page (APP-77): fitted
/// ([`fit_width`]), its own scroller, zoomed by a pinch, a double-tap or
/// double-click, `=` / `-` / `0` or the bar's buttons; × , Esc or a tap on
/// the backdrop closes it (a tap on the image never does — it is touched to
/// pan).
pub(crate) fn open_full(src: &str, alt: &str) {
    use std::rc::Rc;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let Some(doc) = crate::dom::window().document() else { return };
    let mk = |tag: &str| doc.create_element(tag).ok();
    let (Some(wrap), Some(bar), Some(scroller), Some(img)) = (mk("div"), mk("div"), mk("div"), mk("img")) else { return };
    let _ = wrap.set_attribute("class", "lightbox");
    let _ = wrap.set_attribute("role", "dialog");
    let _ = wrap.set_attribute("aria-label", if alt.is_empty() { "image" } else { alt });
    let _ = wrap.set_attribute("tabindex", "-1");
    let _ = bar.set_attribute("class", "lb-bar");
    for (z, label, text) in [("out", "zoom out", "−"), ("fit", "fit to the screen", "fit"), ("in", "zoom in", "+"), ("close", "close", "×")] {
        let Some(b) = mk("button") else { return };
        let _ = b.set_attribute("type", "button");
        let _ = b.set_attribute("data-z", z);
        let _ = b.set_attribute("aria-label", label);
        let _ = b.set_attribute("title", label);
        b.set_text_content(Some(text));
        let _ = bar.append_child(&b);
    }
    let _ = scroller.set_attribute("class", "lb-scroll");
    let _ = scroller.set_attribute("tabindex", "-1");
    let _ = img.set_attribute("src", src);
    let _ = img.set_attribute("alt", alt);
    let _ = img.set_attribute("draggable", "false");
    let _ = scroller.append_child(&img);
    let _ = wrap.append_child(&scroller);
    let _ = wrap.append_child(&bar);
    let Some(body) = doc.body() else { return };
    let _ = body.append_child(&wrap);

    let Ok(sc) = scroller.clone().dyn_into::<web_sys::HtmlElement>() else { return };
    let Ok(im) = img.clone().dyn_into::<web_sys::HtmlElement>() else { return };
    let view = Rc::new(RefCell::new(View { z: 1.0, ..View::default() }));
    // The zoom to `z`, about `(ax, ay)` in the scroller's view.
    let zoom: Rc<dyn Fn(f64, f64, f64)> = {
        let (sc, im, view) = (sc.clone(), im.clone(), view.clone());
        Rc::new(move |z: f64, ax: f64, ay: f64| {
            let (z0, fit) = {
                let v = view.borrow();
                (v.z, v.fit)
            };
            let z1 = clamp_zoom(z);
            let (x0, y0) = (sc.scroll_left() as f64, sc.scroll_top() as f64);
            let _ = im.style().set_property("width", &format!("{:.1}px", fit * z1));
            view.borrow_mut().z = z1;
            sc.set_scroll_left(anchored(x0, ax, z0, z1).round() as i32);
            sc.set_scroll_top(anchored(y0, ay, z0, z1).round() as i32);
        })
    };
    // The middle of the view, for a zoom with no point.
    let middle = {
        let sc = sc.clone();
        move || (sc.client_width() as f64 / 2.0, sc.client_height() as f64 / 2.0)
    };
    // Into the view's coordinates.
    let at = {
        let sc = sc.clone();
        move |x: f64, y: f64| {
            let r = sc.get_bounding_client_rect();
            (x - r.left(), y - r.top())
        }
    };
    // The fit, once the image's size is known (and again on a resize).
    let refit = {
        let (sc, im, img, view) = (sc.clone(), im.clone(), img.clone(), view.clone());
        Rc::new(move || {
            let n = |k: &str| js_sys::Reflect::get(&img, &k.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
            let fit = fit_width(n("naturalWidth"), n("naturalHeight"), sc.client_width() as f64, sc.client_height() as f64);
            let z = {
                let mut v = view.borrow_mut();
                v.fit = fit;
                v.z
            };
            let _ = im.style().set_property("width", &format!("{:.1}px", fit * z));
        })
    };
    if js_sys::Reflect::get(&img, &"complete".into()).ok().and_then(|v| v.as_bool()).unwrap_or(false) {
        refit();
    }
    let on_load = {
        let refit = refit.clone();
        Closure::<dyn FnMut()>::new(move || refit())
    };
    let _ = img.add_event_listener_with_callback("load", on_load.as_ref().unchecked_ref());
    on_load.forget();

    let close = {
        let w = wrap.clone();
        Rc::new(move || w.remove())
    };
    // Clicks: a button's, the backdrop's (closes); the image's does nothing.
    let on_click = {
        let (close, zoom, middle) = (close.clone(), zoom.clone(), middle.clone());
        let (view, sc) = (view.clone(), sc.clone());
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
            let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
            ev.stop_propagation();
            let (mx, my) = middle();
            let z = view.borrow().z;
            match t.get_attribute("data-z").as_deref() {
                Some("in") => zoom(z * ZOOM_STEP, mx, my),
                Some("out") => zoom(z / ZOOM_STEP, mx, my),
                Some("fit") => zoom(1.0, mx, my),
                Some("close") => close(),
                _ if t.tag_name().eq_ignore_ascii_case("img") => {}
                _ if t.class_list().contains("lb-bar") => {}
                _ => close(),
            }
            let _ = sc.focus();
        })
    };
    let _ = wrap.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
    on_click.forget();
    // A double-click on the image: the fit ↔ 2.5×, about the pointer.
    let on_dbl = {
        let (zoom, at, view) = (zoom.clone(), at.clone(), view.clone());
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
            ev.prevent_default();
            let (ax, ay) = at(ev.client_x() as f64, ev.client_y() as f64);
            let z = view.borrow().z;
            zoom(if z > 1.01 { 1.0 } else { ZOOM_TAP }, ax, ay);
        })
    };
    let _ = img.add_event_listener_with_callback("dblclick", on_dbl.as_ref().unchecked_ref());
    on_dbl.forget();
    // Keys: Esc closes; = / - / 0 zoom (the table's, AGS-28).
    let on_key = {
        let (close, zoom, middle, view) = (close.clone(), zoom.clone(), middle.clone(), view.clone());
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |k: web_sys::KeyboardEvent| {
            use crate::bindings::{is, Context::Dialog};
            let (mx, my) = middle();
            let z = view.borrow().z;
            if is(&k, Dialog, "dialog.close") {
                close();
            } else if is(&k, Dialog, "image.zoom-in") {
                zoom(z * ZOOM_STEP, mx, my);
            } else if is(&k, Dialog, "image.zoom-out") {
                zoom(z / ZOOM_STEP, mx, my);
            } else if is(&k, Dialog, "image.fit") {
                zoom(1.0, mx, my);
            } else {
                return;
            }
            k.prevent_default();
            k.stop_propagation();
        })
    };
    let _ = wrap.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    // Touch: one finger scrolls natively (touch-action: pan-x pan-y); two
    // pinch, about their middle; a double-tap zooms about the tap.
    let pair = |ev: &web_sys::TouchEvent| {
        let t = ev.touches();
        let (a, b) = (t.get(0)?, t.get(1)?);
        let (ax, ay, bx, by) = (a.client_x() as f64, a.client_y() as f64, b.client_x() as f64, b.client_y() as f64);
        Some(((bx - ax).hypot(by - ay), (ax + bx) / 2.0, (ay + by) / 2.0))
    };
    let passive_off = web_sys::AddEventListenerOptions::new();
    passive_off.set_passive(false);
    let on_start = {
        let view = view.clone();
        Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
            let mut v = view.borrow_mut();
            if let Some((d, _, _)) = pair(&ev) {
                ev.prevent_default();
                v.pinch = Some((d.max(1.0), v.z));
                v.start = None;
            } else if let Some(t) = ev.touches().get(0) {
                v.start = Some((t.client_x() as f64, t.client_y() as f64, js_sys::Date::now()));
            }
        })
    };
    let on_move = {
        let (view, zoom, at) = (view.clone(), zoom.clone(), at.clone());
        Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
            let pinch = view.borrow().pinch;
            let (Some((d0, z0)), Some((d, mx, my))) = (pinch, pair(&ev)) else { return };
            ev.prevent_default();
            let (ax, ay) = at(mx, my);
            zoom(z0 * d / d0, ax, ay);
        })
    };
    let on_end = {
        let (view, zoom, at) = (view.clone(), zoom.clone(), at.clone());
        Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |ev: web_sys::TouchEvent| {
            let mut v = view.borrow_mut();
            if v.pinch.is_some() {
                if ev.touches().length() < 2 {
                    v.pinch = None;
                    v.tap = None;
                }
                return;
            }
            let (Some((x0, y0, t0)), Some(t)) = (v.start.take(), ev.changed_touches().get(0)) else { return };
            let (x, y, now) = (t.client_x() as f64, t.client_y() as f64, js_sys::Date::now());
            if !is_tap(x0, y0, t0, x, y, now) {
                v.tap = None;
                return;
            }
            match v.tap.take() {
                Some((px, py, pt)) if is_double(px, py, pt, x, y, now) => {
                    ev.prevent_default(); // no click, no dblclick after it
                    let z = v.z;
                    drop(v);
                    let (ax, ay) = at(x, y);
                    zoom(if z > 1.01 { 1.0 } else { ZOOM_TAP }, ax, ay);
                }
                _ => v.tap = Some((x, y, now)),
            }
        })
    };
    for (name, f) in [("touchstart", &on_start), ("touchmove", &on_move), ("touchend", &on_end)] {
        let _ = scroller.add_event_listener_with_callback_and_add_event_listener_options(name, f.as_ref().unchecked_ref(), &passive_off);
    }
    on_start.forget();
    on_move.forget();
    on_end.forget();
    // Safari's own pinch would zoom the page under the viewer.
    let hold = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| ev.prevent_default());
    for name in ["gesturestart", "gesturechange"] {
        let _ = wrap.add_event_listener_with_callback_and_add_event_listener_options(name, hold.as_ref().unchecked_ref(), &passive_off);
    }
    hold.forget();
    let _ = sc.focus();
}

// ── COMMENT-10: a ticket body's images, held to its ticket ─────────────

/// An image as a body's ticket holds it: its type and bytes (base64, as a
/// frame is handed them), and its `blob:` URL once the page drew it.
pub(crate) struct Got {
    pub(crate) mime: String,
    pub(crate) data: String,
    url: std::cell::OnceCell<Option<String>>,
}

impl Got {
    /// Its `blob:` URL, made the first time it is asked for.
    fn url(&self) -> Option<String> {
        self.url
            .get_or_init(|| {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD.decode(self.data.as_bytes()).ok()?;
                blob_url(&bytes, &self.mime)
            })
            .clone()
    }
}

/// What the page knows of an image in a ticket's body.
enum Held {
    /// Asked for: what to do when it comes.
    Asked(Vec<Box<dyn FnOnce(Option<std::rc::Rc<Got>>)>>),
    Got(Option<std::rc::Rc<Got>>),
}

thread_local! {
    /// Each (ticket, hash) a body has asked for.
    static HELD: RefCell<HashMap<(String, String), Held>> = RefCell::new(HashMap::new());
}

/// Whether `h` is a hash as a frame asks for one: 64 lower-case hex digits.
pub(crate) fn is_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// The key an image of `ticket`'s body is held under.
fn held_key(ticket: &str, hash: &str) -> (String, String) {
    (ticket.trim().to_ascii_uppercase(), hash.trim().to_ascii_lowercase())
}

/// The image `hash` as `ticket`'s body shows it — `media.get { hash, ticket
/// }`, once per page load — handed to `then` (`None`: not found there, or
/// not the reader's).
pub(crate) fn held(ticket: &str, hash: &str, then: impl FnOnce(Option<std::rc::Rc<Got>>) + 'static) {
    let key = held_key(ticket, hash);
    if let Some(g) = held_now(ticket, hash) {
        return then(g);
    }
    let first = HELD.with(|h| {
        let mut h = h.borrow_mut();
        let first = !h.contains_key(&key);
        if let Held::Asked(waiting) = h.entry(key.clone()).or_insert_with(|| Held::Asked(Vec::new())) {
            waiting.push(Box::new(then));
        }
        first
    });
    if first {
        fetch_held(key);
    }
}

/// [`held`]'s answer when it is already in hand: `Some(got)`.
pub(crate) fn held_now(ticket: &str, hash: &str) -> Option<Option<std::rc::Rc<Got>>> {
    HELD.with(|h| match h.borrow().get(&held_key(ticket, hash)) {
        Some(Held::Got(g)) => Some(g.clone()),
        _ => None,
    })
}

fn fetch_held(key: (String, String)) {
    wasm_bindgen_futures::spawn_local(async move {
        let got = async {
            let client = crate::session::media_client()?;
            let h: menger_tickets_api::vocab::MediaHash = crate::typed::typed(&key.1).ok()?;
            let t = crate::typed::typed(&key.0).ok()?;
            let events = client.get_all(h, Some(t)).await.ok()?;
            events.into_iter().find_map(|ev| match ev {
                MediaEvent::Media { mime, data, .. } => Some(std::rc::Rc::new(Got { mime, data, url: std::cell::OnceCell::new() })),
                _ => None,
            })
        }
        .await;
        let waiting = HELD.with(|h| match h.borrow_mut().insert(key, Held::Got(got.clone())) {
            Some(Held::Asked(w)) => w,
            _ => Vec::new(),
        });
        for then in waiting {
            then(got.clone());
        }
    });
}

/// COMMENT-10: every `img.bimg` under `root` — an unframed body of
/// `ticket` — drawn from `media.get` held to that ticket, or, not found
/// there (or no ticket: a body that is no ticket's yet), [`missing`].
pub(crate) fn hydrate_body(root: &web_sys::Element, ticket: Option<&str>) {
    listen();
    let Ok(nodes) = root.query_selector_all("img.bimg[data-media]:not([src])") else { return };
    for i in 0..nodes.length() {
        let Some(el) = nodes.get(i).and_then(|n| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(n).ok()) else { continue };
        let Some(hash) = el.get_attribute("data-media") else { continue };
        let Some(ticket) = ticket else {
            missing(&el);
            continue;
        };
        let _ = el.set_attribute("data-state", "pending");
        let fill = move |g: Option<std::rc::Rc<Got>>| match g.and_then(|g| g.url()) {
            Some(u) => {
                let _ = el.set_attribute("src", &u);
                let _ = el.set_attribute("data-state", "drawn");
            }
            None => missing(&el),
        };
        match held_now(ticket, &hash) {
            Some(g) => fill(g),
            None => held(ticket, &hash, fill),
        }
    }
}

/// What a body's image not found there says beside its alt.
pub(crate) const NOT_AVAILABLE: &str = "not available";

/// COMMENT-10: a body's image that is not its ticket's (or is gone) — its
/// alt, and a mark that says so, in its place.
fn missing(el: &web_sys::Element) {
    let Some(doc) = el.owner_document() else { return };
    let alt = el.get_attribute("alt").unwrap_or_default();
    let (Ok(span), Ok(mark)) = (doc.create_element("span"), doc.create_element("small")) else { return };
    let _ = span.set_attribute("class", "bimg-missing");
    let _ = span.set_attribute("title", "This image is not available here: it is kept for another ticket, or it is gone");
    if let Some(h) = el.get_attribute("data-media") {
        let _ = span.set_attribute("data-media", &h);
    }
    span.set_text_content(Some(alt.trim()));
    mark.set_text_content(Some(NOT_AVAILABLE));
    let _ = span.append_child(&mark);
    let _ = el.replace_with_with_node_1(&span);
}

/// COMMENT-10: a frame's image, tapped — full screen over the app, when
/// the host holds it for `ticket` (the frame's own stamp).
pub(crate) fn zoom(ticket: &str, hash: &str, alt: &str) {
    if let Some(Some(g)) = held_now(ticket, hash) {
        if let Some(u) = g.url() {
            open_full(&u, alt);
        }
    }
}

// ── attaching ───────────────────────────────────────────────────────────

/// CHAT-30: where an image on its way stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpState {
    /// Being read and put: a spinner on its thumbnail.
    Uploading,
    /// Not kept: why, and whether trying again can help (a dropped
    /// connection can; a file the server's rule refuses cannot).
    Failed { why: String, retry: bool },
}

/// CHAT-30: an image on its way, drawn at once as a thumbnail of the file
/// itself, until `media.put` answers `stored` (then it is the draft's
/// markdown, and the draft's thumbnail) or it fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Up {
    pub(crate) key: u32,
    pub(crate) alt: String,
    /// The file's own `blob:` URL — none for a file that is no image.
    pub(crate) url: Option<String>,
    pub(crate) state: UpState,
}

/// Whether a box's send waits: an image is on its way, or one failed and
/// is neither tried again nor removed — a message never goes without it.
pub(crate) fn waits(ups: &[Up]) -> bool {
    !ups.is_empty()
}

/// What the box says of its images on their way, for a screen reader.
pub(crate) fn status_of(ups: &[Up]) -> Option<String> {
    let going = ups.iter().filter(|u| u.state == UpState::Uploading).count();
    let failed = ups.len() - going;
    match (going, failed) {
        (0, 0) => None,
        (1, 0) => Some("uploading an image…".to_string()),
        (n, 0) => Some(format!("uploading {n} images…")),
        (0, 1) => Some("an image was not added".to_string()),
        (0, n) => Some(format!("{n} images were not added")),
        (g, f) => Some(format!("uploading {g}, {f} not added")),
    }
}

/// One box's attaching: its draft, the box, the ticket an image is for, and
/// the images on their way (CHAT-30) with their files, kept to try again.
#[derive(Clone, Copy)]
pub(crate) struct Attach {
    body: RwSignal<String>,
    area: NodeRef<Textarea>,
    ticket: StoredValue<Option<String>>,
    /// CHAT-14: the chat an image is for, in a chat's composer — seen by
    /// its participants alone.
    chat: StoredValue<Option<u64>>,
    ups: RwSignal<Vec<Up>>,
    files: StoredValue<HashMap<u32, web_sys::File>, LocalStorage>,
    next: StoredValue<u32>,
}

impl Attach {
    pub(crate) fn new(body: RwSignal<String>, area: NodeRef<Textarea>, ticket: Option<String>) -> Self {
        Self {
            body,
            area,
            ticket: StoredValue::new(ticket),
            chat: StoredValue::new(None),
            ups: RwSignal::new(Vec::new()),
            files: StoredValue::new_local(HashMap::new()),
            next: StoredValue::new(0),
        }
    }

    /// CHAT-14: a chat composer's attaching — images put for `chat`.
    pub(crate) fn for_chat(body: RwSignal<String>, area: NodeRef<Textarea>, chat: u64) -> Self {
        Self { chat: StoredValue::new(Some(chat)), ..Self::new(body, area, None) }
    }

    /// Whether the box's send waits ([`waits`]): an image on its way, or
    /// one that failed and is still shown.
    pub(crate) fn busy(self) -> bool {
        self.ups.with(|u| waits(u))
    }

    /// [`Attach::busy`], untracked.
    pub(crate) fn busy_untracked(self) -> bool {
        self.ups.with_untracked(|u| waits(u))
    }

    /// Whether one of the images on their way failed (a send then waits for
    /// *retry* or *remove*, not for the upload).
    pub(crate) fn failed(self) -> bool {
        self.ups.with(|u| u.iter().any(|u| u.state != UpState::Uploading))
    }

    /// A paste: its images attached (and the paste itself not typed).
    pub(crate) fn paste(self, ev: web_sys::ClipboardEvent) {
        let files = ev.clipboard_data().map(|d| files_of(d.files())).unwrap_or_default();
        if !files.is_empty() {
            ev.prevent_default();
            self.files(files);
        }
    }

    /// A drag over the box: a file may be dropped.
    pub(crate) fn dragover(self, ev: web_sys::DragEvent) {
        let has_files = ev.data_transfer().is_some_and(|d| d.types().iter().any(|t| t.as_string().as_deref() == Some("Files")));
        if has_files {
            ev.prevent_default();
        }
    }

    /// A drop: its images attached.
    pub(crate) fn dropped(self, ev: web_sys::DragEvent) {
        let files = ev.data_transfer().map(|d| files_of(d.files())).unwrap_or_default();
        if !files.is_empty() {
            ev.prevent_default();
            self.files(files);
        }
    }

    /// Each file shown at once, then read, checked, put, and its markdown
    /// put in the box.
    pub(crate) fn files(self, files: Vec<web_sys::File>) {
        for f in files {
            self.one(f);
        }
    }

    /// CHAT-30: the file's thumbnail with a spinner, in the same turn as
    /// the paste — then its upload.
    fn one(self, file: web_sys::File) {
        let key = self.next.get_value();
        self.next.set_value(key + 1);
        let ty = file.type_();
        let url = (ACCEPT.split(',').any(|a| a == ty)).then(|| web_sys::Url::create_object_url_with_blob(&file).ok()).flatten();
        self.ups.update(|u| u.push(Up { key, alt: alt_of(&file.name()), url, state: UpState::Uploading }));
        self.files.update_value(|f| {
            f.insert(key, file);
        });
        self.go(key);
    }

    /// The upload of the image `key` on its way (again, after a failure).
    fn go(self, key: u32) {
        let Some(file) = self.files.with_value(|f| f.get(&key).cloned()) else { return };
        self.set_state(key, UpState::Uploading);
        wasm_bindgen_futures::spawn_local(async move {
            let name = file.name();
            let r = self.upload(&file, &name).await;
            // Removed while it was on its way: nothing goes in the box.
            if !self.ups.try_with_untracked(|u| u.iter().any(|u| u.key == key)).unwrap_or(false) {
                return;
            }
            match r {
                Ok((alt, hash)) => {
                    // The thumbnail and the comment draw from the file itself.
                    let url = self.ups.with_untracked(|u| u.iter().find(|u| u.key == key).and_then(|u| u.url.clone()));
                    if let Some(u) = url {
                        settle(&hash, Known::Url(u));
                    }
                    self.files.update_value(|f| {
                        f.remove(&key);
                    });
                    self.ups.update(|u| u.retain(|u| u.key != key));
                    self.put_markdown(&alt, &hash);
                }
                Err((why, retry)) => self.set_state(key, UpState::Failed { why, retry }),
            }
        });
    }

    fn set_state(self, key: u32, state: UpState) {
        self.ups.try_update(|u| {
            if let Some(one) = u.iter_mut().find(|u| u.key == key) {
                one.state = state;
            }
        });
    }

    /// *remove* on an image on its way, or one that failed: gone, never sent.
    fn drop_up(self, key: u32) {
        let url = self.ups.with_untracked(|u| u.iter().find(|u| u.key == key).and_then(|u| u.url.clone()));
        self.ups.update(|u| u.retain(|u| u.key != key));
        self.files.update_value(|f| {
            f.remove(&key);
        });
        if let Some(u) = url {
            let _ = web_sys::Url::revoke_object_url(&u);
        }
    }

    /// The file read, checked and put: its alt and hash, or why not and
    /// whether trying again can help — a dropped connection or a server's
    /// error can; the file's own bytes, or a refusal by the rule, cannot.
    async fn upload(self, file: &web_sys::File, name: &str) -> Result<(String, String), (String, bool)> {
        use base64::Engine;
        let fixed = |why: String| (why, false);
        // Past the cap, said before the bytes are read.
        let size = file.size() as usize;
        if size > rules::MAX_BYTES {
            return Err(fixed(format!("{name}: {}", rules::Problem::TooLarge { bytes: size, max: rules::MAX_BYTES })));
        }
        let buf = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await.map_err(|_| fixed(format!("{name}: could not be read")))?;
        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
        let found = refusal(name, &file.type_(), &bytes).map_err(fixed)?;
        let client = crate::session::media_client().ok_or_else(|| ("not connected — the image was not sent".to_string(), true))?;
        let ticket = crate::typed::opt(self.ticket.get_value()).map_err(fixed)?;
        let chat = self.chat.get_value().map(menger_tickets_api::vocab::ChatId::new);
        let mime = crate::typed::typed(found.mime).map_err(fixed)?;
        let data = crate::typed::typed(&base64::engine::general_purpose::STANDARD.encode(&bytes)).map_err(fixed)?;
        let events = client.put_all(ticket, chat, mime, data).await.map_err(|e| (format!("{name}: {}", crate::session::describe(&e)), true))?;
        for ev in events {
            match ev {
                MediaEvent::Stored { hash, .. } => return Ok((alt_of(name), hash)),
                MediaEvent::Refused { message, .. } => return Err(fixed(format!("{name}: {message}"))),
                MediaEvent::Error { message } => return Err((format!("{name}: {message}"), true)),
                MediaEvent::NotFound { ticket, .. } => {
                    return Err(fixed(match self.chat.get_value() {
                        Some(c) => format!("{name}: not kept — chat {c} is not one you are in"),
                        None => format!("{name}: not kept — {} is no ticket you can add images to", ticket.unwrap_or_default()),
                    }))
                }
                MediaEvent::Media { .. } => {}
            }
        }
        Err((format!("{name}: no answer"), true))
    }

    /// The image's markdown into the box at its cursor, the cursor after it.
    fn put_markdown(self, alt: &str, hash: &str) {
        let el = self.area.get_untracked();
        let at = el.as_ref().and_then(|a| a.selection_start().ok().flatten()).map(|n| n as usize);
        let text = self.body.get_untracked();
        let at = at.unwrap_or_else(|| text.encode_utf16().count());
        let (next, cursor) = insert(&text, at, &markdown(alt, hash));
        self.body.set(next.clone());
        if let Some(a) = el {
            a.set_value(&next);
            let _ = a.set_selection_range(cursor as u32, cursor as u32);
            let _ = a.focus();
        }
    }

    /// The image out of the draft.
    fn take_out(self, hash: &str) {
        let next = remove(&self.body.get_untracked(), hash);
        self.body.set(next);
    }
}

fn files_of(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
    let Some(list) = list else { return Vec::new() };
    (0..list.length()).filter_map(|i| list.get(i)).collect()
}

/// A box's images: those on their way (CHAT-30: a spinner, or why one
/// failed with *retry* and *remove*), the draft's thumbnails, the Image
/// button (and its picker).
#[component]
pub(crate) fn Attachments(a: Attach) -> impl IntoView {
    let picker = NodeRef::<leptos::html::Input>::new();
    let going = move || {
        a.ups
            .get()
            .into_iter()
            .map(|up| {
                let key = up.key;
                let (failed, why, retry) = match &up.state {
                    UpState::Uploading => (false, String::new(), false),
                    UpState::Failed { why, retry } => (true, why.clone(), *retry),
                };
                let title = if failed { why.clone() } else { format!("{} — uploading…", up.alt) };
                view! {
                    <span class="cthumb cup" class:failed=failed data-up=key.to_string() title=title>
                        {match up.url.clone() {
                            Some(u) => view! { <img src=u alt=up.alt.clone() /> }.into_any(),
                            None => view! { <span class="cup-name">{up.alt.clone()}</span> }.into_any(),
                        }}
                        {(!failed).then(|| view! {
                            <span class="cup-spin" aria-hidden="true"></span>
                            <span class="cup-still">"uploading…"</span>
                        })}
                        {failed.then(|| view! {
                            <span class="cup-why" role="alert">{why.clone()}</span>
                            <span class="cup-acts">
                                {retry.then(|| view! { <button type="button" class="cup-retry" on:click=move |_| a.go(key)>"retry"</button> })}
                                <button type="button" class="cup-remove" on:click=move |_| a.drop_up(key)>"remove"</button>
                            </span>
                        })}
                        {(!failed).then(|| view! {
                            <button type="button" class="cthumb-x" title="stop and remove the image" aria-label="stop and remove the image" on:click=move |_| a.drop_up(key)>"×"</button>
                        })}
                    </span>
                }
            })
            .collect_view()
    };
    let thumbs = move || {
        a.body
            .with(|b| refs(b))
            .into_iter()
            .map(|(alt, hash)| {
                let (h, x) = (hash.clone(), hash.clone());
                let title = format!("{alt} — click to see it whole");
                view! {
                    <span class="cthumb" data-media-hash=hash.clone()>
                        <img src=move || url(&h) alt=alt.clone() title=title />
                        <button type="button" class="cthumb-x" title="remove the image" aria-label="remove the image" on:click=move |_| a.take_out(&x)>"×"</button>
                    </span>
                }
            })
            .collect_view()
    };
    view! {
        <span class="cattach">
            <span class="cthumbs">{thumbs}{going}</span>
            <button
                type="button"
                class="cpick"
                title="Add an image — or paste a screenshot, or drop a file on the box"
                aria-label="Add an image"
                on:click=move |_| {
                    if let Some(i) = picker.get() {
                        i.click();
                    }
                }
            >
                // CHAT-29: on a phone's composer the icon stands alone beside
                // send; elsewhere the word shows and the icon does not.
                <svg class="cpick-ico" viewBox="0 0 20 20" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round">
                    <rect x="2.5" y="3.5" width="15" height="13" rx="2"></rect>
                    <circle cx="7.5" cy="8" r="1.6"></circle>
                    <path d="M3 15l4.5-4.5 3 3 2.5-2.5 4.5 4.5"></path>
                </svg>
                <span class="cpick-l">"Image"</span>
            </button>
            <input
                type="file"
                class="cfile"
                accept=ACCEPT
                multiple=true
                hidden=true
                node_ref=picker
                on:change=move |ev| {
                    let input = event_target::<web_sys::HtmlInputElement>(&ev);
                    let files = files_of(input.files());
                    input.set_value("");
                    a.files(files);
                }
            />
            <span class="cnote cups" role="status">{move || a.ups.with(|u| status_of(u))}</span>
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_77_a_tall_screenshot_fits_the_width_and_an_ordinary_one_the_screen() {
        // 390x8000 on a 390x844 phone: the whole width, scrolled down.
        assert_eq!(fit_width(390.0, 8000.0, 390.0, 844.0), 390.0);
        // a 1170-wide one too, at the screen's width.
        assert_eq!(fit_width(1170.0, 24000.0, 390.0, 844.0), 390.0);
        // a landscape photo on a phone fits inside, with room round it.
        let w = fit_width(4000.0, 3000.0, 390.0, 844.0);
        assert!((w - 358.0).abs() < 0.01, "{w}");
        // a small image stays its size.
        assert_eq!(fit_width(120.0, 80.0, 1440.0, 900.0), 120.0);
        // a desktop's full-page shot on a phone: the width, not 152px.
        assert_eq!(fit_width(1440.0, 8000.0, 390.0, 844.0), 390.0);
        // a wide strip fits the height and scrolls across.
        assert_eq!(fit_width(20000.0, 400.0, 1440.0, 900.0), 20000.0_f64.min(900.0 * 20000.0 / 400.0));
        // nothing known yet: the screen's width.
        assert_eq!(fit_width(0.0, 0.0, 390.0, 844.0), 390.0);
    }

    #[test]
    fn app_77_a_zoom_keeps_the_point_under_the_fingers() {
        // at the fit, the point 100px down: 2x puts it 200px into the content
        assert_eq!(anchored(0.0, 100.0, 1.0, 2.0), 100.0);
        assert_eq!(anchored(300.0, 100.0, 2.0, 4.0), 700.0);
        // back to the fit never scrolls before the start
        assert_eq!(anchored(50.0, 400.0, 4.0, 1.0), 0.0);
        assert_eq!(clamp_zoom(0.2), 1.0);
        assert_eq!(clamp_zoom(99.0), ZOOM_MAX);
        assert_eq!(clamp_zoom(f64::NAN), 1.0);
        assert!(is_double(10.0, 10.0, 0.0, 20.0, 15.0, 250.0));
        assert!(!is_double(10.0, 10.0, 0.0, 10.0, 10.0, 400.0));
        assert!(!is_double(10.0, 10.0, 0.0, 80.0, 10.0, 100.0));
    }

    fn h(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    /// A comment's text: words, a media image, a data one, a remote one; a
    /// malformed one and an unknown scheme stay words.
    #[test]
    fn a_comments_images_are_cut_from_its_words() {
        let a = h('a');
        let text = format!("the hold ![the curve](media:{a}) and ![d](data:image/png;base64,AAA) or ![x](https://e.com/x.png) ![no](javascript:1) ![open");
        assert_eq!(
            segments(&text),
            vec![
                Seg::Text("the hold ".into()),
                Seg::Media { alt: "the curve".into(), hash: a.clone() },
                Seg::Text(" and ".into()),
                Seg::Data { alt: "d".into(), url: "data:image/png;base64,AAA".into() },
                Seg::Text(" or ".into()),
                Seg::Remote { alt: "x".into(), url: "https://e.com/x.png".into() },
                Seg::Text(" ![no](javascript:1) ![open".into()),
            ]
        );
        assert_eq!(segments("plain"), vec![Seg::Text("plain".into())]);
        assert_eq!(segments(&format!("![svg](data:image/svg+xml,<svg/>) ![m](media:{})", h('B'))).len(), 2, "an SVG data URL is words");
    }

    /// The draft's thumbnails are its media images, once each; × takes one
    /// out, leaving the rest.
    #[test]
    fn the_drafts_images_are_its_thumbnails() {
        let (a, b) = (h('a'), h('b'));
        let draft = format!("see {} and {} and {}", markdown("one", &a), markdown("two", &b), markdown("one again", &a));
        assert_eq!(refs(&draft), vec![("one".to_string(), a.clone()), ("two".to_string(), b.clone())]);
        assert_eq!(remove(&draft, &a), format!("see and {} and", markdown("two", &b)));
        assert!(refs(&remove(&draft, &b)).iter().all(|(_, x)| *x == a));
    }

    /// An image goes in at the cursor, on its own, the cursor after it —
    /// counted as the textarea counts (UTF-16).
    #[test]
    fn an_image_goes_in_at_the_cursor() {
        assert_eq!(insert("", 0, "![a](media:x)"), ("![a](media:x) ".to_string(), 14));
        assert_eq!(insert("look here", 4, "IMG"), ("look IMG here".to_string(), 8));
        assert_eq!(insert("end", 3, "IMG"), ("end IMG ".to_string(), 8));
        assert_eq!(insert("line\n", 5, "IMG"), ("line\nIMG ".to_string(), 9));
        // "é" is one unit, "😀" two: the offset after them is 3.
        assert_eq!(insert("é😀x", 3, "I"), ("é😀 I x".to_string(), 6));
    }

    /// COMMENT-10: a frame names an image by its hash alone; the ticket an
    /// image is held to is folded as the store spells it.
    #[test]
    fn a_tap_lifts_where_it_landed_and_soon() {
        assert!(is_tap(100.0, 200.0, 0.0, 106.0, 207.0, 300.0));
        assert!(!is_tap(100.0, 200.0, 0.0, 100.0, 230.0, 100.0), "a scroll");
        assert!(!is_tap(100.0, 200.0, 0.0, 100.0, 200.0, 650.0), "a hold");
    }

    #[test]
    fn a_frame_asks_by_a_hash_and_its_ticket_is_folded() {
        assert!(is_hash(&h('a')) && is_hash(&"0f".repeat(32)));
        assert!(!is_hash(&h('A')) && !is_hash(&h('g')) && !is_hash("ab") && !is_hash(&format!("media:{}", h('a'))));
        assert_eq!(held_key(" kiln-4 ", &h('B')), ("KILN-4".to_string(), h('b')));
    }

    #[test]
    fn an_image_is_named_by_its_file() {
        assert_eq!(alt_of("kiln curve.png"), "kiln curve");
        assert_eq!(alt_of("image.png"), "screenshot", "a pasted screenshot's generic name");
        assert_eq!(alt_of("a [b] (c).jpg"), "a b c");
        assert_eq!(alt_of("noext"), "noext");
    }

    /// An image is for the element's ticket, else the page's; none on the app.
    #[test]
    fn an_image_is_for_the_comments_ticket() {
        let anchor = |route: &str, ticket: Option<&str>| Anchor {
            route: route.into(),
            path: "p".into(),
            label: "l".into(),
            excerpt: String::new(),
            ticket: ticket.map(Into::into),
            version: None,
        };
        assert_eq!(ticket_of(&anchor("#/ticket/KILN-4", Some("KILN-2"))).as_deref(), Some("KILN-2"));
        assert_eq!(ticket_of(&anchor("#/ticket/KILN-4?view=full", None)).as_deref(), Some("KILN-4"));
        assert_eq!(ticket_of(&anchor("#/comments", None)), None);
        assert_eq!(ticket_of(&anchor("#/board", Some(" "))), None);
    }

    /// A file is refused before it is sent, naming why — the server's rule.
    #[test]
    fn a_file_is_refused_here_first() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 3, 8, 6, 0, 0, 0]);
        assert_eq!(refusal("a.png", "image/png", &png).map(|s| (s.width, s.height)), Ok((4, 3)));
        assert_eq!(refusal("a.png", "", &png).map(|s| s.mime), Ok("image/png"), "no type given: its bytes'");
        let e = refusal("logo.svg", "image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap_err();
        assert!(e.starts_with("logo.svg: an SVG is not kept"), "{e}");
        let e = refusal("notes.txt", "text/plain", b"words").unwrap_err();
        assert!(e.contains("not an image menger keeps"), "{e}");
        let mut big = png.clone();
        big.resize(9 * 1024 * 1024, 0);
        assert!(refusal("big.png", "image/png", &big).unwrap_err().contains("at most 7.0 MB"));
    }

    fn up(key: u32, state: UpState) -> Up {
        Up { key, alt: "screenshot".into(), url: None, state }
    }

    /// CHAT-30: a box's send waits while an image is on its way, and while
    /// one that failed is still shown; what the box says of them.
    #[test]
    fn chat_30_send_waits_for_the_images_on_their_way() {
        let failed = UpState::Failed { why: "a.png: no answer".into(), retry: true };
        assert!(!waits(&[]));
        assert!(waits(&[up(0, UpState::Uploading)]));
        assert!(waits(&[up(0, failed.clone())]), "never posted without it");
        assert_eq!(status_of(&[]), None);
        assert_eq!(status_of(&[up(0, UpState::Uploading)]).as_deref(), Some("uploading an image…"));
        assert_eq!(status_of(&[up(0, UpState::Uploading), up(1, UpState::Uploading)]).as_deref(), Some("uploading 2 images…"));
        assert_eq!(status_of(&[up(0, failed.clone())]).as_deref(), Some("an image was not added"));
        assert_eq!(status_of(&[up(0, UpState::Uploading), up(1, failed)]).as_deref(), Some("uploading 1, 1 not added"));
    }
}
