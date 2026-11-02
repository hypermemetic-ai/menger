//! The docs store on its own file, its bodies Yjs CRDTs: a document made is
//! read back whole; two writers' concurrent updates converge, in the store
//! and in each writer's copy; an agent's whole-body save merges with a live
//! edit made meanwhile; every change is kept with who and when, a replay at
//! a midpoint is the document then; a squash folds the changes up to a
//! moment into one snapshot, keeping who wrote what, and a replay before it
//! is gone; a restore is a change more; a delete reads as none; the list is
//! newest first, searched, paged; presence comes and goes; every write is
//! said; the file survives a reopen, its snapshot and the changes after it.

use super::*;
use crate::crdt::{unb64, BODY};
use base64::Engine;
use yrs::updates::decoder::Decode;
use yrs::{GetString, ReadTxn, StateVector, Text, Transact, Update};

fn open(dir: &tempfile::TempDir) -> Arc<DocsStore> {
    Arc::new(DocsStore::open(&dir.path().join("docs.db")).unwrap())
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// A browser's copy of `id`: a CRDT synced from the store.
async fn client(s: &Arc<DocsStore>, id: &str) -> yrs::Doc {
    let d = yrs::Doc::new();
    let st = s.state(id, None).await.unwrap();
    d.transact_mut().apply_update(Update::decode_v1(&unb64(&st.update).unwrap()).unwrap()).unwrap();
    d
}

/// Types `what` at `at` in the copy `d`; answers the update, base64.
fn typed(d: &yrs::Doc, at: u32, what: &str) -> String {
    let t = d.get_or_insert_text(BODY);
    let mut txn = d.transact_mut();
    t.insert(&mut txn, at, what);
    b64(&txn.encode_update_v1())
}

fn take(d: &yrs::Doc, update: &str) {
    d.transact_mut().apply_update(Update::decode_v1(&unb64(update).unwrap()).unwrap()).unwrap();
}

fn text(d: &yrs::Doc) -> String {
    let t = d.get_or_insert_text(BODY);
    let s = t.get_string(&d.transact());
    s
}

async fn body(s: &Arc<DocsStore>, id: &str) -> String {
    s.get(id).await.unwrap().body.unwrap()
}

#[tokio::test]
async fn a_document_is_made_and_read_back_whole() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let d = s.create("acme", "The kiln", "# Fire\n", "owner", "key:o").await.unwrap();
    assert!(d.id.parse::<crate::DocId>().is_ok(), "{}", d.id);
    assert_eq!((d.org.as_str(), d.title.as_str(), d.body.as_deref(), d.revision, d.bytes), ("acme", "The kiln", Some("# Fire\n"), 1, 7));
    assert_eq!(s.get(&d.id).await.unwrap(), d);
    assert!(matches!(s.get("zzzzzzzz").await, Err(StoreError::NotFound(_))));
    let h = s.history(&d.id, 10, None).await.unwrap();
    assert_eq!(h.iter().map(|r| (r.seq, r.kind.as_str(), r.principal.as_str())).collect::<Vec<_>>(), [(1, "create", "key:o")]);
    assert_eq!(text(&client(&s, &d.id).await), "# Fire\n", "a copy synced from the store holds the body");
    assert_ne!(s.create("acme", "Another", "", "owner", "key:o").await.unwrap().id, d.id);
}

#[tokio::test]
async fn two_writers_typing_at_once_converge() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let id = s.create("acme", "Paired", "ab", "a", "key:a").await.unwrap().id;
    let (one, two) = (client(&s, &id).await, client(&s, &id).await);
    // Both type at once, each before hearing the other.
    let u1 = typed(&one, 1, "ONE");
    let u2 = typed(&two, 1, "TWO");
    s.update(&id, &u1, "one", "key:1", Some("s-1".into())).await.unwrap();
    let w = s.update(&id, &u2, "two", "key:2", Some("s-2".into())).await.unwrap();
    assert_eq!(w.seq, Some(3));
    // Each hears the other's.
    take(&one, &u2);
    take(&two, &u1);
    let merged = body(&s, &id).await;
    assert_eq!((text(&one), text(&two)), (merged.clone(), merged.clone()), "the store and both copies agree");
    assert!(merged.contains("ONE") && merged.contains("TWO") && merged.starts_with('a') && merged.ends_with('b'), "nothing lost: {merged}");
    // An update that is not one is refused, and nothing is written.
    assert!(matches!(s.update(&id, "/////w==", "x", "key:x", None).await, Err(StoreError::BadUpdate(_))));
    assert_eq!(s.get(&id).await.unwrap().revision, 3);
}

