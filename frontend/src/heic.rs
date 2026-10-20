//! CHAT-58: an iPhone's HEIC, made a JPEG here before it is sent.
//!
//! An iPhone's photos and screenshots are often HEIC/HEIF as files (from
//! Files, a share sheet), and the server keeps only PNG, JPEG, GIF and WebP
//! (`menger_tickets_api::media::check`; unchanged). So every image path —
//! the chat composer, a comment box, paste, drop, pick, all through
//! [`crate::media::Attach`] — turns one ([`is_heic`]: by its type, or by its
//! `ftyp` brand) into a JPEG ([`convert`]), or a PNG when it has alpha:
//! decoded by the browser (`createImageBitmap`, else an `<img>` from a
//! `blob:` URL — Safari reads HEIC itself), drawn on a canvas, encoded at
//! 0.9, and scaled down until it is within the cap. A browser that cannot
//! decode it (Chrome and Firefox on a desktop) is refused plainly:
//! [`UNREADABLE`].

use wasm_bindgen::JsCast;

/// What a browser that cannot decode a HEIC says.
pub(crate) const UNREADABLE: &str = "HEIC can't be read in this browser; send it as JPEG or PNG";

/// The types a HEIC/HEIF file is said to be.
const TYPES: [&str; 4] = ["image/heic", "image/heif", "image/heic-sequence", "image/heif-sequence"];

/// The `ftyp` brands of a HEIC/HEIF image (`mif1`/`msf1` are HEIF's own).
const BRANDS: [&[u8; 4]; 8] = [b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"msf1"];

/// The largest HEIC read to be converted: it is made smaller than the cap
/// here, so its own size may be past it.
pub(crate) const MAX_SOURCE: usize = 64 * 1024 * 1024;

/// The most pixels a canvas is drawn at (iOS Safari's canvas limit is
/// about 16.7 million; a 48 MP photo is drawn smaller).
const MAX_PIXELS: f64 = 16_000_000.0;

/// Whether the file is a HEIC/HEIF: said to be one, or its bytes begin with
/// an `ftyp` box naming a HEIF brand — an AVIF (`avif`/`avis`, also `mif1`
/// compatible) is not one.
pub(crate) fn is_heic(declared: &str, bytes: &[u8]) -> bool {
    let declared = declared.trim().to_ascii_lowercase();
    if TYPES.contains(&declared.as_str()) {
        return true;
    }
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let major = &bytes[8..12];
    if major == b"avif" || major == b"avis" {
        return false;
    }
    let end = (u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize).clamp(16, bytes.len());
    std::iter::once(major).chain(bytes[16..end].chunks_exact(4)).any(|b| BRANDS.iter().any(|h| &h[..] == b))
}

/// Whether a file's name says HEIC/HEIF — for the size check made before
/// its bytes are read (a drop may give no type).
pub(crate) fn named_heic(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| matches!(ext.to_ascii_lowercase().as_str(), "heic" | "heif" | "hif"))
}

/// The type it is made: a PNG when any pixel is not opaque, else a JPEG.
pub(crate) fn out_mime(has_alpha: bool) -> &'static str {
    if has_alpha {
        "image/png"
    } else {
        "image/jpeg"
    }
}

/// Whether RGBA pixels hold any that is not opaque.
pub(crate) fn has_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|p| p[3] != 255)
}

/// The size it is first drawn at: its own, or smaller to fit the canvas.
pub(crate) fn first_size(w: u32, h: u32) -> (u32, u32) {
    let px = f64::from(w) * f64::from(h);
    if px <= MAX_PIXELS {
        return (w, h);
    }
    scaled(w, h, (MAX_PIXELS / px).sqrt())
}

/// The next size to try when `len` bytes came out past `max`: smaller by
/// the square root of the excess, with room — none when it is already tiny.
pub(crate) fn next_size(w: u32, h: u32, len: usize, max: usize) -> Option<(u32, u32)> {
    if len <= max || w <= 16 || h <= 16 {
        return None;
    }
    Some(scaled(w, h, (max as f64 / len as f64).sqrt() * 0.9))
}

fn scaled(w: u32, h: u32, k: f64) -> (u32, u32) {
    (((f64::from(w) * k).floor() as u32).max(1), ((f64::from(h) * k).floor() as u32).max(1))
}

// ── the conversion, in the browser ──────────────────────────────────────

/// What the browser decoded.
enum Decoded {
    Bitmap(web_sys::ImageBitmap),
    Img(web_sys::HtmlImageElement),
}

