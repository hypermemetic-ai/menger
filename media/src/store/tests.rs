//! v1's media store tests (tickets-api/src/store/tests_media.rs at v0.0.1),
//! on the service's own file: `put` keeps an image content-addressed and
//! answers what it is; `bytes` answers them back; the same image twice is
//! one row and each owner's; a non-image, an SVG, a mismatch and an image
//! past the cap are refused naming why and keep nothing; images survive a
//! reopen, and a file without the tables gains them, empty. Who sees one
//! is the activation's (tests/wire_media.rs), asked of the tickets service.

use super::*;
use crate::kinds;
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
    let put = s.put(on("OPEN-2"), b64(&bytes), None, Some("key:aa".into())).await.unwrap();
    assert!(!put.duplicate);
    assert_eq!(put.info.hash, image::hash(&bytes));
    assert_eq!(put.info.hash.len(), 64);
    assert_eq!((put.info.mime.as_str(), put.info.bytes, put.info.width, put.info.height), ("image/png", bytes.len() as u64, 640, 480));
    let (info, got) = s.bytes(&put.info.hash).await.unwrap();
    assert_eq!((info, got), (put.info.clone(), bytes.clone()));
    assert_eq!(s.kept(&put.info.hash).await.unwrap(), Kept { info: put.info.clone(), owners: vec![on("OPEN-2")] });
    // A `data:` URL's prefix, and whitespace, are read past.
    let again = s.put(on("OPEN-2"), format!("data:image/png;base64,{}\n", b64(&bytes)), None, None).await.unwrap();
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
    let first = s.put(on("A-1"), b64(&bytes), None, None).await.unwrap();
    let same = s.put(on("A-1"), b64(&bytes), None, None).await.unwrap();
    let other = s.put(on("B-1"), b64(&bytes), None, None).await.unwrap();
    let app = s.put(Owner::App(Some("key:aa".into())), b64(&bytes), None, Some("key:aa".into())).await.unwrap();
    assert!(!first.duplicate && same.duplicate && other.duplicate && app.duplicate);
    assert_eq!((same.info.hash.as_str(), other.info.hash.as_str()), (first.info.hash.as_str(), first.info.hash.as_str()));
    let conn = Connection::open(&path).unwrap();
    let count = |t: &str| conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!((count("media"), count("media_owners")), (1, 3), "one image, kept for two tickets and a putter");
    assert_eq!(s.count(), 1);
    assert_eq!(s.kept(&first.info.hash).await.unwrap().owners, vec![on("A-1"), on("B-1"), Owner::App(Some("key:aa".into()))], "in the order put");
}

#[tokio::test]
async fn any_file_is_kept_by_its_bytes_and_only_a_broken_image_or_too_much_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    // EDITOR-10: an SVG, a page, a note — kept, as files: never served as themselves.
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#;
    let kept = s.put(on("OPEN-2"), b64(svg), Some("x.svg".into()), None).await.unwrap();
    assert_eq!((kept.info.mime.as_str(), kept.info.kind.as_str(), kept.info.name.as_deref()), (kinds::OCTET_STREAM, "file", Some("x.svg")));
    let pdf = s.put(on("OPEN-2"), b64(b"%PDF-1.7 a document"), Some("deck.pdf".into()), None).await.unwrap();
    assert_eq!((pdf.info.mime.as_str(), pdf.info.kind.as_str()), ("application/pdf", "pdf"));
    // The bytes decide, never the name.
    let named = s.put(on("OPEN-2"), b64(&png(1, 1, 0)), Some("not-a-gif.gif".into()), None).await.unwrap();
    assert_eq!(named.info.mime, "image/png");
    let p = refused(s.put(on("OPEN-2"), b64(&png(1, 1, 0)[..14]), None, None).await.unwrap_err());
    assert_eq!(p.reason(), "truncated");
    let mut nine = png(1, 1, 0);
    nine.resize(9 * 1024 * 1024, 0);
    let p = refused(s.put(on("OPEN-2"), b64(&nine), None, None).await.unwrap_err());
    assert_eq!(p, Problem::TooLarge { bytes: 9 * 1024 * 1024, max: image::MAX_BYTES as u64 });
    assert!(p.to_string().contains("at most 7.0 MB"), "{p}");
    let p = refused(s.put(on("OPEN-2"), "not base64 at all!".into(), None, None).await.unwrap_err());
    assert_eq!(p.reason(), "bad_data");
    assert_eq!(s.count(), 3, "the svg, the pdf, the png");
    // At the cap exactly, kept.
    let mut seven = png(1, 1, 0);
    seven.resize(image::MAX_BYTES, 0);
    assert!(s.put(on("OPEN-2"), b64(&seven), None, None).await.is_ok());
}

