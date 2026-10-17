//! COMMENT-5: images in comments — attached in every box that writes one,
//! drawn in every comment that shows one.
//!
//! **Attaching** ([`Attach`], [`Attachments`]): a screenshot pasted into the
//! box, a file dropped on it, or one picked with its Image button is read,
//! checked here by the server's own rule (`menger_tickets_api::media::check`:
//! PNG, JPEG, GIF or WebP by its bytes, never SVG, at most 7 MiB — a file
//! refused says why and is never sent), and kept with `media.put` for the
//! ticket the comment is on (none for a comment on the app). Its markdown,
//! `![name](media:<hash>)`, goes into the box at the cursor, and the box
//! shows it as a thumbnail — the thumbnails are the draft's own `media:`
//! images, so deleting the markdown removes one, and × removes both.
//!
//! **Drawing** ([`hydrate`]): a comment's `media:` image is an `img.cimg`
//! with no `src` (src/md.rs, [`segments`]); the page asks `media.get` for
//! its bytes — once per image per page load — and sets a `blob:` URL (the
//! CSP allows `blob:` and `data:` images; nothing else is drawn). An image
//! the reader may not see is `not_found` and stays its alt. A click opens it
//! full size ([`open_full`]); a click or Esc closes it.
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
            let events = client.get_all(h).await.map_err(|e| crate::session::describe(&e))?;
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
        let Some(img) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).filter(|el| el.matches("img.cimg, .cthumb img").unwrap_or(false)) else {
            return;
        };
        let Some(src) = img.get_attribute("src").filter(|s| !s.is_empty()) else { return };
        ev.prevent_default();
        ev.stop_propagation();
        open_full(&src, &img.get_attribute("alt").unwrap_or_default());
    });
    let _ = doc.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// The image at `src`, full size, over the page: a click or Esc closes it.
pub(crate) fn open_full(src: &str, alt: &str) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let Some(doc) = crate::dom::window().document() else { return };
    let (Ok(wrap), Ok(img)) = (doc.create_element("div"), doc.create_element("img")) else { return };
    let _ = wrap.set_attribute("class", "lightbox");
    let _ = wrap.set_attribute("role", "dialog");
    let _ = wrap.set_attribute("aria-label", if alt.is_empty() { "image" } else { alt });
    let _ = wrap.set_attribute("tabindex", "-1");
    let _ = img.set_attribute("src", src);
    let _ = img.set_attribute("alt", alt);
    let _ = wrap.append_child(&img);
    let Some(body) = doc.body() else { return };
    let _ = body.append_child(&wrap);
    let w = wrap.clone();
    let close = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        // A click anywhere on it; of the keys, the dialog's close (Esc).
        if let Some(k) = ev.dyn_ref::<web_sys::KeyboardEvent>() {
            if !crate::bindings::is(k, crate::bindings::Context::Dialog, "dialog.close") {
                return;
            }
        }
        ev.prevent_default();
        ev.stop_propagation();
        w.remove();
    });
    let _ = wrap.add_event_listener_with_callback("click", close.as_ref().unchecked_ref());
    let _ = wrap.add_event_listener_with_callback("keydown", close.as_ref().unchecked_ref());
    close.forget();
    if let Ok(h) = wrap.dyn_into::<web_sys::HtmlElement>() {
        let _ = h.focus();
    }
}

// ── attaching ───────────────────────────────────────────────────────────

/// One box's attaching: its draft, the box, the ticket an image is for,
/// what it says (uploading, or why a file was refused) and how many uploads
/// are out.
#[derive(Clone, Copy)]
pub(crate) struct Attach {
    body: RwSignal<String>,
    area: NodeRef<Textarea>,
    ticket: StoredValue<Option<String>>,
    note: RwSignal<Option<(bool, String)>>,
    out: RwSignal<u32>,
}

impl Attach {
    pub(crate) fn new(body: RwSignal<String>, area: NodeRef<Textarea>, ticket: Option<String>) -> Self {
        Self { body, area, ticket: StoredValue::new(ticket), note: RwSignal::new(None), out: RwSignal::new(0) }
    }

    /// Whether an image is still on its way (the box's Send waits for it).
    pub(crate) fn busy(self) -> bool {
        self.out.get() > 0
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

    /// Each file read, checked, put, and its markdown put in the box.
    pub(crate) fn files(self, files: Vec<web_sys::File>) {
        for f in files {
            self.one(f);
        }
    }

    fn one(self, file: web_sys::File) {
        let name = file.name();
        self.out.update(|n| *n += 1);
        self.note.set(Some((false, format!("adding {}…", alt_of(&name)))));
        wasm_bindgen_futures::spawn_local(async move {
            let r = self.upload(&file, &name).await;
            self.out.try_update(|n| *n = n.saturating_sub(1));
            match r {
                Ok((alt, hash)) => {
                    self.note.try_set(None);
                    self.put_markdown(&alt, &hash);
                }
                Err(e) => {
                    self.note.try_set(Some((true, e)));
                }
            }
        });
    }

    async fn upload(self, file: &web_sys::File, name: &str) -> Result<(String, String), String> {
        use base64::Engine;
        // Past the cap, said before the bytes are read.
        let size = file.size() as usize;
        if size > rules::MAX_BYTES {
            return Err(format!("{name}: {}", rules::Problem::TooLarge { bytes: size, max: rules::MAX_BYTES }));
        }
        let buf = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await.map_err(|_| format!("{name}: could not be read"))?;
        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
        let found = refusal(name, &file.type_(), &bytes)?;
        let client = crate::session::media_client().ok_or_else(|| "not connected — the image was not sent".to_string())?;
        let ticket = crate::typed::opt(self.ticket.get_value())?;
        let mime = crate::typed::typed(found.mime)?;
        let data = crate::typed::typed(&base64::engine::general_purpose::STANDARD.encode(&bytes))?;
        let events = client.put_all(ticket, mime, data).await.map_err(|e| crate::session::describe(&e))?;
        for ev in events {
            match ev {
                MediaEvent::Stored { hash, .. } => {
                    // The thumbnail and the comment draw from the file itself.
                    if let Ok(u) = web_sys::Url::create_object_url_with_blob(file) {
                        settle(&hash, Known::Url(u));
                    }
                    return Ok((alt_of(name), hash));
                }
                MediaEvent::Refused { message, .. } | MediaEvent::Error { message } => return Err(format!("{name}: {message}")),
                MediaEvent::NotFound { ticket, .. } => {
                    return Err(format!("{name}: not kept — {} is no ticket you can add images to", ticket.unwrap_or_default()))
                }
                MediaEvent::Media { .. } => {}
            }
        }
        Err(format!("{name}: no answer"))
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

/// A box's images: the draft's thumbnails, the Image button (and its
/// picker), and what it says.
#[component]
pub(crate) fn Attachments(a: Attach) -> impl IntoView {
    let picker = NodeRef::<leptos::html::Input>::new();
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
            <span class="cthumbs">{thumbs}</span>
            <button
                type="button"
                class="cpick"
                title="Add an image — or paste a screenshot, or drop a file on the box"
                on:click=move |_| {
                    if let Some(i) = picker.get() {
                        i.click();
                    }
                }
            >
                "Image"
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
            {move || a.note.get().map(|(bad, n)| view! { <span class="cnote" class:bad=bad role=if bad { "alert" } else { "status" }>{n}</span> })}
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
