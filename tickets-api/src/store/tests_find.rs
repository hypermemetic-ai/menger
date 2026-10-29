//! SCALE-6: `find` — the jump palette's rule, answered by the store and
//! paged by MNG-9's rules.
//!
//! The oracle below is the v1 palette's rule (`frontend/src/keys.rs` at
//! v0.0.1), copied; v2 has no frontend for it to be checked against.

use super::*;

/// `palette_match` from v1's `frontend/src/keys.rs`, verbatim.
fn palette_match(code: &str, title: &str, query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || contains_ignore_ascii_case(code, q) || contains_ignore_ascii_case(title, q)
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    // UTF-8 is self-synchronizing, so a byte-window scan cannot match
    // across a character boundary by accident.
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// The server's rule on an id and a title: a blank query is no needle and
/// matches everything.
fn server_match(code: &str, title: &str, query: &str) -> bool {
    Needle::of(query).is_none_or(|n| n.within(code) || n.within(title))
}

/// Writes tickets straight through the schema, `(id, title)` each, the id
/// split into epic and sequence at its last `-` — any id, including the
/// lowercase ones only a hand edit makes.
fn write_titled(path: &Path, rows: &[(&str, &str)]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(db::SCHEMA).unwrap();
    for (id, title) in rows {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        db::upsert(
            &conn,
            &Ticket {
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                archived: None,
                frozen: false,
                hidden: false,
                labels: Vec::new(),
                id: (*id).into(),
                epic: epic.into(),
                shortcode: epic.into(),
                address: String::new(),
                seq: seq.parse().unwrap(),
                title: title.to_string(),
                status: Status::Pending,
                kind: Kind::Scope,
                severity: None,
                blocked_by: Vec::new(),
                unlocks: Vec::new(),
                parent: None,
                child_count: 0,
                commits: Vec::new(),
                path: None,
                placement: None,
                body: format!("the body of {id} is not searched: needle"),
                created_unix: 0,
                updated_unix: 0,
                moved_from: Vec::new(),
                epic_archived: None,
            },
        )
        .unwrap();
    }
}

fn found(page: &Page) -> Vec<&str> {
    page.tickets.iter().map(|t| t.id.as_str()).collect()
}

/// The rule against the palette's code on what its cases do not name:
/// random ids, titles and queries over an alphabet of both cases, digits,
/// `-`, spaces and tabs, and multi-byte characters — the Kelvin sign and
/// the dotted capital I among them, whose Unicode lowercase is ASCII but
/// which ASCII folding leaves alone — with queries padded in Unicode
/// whitespace, which `trim` removes on both sides.
#[test]
fn the_server_agrees_with_the_palettes_code_on_random_text() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) % n as u64) as usize
        }
    }
    const ALPHABET: [&str; 16] = ["a", "A", "b", "B", "z", "Z", "1", "-", " ", "\t", "é", "É", "—", "\u{212A}", "İ", "k"];
    const PAD: [&str; 4] = ["", " ", "\t", "\u{3000}"];
    let mut rng = Rng(6);
    let text = |rng: &mut Rng, max: usize| -> String {
        (0..rng.below(max + 1)).map(|_| ALPHABET[rng.below(ALPHABET.len())]).collect()
    };
    let (mut hits, mut misses) = (0, 0);
    for _ in 0..40_000 {
        let code = text(&mut rng, 6);
        let title = text(&mut rng, 14);
        let query = format!("{}{}{}", PAD[rng.below(4)], text(&mut rng, 3), PAD[rng.below(4)]);
        let expected = palette_match(&code, &title, &query);
        assert_eq!(server_match(&code, &title, &query), expected, "{code:?} {title:?} {query:?}");
        if expected { hits += 1 } else { misses += 1 }
    }
    assert!(hits > 5_000 && misses > 5_000, "{hits} hits, {misses} misses");
}

/// Acceptance 1: against a store holding exactly `AGS-1`, `AGS-18` and
/// `MFE-7 · "The Ticket Page"`.
#[test]
fn find_answers_the_tickets_examples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("examples.db");
    write_titled(
        &path,
        &[("AGS-1", "Keyboard navigation"), ("AGS-18", "Keyboard navigation, the palette"), ("MFE-7", "The Ticket Page")],
    );
    let store = Store::open(&path).unwrap();

    let page = store.find_page(SYS, "ags-1", None, None).unwrap();
    assert_eq!(found(&page), ["AGS-1", "AGS-18"], "in that order");
    assert_eq!((page.next, page.remaining), (None, 0));

    let page = store.find_page(SYS, "ticket page", None, None).unwrap();
    assert_eq!(found(&page), ["MFE-7"]);

    let page = store.find_page(SYS, "zzz", None, None).unwrap();
    assert!(page.tickets.is_empty());
    assert_eq!((page.next, page.remaining), (None, 0), "no rows and page {{ next: null, remaining: 0 }}");

    // Ids and titles only: every body holds "needle".
    assert!(store.find_page(SYS, "needle", None, None).unwrap().tickets.is_empty());
}