#[tokio::test]
async fn an_agents_whole_body_save_merges_with_a_live_edit() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let id = s.create("acme", "Shared", "first line\nsecond line\n", "a", "key:a").await.unwrap().id;
    let person = client(&s, &id).await;
    // The agent reads revision 1 and rewrites the first line…
    let read = s.get(&id).await.unwrap();
    // …while a person types into the second.
    let live = typed(&person, "first line\nsecond".len() as u32, " (typed live)");
    s.update(&id, &live, "person", "key:p", None).await.unwrap();
    let saved = s.save(&id, "FIRST LINE\nsecond line\n", Some(read.revision), "agent", "key:g").await.unwrap();
    assert!(saved.stale, "it read revision 1; the document was at 2");
    assert_eq!(body(&s, &id).await, "FIRST LINE\nsecond (typed live) line\n", "both kept: the save merged, it did not overwrite");
    // The same body again writes nothing.
    let again = s.save(&id, "FIRST LINE\nsecond (typed live) line\n", None, "agent", "key:g").await.unwrap();
    assert_eq!(again.seq, None);
    // Multi-byte text diffs on character boundaries.
    s.save(&id, "héllo — wörld ✓\n", None, "agent", "key:g").await.unwrap();
    s.save(&id, "héllo, wörld ✓✓\n", None, "agent", "key:g").await.unwrap();
    assert_eq!(body(&s, &id).await, "héllo, wörld ✓✓\n");
}

#[tokio::test]
async fn every_change_is_kept_and_a_replay_is_the_document_then() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let id = s.create("acme", "Built", "", "a", "key:a").await.unwrap().id;
    let c = client(&s, &id).await;
    for (i, w) in ["one ", "two ", "three"].iter().enumerate() {
        let at = text(&c).len() as u32;
        s.update(&id, &typed(&c, at, w), &format!("w{i}"), "key:w", None).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(3)).await; // apart in time
    }
    s.rename(&id, "Built up", "a", "key:a").await.unwrap();
    let h = s.history(&id, 10, None).await.unwrap();
    assert_eq!(h.iter().map(|r| (r.seq, r.kind.as_str())).collect::<Vec<_>>(), [(5, "rename"), (4, "update"), (3, "update"), (2, "update"), (1, "create")], "newest first");
    assert!(h.iter().all(|r| r.at > 1_700_000_000_000), "at in milliseconds");
    let mid = s.replay(&id, Some(3), None).await.unwrap();
    assert_eq!((mid.body.as_deref(), mid.title.as_str(), mid.revision, mid.by.as_str()), (Some("one two "), "Built", 3, "w1"));
    assert_eq!(s.replay(&id, Some(5), None).await.unwrap().title, "Built up");
    let by_time = s.replay(&id, None, Some(h[2].at)).await.unwrap();
    assert_eq!(by_time.revision, 3, "after the last change made by then");
    assert_eq!(s.history(&id, 2, Some(3)).await.unwrap().iter().map(|r| r.seq).collect::<Vec<_>>(), [2, 1], "paged back by seq");
    // A restore is a change more.
    let back = s.restore(&id, 2, "b", "key:b").await.unwrap();
    assert_eq!((back.seq, back.doc.body.as_deref()), (Some(6), Some("one ")));
}

