//! COMMENT-5: the media store — `put` keeps an image content-addressed and
//! answers what it is; `get` answers its bytes; the same image twice is one
//! row; an image is seen through the reach of a ticket it was put for, and
//! put on the app by its putter alone; a non-image, an SVG, a mismatch and
//! an image past the cap are refused naming why and keep nothing; images
//! survive a reopen, and a file without the tables gains them, empty.

use super::tests_reach::{tracker, viewer_of};
use super::*;
use crate::access::{Access, Editor, Held, VerifiedUser};
use crate::address::{OrgId, ShortcodeId};
use crate::members::Role;
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

/// The system's editor permit on `id`, as a put takes it.
fn on(id: &str) -> OnMedia {
    SYS.permit::<Editor>(id).into()
}

/// A signed-in viewer of `shortcodes` who is `principal`.
fn someone(principal: &str, shortcodes: &[&str]) -> VerifiedUser {
    let held = shortcodes.iter().map(|s| (Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(s) }, Role::Viewer)).collect();
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), held)
}

fn refused(e: StoreError) -> crate::media::Problem {
    match e {
        StoreError::MediaRefused(p) => p,
        other => panic!("not a refusal: {other:?}"),
    }
}

#[tokio::test]
async fn an_image_is_kept_by_its_hash_and_answered_with_its_bytes() {
    let s = tracker().await;
    let bytes = png(640, 480, 1);
    let put = s.put_media(on("OPEN-2"), "image/png", b64(&bytes), None).await.unwrap();
    assert!(!put.duplicate);
    assert_eq!(put.info.hash, media::hash(&bytes));
    assert_eq!(put.info.hash.len(), 64);
    assert_eq!((put.info.mime.as_str(), put.info.bytes, put.info.width, put.info.height), ("image/png", bytes.len() as u64, 640, 480));
    let (info, got) = s.media(SYS, &put.info.hash).await.unwrap();
    assert_eq!((info, got), (put.info.clone(), bytes.clone()));
    // A `data:` URL's prefix, and whitespace, are read past.
    let again = s.put_media(on("OPEN-2"), "image/png", format!("data:image/png;base64,{}\n", b64(&bytes)), None).await.unwrap();
    assert_eq!(again.info, put.info);
    assert!(matches!(s.media(SYS, &"0".repeat(64)).await, Err(StoreError::MediaNotFound(_))));
}

#[tokio::test]
async fn the_same_image_twice_is_one_row_and_each_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.db");
    let s = Store::open(&path).unwrap();
    s.create(SYS, tests_reach::new("A", Kind::Scope, None, &[], "")).await.unwrap();
    s.create(SYS, tests_reach::new("B", Kind::Scope, None, &[], "")).await.unwrap();
    let bytes = png(2, 2, 7);
    let first = s.put_media(on("A-1"), "image/png", b64(&bytes), None).await.unwrap();
    let same = s.put_media(on("A-1"), "image/png", b64(&bytes), None).await.unwrap();
    let other = s.put_media(on("B-1"), "image/png", b64(&bytes), None).await.unwrap();
    assert!(!first.duplicate && same.duplicate && other.duplicate);
    assert_eq!((same.info.hash.as_str(), other.info.hash.as_str()), (first.info.hash.as_str(), first.info.hash.as_str()));
    let conn = Connection::open(&path).unwrap();
    let count = |t: &str| conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!((count("media"), count("media_owners")), (1, 2), "one image, kept for two tickets");
    assert_eq!(s.media_count(SYS), 1);
    // B's viewer sees it now; one of neither does not.
    assert!(s.media_info(&viewer_of(&["B"]), &first.info.hash).is_ok());
    assert!(s.media_info(&viewer_of(&["NONE"]), &first.info.hash).is_err());
}