/// What `find` must answer, worked out from the whole store as a list:
/// the palette's code over every ticket past the cursor (a position — a
/// ticket that does not match is still one), the first `limit` the page,
/// the rest counted.
fn oracle(store: &Store, query: &str, after: Option<&str>, limit: Option<u32>) -> (Vec<String>, Option<String>, u64) {
    let all = store.page(SYS, PageQuery::default()).unwrap().tickets;
    let start = match after {
        None => 0,
        Some(id) => all.iter().position(|t| t.id == id.to_ascii_uppercase()).expect("a cursor that names a ticket") + 1,
    };
    let matching: Vec<&TicketSummary> = all[start..].iter().filter(|t| palette_match(&t.id, &t.title, query)).collect();
    let take = limit.map_or(matching.len(), |n| (n as usize).min(matching.len()));
    let rows: Vec<String> = matching[..take].iter().map(|t| t.id.to_string()).collect();
    let remaining = (matching.len() - take) as u64;
    let next = if remaining == 0 { None } else { rows.last().cloned().or(after.map(str::to_ascii_uppercase)) };
    (rows, next, remaining)
}

fn answered(page: Page) -> (Vec<String>, Option<String>, u64) {
    (page.tickets.into_iter().map(|t| t.id.into_string()).collect(), page.next, page.remaining)
}

/// Acceptance 3, and MNG-9's cursor rules: on a seeded store, `limit: 2`
/// then `after = next` visits every match exactly once, in (epic, seq)
/// order, `remaining` counting down exactly; every page — from any
/// cursor, matching or not, at any limit — is what the palette's code over
/// the store as a list gives; a blank query is the unfiltered order and a
/// padded or mixed-case one the same as its trim; and a cursor naming no
/// ticket is the cursor error.
#[test]
fn find_pages_every_match_exactly_once_by_mng9s_rules() {
    let store = Store::new();
    store
        .seed_synthetic(SYS, SeedSpec {
            count: 1_500,
            seed: 6,
            into_existing: false,
        })
        .unwrap();
    let all = store.page(SYS, PageQuery::default()).unwrap().tickets;
    let (first_epic, root_title) = (all[0].epic.clone(), all[0].title.clone());
    let queries = [
        "".to_string(),
        "   ".to_string(),
        "the".to_string(),
        " CaChE ".to_string(),
        "-1".to_string(),
        format!("{}-", first_epic.to_lowercase()),
        root_title[2..9].to_string(),
        "under load".to_string(),
        "zzz".to_string(),
    ];
    for query in &queries {
        // The walk, two at a time.
        let expected = oracle(&store, query, None, None).0;
        let mut walked: Vec<String> = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = store.find_page(SYS, query, after.as_deref(), Some(2)).unwrap();
            assert!(page.tickets.len() <= 2);
            walked.extend(page.tickets.iter().map(|t| t.id.to_string()));
            assert_eq!(page.remaining as usize, expected.len() - walked.len(), "{query:?} after {after:?}");
            match page.next {
                Some(next) => {
                    assert_eq!(Some(&next), walked.last(), "the cursor is the page's last row");
                    after = Some(next);
                }
                None => break,
            }
        }
        assert_eq!(walked, expected, "{query:?}: every match once, in order");

        // Any cursor, any limit.
        for cursor in [None, Some(all[0].id.as_str()), Some(all[700].id.as_str()), Some(all[all.len() - 1].id.as_str())] {
            for limit in [None, Some(0), Some(1), Some(50)] {
                assert_eq!(
                    answered(store.find_page(SYS, query, cursor, limit).unwrap()),
                    oracle(&store, query, cursor, limit),
                    "{query:?} after {cursor:?} limit {limit:?}"
                );
            }
        }
    }

    // Blank is the unfiltered order, by arithmetic; padding and case fold.
    let unfiltered = |after: Option<&str>| store.page(SYS, PageQuery { after, limit: Some(50), ..PageQuery::default() }).unwrap();
    for blank in ["", " ", "\t \n"] {
        assert_eq!(store.find_page(SYS, blank, None, Some(50)).unwrap(), unfiltered(None));
        assert_eq!(store.find_page(SYS, blank, Some(all[9].id.as_str()), Some(50)).unwrap(), unfiltered(Some(all[9].id.as_str())));
    }
    assert_eq!(store.find_page(SYS, "  under LOAD ", None, None).unwrap(), store.find_page(SYS, "under load", None, None).unwrap());

    assert!(matches!(store.find_page(SYS, "the", Some("NO-SUCH-1"), Some(2)), Err(StoreError::CursorNotFound(id)) if id == "NO-SUCH-1"));
    assert!(matches!(store.find_page(SYS, "", Some("no-such-1"), None), Err(StoreError::CursorNotFound(_))));
}
