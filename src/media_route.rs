//! V2-24, EDITOR-10: **`GET /media/<hash>`** — a kept file, to a signed-in
//! session that may see it. The permit is the media service's word, asked
//! `media.info` as the session (a client through plexus-core's in-memory
//! transport: the gate admits or refuses it, the service decides whose the
//! file is); then the egress is asked of the session's allowance; then the
//! bytes, by the [`menger_media::BlobStore`]'s choice:
//!
//! - a store that signs addresses (an object store) answers one, and the
//!   route redirects there (`307`, never cached) — the bytes never pass
//!   through menger;
//! - otherwise (the disk) the route streams them itself, a megabyte at a
//!   time, honouring one `Range` (`206`, `Content-Range`; `416` past the
//!   end) so a video seeks.
//!
//! Served by kind ([`menger_media::kinds::served_inline`]): an image,
//! video, audio or PDF as itself, `inline`; any other file — an SVG, an
//! HTML page — as `application/octet-stream`, an `attachment`, so nothing
//! menger keeps runs as a page of this origin. Always `nosniff`; kept
//! forever (content-addressed); framed by this origin alone (a PDF in the
//! editor's viewer), and every answer but a PDF's under a sandboxing policy.

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use menger_media::allowance::{source, Allowance};
use menger_media::{kinds, MediaInfo, MediaStore};
use plexus_core::plexus::DynamicHub;
use std::sync::Arc;
use std::time::Duration;

/// What the route serves from.
#[derive(Clone)]
pub struct MediaServe {
    /// The running service: `media.info` asked as the session.
    pub service: Arc<DynamicHub>,
    /// The media service's store: its rows and its blob store.
    pub store: Arc<MediaStore>,
    /// The allowance the egress is asked of.
    pub allowance: Arc<dyn Allowance>,
}

/// How long a signed address lasts.
pub const PRESIGN_TTL: Duration = Duration::from_secs(300);

/// How much is read and sent at a time.
const PIECE: usize = 1024 * 1024;

/// The policy a served file is under: nothing in it runs, and only this
/// origin frames it. A PDF goes without `sandbox` — the browser's viewer
/// does not draw in a sandboxed document.
const SANDBOXED: &str = "default-src 'none'; img-src 'self'; media-src 'self'; style-src 'unsafe-inline'; frame-ancestors 'self'; sandbox";
const PDF_POLICY: &str = "frame-ancestors 'self'";

/// A `Range` that starts past the file's end: `416`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsatisfiable;

/// One `Range: bytes=…` against `size`: `Ok(None)` for none (or one this
/// does not read — several ranges, another unit: the whole file),
/// [`Unsatisfiable`] for one past the end.
pub fn range(headers: &HeaderMap, size: u64) -> Result<Option<(u64, u64)>, Unsatisfiable> {
    let Some(spec) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().strip_prefix("bytes=")) else { return Ok(None) };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((a, b)) = spec.split_once('-') else { return Ok(None) };
    let (a, b) = (a.trim(), b.trim());
    let (start, end) = match (a.parse::<u64>().ok(), b.parse::<u64>().ok()) {
        (Some(s), Some(e)) if s <= e => (s, e.min(size.saturating_sub(1))),
        (Some(s), None) if b.is_empty() => (s, size.saturating_sub(1)),
        (None, Some(n)) if a.is_empty() && n > 0 => (size.saturating_sub(n), size.saturating_sub(1)),
        _ => return Ok(None),
    };
    if size == 0 || start >= size {
        return Err(Unsatisfiable);
    }
    Ok(Some((start, end)))
}