#[tokio::test]
async fn a_squash_folds_the_changes_and_keeps_who_wrote_what() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let id = s.create("acme", "Long", "", "a", "key:a").await.unwrap().id;
    let c = client(&s, &id).await;
    for (who, w) in [("ann", "a"), ("bob", "b"), ("ann", "c"), ("bob", "d"), ("ann", "e")] {
        let at = text(&c).len() as u32;
        s.update(&id, &typed(&c, at, w), who, &format!("key:{who}"), None).await.unwrap();
    }
    let before = s.history(&id, 100, None).await.unwrap().len();
    let removed = s.squash(&id, 4, "admin", "key:admin").await.unwrap();
    assert_eq!(removed, 3, "4 changes became 1");
    let h = s.history(&id, 100, None).await.unwrap();
    assert_eq!(h.len(), before - 3, "the history is shorter");
    let snap = h.iter().find(|r| r.kind == "snapshot").expect("a snapshot");
    assert_eq!((snap.seq, snap.by.as_str()), (4, "admin"));
    let mut authors: Vec<(String, u32)> = snap.authors.clone().unwrap().into_iter().map(|a| (a.by, a.changes)).collect();
    authors.sort();
    assert_eq!(authors, [("a".to_string(), 1), ("ann".into(), 2), ("bob".into(), 1)], "who wrote what, kept");
    assert_eq!(s.replay(&id, Some(4), None).await.unwrap().body.as_deref(), Some("abc"), "the snapshot is the document then");
    assert!(matches!(s.replay(&id, Some(2), None).await, Err(StoreError::NoRevision(_, 2))), "before it: gone");
    assert_eq!(body(&s, &id).await, "abcde", "the document is as it was");
    // A second squash folds the first's authors in.
    s.squash(&id, 6, "admin", "key:admin").await.unwrap();
    let snap = s.history(&id, 100, None).await.unwrap().into_iter().find(|r| r.kind == "snapshot").unwrap();
    assert_eq!(snap.authors.unwrap().iter().map(|a| a.changes).sum::<u32>(), 6);
    // And a fresh load reads it all back.
    s.loaded.lock().unwrap().clear();
    assert_eq!(text(&client(&s, &id).await), "abcde");
}

#[tokio::test]
async fn a_deleted_document_reads_as_none_and_keeps_its_changes() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let d = s.create("acme", "Gone soon", "x", "a", "key:a").await.unwrap();
    let (org, _) = s.delete(&d.id, "admin", "key:admin").await.unwrap();
    assert_eq!(org, "acme");
    assert!(matches!(s.get(&d.id).await, Err(StoreError::NotFound(_))));
    assert!(matches!(s.save(&d.id, "y", None, "a", "key:a").await, Err(StoreError::NotFound(_))));
    assert!(matches!(s.state(&d.id, None).await, Err(StoreError::NotFound(_))));
    assert_eq!(s.count(), 0);
    let kept: i64 = s.lock().query_row("SELECT count(*) FROM doc_log WHERE id = ?1", [&d.id], |r| r.get(0)).unwrap();
    assert_eq!(kept, 2, "create and delete");
}

#[tokio::test]
async fn the_list_is_newest_first_searched_and_paged() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let mut ids = Vec::new();
    for (org, t) in [("acme", "Kiln glaze"), ("acme", "Firing log"), ("other", "Kiln wiring"), ("acme", "The KILN, weekly")] {
        ids.push(s.create(org, t, "body", "a", "key:a").await.unwrap().id);
    }
    let all = s.list(Listing { orgs: None, limit: 10, ..Default::default() }).await.unwrap();
    assert_eq!(all.rows.iter().map(|d| d.id.clone()).collect::<Vec<_>>(), ids.iter().rev().cloned().collect::<Vec<_>>(), "newest first");
    assert!(all.rows.iter().all(|d| d.body.is_none() && d.bytes == 4), "rows carry no body");
    let acme_kiln = s.list(Listing { orgs: Some(vec!["acme".into()]), query: Some("kiln".into()), limit: 10, after: None }).await.unwrap();
    assert_eq!(acme_kiln.rows.iter().map(|d| d.title.as_str()).collect::<Vec<_>>(), ["The KILN, weekly", "Kiln glaze"]);
    assert!(s.list(Listing { orgs: Some(vec![]), limit: 10, ..Default::default() }).await.unwrap().rows.is_empty());
    let first = s.list(Listing { orgs: None, limit: 3, ..Default::default() }).await.unwrap();
    assert_eq!((first.rows.len(), first.next.clone(), first.remaining), (3, Some(ids[1].clone()), 1));
    let second = s.list(Listing { orgs: None, limit: 3, after: first.next, ..Default::default() }).await.unwrap();
    assert_eq!((second.rows.iter().map(|d| d.id.clone()).collect::<Vec<_>>(), second.next), (vec![ids[0].clone()], None));
}

