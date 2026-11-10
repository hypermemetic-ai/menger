//! The media vocabulary (v1's `media.rs`, COMMENT-5, carried whole) —
//! [`MediaEvent`], the image types menger keeps ([`TYPES`]), the most one
//! may be ([`MAX_BYTES`]) and the rule that reads an image's type and size
//! from its own bytes ([`sniff`], [`check`]). The server refuses with it; a
//! client may refuse a file with it before sending one.
//!
//! **What is kept**: PNG, JPEG, GIF and WebP, each recognised by its magic
//! bytes — never by the name or the type a caller says — and never SVG (an
//! SVG is a document, and can run script); a declared type that is not
//! what the bytes are is refused. Width and height are read from the
//! header. At most [`MAX_BYTES`] each.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The most an image may be, in bytes: 7 MiB. An image travels as base64
/// (4/3 of its size) inside one WebSocket message, and the transport takes
/// a message up to 10 MiB (jsonrpsee's default, plexus-transport's): 8 MiB
/// is 11.2 MB on the wire, 7 MiB 9.8 MB.
pub const MAX_BYTES: usize = 7 * 1024 * 1024;

/// The image types menger keeps, as their media types.
pub const TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The scheme a body names a kept image by: `![alt](media:<hash>)`.
pub const SCHEME: &str = "media:";

/// What an image's bytes say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sniffed {
    /// One of [`TYPES`].
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Why an image is not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Past [`MAX_BYTES`].
    TooLarge { bytes: u64, max: u64 },
    /// Nothing at all.
    Empty,
    /// The data is not base64.
    NotBase64(String),
    /// An SVG — `found` says so — or anything else that is none of
    /// [`TYPES`] by its bytes.
    NotAnImage { declared: String, found: Option<&'static str> },
    /// One of [`TYPES`], but not the one declared.
    Mismatch { declared: String, found: &'static str },
    /// The right magic, but no width and height where its header keeps them.
    Truncated { mime: &'static str },
    /// EDITOR-10: a chunk that is not the next one (`expected`, `got`).
    OutOfOrder { expected: u64, got: u64 },
    /// EDITOR-10: `finish` before every byte said at `begin` came.
    Incomplete { received: u64, size: u64 },
    /// EDITOR-10: chunks past the size said at `begin`.
    PastSize { size: u64 },
}

impl Problem {
    /// The wire's word for it — `media.put`'s `refused { reason }`.
    pub fn reason(&self) -> &'static str {
        match self {
            Problem::TooLarge { .. } => "too_large",
            Problem::Empty | Problem::NotBase64(_) => "bad_data",
            Problem::NotAnImage { .. } => "not_an_image",
            Problem::Mismatch { .. } => "type_mismatch",
            Problem::Truncated { .. } => "truncated",
            Problem::OutOfOrder { .. } => "out_of_order",
            Problem::Incomplete { .. } => "incomplete",
            Problem::PastSize { .. } => "past_size",
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kept = TYPES.join(", ");
        match self {
            Problem::TooLarge { bytes, max } => write!(f, "the file is {} ({bytes} bytes); at most {} ({max} bytes)", size(*bytes), size(*max)),
            Problem::Empty => f.write_str("no file: the data is empty"),
            Problem::NotBase64(why) => write!(f, "the data is not base64: {why}"),
            Problem::NotAnImage { found: Some(svg), .. } if *svg == "image/svg+xml" => {
                write!(f, "an SVG is not kept — it is a document and can run script; menger keeps {kept}")
            }
            Problem::NotAnImage { declared, .. } => write!(f, "not an image menger keeps (said to be {declared}; its bytes are none of {kept})"),
            Problem::Mismatch { declared, found } => write!(f, "the bytes are {found}, not {declared} as declared"),
            Problem::Truncated { mime } => write!(f, "a truncated {mime}: its header holds no width and height"),
            Problem::OutOfOrder { expected, got } => write!(f, "chunk {got} is not the next one; send chunk {expected}"),
            Problem::Incomplete { received, size } => write!(f, "{received} of {size} bytes came; send the rest before finishing"),
            Problem::PastSize { size } => write!(f, "the chunks are past the {size} bytes said at the start"),
        }
    }
}

/// `bytes` in words: `9.0 MB`, `512 KB`, `12 bytes`.
pub fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 * 1024 => format!("{:.1} GB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{} KB", b / 1024),
        b => format!("{b} bytes"),
    }
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}

fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}

fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
}

/// A JPEG's size: the first start-of-frame segment's (SOF0–SOF15, but the
/// DHT, JPG and DAC markers that share the range).
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        // Fill bytes, and the markers that stand alone.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = be16(b, i + 2)? as usize;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((be16(b, i + 7)?, be16(b, i + 5)?));
        }
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

/// A WebP's size: from its first chunk — lossy (`VP8 `), lossless
/// (`VP8L`) or extended (`VP8X`).
fn webp_size(b: &[u8]) -> Option<(u32, u32)> {
    match b.get(12..16)? {
        b"VP8 " => (b.get(23..26)? == [0x9d, 0x01, 0x2a]).then_some(())
            .and_then(|_| Some((le16(b, 26)? & 0x3fff, le16(b, 28)? & 0x3fff))),
        b"VP8L" => {
            if *b.get(20)? != 0x2f {
                return None;
            }
            let s = b.get(21..25)?;
            let bits = u32::from_le_bytes(s.try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le24(b, 24)? + 1, le24(b, 27)? + 1)),
        _ => None,
    }
}

/// Whether `b` reads as an SVG (or any XML that might be one): what to
/// name when refusing it.
fn looks_like_svg(b: &[u8]) -> bool {
    let head = &b[..b.len().min(1024)];
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg") || (t.starts_with("<?xml") && text.contains("<svg")) || (t.starts_with("<!doctype svg"))
}

/// What `b`'s magic bytes say it is: `Ok(None)` none of [`TYPES`],
/// `Err` one of them whose header is cut short.
pub fn sniff(b: &[u8]) -> Result<Option<Sniffed>, Problem> {
    let (mime, dims) = if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        ("image/png", (b.get(12..16) == Some(b"IHDR")).then_some(()).and_then(|_| Some((be32(b, 16)?, be32(b, 20)?))))
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("image/jpeg", jpeg_size(b))
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        ("image/gif", le16(b, 6).zip(le16(b, 8)))
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        ("image/webp", webp_size(b))
    } else {
        return Ok(None);
    };
    match dims {
        Some((width, height)) => Ok(Some(Sniffed { mime, width, height })),
        None => Err(Problem::Truncated { mime }),
    }
}

/// `declared`, as a media type is compared: its essence, lower case
/// (`image/PNG; charset=x` is `image/png`); `image/jpg` is `image/jpeg`.
pub fn essence(declared: &str) -> String {
    let e = declared.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match e.as_str() {
        "image/jpg" | "image/pjpeg" => "image/jpeg".into(),
        _ => e,
    }
}

/// The rule `put` keeps an image by: at most [`MAX_BYTES`], one of
/// [`TYPES`] by its bytes, and the type `declared` says.
pub fn check(declared: &str, b: &[u8]) -> Result<Sniffed, Problem> {
    if b.is_empty() {
        return Err(Problem::Empty);
    }
    if b.len() > MAX_BYTES {
        return Err(Problem::TooLarge { bytes: b.len() as u64, max: MAX_BYTES as u64 });
    }
    let declared = essence(declared);
    let found = match sniff(b)? {
        Some(s) => s,
        None => {
            let found = looks_like_svg(b).then_some("image/svg+xml");
            return Err(Problem::NotAnImage { declared, found });
        }
    };
    if found.mime != declared {
        return Err(Problem::Mismatch { declared, found: found.mime });
    }
    Ok(found)
}