/// `Content-Disposition` for a file: inline for media, else an attachment;
/// its name in both spellings (RFC 6266), the plain one ASCII alone.
pub fn disposition(info: &MediaInfo) -> String {
    let how = if kinds::served_inline(&info.mime) { "inline" } else { "attachment" };
    let name = info.name.clone().unwrap_or_else(|| format!("{}.{}", &info.hash[..12], extension(&info.mime)));
    let plain: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
    let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>().replace('+', "%20");
    format!("{how}; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "audio/mp4" => "m4a",
        "audio/flac" => "flac",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

/// The route.
pub async fn get(
    State((serve, login)): State<(MediaServe, Option<Arc<crate::web_login::Login>>)>,
    axum::extract::Path(hash): axum::extract::Path<String>,
    method: axum::http::Method,
    headers: HeaderMap,
) -> Response {
    use crate::shell::NO_STORE;
    let miss = |why: &'static str| (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, NO_STORE)], why).into_response();
    let Some(caller) = login.as_ref().and_then(|l| l.session(&headers)) else { return miss("no session") };
    let principal = caller.principal().map(|p| p.to_string()).unwrap_or_default();
    let info = match menger_media::fetch(serve.service.clone(), caller, &hash).await {
        menger_media::Fetched::Found(info) => info,
        menger_media::Fetched::NotFound => return miss("no such file"),
        menger_media::Fetched::Refused(why) => {
            tracing::debug!("/media/{hash} refused: {why}");
            return miss("no such file");
        }
    };
    answer(&serve, &principal, info, &headers, method == axum::http::Method::HEAD).await
}

/// The answer for a file the session may see: the egress asked of
/// `principal`'s allowance, then a redirect to a signed address or the
/// bytes streamed.
pub async fn answer(serve: &MediaServe, principal: &str, info: MediaInfo, headers: &HeaderMap, head: bool) -> Response {
    use crate::shell::{IMMUTABLE, NO_STORE};
    let size = info.bytes;
    let span = match range(headers, size) {
        Ok(r) => r,
        Err(Unsatisfiable) => {
            return (StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{size}")), (header::CACHE_CONTROL, NO_STORE.to_string())], "").into_response();
        }
    };
    let (start, end) = span.unwrap_or((0, size.saturating_sub(1)));
    let len = if size == 0 { 0 } else { end - start + 1 };
    // The egress, asked of the session's allowance before a byte goes (a
    // HEAD sends none).
    if !head {
        match serve.allowance.reserve(principal, source::EGRESS_BYTES, len).await {
            Ok(r) => serve.allowance.commit(&r.id).await,
            Err(d) => return (StatusCode::TOO_MANY_REQUESTS, [(header::CACHE_CONTROL, NO_STORE)], d.message).into_response(),
        }
    }
    if let Some(url) = serve.store.presign(&info.hash, PRESIGN_TTL).await {
        return (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, url), (header::CACHE_CONTROL, "private, no-store".to_string())]).into_response();
    }
    let inline = kinds::served_inline(&info.mime);
    let mime = if inline { info.mime.clone() } else { kinds::OCTET_STREAM.to_string() };
    let policy = if info.mime == "application/pdf" { PDF_POLICY } else { SANDBOXED };
    let mut out = HeaderMap::new();
    let mut put = |k: header::HeaderName, v: &str| {
        if let Ok(v) = HeaderValue::from_str(v) {
            out.insert(k, v);
        }
    };
    put(header::CONTENT_TYPE, &mime);
    put(header::CONTENT_DISPOSITION, &disposition(&info));
    put(header::CONTENT_LENGTH, &len.to_string());
    put(header::ACCEPT_RANGES, "bytes");
    put(header::CACHE_CONTROL, IMMUTABLE);
    put(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    put(header::X_FRAME_OPTIONS, "SAMEORIGIN");
    put(header::CONTENT_SECURITY_POLICY, policy);
    put(header::ETAG, &format!("\"{}\"", info.hash));
    let status = if span.is_some() {
        put(header::CONTENT_RANGE, &format!("bytes {start}-{end}/{size}"));
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let (store, hash) = (serve.store.clone(), info.hash.clone());
    let body = futures::stream::unfold((start, len), move |(at, left)| {
        let (store, hash) = (store.clone(), hash.clone());
        async move {
            if left == 0 {
                return None;
            }
            let want = PIECE.min(left as usize);
            match store.read_at(&hash, at, want).await {
                Ok(piece) if !piece.is_empty() => {
                    let n = piece.len() as u64;
                    Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(piece)), (at + n, left - n)))
                }
                Ok(_) => None,
                Err(e) => Some((Err(std::io::Error::other(e.to_string())), (at, 0))),
            }
        }
    });
    (status, out, axum::body::Body::from_stream(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_media::allowance::{Denied, Reservation, Unlimited};
    use menger_media::{BlobError, BlobStore, Owner};

    /// An object store's stand-in: keeps bytes in memory and signs an
    /// address for each.
    #[derive(Debug, Default)]
    struct Signing(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

    #[async_trait::async_trait]
    impl BlobStore for Signing {
        async fn put(&self, hash: &str, bytes: Vec<u8>) -> Result<(), BlobError> {
            self.0.lock().unwrap().insert(hash.into(), bytes);
            Ok(())
        }
        async fn begin_upload(&self, _: &str) -> Result<(), BlobError> {
            Ok(())
        }
        async fn append(&self, _: &str, _: u64, _: Vec<u8>) -> Result<u64, BlobError> {
            Err(BlobError::Io("not in this fake".into()))
        }
        async fn uploaded(&self, _: &str) -> Result<u64, BlobError> {
            Ok(0)
        }
        async fn seal(&self, _: &str) -> Result<(), BlobError> {
            Ok(())
        }
        async fn read_sealed(&self, _: &str, _: u64, _: usize) -> Result<Vec<u8>, BlobError> {
            Ok(Vec::new())
        }
        async fn commit(&self, _: &str, _: &str) -> Result<(), BlobError> {
            Ok(())
        }
        async fn abort(&self, _: &str) -> Result<(), BlobError> {
            Ok(())
        }
        async fn get_range(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>, BlobError> {
            let b = self.0.lock().unwrap().get(hash).cloned().ok_or(BlobError::NotFound(hash.into()))?;
            let at = (offset as usize).min(b.len());
            Ok(b[at..(at + len).min(b.len())].to_vec())
        }
        async fn stat(&self, hash: &str) -> Result<Option<u64>, BlobError> {
            Ok(self.0.lock().unwrap().get(hash).map(|b| b.len() as u64))
        }
        async fn delete(&self, hash: &str) -> Result<(), BlobError> {
            self.0.lock().unwrap().remove(hash);
            Ok(())
        }
        async fn list_orphans(&self, _: Duration) -> Result<Vec<String>, BlobError> {
            Ok(Vec::new())
        }
        async fn presign(&self, hash: &str, ttl: Duration) -> Option<String> {
            Some(format!("https://bucket.example/{hash}?expires={}&sig=fake", ttl.as_secs()))
        }
        fn describe(&self) -> String {
            "a signing fake".into()
        }
    }

    /// An allowance with nothing left.
    #[derive(Debug)]
    struct Spent;

    #[async_trait::async_trait]
    impl Allowance for Spent {
        async fn reserve(&self, _: &str, source: &str, _: u64) -> Result<Reservation, Denied> {
            Err(Denied { remaining: Some(0), message: format!("{source}: none left") })
        }
        async fn commit(&self, _: &str) {}
        async fn release(&self, _: &str) {}
    }

    async fn serve_over(blobs: Arc<dyn BlobStore>, allowance: Arc<dyn Allowance>) -> (tempfile::TempDir, MediaServe, MediaInfo) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MediaStore::open_with(&dir.path().join("media.db"), blobs).unwrap());
        let put = store.put_bytes(Owner::App(Some("key:aa".into())), b"%PDF-1.7 hello".to_vec(), Some("deck.pdf".into()), Some("key:aa".into()), 1 << 20).await.unwrap();
        let service = Arc::new(DynamicHub::new("menger"));
        (dir, MediaServe { service, store, allowance }, put.info)
    }

    #[tokio::test]
    async fn a_store_that_signs_is_redirected_to_and_the_disk_is_streamed() {
        let (_d, serve, info) = serve_over(Arc::new(Signing::default()), Arc::new(Unlimited)).await;
        let r = answer(&serve, "key:aa", info.clone(), &HeaderMap::new(), false).await;
        assert_eq!(r.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(r.headers()[header::LOCATION], format!("https://bucket.example/{}?expires=300&sig=fake", info.hash));
        assert_eq!(r.headers()[header::CACHE_CONTROL], "private, no-store", "a signed address is never kept");
        // The disk signs nothing: the bytes, by range.
        let dir = tempfile::tempdir().unwrap();
        let disk: Arc<dyn BlobStore> = Arc::new(menger_media::DiskStore::open(dir.path()).unwrap());
        let (_d2, serve, info) = serve_over(disk, Arc::new(Unlimited)).await;
        let mut h = HeaderMap::new();
        h.insert(header::RANGE, HeaderValue::from_static("bytes=1-3"));
        let r = answer(&serve, "key:aa", info, &h, false).await;
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(r.headers()[header::CONTENT_RANGE], "bytes 1-3/14");
        assert_eq!(r.headers()[header::CONTENT_TYPE], "application/pdf");
        assert_eq!(r.headers()[header::CONTENT_DISPOSITION], "inline; filename=\"deck.pdf\"; filename*=UTF-8''deck.pdf");
        assert_eq!(r.headers()[header::X_FRAME_OPTIONS], "SAMEORIGIN", "the editor frames a PDF; nobody else");
        let body = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert_eq!(&body[..], b"PDF");
    }

    #[tokio::test]
    async fn no_egress_left_is_no_bytes_and_no_address() {
        let (_d, serve, info) = serve_over(Arc::new(Signing::default()), Arc::new(Spent)).await;
        let r = answer(&serve, "key:aa", info, &HeaderMap::new(), false).await;
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(r.headers().get(header::LOCATION).is_none());
    }

    fn h(r: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(header::RANGE, HeaderValue::from_str(r).unwrap());
        m
    }

    #[test]
    fn a_range_is_read_as_one_span_or_the_whole() {
        assert_eq!(range(&HeaderMap::new(), 10), Ok(None));
        assert_eq!(range(&h("bytes=0-"), 10), Ok(Some((0, 9))));
        assert_eq!(range(&h("bytes=2-4"), 10), Ok(Some((2, 4))));
        assert_eq!(range(&h("bytes=5-99"), 10), Ok(Some((5, 9))));
        assert_eq!(range(&h("bytes=-3"), 10), Ok(Some((7, 9))));
        assert_eq!(range(&h("bytes=10-"), 10), Err(Unsatisfiable));
        assert_eq!(range(&h("bytes=0-1,4-5"), 10), Ok(None), "several: the whole");
        assert_eq!(range(&h("items=0-1"), 10), Ok(None));
    }

    #[test]
    fn media_is_inline_and_anything_else_an_attachment_named() {
        let info = |mime: &str, name: Option<&str>| MediaInfo { hash: "ab".repeat(32), mime: mime.into(), bytes: 1, width: 0, height: 0, kind: kinds::Kind::of(mime).as_str().into(), name: name.map(str::to_string) };
        assert_eq!(disposition(&info("video/mp4", Some("clip one.mp4"))), "inline; filename=\"clip one.mp4\"; filename*=UTF-8''clip%20one.mp4");
        assert_eq!(disposition(&info(kinds::OCTET_STREAM, Some("évil\".svg"))), "attachment; filename=\"_vil_.svg\"; filename*=UTF-8''%C3%A9vil%22.svg");
        assert_eq!(disposition(&info("application/pdf", None)), format!("inline; filename=\"{}.pdf\"; filename*=UTF-8''{}.pdf", "ab".repeat(6), "ab".repeat(6)));
    }
}