impl Decoded {
    fn size(&self) -> (u32, u32) {
        match self {
            Decoded::Bitmap(b) => (b.width(), b.height()),
            Decoded::Img(i) => (i.natural_width(), i.natural_height()),
        }
    }

    fn draw(&self, ctx: &web_sys::CanvasRenderingContext2d, w: u32, h: u32) -> Option<()> {
        let (w, h) = (f64::from(w), f64::from(h));
        match self {
            Decoded::Bitmap(b) => ctx.draw_image_with_image_bitmap_and_dw_and_dh(b, 0.0, 0.0, w, h),
            Decoded::Img(i) => ctx.draw_image_with_html_image_element_and_dw_and_dh(i, 0.0, 0.0, w, h),
        }
        .ok()
    }
}

/// The bytes decoded by the browser: `createImageBitmap`, else an `<img>`.
async fn decode(bytes: &[u8], declared: &str) -> Option<Decoded> {
    let arr = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(if declared.trim().is_empty() { "image/heic" } else { declared });
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&arr, &opts).ok()?;
    let win = crate::dom::window();
    if let Ok(p) = win.create_image_bitmap_with_blob(&blob) {
        if let Ok(b) = wasm_bindgen_futures::JsFuture::from(p).await {
            if let Ok(b) = b.dyn_into::<web_sys::ImageBitmap>() {
                if b.width() > 0 && b.height() > 0 {
                    return Some(Decoded::Bitmap(b));
                }
            }
        }
    }
    let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
    let img = web_sys::HtmlImageElement::new().ok()?;
    img.set_src(&url);
    let ok = wasm_bindgen_futures::JsFuture::from(img.decode()).await.is_ok();
    let _ = web_sys::Url::revoke_object_url(&url);
    (ok && img.natural_width() > 0).then_some(Decoded::Img(img))
}

/// A canvas `w` × `h` with the image drawn on it (on the theme's `--card` for a JPEG, which has no alpha).
fn canvas(d: &Decoded, w: u32, h: u32, opaque: bool) -> Option<web_sys::HtmlCanvasElement> {
    let doc = crate::dom::window().document()?;
    let canvas: web_sys::HtmlCanvasElement = doc.create_element("canvas").ok()?.dyn_into().ok()?;
    canvas.set_width(w);
    canvas.set_height(h);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok().flatten()?.dyn_into().ok()?;
    if opaque {
        let card = doc.document_element().and_then(|el| crate::dom::window().get_computed_style(&el).ok().flatten()).and_then(|s| s.get_property_value("--card").ok()).map(|v| v.trim().to_string()).unwrap_or_default();
        ctx.set_fill_style_str(if card.is_empty() { "Canvas" } else { &card });
        ctx.fill_rect(0.0, 0.0, f64::from(w), f64::from(h));
    }
    d.draw(&ctx, w, h)?;
    Some(canvas)
}

/// Whether the image has alpha, read off a small copy of it.
fn alpha_of(d: &Decoded, w: u32, h: u32) -> bool {
    let (pw, ph) = scaled(w, h, (256.0 / f64::from(w.max(h))).min(1.0));
    let Some(c) = canvas(d, pw, ph, false) else { return false };
    let Some(ctx) = c.get_context("2d").ok().flatten().and_then(|x| x.dyn_into::<web_sys::CanvasRenderingContext2d>().ok()) else { return false };
    ctx.get_image_data(0.0, 0.0, f64::from(pw), f64::from(ph)).is_ok_and(|data| has_alpha(&data.data()))
}

