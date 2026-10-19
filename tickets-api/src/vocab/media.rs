//! COMMENT-5: what names and carries an image — [`MediaHash`],
//! [`MediaType`], [`MediaData`]. What an image must be (a kept type, by its
//! bytes; at most 7 MiB) is the store's rule, refused in band naming the
//! limit ([`crate::media::check`]); these hold only the spelling.

use super::{not_a, too_long};

/// The most `data` may be on the wire: room for [`crate::media::MAX_BYTES`]
/// in base64 and more — an image past the cap is refused by the store, in
/// band, naming the cap, not here.
pub const MEDIA_DATA_WIRE_MAX: usize = 64 * 1024 * 1024;

fn hash(text: &str) -> Result<String, String> {
    let t = text.trim();
    let t = t.strip_prefix(crate::media::SCHEME).unwrap_or(t);
    match t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(not_a(t, "an image's hash", "its SHA-256, 64 hex digits (as `media.put` answers it)")),
    }
}

text_type!(
    /// An image's name: its SHA-256, 64 hex digits (lower-cased; a
    /// `media:` before it is read past), as `media.put` answers it.
    MediaHash,
    hash
);

fn media_type(text: &str) -> Result<String, String> {
    let t = text.trim();
    let shape = "a media type, `type/subtype` (image/png, image/jpeg, image/gif or image/webp)";
    let token = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b));
    let essence = t.split(';').next().unwrap_or("").trim();
    let ok = t.len() <= 255 && essence.split_once('/').is_some_and(|(a, b)| token(a) && token(b));
    match ok {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(not_a(t, "a media type", shape)),
    }
}

text_type!(
    /// What an image is said to be: a media type, `type/subtype` (lower-
    /// cased). Which ones are kept, and whether the bytes are that, is the
    /// store's rule (`image/png`, `image/jpeg`, `image/gif`, `image/webp`).
    MediaType,
    media_type
);

fn data(text: &str) -> Result<String, String> {
    match text.len() <= MEDIA_DATA_WIRE_MAX {
        true => Ok(text.to_string()),
        false => Err(too_long("an image's data", text.len(), MEDIA_DATA_WIRE_MAX)),
    }
}

text_type!(
    /// An image's bytes, in standard base64 (`base64 -w0 shot.png`); a
    /// `data:image/png;base64,` before them is read past by the store.
    MediaData,
    data
);