/// The hash a body's `media:<hash>` names, when `dest` is one: 64 hex
/// digits, lower-cased.
pub fn hash_of(dest: &str) -> Option<String> {
    let h = dest.trim().strip_prefix(SCHEME)?.trim();
    (h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

/// The bytes a `data` parameter carries: base64, standard alphabet, a
/// `data:…;base64,` before it and whitespace inside it read past.
pub fn decode(data: &str) -> Result<Vec<u8>, Problem> {
    use base64::Engine;
    let t = data.trim();
    let t = match t.strip_prefix("data:").and_then(|r| r.split_once(',')) {
        Some((head, rest)) if head.ends_with(";base64") => rest,
        _ => t,
    };
    let clean: String = t.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Err(Problem::Empty);
    }
    base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()).map_err(|e| Problem::NotBase64(e.to_string()))
}

/// An image's name: its SHA-256, in lower-case hex.
pub fn hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// What a kept image is, without its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaInfo {
    /// Its SHA-256, 64 lower-case hex digits — its name.
    pub hash: String,
    /// What its bytes are (`image/png`, `video/mp4`, `application/pdf`, …;
    /// `application/octet-stream` for any other file).
    pub mime: String,
    /// Its size, in bytes.
    pub bytes: u64,
    /// An image's, from its header; 0 for anything else.
    pub width: u32,
    pub height: u32,
    /// EDITOR-10: how it is drawn — `image`, `video`, `audio`, `pdf` or `file`.
    #[serde(default = "image_kind")]
    pub kind: String,
    /// EDITOR-10: its name as it came (`clip.mp4`), when one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn image_kind() -> String {
    "image".into()
}

/// What the `media` service streams. Every method returns
/// `impl Stream<Item = MediaEvent>`; a refusal is in band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaEvent {
    /// `put`'s and `finish`'s answer: the file as kept — new, or the same
    /// bytes kept already (`duplicate`: no second row).
    Stored {
        hash: String,
        mime: String,
        bytes: u64,
        width: u32,
        height: u32,
        /// How it is drawn: `image`, `video`, `audio`, `pdf` or `file`.
        #[serde(default = "image_kind")]
        kind: String,
        /// Its name as it came, when one was given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The same bytes were kept already (for this ticket or another):
        /// nothing new was stored but, for a new ticket, that it is its too.
        #[serde(default)]
        duplicate: bool,
    },
    /// `get`'s answer: the file and its bytes, in base64.
    Media {
        hash: String,
        mime: String,
        bytes: u64,
        width: u32,
        height: u32,
        #[serde(default = "image_kind")]
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The bytes, standard base64.
        data: String,
    },
    /// `info`'s answer: what a kept file is, without its bytes.
    Info {
        hash: String,
        mime: String,
        bytes: u64,
        width: u32,
        height: u32,
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// `begin`'s answer: the upload's id, its size, the bytes it has
    /// (`received`: 0 when begun, more when resumed — the next chunk's
    /// `offset`) and the most one chunk may carry (`chunk`, in bytes before
    /// base64).
    Upload { upload: String, size: u64, received: u64, chunk: u64 },
    /// `chunk`'s answer: the chunk is kept — `received` of `size` bytes so
    /// far; `duplicate` for a chunk sent again (nothing written twice).
    Chunked {
        upload: String,
        received: u64,
        size: u64,
        #[serde(default)]
        duplicate: bool,
    },
    /// `usage`'s answer: what a principal has put — the files (each once)
    /// and their bytes.
    Usage { principal: String, files: u64, bytes: u64 },
    /// V2-69: `usage { everyone }`'s last answer — the files no one is
    /// charged for (a link's preview, kept as anyone's; a ticket's file kept
    /// before its uploader was recorded), so a recount can say them.
    Unattributed { files: u64, bytes: u64 },
    /// `unfurl`'s answer: what a public page is — its title, description,
    /// site and preview image (kept, by its hash), the kind of card it
    /// draws as, and for an allow-listed provider the player's address.
    Unfurled {
        url: String,
        /// `link`, `video`, `tweet`, `repo`, `design` or `image`.
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// The site's name (`YouTube`, `GitHub`), or its host.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        site: Option<String>,
        /// The preview image, kept: its hash (`/media/<hash>`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<String>,
        /// The provider (`youtube`, `vimeo`, `x`, `github`, `figma`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        /// The player to frame, for YouTube, Vimeo, X and Figma alone: an
        /// https address this service built from the link, never one a page gave.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        embed: Option<String>,
    },
    /// `get` of a file there is none of — or one the caller may not see
    /// (ACCESS-10: one answer). `put` for a ticket that is none, or out of
    /// reach, says it of the ticket (`ticket`); `chunk` or `finish` of an
    /// upload that is none, or another's, of the upload.
    NotFound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upload: Option<String>,
        /// V2-29: `put` for a document that is none, or that the docs
        /// service says you may not write.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        doc: Option<String>,
    },
    /// Refused, naming why in `reason` — `too_large` (with `bytes` and
    /// `max`), `truncated`, `bad_data`, `out_of_order`, `incomplete`,
    /// `past_size`, `over_allowance` (`max` what is left, when known); for a fetch `bad_url`, `blocked_address`,
    /// `too_many_redirects`, `timeout`, `bad_type`, `http_status`,
    /// `unreachable` — and `message` in words. Nothing was kept.
    Refused {
        reason: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u64>,
    },
    /// Any other in-band refusal, naming the problem; nothing was written.
    Error { message: String },
}

