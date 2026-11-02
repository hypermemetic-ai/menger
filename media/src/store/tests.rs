//! v1's media store tests (tickets-api/src/store/tests_media.rs at v0.0.1),
//! on the service's own file: `put` keeps an image content-addressed and
//! answers what it is; `bytes` answers them back; the same image twice is
//! one row and each owner's; a non-image, an SVG, a mismatch and an image
//! past the cap are refused naming why and keep nothing; images survive a
//! reopen, and a file without the tables gains them, empty. Who sees one
//! is the activation's (tests/wire_media.rs), asked of the tickets service.

use super::*;
use base64::Engine;

/// A PNG of `w`×`h` as the bytes `sniff` reads — the signature, IHDR, and
/// `pad` bytes more (so two of the same size can differ).
fn png(w: u32, h: u32, pad: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, pad]);
    b
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn on(id: &str) -> Owner {
    Owner::Ticket(id.to_string())
}

fn refused(e: StoreError) -> Problem {
    match e {
        StoreError::Refused(p) => p,
        other => panic!("not a refusal: {other:?}"),
    }
}

fn open(dir: &tempfile::TempDir) -> Arc<MediaStore> {
    Arc::new(MediaStore::open(&dir.path().join("media.db")).unwrap())
}

#[tokio::test]
async fn an_image_is_kept_by_its_hash_and_answered_with_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let bytes = png(640, 480, 1);
    let put = s.put(on("OPEN-2"), "image/png", b64(&bytes), Some("key:aa".into())).await.unwrap();
    assert!(!put.duplicate);
    assert_eq!(put.info.hash, image::hash(&bytes));
    assert_eq!(put.info.hash.len(), 64);
    assert_eq!((put.info.mime.as_str(), put.info.bytes, put.info.width, put.info.height), ("image/png", bytes.len() as u64, 640, 480));
    let (info, got) = s.bytes(&put.info.hash).await.unwrap();
    assert_eq!((info, got), (put.info.clone(), bytes.clone()));
    assert_eq!(s.kept(&put.info.hash).await.unwrap(), Kept { info: put.info.clone(), owners: vec![on("OPEN-2")] });
    // A `data:` URL's prefix, and whitespace, are read past.
    let again = s.put(on("OPEN-2"), "image/png", format!("data:image/png;base64,{}\n", b64(&bytes)), None).await.unwrap();
    assert_eq!(again.info, put.info);
    assert!(again.duplicate);
    assert!(matches!(s.bytes(&"0".repeat(64)).await, Err(StoreError::NotFound(_))));
    assert!(matches!(s.kept(&"0".repeat(64)).await, Err(StoreError::NotFound(_))));
}

#[tokio::test]
async fn the_same_image_twice_is_one_row_and_each_owners() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.db");
    let s = open(&dir);
    let bytes = png(2, 2, 7);
    let first = s.put(on("A-1"), "image/png", b64(&bytes), None).await.unwrap();
    let same = s.put(on("A-1"), "image/png", b64(&bytes), None).await.unwrap();
    let other = s.put(on("B-1"), "image/png", b64(&bytes), None).await.unwrap();
    let app = s.put(Owner::App(Some("key:aa".into())), "image/png", b64(&bytes), Some("key:aa".into())).await.unwrap();
    assert!(!first.duplicate && same.duplicate && other.duplicate && app.duplicate);
    assert_eq!((same.info.hash.as_str(), other.info.hash.as_str()), (first.info.hash.as_str(), first.info.hash.as_str()));
    let conn = Connection::open(&path).unwrap();
    let count = |t: &str| conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!((count("media"), count("media_owners")), (1, 3), "one image, kept for two tickets and a putter");
    assert_eq!(s.count(), 1);
    assert_eq!(s.kept(&first.info.hash).await.unwrap().owners, vec![on("A-1"), on("B-1"), Owner::App(Some("key:aa".into()))], "in the order put");
}

#[tokio::test]
async fn a_non_image_an_svg_a_mismatch_and_too_much_keep_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#;
    let p = refused(s.put(on("OPEN-2"), "image/svg+xml", b64(svg), None).await.unwrap_err());
    assert_eq!(p.reason(), "not_an_image");
    assert!(p.to_string().contains("SVG"), "{p}");
    let p = refused(s.put(on("OPEN-2"), "text/plain", b64(b"a note, not a picture"), None).await.unwrap_err());
    assert!(p.to_string().contains("not an image menger keeps"), "{p}");
    let p = refused(s.put(on("OPEN-2"), "image/gif", b64(&png(1, 1, 0)), None).await.unwrap_err());
    assert_eq!(p.reason(), "type_mismatch");
    let mut nine = png(1, 1, 0);
    nine.resize(9 * 1024 * 1024, 0);
    let p = refused(s.put(on("OPEN-2"), "image/png", b64(&nine), None).await.unwrap_err());
    assert_eq!(p, Problem::TooLarge { bytes: 9 * 1024 * 1024, max: image::MAX_BYTES });
    assert!(p.to_string().contains("at most 7.0 MB"), "{p}");
    let p = refused(s.put(on("OPEN-2"), "image/png", "not base64 at all!".into(), None).await.unwrap_err());
    assert_eq!(p.reason(), "bad_data");
    assert_eq!(s.count(), 0, "nothing kept");
    // At the cap exactly, kept.
    let mut seven = png(1, 1, 0);
    seven.resize(image::MAX_BYTES, 0);
    assert!(s.put(on("OPEN-2"), "image/png", b64(&seven), None).await.is_ok());
    assert_eq!(s.count(), 1, "only the one at the cap");
}

fn has_tables(path: &Path) -> bool {
    let conn = Connection::open(path).unwrap();
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('media', 'media_owners')", [], |r| r.get::<_, i64>(0)).unwrap() == 2
}

#[tokio::test]
async fn images_survive_a_reopen_and_an_older_file_gains_the_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.db");
    let bytes = png(12, 34, 5);
    let hash = {
        let s = open(&dir);
        s.put(on("K-1"), "image/png", b64(&bytes), Some("key:k".into())).await.unwrap().info.hash
    };
    {
        let s = open(&dir);
        let (info, got) = s.bytes(&hash).await.unwrap();
        assert_eq!((info.width, info.height, got), (12, 34, bytes.clone()));
        assert_eq!(s.kept(&hash).await.unwrap().owners, vec![on("K-1")], "whose it is, as stored");
    }
    // A file with no tables (an empty one): the open makes them.
    Connection::open(&path).unwrap().execute_batch("DROP TABLE media; DROP TABLE media_owners").unwrap();
    assert!(!has_tables(&path));
    let s = open(&dir);
    assert!(has_tables(&path));
    assert_eq!(s.count(), 0);
    // A directory not there yet is made.
    let deep = dir.path().join("a/b/media.db");
    assert!(MediaStore::open(&deep).is_ok() && deep.exists());
}
