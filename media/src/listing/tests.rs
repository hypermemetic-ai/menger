//! EDITOR-32: `media.list`'s read — the store's rows (only files kept for a
//! ticket or a note, newest first, by kind, paged by `after`) and the
//! reach (an owner the caller does not read left off its row, a row left
//! with none left out, a ticket named by its id now). Over the wire, as
//! the caller the seam asks: tests/wire_media.rs.

use super::*;
use crate::store::Owner;

/// A PNG's header for `w`×`h`, `pad` making two of a size differ.
fn png(w: u32, h: u32, pad: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, pad]);
    b
}

/// A PDF's first bytes.
fn pdf(pad: u8) -> Vec<u8> {
    let mut b = b"%PDF-1.7\n".to_vec();
    b.push(pad);
    b
}

fn t(id: &str) -> Owner {
    Owner::Ticket(id.into())
}

async fn keep(s: &Arc<MediaStore>, owner: Owner, bytes: Vec<u8>, name: &str) -> String {
    s.put_bytes(owner, bytes, Some(name.into()), Some("key:aa".into()), 1 << 20).await.unwrap().info.hash
}

/// Every owner read, each by its own id.
async fn all(o: Owner) -> Option<String> {
    match o {
        Owner::Ticket(id) | Owner::Note(id) => Some(id),
        _ => None,
    }
}

fn hashes(p: &Page) -> Vec<&str> {
    p.rows.iter().map(|r| r.file.info.hash.as_str()).collect()
}

#[tokio::test]
async fn the_list_is_newest_first_by_kind_paged_and_only_what_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(MediaStore::open(&dir.path().join("media.db")).unwrap());
    let a = keep(&s, t("KILN-1"), png(4, 3, 1), "a.png").await;
    let b = keep(&s, Owner::Note("7k3mq2xa".into()), pdf(1), "b.pdf").await;
    // Kept with no ticket (the putter's) and a link's preview: used nowhere.
    let _own = keep(&s, Owner::App(Some("key:aa".into())), png(1, 1, 2), "own.png").await;
    let _any = keep(&s, Owner::Anyone, png(1, 1, 3), "card.png").await;
    let c = keep(&s, t("KILN-2"), png(8, 8, 4), "c.png").await;
    // The same bytes for a second ticket: one file, both owners, in order.
    assert_eq!(keep(&s, t("KILN-3"), png(4, 3, 1), "a.png").await, a);

    let p = page(&s, None, None, 10, all).await.unwrap();
    assert_eq!(hashes(&p), [c.as_str(), b.as_str(), a.as_str()], "newest first; the putter's and anyone's are not listed");
    assert_eq!(p.next, None);
    let first = &p.rows[2];
    assert_eq!((first.tickets.clone(), first.notes.clone()), (vec!["KILN-1".to_string(), "KILN-3".to_string()], vec![]));
    assert_eq!((first.file.info.kind.as_str(), first.file.info.width, first.file.info.name.as_deref()), ("image", 4, Some("a.png")));
    assert!(first.file.at > 0);
    assert_eq!(p.rows[1].notes, ["7k3mq2xa"]);

    // By kind.
    let pdfs = page(&s, Some(MediaKind::Pdf), None, 10, all).await.unwrap();
    assert_eq!(hashes(&pdfs), [b.as_str()]);
    assert!(page(&s, Some(MediaKind::Video), None, 10, all).await.unwrap().rows.is_empty());

    // Paged: two, then the rest from `next`.
    let one = page(&s, None, None, 2, all).await.unwrap();
    assert_eq!((hashes(&one), one.next.as_deref()), (vec![c.as_str(), b.as_str()], Some(b.as_str())));
    let two = page(&s, None, one.next.as_deref(), 2, all).await.unwrap();
    assert_eq!((hashes(&two), two.next.as_deref()), (vec![a.as_str()], None));
    // An `after` that names nothing.
    assert!(matches!(page(&s, None, Some(&"0".repeat(64)), 2, all).await, Err(StoreError::NotFound(_))));
}

#[tokio::test]
async fn an_owner_out_of_reach_is_left_off_and_a_file_with_none_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(MediaStore::open(&dir.path().join("media.db")).unwrap());
    let both = keep(&s, t("KILN-1"), png(2, 2, 1), "both.png").await;
    keep(&s, t("SECRET-1"), png(2, 2, 1), "both.png").await;
    let hidden = keep(&s, t("SECRET-2"), png(3, 3, 2), "hidden.png").await;
    let note = keep(&s, Owner::Note("secretdc".into()), pdf(3), "d.pdf").await;
    let moved = keep(&s, t("OLD-4"), png(5, 5, 3), "moved.png").await;

    let asked = std::sync::Mutex::new(Vec::new());
    // A reader of KILN, and of OLD-4 (now NEW-9); nothing of SECRET or the note.
    let reader = |o: Owner| {
        asked.lock().unwrap().push(o.clone());
        async move {
            match o {
                Owner::Ticket(id) if id.starts_with("KILN") => Some(id),
                Owner::Ticket(id) if id == "OLD-4" => Some("NEW-9".to_string()),
                _ => None,
            }
        }
    };
    let p = page(&s, None, None, 10, reader).await.unwrap();
    assert_eq!(hashes(&p), [moved.as_str(), both.as_str()], "{hidden} and {note} are out of reach: not listed");
    assert_eq!(p.rows[0].tickets, ["NEW-9"], "a ticket by its id now");
    assert_eq!(p.rows[1].tickets, ["KILN-1"], "SECRET-1 is left off the row it shares");
    let mut asked = asked.into_inner().unwrap();
    let n = asked.len();
    asked.sort_by_key(|o| format!("{o:?}"));
    asked.dedup();
    assert_eq!((n, asked.len()), (5, 5), "each owner asked once a call");
}