/// MACRO-21: a client failure, in band — the emitted `MediaClient`'s.
impl plexus_rpc::client::FromClientError for MediaEvent {
    fn from_client_error(error: plexus_rpc::client::ClientError) -> Self {
        MediaEvent::Error { message: error.to_string() }
    }
}

impl From<Problem> for MediaEvent {
    fn from(p: Problem) -> Self {
        let (bytes, max) = match &p {
            Problem::TooLarge { bytes, max } => (Some(*bytes), Some(*max)),
            _ => (None, None),
        };
        MediaEvent::Refused { reason: p.reason().to_string(), message: format!("{p} (nothing was kept)"), bytes, max }
    }
}

impl MediaEvent {
    /// `put`'s answer as `info`.
    pub fn stored(info: MediaInfo, duplicate: bool) -> Self {
        let MediaInfo { hash, mime, bytes, width, height, kind, name } = info;
        MediaEvent::Stored { hash, mime, bytes, width, height, kind, name, duplicate }
    }

    /// `info`'s answer.
    pub fn info(info: MediaInfo) -> Self {
        let MediaInfo { hash, mime, bytes, width, height, kind, name } = info;
        MediaEvent::Info { hash, mime, bytes, width, height, kind, name }
    }

    /// A refusal that is no [`Problem`] of the bytes.
    pub fn refused(reason: &str, message: impl Into<String>) -> Self {
        MediaEvent::Refused { reason: reason.into(), message: message.into(), bytes: None, max: None }
    }