/// A file of `n` bytes that is an MP4 by its first ones.
fn mp4(n: usize) -> Vec<u8> {
    let mut b = [&[0, 0, 0, 0x20][..], b"ftypisom"].concat();
    b.extend((0..n - 12).map(|i| (i % 251) as u8));
    b
}

#[tokio::test]
async fn a_large_file_goes_up_in_chunks_to_disk_resumes_and_reads_by_range() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let file = mp4(5 * 1024 * 1024 + 17);
    let begun = s.begin(on("V-1"), Some("walk.mp4".into()), file.len() as u64, None, "key:aa".into(), Some("r1".into())).await.unwrap();
    assert_eq!((begun.received, begun.new), (0, true));
    let id = begun.upload.clone();
    let part = dir.path().join("media/blobs/tmp").join(format!("{id}.part"));
    assert!(part.is_file(), "the bytes go to a part file");
    let step = 2 * 1024 * 1024;
    let c = s.chunk(&id, "key:aa", 0, file[..step].to_vec()).await.unwrap();
    assert_eq!((c.received, c.duplicate), (step as u64, false));
    // Another's upload is none; a gap is refused naming the offset expected; a chunk again is a duplicate.
    assert!(matches!(s.chunk(&id, "key:bb", step as u64, file[step..2 * step].to_vec()).await, Err(StoreError::NotFound(_))));
    assert_eq!(refused(s.chunk(&id, "key:aa", 3 * step as u64, vec![1]).await.unwrap_err()), Problem::OutOfOrder { expected: step as u64, got: 3 * step as u64 });
    assert!(s.chunk(&id, "key:aa", 0, file[..step].to_vec()).await.unwrap().duplicate);
    // Finished too soon: refused, the upload kept.
    assert_eq!(refused(s.finish(&id, "key:aa").await.unwrap_err()).reason(), "incomplete");
    // A dropped connection, and a restart: the store reopened, the upload resumed from what it has.
    drop(s);
    let s = open(&dir);
    let again = s.resume(&id, "key:aa").await.unwrap();
    assert_eq!((again.received, again.size, again.new), (step as u64, file.len() as u64, false));
    assert!(matches!(s.resume(&id, "key:bb").await, Err(StoreError::NotFound(_))), "not another's");
    let mut at = step;
    while at < file.len() {
        let end = (at + step).min(file.len());
        s.chunk(&id, "key:aa", at as u64, file[at..end].to_vec()).await.unwrap();
        at = end;
    }
    assert_eq!(refused(s.chunk(&id, "key:aa", file.len() as u64, vec![9]).await.unwrap_err()).reason(), "past_size");
    let done = s.finish(&id, "key:aa").await.unwrap();
    assert_eq!(done.reservation.as_deref(), Some("r1"));
    let info = done.put.info;
    assert_eq!((info.hash.clone(), info.mime.as_str(), info.kind.as_str(), info.name.as_deref(), info.bytes), (image::hash(&file), "video/mp4", "video", Some("walk.mp4"), file.len() as u64));
    assert!(dir.path().join("media/blobs").join(&info.hash).is_file(), "kept on disk by its hash");
    assert!(!part.exists(), "the part file renamed");
    let conn = Connection::open(dir.path().join("media.db")).unwrap();
    let (len, disk): (i64, i64) = conn.query_row("SELECT length(data), blob FROM media WHERE hash = ?1", [&info.hash], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((len, disk), (0, 1), "no bytes in the database");
    // Read by range, and whole.
    assert_eq!(s.read_at(&info.hash, 1000, 5000).await.unwrap(), file[1000..6000].to_vec());
    assert_eq!(s.read_at(&info.hash, file.len() as u64 - 3, 100).await.unwrap(), file[file.len() - 3..].to_vec());
    assert!(s.read_at(&info.hash, file.len() as u64, 10).await.unwrap().is_empty());
    assert_eq!(s.bytes(&info.hash).await.unwrap().1, file);
    // The same bytes again: one file, the part dropped.
    let twice = s.begin(on("V-2"), None, file.len() as u64, None, "key:aa".into(), None).await.unwrap();
    s.chunk(&twice.upload, "key:aa", 0, file[..step].to_vec()).await.unwrap();
    let mut at = step;
    while at < file.len() {
        let end = (at + step).min(file.len());
        s.chunk(&twice.upload, "key:aa", at as u64, file[at..end].to_vec()).await.unwrap();
        at = end;
    }
    let dup = s.finish(&twice.upload, "key:aa").await.unwrap();
    assert!(dup.put.duplicate);
    assert_eq!(s.kept(&info.hash).await.unwrap().owners, vec![on("V-1"), on("V-2")]);
    assert_eq!(s.usage("key:aa").await.unwrap(), (1, file.len() as u64), "each file once");
    // A cap, when the server names one; nothing empty.
    assert_eq!(refused(s.begin(on("V-1"), None, 11, Some(10), "key:aa".into(), None).await.unwrap_err()).reason(), "too_large");
    assert_eq!(refused(s.begin(on("V-1"), None, 0, None, "key:aa".into(), None).await.unwrap_err()).reason(), "bad_data");
}