#[tokio::test]
async fn an_image_is_seen_through_its_tickets_reach() {
    let s = tracker().await;
    let secret = s.put_media(on("SECRET-2"), "image/png", b64(&png(9, 9, 0)), None).await.unwrap().info.hash;
    let open = s.put_media(on("OPEN-3"), "image/png", b64(&png(9, 9, 1)), None).await.unwrap().info.hash;
    let reader = viewer_of(&["OPEN"]);
    assert!(s.media(&reader, &open).await.is_ok(), "OPEN-3's, to OPEN's viewer");
    // ACCESS-10: one out of reach is one that is not there.
    match s.media(&reader, &secret).await {
        Err(StoreError::MediaNotFound(h)) => assert_eq!(h, secret),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        s.media(&reader, &secret).await.unwrap_err().to_string(),
        s.media(&reader, &"a".repeat(64)).await.unwrap_err().to_string().replace(&"a".repeat(64), &secret),
        "not found and not yours, one answer"
    );
    assert!(s.media(&viewer_of(&["SECRET"]), &secret).await.is_ok());
    // A write takes the permit's own ticket: one gone is not found.
    let gone = s.put_media(on("OPEN-99"), "image/png", b64(&png(1, 1, 3)), None).await.unwrap_err();
    assert!(matches!(gone, StoreError::NotFound(_)), "{gone:?}");
}

#[tokio::test]
async fn an_image_on_the_app_is_its_putters() {
    let s = tracker().await;
    let (ada, bob) = ("idp:00000000-0000-4000-8000-00000000000a", "idp:00000000-0000-4000-8000-00000000000b");
    let hash = s.put_media(OnMedia::app(), "image/png", b64(&png(3, 3, 0)), Some(ada.into())).await.unwrap().info.hash;
    assert!(s.media_info(&someone(ada, &["OPEN"]), &hash).is_ok(), "its putter's");
    assert!(s.media_info(&someone(bob, &["OPEN"]), &hash).is_err(), "not another's");
    assert!(s.media_info(SYS, &hash).is_ok(), "the system's, as an admin's of *");
}

#[tokio::test]
async fn a_non_image_an_svg_a_mismatch_and_too_much_keep_nothing() {
    let s = tracker().await;
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#;
    let p = refused(s.put_media(on("OPEN-2"), "image/svg+xml", b64(svg), None).await.unwrap_err());
    assert_eq!(p.reason(), "not_an_image");
    assert!(p.to_string().contains("SVG"), "{p}");
    let p = refused(s.put_media(on("OPEN-2"), "text/plain", b64(b"a note, not a picture"), None).await.unwrap_err());
    assert!(p.to_string().contains("not an image menger keeps"), "{p}");
    let p = refused(s.put_media(on("OPEN-2"), "image/gif", b64(&png(1, 1, 0)), None).await.unwrap_err());
    assert_eq!(p.reason(), "type_mismatch");
    let mut nine = png(1, 1, 0);
    nine.resize(9 * 1024 * 1024, 0);
    let p = refused(s.put_media(on("OPEN-2"), "image/png", b64(&nine), None).await.unwrap_err());
    assert_eq!(p, crate::media::Problem::TooLarge { bytes: 9 * 1024 * 1024, max: crate::media::MAX_BYTES });
    assert!(p.to_string().contains("at most 7.0 MB"), "{p}");
    let p = refused(s.put_media(on("OPEN-2"), "image/png", "not base64 at all!".into(), None).await.unwrap_err());
    assert_eq!(p.reason(), "bad_data");
    // At the cap exactly, kept.
    let mut seven = png(1, 1, 0);
    seven.resize(crate::media::MAX_BYTES, 0);
    assert!(s.put_media(on("OPEN-2"), "image/png", b64(&seven), None).await.is_ok());
    assert_eq!(s.media_count(SYS), 1, "only the one at the cap");
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
        let s = Store::open(&path).unwrap();
        s.create(SYS, tests_reach::new("K", Kind::Scope, None, &[], "")).await.unwrap();
        s.put_media(on("K-1"), "image/png", b64(&bytes), None).await.unwrap().info.hash
    };
    {
        let s = Store::open(&path).unwrap();
        let (info, got) = s.media(&viewer_of(&["K"]), &hash).await.unwrap();
        assert_eq!((info.width, info.height, got), (12, 34, bytes.clone()));
        assert!(s.media_info(&viewer_of(&["OTHER"]), &hash).is_err(), "whose it is, as stored");
    }
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }), "images are no derived state");
    // A file written before COMMENT-5 has no tables: the open makes them.
    Connection::open(&path).unwrap().execute_batch("DROP TABLE media; DROP TABLE media_owners").unwrap();
    assert!(!has_tables(&path));
    let s = Store::open(&path).unwrap();
    assert!(has_tables(&path));
    assert_eq!(s.media_count(SYS), 0);
    assert_eq!(s.get(SYS, "K-1").await.unwrap().kind, Kind::Scope, "the tickets as they were");
}