#[tokio::test]
async fn presence_comes_and_goes() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir);
    let id = s.create("acme", "Here", "", "a", "key:a").await.unwrap().id;
    let mut rx = s.subscribe();
    let who = crate::presence::Who { principal: "key:p".into(), name: "pat".into(), agent: false };
    let held = crate::presence::Held { store: Arc::clone(&s), id: id.clone(), session: "s-1".into() };
    let p = s.arrive(&id, "acme", "s-1", &who, true, None, None);
    assert_eq!((p.name.as_str(), p.agent, p.colour.len()), ("pat", false, 7));
    assert!(matches!(rx.recv().await.unwrap(), Change::Present { .. }));
    s.arrive(&id, "acme", "s-1", &who, false, Some(42), Some("AAE=".into()));
    rx.recv().await.unwrap();
    let now = s.present(&id);
    assert_eq!((now.len(), now[0].client, now[0].awareness.as_deref()), (1, Some(42), Some("AAE=")), "the caret moved");
    assert!(s.taken(&id, "s-1", "key:other") && !s.taken(&id, "s-1", "key:p"));
    drop(held);
    assert!(matches!(rx.recv().await.unwrap(), Change::Left { ref session, client: Some(42), .. } if session == "s-1"));
    assert!(s.present(&id).is_empty(), "gone with its watch");
}

#[tokio::test]
async fn every_write_is_said_and_the_file_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let s = open(&dir);
        let mut rx = s.subscribe();
        let d = s.create("acme", "Said", "a", "w", "key:w").await.unwrap();
        let c = client(&s, &d.id).await;
        // Past a snapshot: a load reads the snapshot and the changes after it.
        for i in 0..(crate::crdt::SNAPSHOT_EVERY + 5) {
            let at = text(&c).len() as u32;
            s.update(&d.id, &typed(&c, at, &format!("{}", i % 10)), "w", "key:w", None).await.unwrap();
        }
        s.rename(&d.id, "Said again", "w", "key:w").await.unwrap();
        assert!(matches!(rx.recv().await.unwrap(), Change::Created(_)));
        assert!(matches!(rx.recv().await.unwrap(), Change::Updated { seq: 2, .. }));
        let state_seq: u32 = s.lock().query_row("SELECT state_seq FROM docs WHERE id = ?1", [&d.id], |r| r.get(0)).unwrap();
        assert!(state_seq > 1, "a snapshot was kept");
        d.id
    };
    let s = open(&dir);
    let d = s.get(&id).await.unwrap();
    assert_eq!((d.title.as_str(), d.revision), ("Said again", crate::crdt::SNAPSHOT_EVERY + 7));
    let c = client(&s, &id).await;
    assert_eq!(text(&c), d.body.clone().unwrap(), "the CRDT loaded from the file is the text kept");
    // A client's diff by its own vector is empty when it is up to date.
    let sv = c.transact().state_vector();
    let st = s.state(&id, Some(b64(&yrs::updates::encoder::Encode::encode_v1(&sv)))).await.unwrap();
    let empty = yrs::Doc::new();
    take(&empty, &st.update);
    assert_eq!(text(&empty), "", "nothing it lacks");
    let _ = StateVector::default();
}

/// V2-31's seam: every write's bytes reserved before it is kept — a meter
/// that refuses keeps nothing.
#[tokio::test]
async fn the_meter_is_asked_before_a_write() {
    struct Full;
    impl crate::quota::Quota for Full {
        fn reserve(&self, author: &str, bytes: u64) -> Result<crate::quota::Reserved, String> {
            if bytes > 3 {
                return Err(format!("{author} is out of docs.bytes"));
            }
            Ok(crate::quota::Reserved { author: author.into(), bytes })
        }
        fn commit(&self, _: crate::quota::Reserved) {}
    }
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(DocsStore::open(&dir.path().join("docs.db")).unwrap().metered(Arc::new(Full)));
    let id = s.create("acme", "Metered", "ab", "a", "key:a").await.unwrap().id;
    assert!(matches!(s.save(&id, "a much longer body", None, "a", "key:a").await, Err(StoreError::Quota(m)) if m.contains("key:a")));
    assert_eq!((body(&s, &id).await, s.get(&id).await.unwrap().revision), ("ab".to_string(), 1), "nothing kept");
}
