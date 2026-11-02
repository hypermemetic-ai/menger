//! EDITOR-10: **what any file is** — a doc absorbs anything from outside,
//! so the service keeps any file, not only an image: its type read from its
//! own magic bytes ([`sniff`]) and never from the name or the type a client
//! says, and the *kind* it is drawn as ([`Kind`]): an image inline, video
//! and audio as players, a PDF in the browser's viewer, anything else as a
//! file card with its name, size and a download.
//!
//! | kind | kept as, by its bytes |
//! |---|---|
//! | `image` | PNG, JPEG, GIF, WebP ([`crate::image::sniff`], with its width and height) |
//! | `video` | MP4 (`ftyp`), QuickTime (`ftypqt`), WebM (EBML, doctype `webm`) |
//! | `audio` | MP3 (ID3 or an MPEG frame), Ogg, WAV (`RIFF…WAVE`), M4A (`ftypM4A`), FLAC |
//! | `pdf` | `%PDF-` |
//! | `file` | anything else — `application/octet-stream`, served as a download |
//!
//! An SVG or an HTML page is a `file`: it is a document that can run
//! script, so it is never served as itself ([`served_inline`]).

/// The most `media.put` takes in one message: 7 MiB, as an image always
/// was (the socket's message is 10 MiB, and base64 is 4/3).
pub const PUT_MAX: usize = crate::image::MAX_BYTES;

/// The most one chunked upload may be when the server names none
/// (`--media-max-bytes`): 200 MiB.
pub const DEFAULT_MAX_UPLOAD: u64 = 200 * 1024 * 1024;

/// The most one `media.chunk` carries: 4 MiB of bytes, 5.3 MiB of base64 —
/// well under the socket's 10 MiB message.
pub const CHUNK_MAX: usize = 4 * 1024 * 1024;

/// What any kept file's bytes are when they are none of the kinds below.
pub const OCTET_STREAM: &str = "application/octet-stream";

/// How a kept file is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Pdf,
    File,
}

impl Kind {
    /// The wire's word: `image`, `video`, `audio`, `pdf`, `file`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Pdf => "pdf",
            Kind::File => "file",
        }
    }

    /// The kind a kept media type is drawn as.
    pub fn of(mime: &str) -> Kind {
        match mime {
            m if crate::image::TYPES.contains(&m) => Kind::Image,
            m if m.starts_with("video/") => Kind::Video,
            m if m.starts_with("audio/") => Kind::Audio,
            "application/pdf" => Kind::Pdf,
            _ => Kind::File,
        }
    }
}

/// What a file's bytes say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sniffed {
    pub mime: &'static str,
    pub kind: Kind,
    /// An image's, from its header; 0 for anything else.
    pub width: u32,
    pub height: u32,
}

impl Sniffed {
    fn of(mime: &'static str) -> Self {
        Sniffed { mime, kind: Kind::of(mime), width: 0, height: 0 }
    }
}

/// An ISO base media file's (`ftyp`) type by its major brand: MP4, QuickTime
/// or M4A; `None` for a brand that names none of them (HEIC, AVIF, …).
fn ftyp(b: &[u8]) -> Option<&'static str> {
    if b.get(4..8) != Some(b"ftyp") {
        return None;
    }
    let brand = b.get(8..12)?;
    match brand {
        b"qt  " => Some("video/quicktime"),
        b"M4A " | b"M4B " => Some("audio/mp4"),
        b"isom" | b"iso2" | b"iso4" | b"iso5" | b"iso6" | b"mp41" | b"mp42" | b"avc1" | b"M4V " | b"M4VH" | b"M4VP" | b"dash" | b"MSNV" | b"3gp4" | b"3gp5" | b"3gp6" | b"3g2a" | b"f4v " | b"mmp4" => Some("video/mp4"),
        _ => None,
    }
}

/// A Matroska file's doctype (`webm`), read from its EBML header's first
/// bytes: a WebM is kept as one, any other Matroska as a file.
fn webm(b: &[u8]) -> bool {
    b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) && b[..b.len().min(64)].windows(4).any(|w| w == b"webm")
}

/// An MP3: an ID3 tag, or an MPEG audio frame's sync (11 bits set, layer
/// III, a bitrate that is not "bad").
fn mp3(b: &[u8]) -> bool {
    if b.starts_with(b"ID3") {
        return true;
    }
    matches!(b, [0xFF, second, third, ..] if second & 0xE0 == 0xE0 && (second >> 1) & 0x03 == 0x01 && third >> 4 != 0x0F)
}