    /// `not_found` naming a hash.
    pub fn no_hash(hash: &str) -> Self {
        MediaEvent::NotFound { hash: Some(hash.into()), ticket: None, upload: None, doc: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` PNG's first bytes: the signature and IHDR.
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    #[test]
    fn each_kept_type_is_read_from_its_bytes_with_its_size() {
        assert_eq!(sniff(&png(3, 2)), Ok(Some(Sniffed { mime: "image/png", width: 3, height: 2 })));
        let gif = [b"GIF89a".as_slice(), &[5, 0, 7, 0, 0, 0, 0]].concat();
        assert_eq!(sniff(&gif), Ok(Some(Sniffed { mime: "image/gif", width: 5, height: 7 })));
        // JPEG: SOI, an APP0 of 16 bytes, then SOF0 (height 0x0102, width 0x0304).
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend_from_slice(&[0; 14]);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0x02, 0x03, 0x04, 0x03]);
        assert_eq!(sniff(&jpeg), Ok(Some(Sniffed { mime: "image/jpeg", width: 0x0304, height: 0x0102 })));
        // WebP, the three chunk kinds.
        let riff = |chunk: &[u8], rest: &[u8]| [b"RIFF".as_slice(), &[0; 4], b"WEBP", chunk, &[0; 4], rest].concat();
        let lossy = riff(b"VP8 ", &[0, 0, 0, 0x9d, 0x01, 0x2a, 10, 0, 20, 0]);
        assert_eq!(sniff(&lossy), Ok(Some(Sniffed { mime: "image/webp", width: 10, height: 20 })));
        let bits: u32 = (9) | (19 << 14);
        let lossless = riff(b"VP8L", &[[0x2f].as_slice(), &bits.to_le_bytes()].concat());
        assert_eq!(sniff(&lossless), Ok(Some(Sniffed { mime: "image/webp", width: 10, height: 20 })));
        let ext = riff(b"VP8X", &[0, 0, 0, 0, 9, 0, 0, 19, 0, 0]);
        assert_eq!(sniff(&ext), Ok(Some(Sniffed { mime: "image/webp", width: 10, height: 20 })));
        assert_eq!(sniff(b"hello"), Ok(None));
        assert_eq!(sniff(&png(1, 1)[..14]), Err(Problem::Truncated { mime: "image/png" }));
    }

    #[test]
    fn an_svg_a_non_image_a_mismatch_and_too_much_are_refused_naming_why() {
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        let e = check("image/svg+xml", svg).unwrap_err();
        assert_eq!(e.reason(), "not_an_image");
        assert!(e.to_string().contains("an SVG is not kept"), "{e}");
        // Said to be a PNG, it is still an SVG.
        assert!(check("image/png", svg).unwrap_err().to_string().contains("SVG"));
        let e = check("text/plain", b"just words").unwrap_err();
        assert!(e.to_string().contains("not an image menger keeps") && e.to_string().contains("image/webp"), "{e}");
        let e = check("image/jpeg", &png(1, 1)).unwrap_err();
        assert_eq!(e, Problem::Mismatch { declared: "image/jpeg".into(), found: "image/png" });
        let mut big = png(1, 1);
        big.resize(9 * 1000 * 1000, 0);
        let e = check("image/png", &big).unwrap_err();
        assert_eq!(e, Problem::TooLarge { bytes: 9_000_000, max: MAX_BYTES as u64 });
        assert_eq!(e.to_string(), "the file is 8.6 MB (9000000 bytes); at most 7.0 MB (7340032 bytes)");
        assert_eq!(check("image/png", b"").unwrap_err(), Problem::Empty);
        assert_eq!(check("IMAGE/PNG; x=y", &png(4, 4)).map(|s| (s.width, s.height)), Ok((4, 4)));
    }

    #[test]
    fn a_body_names_an_image_by_media_and_its_hash() {
        let h = "9F".repeat(32);
        assert_eq!(hash_of(&format!("media:{h}")), Some("9f".repeat(32)));
        assert_eq!(hash_of("media:abc"), None);
        assert_eq!(hash_of("https://e.com/x.png"), None);
    }

    #[test]
    fn a_refusal_on_the_wire_names_its_reason_and_limit() {
        let ev: MediaEvent = Problem::TooLarge { bytes: 9, max: 7 }.into();
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "refused");
        assert_eq!(v["reason"], "too_large");
        assert_eq!((v["bytes"].as_u64(), v["max"].as_u64()), (Some(9), Some(7)));
        let nf = serde_json::to_value(MediaEvent::no_hash("ab")).unwrap();
        assert_eq!(nf, serde_json::json!({"type": "not_found", "hash": "ab"}));
    }

    #[test]
    fn data_is_read_past_its_prefix_and_whitespace() {
        use base64::Engine;
        let b = png(2, 2);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&b);
        assert_eq!(decode(&b64), Ok(b.clone()));
        assert_eq!(decode(&format!("data:image/png;base64,{b64}\n")), Ok(b.clone()));
        assert_eq!(decode(&format!(" {}\n{} ", &b64[..10], &b64[10..])), Ok(b));
        assert_eq!(decode("   "), Err(Problem::Empty));
        assert_eq!(decode("not base64 at all!").unwrap_err().reason(), "bad_data");
        assert_eq!(hash(b"").len(), 64);
    }
}