/// The canvas encoded as `mime` (a JPEG at 0.9): its bytes.
fn encode(c: &web_sys::HtmlCanvasElement, mime: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let url = c.to_data_url_with_type_and_encoder_options(mime, &wasm_bindgen::JsValue::from_f64(0.9)).ok()?;
    let (head, data) = url.split_once(',')?;
    // A browser that cannot encode the type answers a PNG: not what was asked.
    if !head.starts_with(&format!("data:{mime}")) {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// The HEIC made a JPEG (a PNG when it has alpha) of at most `max` bytes:
/// its bytes and type, or why not.
pub(crate) async fn convert(bytes: &[u8], declared: &str, max: usize) -> Result<(Vec<u8>, &'static str), String> {
    let d = decode(bytes, declared).await.ok_or_else(|| UNREADABLE.to_string())?;
    let (w0, h0) = d.size();
    let mime = out_mime(alpha_of(&d, w0, h0));
    let (mut w, mut h) = first_size(w0, h0);
    for _ in 0..8 {
        let c = canvas(&d, w, h, mime == "image/jpeg").ok_or("the HEIC could not be drawn")?;
        let out = encode(&c, mime).ok_or("the HEIC could not be made a JPEG")?;
        match next_size(w, h, out.len(), max) {
            None if out.len() <= max => return Ok((out, mime)),
            None => break,
            Some(s) => (w, h) = s,
        }
    }
    Err("the HEIC is too large even made smaller".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ftyp` box an iPhone writes (major `heic`; `mif1`, `heic`
    /// compatible), with `major` and `compat` swapped in.
    fn ftyp(major: &[u8; 4], compat: &[&[u8; 4]]) -> Vec<u8> {
        let size = 16 + 4 * compat.len() as u32;
        let mut b = size.to_be_bytes().to_vec();
        b.extend_from_slice(b"ftyp");
        b.extend_from_slice(major);
        b.extend_from_slice(&[0, 0, 0, 0]);
        for c in compat {
            b.extend_from_slice(*c);
        }
        b.extend_from_slice(b"\0\0\x01\x7dmeta");
        b
    }

    #[test]
    fn chat_58_a_heic_is_known_by_its_type_or_its_brand() {
        assert!(is_heic("image/heic", b""), "said to be HEIC");
        assert!(is_heic("IMAGE/HEIF", b""));
        assert!(is_heic("image/heic-sequence", b""));
        for brand in [b"heic", b"heix", b"mif1", b"msf1", b"hevc"] {
            assert!(is_heic("", &ftyp(brand, &[])), "major {}", String::from_utf8_lossy(brand));
            assert!(is_heic("application/octet-stream", &ftyp(b"isom", &[brand])), "compatible {}", String::from_utf8_lossy(brand));
        }
        assert!(is_heic("", &ftyp(b"heic", &[b"mif1", b"heic", b"miaf"])), "an iPhone's");
        // The fixture the WebKit check attaches (scripts/fixtures/tiny.heic).
        assert!(is_heic("", include_bytes!("../../scripts/fixtures/tiny.heic")));
    }

    #[test]
    fn chat_58_what_is_not_a_heic() {
        assert!(!is_heic("", &ftyp(b"avif", &[b"mif1", b"miaf"])), "an AVIF is mif1 too");
        assert!(!is_heic("", &ftyp(b"isom", &[b"iso2", b"mp41"])), "a video");
        assert!(!is_heic("image/png", &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R']));
        assert!(!is_heic("image/jpeg", &[0xFF, 0xD8, 0xFF]));
        assert!(!is_heic("", b"short"));
        // A brand past the box's own size is not its.
        let mut b = ftyp(b"isom", &[]);
        b.extend_from_slice(b"heic");
        assert!(!is_heic("", &b));
    }

    #[test]
    fn chat_58_a_heic_is_named_so() {
        assert!(named_heic("Screenshot 2026-10-19 at 11.00.25 PM.heic"));
        assert!(named_heic("IMG_0001.HEIF"));
        assert!(!named_heic("a.png"));
        assert!(!named_heic("heic"));
    }

    #[test]
    fn chat_58_alpha_makes_a_png_else_a_jpeg() {
        assert!(!has_alpha(&[1, 2, 3, 255, 4, 5, 6, 255]));
        assert!(has_alpha(&[1, 2, 3, 255, 4, 5, 6, 254]));
        assert_eq!(out_mime(false), "image/jpeg");
        assert_eq!(out_mime(true), "image/png");
    }

    #[test]
    fn chat_58_it_is_made_smaller_until_within_the_cap() {
        assert_eq!(first_size(1179, 2556), (1179, 2556), "an iPhone screenshot as it is");
        let (w, h) = first_size(8064, 6048);
        assert!(f64::from(w) * f64::from(h) <= MAX_PIXELS && w > 4000, "a 48 MP photo fits a canvas: {w}×{h}");
        assert!((f64::from(w) / f64::from(h) - 8064.0 / 6048.0).abs() < 0.01, "its shape kept");
        let max = 7 * 1024 * 1024;
        assert_eq!(next_size(4000, 3000, max, max), None, "within the cap: done");
        let (w, h) = next_size(4000, 3000, 2 * max, max).unwrap();
        assert!(w < 2829 && h < 2122, "half the bytes wanted: under 1/√2 the side ({w}×{h})");
        assert_eq!(next_size(10, 10, 2 * max, max), None, "nothing left to shrink");
    }
}