/// What `b`'s magic bytes say it is — any file: an image as
/// [`crate::image::sniff`] reads it (a truncated one is refused there),
/// video, audio, PDF, or a file (`application/octet-stream`).
pub fn sniff(b: &[u8]) -> Result<Sniffed, crate::image::Problem> {
    if let Some(img) = crate::image::sniff(b)? {
        return Ok(Sniffed { mime: img.mime, kind: Kind::Image, width: img.width, height: img.height });
    }
    let mime = if let Some(m) = ftyp(b) {
        m
    } else if webm(b) {
        "video/webm"
    } else if b.starts_with(b"OggS") {
        "audio/ogg"
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WAVE") {
        "audio/wav"
    } else if b.starts_with(b"fLaC") {
        "audio/flac"
    } else if b.starts_with(b"%PDF-") {
        "application/pdf"
    } else if mp3(b) {
        "audio/mpeg"
    } else {
        OCTET_STREAM
    };
    Ok(Sniffed::of(mime))
}

/// Whether a kept type is served as itself, inline (`Content-Disposition:
/// inline`): images, video, audio and PDF. Anything else — an SVG, an HTML
/// page, any file — is `application/octet-stream`, an attachment.
pub fn served_inline(mime: &str) -> bool {
    !matches!(Kind::of(mime), Kind::File)
}

/// A file's name as kept: its last path segment, no control characters, at
/// most 200 characters; `None` for nothing left.
pub fn clean_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().filter(|c| !c.is_control()).take(200).collect();
    let clean = clean.trim().to_string();
    (!clean.is_empty() && clean != "." && clean != "..").then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(b: &[u8]) -> (&'static str, &'static str) {
        let s = sniff(b).unwrap();
        (s.mime, s.kind.as_str())
    }

    #[test]
    fn every_kind_is_read_from_its_bytes_never_its_name() {
        let png = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13][..], b"IHDR", &5u32.to_be_bytes(), &6u32.to_be_bytes()].concat();
        assert_eq!(sniff(&png).map(|s| (s.kind, s.width, s.height)), Ok((Kind::Image, 5, 6)));
        assert_eq!(kind(&[&[0, 0, 0, 0x20][..], b"ftypisom", &[0; 8]].concat()), ("video/mp4", "video"));
        assert_eq!(kind(&[&[0, 0, 0, 0x14][..], b"ftypqt  ", &[0; 8]].concat()), ("video/quicktime", "video"));
        assert_eq!(kind(&[&[0, 0, 0, 0x20][..], b"ftypM4A ", &[0; 8]].concat()), ("audio/mp4", "audio"));
        assert_eq!(kind(&[&[0, 0, 0, 0x20][..], b"ftypheic", &[0; 8]].concat()), (OCTET_STREAM, "file"), "a HEIC is no video");
        assert_eq!(kind(&[&[0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x82, 0x84][..], b"webm", &[0; 8]].concat()), ("video/webm", "video"));
        assert_eq!(kind(&[&[0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x82, 0x88][..], b"matroska"].concat()), (OCTET_STREAM, "file"));
        assert_eq!(kind(b"OggS\0\x02rest"), ("audio/ogg", "audio"));
        assert_eq!(kind(b"RIFF\0\0\0\0WAVEfmt "), ("audio/wav", "audio"));
        assert_eq!(kind(b"fLaC\0\0\0\x22"), ("audio/flac", "audio"));
        assert_eq!(kind(b"ID3\x04\0\0\0\0"), ("audio/mpeg", "audio"));
        assert_eq!(kind(&[0xFF, 0xFB, 0x90, 0x64, 0]), ("audio/mpeg", "audio"));
        assert_eq!(kind(b"%PDF-1.7\n%"), ("application/pdf", "pdf"));
        assert_eq!(kind(b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>"), (OCTET_STREAM, "file"), "an SVG is a file");
        assert_eq!(kind(b"<!doctype html><script>alert(1)</script>"), (OCTET_STREAM, "file"), "a page is a file");
        assert_eq!(kind(b"just words"), (OCTET_STREAM, "file"));
        assert!(sniff(&png[..14]).is_err(), "a truncated image is still refused");
    }

    #[test]
    fn only_media_is_served_inline() {
        for m in ["image/png", "video/mp4", "video/webm", "audio/mpeg", "application/pdf"] {
            assert!(served_inline(m), "{m}");
        }
        for m in [OCTET_STREAM, "image/svg+xml", "text/html", "text/plain"] {
            assert!(!served_inline(m), "{m}");
        }
    }

    #[test]
    fn a_name_is_its_last_segment_without_controls() {
        assert_eq!(clean_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(clean_name("C:\\Users\\me\\deck.pdf").as_deref(), Some("deck.pdf"));
        assert_eq!(clean_name("a\nb\r.txt").as_deref(), Some("ab.txt"));
        assert_eq!(clean_name("  ").as_deref(), None);
        assert_eq!(clean_name("..").as_deref(), None);
        assert_eq!(clean_name(&"x".repeat(500)).map(|n| n.len()), Some(200));
    }
}