#[tokio::test]
async fn an_abandoned_upload_is_swept_with_its_part_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let b = s.begin(on("V-1"), None, 100, None, "key:aa".into(), Some("hold".into())).await.unwrap();
    s.chunk(&b.upload, "key:aa", 0, vec![1; 10]).await.unwrap();
    assert!(s.sweep(UPLOAD_TTL).await.unwrap().is_empty(), "a fresh upload stays");
    let gone = s.sweep(-10).await.unwrap();
    assert_eq!(gone, vec![Dropped { upload: b.upload.clone(), reservation: Some("hold".into()) }]);
    assert!(!dir.path().join("media/blobs/tmp").join(format!("{}.part", b.upload)).exists());
    assert!(matches!(s.chunk(&b.upload, "key:aa", 10, vec![1]).await, Err(StoreError::NotFound(_))));
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
        s.put(on("K-1"), b64(&bytes), None, Some("key:k".into())).await.unwrap().info.hash
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

/// V2-29: an image put for a document is that document's, kept apart from
/// a ticket's and the putter's, and read back as it was put.
#[tokio::test]
async fn an_image_put_for_a_doc_is_the_docs() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let bytes = png(4, 4, 9);
    let put = s.put(Owner::Doc("7k3mq2xa".into()), b64(&bytes), None, Some("key:aa".into())).await.unwrap();
    s.put(on("OPEN-2"), b64(&bytes), None, None).await.unwrap();
    assert_eq!(s.kept(&put.info.hash).await.unwrap().owners, vec![Owner::Doc("7k3mq2xa".into()), on("OPEN-2")]);
}

/// V2-69: what each principal keeps is what they are charged — a put new to
/// the putter is `charged` (a duplicate of another's file too), the same
/// file again (for another ticket, or again) is not; the usage reads agree
/// with it; a row from before uploaders were recorded is its principal's
/// when it was put with no ticket, and no one's (unattributed) for a
/// ticket, as a link's preview is.
#[tokio::test]
async fn each_keeper_is_charged_once_and_the_reads_agree() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let x = png(8, 8, 1);
    let n = x.len() as u64;
    let first = s.put(on("A-1"), b64(&x), None, Some("key:aa".into())).await.unwrap();
    assert!(!first.duplicate && first.charged, "new to everyone: charged");
    let again = s.put(on("A-2"), b64(&x), None, Some("key:aa".into())).await.unwrap();
    assert!(again.duplicate && !again.charged, "theirs already, for another ticket: not twice");
    let other = s.put(Owner::App(Some("key:bb".into())), b64(&x), None, Some("key:bb".into())).await.unwrap();
    assert!(other.duplicate && other.charged, "a duplicate of another's file: charged to them too");
    // Another putter of the same file for a ticket that holds it already keeps no row of their own: nothing charged.
    let same_row = s.put(on("A-1"), b64(&x), None, Some("key:cc".into())).await.unwrap();
    assert!(same_row.duplicate && !same_row.charged);
    assert_eq!(s.usage("key:aa").await.unwrap(), (1, n));
    assert_eq!(s.usage("key:bb").await.unwrap(), (1, n));
    assert_eq!(s.usage("key:cc").await.unwrap(), (0, 0));
    // Rows from before: put_by unrecorded.
    let y = png(9, 9, 2);
    let z = png(10, 10, 3);
    s.put(Owner::App(Some("key:dd".into())), b64(&y), None, None).await.unwrap();
    s.put(on("A-3"), b64(&z), None, None).await.unwrap();
    let preview = s.put_bytes(Owner::Anyone, png(11, 11, 4), None, None, kinds::PUT_MAX).await.unwrap();
    assert!(!preview.charged, "a link's preview is no one's");
    assert_eq!(s.usage("key:dd").await.unwrap(), (1, y.len() as u64), "put with no ticket: its principal's");
    let all = s.usage_charged().await.unwrap();
    assert_eq!(all, vec![("key:aa".into(), 1, n), ("key:bb".into(), 1, n), ("key:dd".into(), 1, y.len() as u64)]);
    assert_eq!(s.unattributed().await.unwrap(), (2, z.len() as u64 + preview.info.bytes), "the ticket's unrecorded file and the preview");
}
