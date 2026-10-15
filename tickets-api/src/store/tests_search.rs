//! SEARCH-2: search inside tickets — what a body is indexed as, how a query
//! is read and corrected, the order of the hits, the filter and the pages;
//! and the index itself: built by a first open, kept by `create`, `amend`
//! and a seed, durable, and held by `--verify-derived` to the tickets.

use super::search::{osa, osa_prefix, parts, text, Part};
use super::*;
use crate::filter::Filter;
use crate::types::{Marked, SearchWord};

fn new(epic: &str, kind: Kind, title: &str, body: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: title.into(),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        body: body.into(),
    }
}

/// A store holding `SR-1`, a scope, then one build per `(title, body)`:
/// `SR-2`, `SR-3`, …
async fn holding(store: &Store, tickets: &[(&str, &str)]) {
    store.create(new("SR", Kind::Scope, "search", "the root")).await.unwrap();
    for (title, body) in tickets {
        store.create(new("SR", Kind::Build, title, body)).await.unwrap();
    }
}

async fn search(store: &Store, query: &str) -> SearchPage {
    store.search(query, None, None, None).await.unwrap()
}

fn ids(page: &SearchPage) -> Vec<&str> {
    page.hits.iter().map(|h| h.summary.id.as_str()).collect()
}

fn also<'p>(page: &'p SearchPage, word: &str) -> Vec<&'p str> {
    let w = page.words.iter().find(|w| w.word == word).unwrap_or_else(|| panic!("no word {word}: {:?}", page.words));
    w.also.iter().map(String::as_str).collect()
}

/// The marked stretches of a passage, as `text` with `[hits]`.
fn marked(runs: &[Marked]) -> String {
    runs.iter().map(|r| if r.hit { format!("[{}]", r.text) } else { r.text.clone() }).collect()
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

// ---------------------------------------------------------------------
// What is indexed

/// The text of a body is what a reader reads: tags, attributes, comments,
/// styles, scripts, component sources, frontmatter and mermaid's styling
/// gone; the text between tags, the attributes a reader sees as words,
/// code as written, entities as their characters.
#[test]
fn a_body_is_indexed_as_the_text_a_reader_reads() {
    let body = "---\nparent: SR-1\nstatus: Pending\n---\n\
        ## Problem\n\n\
        <m-summary ticket=\"SR-2\"><m-badge kind=\"scope\"></m-badge><p><strong>Placements</strong> are kept &amp; stored</p></m-summary>\n\
        <!-- a note to nobody -->\n\
        <m-field data-field=\"standing\">Execution → <m-ticket ref=\"INPUT-2\"></m-ticket>.</m-field>\n\
        <m-option data-field=\"option\" name=\"Archive\" value=\"archive\" cost=\"a second store\"></m-option>\n\
        <style>.x { color: red }</style><script>let hidden = 1;</script>\n\
        | when | then |\n| -- | -- |\n| a **write** | `Vec<String>` moves |\n\n\
        ```mermaid\ngraph TD\n  A[\"the cache\"] --> B\n  classDef d fill:#cfe2f3;\n  class A d;\n```\n\n\
        ```component m-thing\nconsole.log('source');\n```\n\n\
        ```rust\nlet x: Vec<u8> = vec![];\n```\n\
        > a quote &lt;kept&gt;";
    assert_eq!(
        text(body),
        "Problem Placements are kept & stored Execution → INPUT-2. Archive a second store \
         when · then a write · Vec<String> moves the cache \
         let x: Vec<u8> = vec![]; a quote <kept>"
    );
    // Nothing a reader cannot see survives.
    for gone in ["parent", "data", "field", "badge", "kind", "nobody", "color", "hidden", "classDef", "fill", "console", "m-thing"] {
        assert!(!text(body).contains(gone), "{gone} is not text");
    }
}

/// A diagram is read as its labels: quoted ones, their markup stripped, or
/// bracketed and barred ones where nothing is quoted — never its ids,
/// arrows, keywords or styling.
#[test]
fn a_diagram_is_indexed_as_its_labels() {
    let body = "```mermaid\ngraph TD\n  B[\"a ticket body<br/>m-decision\"] -->|\"parsed on every write\"| S[\"inputs store\"]\n  \
        A[the cache] -->|on write| C((the file))\n  subgraph TODAY[\"what can change\"]\n  classDef d fill:#cfe2f3;\n  class S d;\n```";
    assert_eq!(text(body), "a ticket body m-decision parsed on every write inputs store the cache on write the file what can change");
}

/// Pure, and safe on whatever a body holds: a frontmatter that never closes
/// is text, a tag that never closes is text, a code span that never closes
/// is its backticks, an entity cut short is its ampersand — and a
/// multi-byte character beside any of them is never cut.
#[test]
fn text_is_total_and_never_cuts_a_character() {
    assert_eq!(text("---\nno close"), "--- no close");
    assert_eq!(text("a <b never closes"), "a <b never closes");
    assert_eq!(text("an `open span"), "an `open span");
    assert_eq!(text("&amp…; and &#x2014; and &nope;"), "&amp…; and — and &nope;");
    assert_eq!(text("𝔘 <i title=\"ünï\">x</i> ✓"), "𝔘 ünï x ✓");
    assert_eq!(text("its <strong>placement</strong>, equal; a <code>Vec</code>."), "its placement, equal; a Vec.");
    assert_eq!(text("one<br>two</p><p>(three)"), "one two (three)");
    assert_eq!(text("\u{1}soft\u{1}, as any control"), "soft, as any control");
    assert_eq!(text(""), "");
    assert_eq!(text("\u{2}marks\u{3} are spaces"), "marks are spaces");
    let awkward = "---\nid: AM-2\n---\r\n## Divergence\r\n- ünïcode ✓ 𝔘 🧪\t(tab)\r\n```mermaid\ngraph LR\n```\nend";
    assert_eq!(text(awkward), text(awkward), "pure");
    assert_eq!(text(awkward), "Divergence - ünïcode ✓ 𝔘 🧪 (tab) end");
}

// ---------------------------------------------------------------------
// How a query is read

#[test]
fn a_query_is_words_phrases_and_joined_words() {
    let w = |s: &str| Part::Word(s.into());
    let p = |ws: &[&str], prefix: bool| Part::Phrase { words: ws.iter().map(|s| s.to_string()).collect(), prefix };
    assert_eq!(parts("Plcement  WAVE"), [w("plcement"), w("wave")]);
    assert_eq!(parts("FILTER-23 set_status a.b/c"), [p(&["filter", "23"], true), p(&["set", "status"], true), p(&["a", "b", "c"], true)]);
    assert_eq!(parts("the \"ready set\" now"), [w("the"), p(&["ready", "set"], false), w("now")]);
    assert_eq!(parts("\"unclosed phrase"), [p(&["unclosed", "phrase"], false)]);
    assert_eq!(parts("\"one\""), [p(&["one"], false)]);
    assert_eq!(parts("  — ! ? "), []);
    assert_eq!(parts(""), []);
    assert_eq!(parts("Ünïcode"), [w("ünïcode")]);
    assert_eq!(parts(&"w ".repeat(40)).len(), search::MAX_WORDS);
}

/// An edit is an insertion, a deletion, a substitution or a swap of
/// neighbours; past the budget there is no distance at all.
#[test]
fn the_distance_counts_edits_up_to_a_budget() {
    let d = |a: &str, b: &str, max| osa(&chars(a), &chars(b), max);
    assert_eq!(d("placement", "placement", 2), Some(0));
    assert_eq!(d("plcement", "placement", 1), Some(1), "an insertion");
    assert_eq!(d("placemnet", "placement", 1), Some(1), "a swap");
    assert_eq!(d("plabement", "placement", 1), Some(1), "a substitution");
    assert_eq!(d("plcemnet", "placement", 2), Some(2));
    assert_eq!(d("plcemnet", "placement", 1), None, "two edits, a budget of one");
    assert_eq!(d("plcmnt", "placement", 2), None, "three letters short");
    assert_eq!(d("wave", "have", 1), Some(1));
    let p = |a: &str, b: &str, max| osa_prefix(&chars(a), &chars(b), max);
    assert_eq!(p("plcem", "placement", 1), Some((1, 6)), "placem…, one insertion");
    assert_eq!(p("placem", "placement", 1), Some((0, 6)));
    assert_eq!(p("plxyz", "placement", 1), None);
}

// ---------------------------------------------------------------------
// The answers

/// A word matches itself, anything it begins, in the title or the text, any
/// case; every word is required; a hit carries its row, its title and a
/// passage with the matched words marked.
#[tokio::test]
async fn every_word_is_required_and_matches_as_a_prefix() {
    let store = Store::new();
    holding(
        &store,
        &[
            ("Placements are kept", "every non-doc row carries its placement, equal at every moment"),
            ("The order of work", "each ticket's wave, derived from blocked_by"),
            ("Nothing here", "<m-option name=\"Placing\"></m-option> nor anything else"),
        ],
    )
    .await;
    assert_eq!(ids(&search(&store, "placement").await), ["SR-2"]);
    assert_eq!(ids(&search(&store, "PLAC").await), ["SR-2", "SR-4"], "a prefix; an attribute a reader reads");
    assert_eq!(ids(&search(&store, "wave blocked").await), ["SR-3"]);
    assert!(ids(&search(&store, "wave placement").await).is_empty(), "both are required");
    let page = search(&store, "placement").await;
    assert_eq!(page.matched, 1);
    assert_eq!(page.next, None);
    let hit = &page.hits[0];
    assert_eq!(hit.summary.title, "Placements are kept");
    assert!(!hit.corrected);
    assert_eq!(marked(&hit.title), "[Placements] are kept");
    assert_eq!(marked(&hit.snippet), "every non-doc row carries its [placement], equal at every moment");
    assert_eq!(page.words, [SearchWord { word: "placement".into(), also: vec![] }]);
}

/// A blank query, or one with no word, matches nothing — not everything.
#[tokio::test]
async fn a_query_without_a_word_matches_nothing() {
    let store = Store::new();
    holding(&store, &[("a title", "a body")]).await;
    for q in ["", "   ", "— ? !"] {
        assert_eq!(search(&store, q).await, SearchPage::default(), "{q:?}");
    }
}

/// A title weighs more than the text: the ticket that names the word in
/// its title comes before the one that only mentions it.
#[tokio::test]
async fn a_title_outranks_a_mention() {
    let store = Store::new();
    holding(
        &store,
        &[
            ("Unrelated work", "a long body that mentions the roll-up once among many other words of no interest at all"),
            ("The roll-up", "the body"),
        ],
    )
    .await;
    assert_eq!(ids(&search(&store, "roll").await), ["SR-3", "SR-2"]);
}

/// A word the index does not know is corrected to its words one edit away
/// (two from eight letters), the first letter kept; the tickets matched
/// without a correction come first; the corrections are said.
#[tokio::test]
async fn a_typo_is_corrected_and_ranked_after_what_matched_as_typed() {
    let store = Store::new();
    holding(
        &store,
        &[
            ("Placement", "where a ticket is placed"),
            ("Waves", "the wave a ticket is in; its placement"),
            ("Places", "no correction reaches here"),
        ],
    )
    .await;
    // `plcement` (8 letters, two edits): placement.
    let page = search(&store, "plcement").await;
    assert_eq!(ids(&page), ["SR-2", "SR-3"]);
    assert!(page.hits.iter().all(|h| h.corrected));
    assert_eq!(also(&page, "plcement"), ["placement"]);
    assert_eq!(marked(&page.hits[1].snippet), "the wave a ticket is in; its [placement]");
    // As typed first, then corrected: `wav` matches as a prefix, `palcement`
    // (a swap) by correction — both words, so only SR-3.
    assert_eq!(ids(&search(&store, "waves palcement").await), ["SR-3"]);
    // The first letter is kept; three letters are never corrected; a word
    // with a digit is never corrected; past the budget there is nothing.
    for (q, why) in [("lpacement", "the first letter"), ("plc", "three letters"), ("plac3ment", "a digit"), ("plcmnt", "three edits")] {
        let page = search(&store, q).await;
        assert!(page.hits.is_empty(), "{why}: {q} → {:?}", ids(&page));
        assert!(page.words[0].also.is_empty(), "{why}");
    }
}

/// The last word is still being typed: its corrections are prefixes too.
#[tokio::test]
async fn the_last_word_is_corrected_as_a_prefix() {
    let store = Store::new();
    holding(&store, &[("Placements", "kept"), ("Other", "placement again")]).await;
    let page = search(&store, "plcem").await;
    assert_eq!(ids(&page), ["SR-2", "SR-3"]);
    assert_eq!(also(&page, "plcem"), ["placement"], "shown as the commonest word it begins");
    // Not the last word: whole words only.
    assert!(search(&store, "plcem kept").await.hits.is_empty());
}

/// A word some ticket repeats as a typo is rare: corrected too — but only
/// to a word five times as common, so a rare word that is meant stays
/// itself.
#[tokio::test]
async fn a_rare_word_is_corrected_only_to_a_far_commoner_one() {
    let store = Store::new();
    let mut tickets = vec![("Typo", "the plcement is wrong here")];
    tickets.extend(std::iter::repeat_n(("Right", "the placement"), 5));
    tickets.push(("Meant", "roaring water"));
    tickets.push(("Near", "roaming water"));
    holding(&store, &tickets).await;
    let page = search(&store, "plcement").await;
    assert_eq!(page.hits.len(), 6, "the typo as typed, then the five");
    assert_eq!(page.hits[0].summary.id, "SR-2");
    assert!(!page.hits[0].corrected && page.hits[1..].iter().all(|h| h.corrected));
    assert_eq!(also(&page, "plcement"), ["placement"]);
    let page = search(&store, "roaring").await;
    assert_eq!(ids(&page), ["SR-8"], "roaming is no commoner");
    assert!(also(&page, "roaring").is_empty());
}

/// A joined word, and a quoted one, is a phrase: adjacent, in order.
#[tokio::test]
async fn a_joined_or_quoted_word_is_a_phrase() {
    let store = Store::new();
    holding(
        &store,
        &[
            ("Adjacent", "see FILTER-23 for it, and set_status"),
            ("Apart", "the filter is 23 words long; status set"),
        ],
    )
    .await;
    assert_eq!(ids(&search(&store, "FILTER-23").await), ["SR-2"]);
    assert_eq!(ids(&search(&store, "filter-2").await), ["SR-2"], "the last part a prefix");
    assert_eq!(ids(&search(&store, "set_status").await), ["SR-2"]);
    assert_eq!(ids(&search(&store, "\"status set\"").await), ["SR-3"]);
    let both = search(&store, "filter 23").await;
    let mut anywhere = ids(&both);
    anywhere.sort();
    assert_eq!(anywhere, ["SR-2", "SR-3"], "two words: anywhere");
    assert!(search(&store, "\"filter 2\"").await.hits.is_empty(), "a quoted phrase is exact");
}

/// A filter keeps the matches it admits — counted as it keeps them — and a
/// page is a window of what is left.
#[tokio::test]
async fn a_filter_keeps_what_it_admits_and_pages_are_windows() {
    let store = Store::new();
    let tickets: Vec<(String, String)> = (0..7).map(|i| (format!("Ticket {i}"), format!("the cache, mentioned {i}"))).collect();
    let refs: Vec<(&str, &str)> = tickets.iter().map(|(t, b)| (t.as_str(), b.as_str())).collect();
    holding(&store, &refs).await;
    for id in ["SR-3", "SR-5", "SR-6"] {
        store.set_status(id, Status::Ready).await.unwrap();
    }
    let ready = Lens::of(&Filter::parse("status Ready").unwrap()).unwrap();
    let page = store.search("cache", Some(ready.clone()), None, None).await.unwrap();
    let mut got = ids(&page);
    got.sort();
    assert_eq!(got, ["SR-3", "SR-5", "SR-6"]);
    assert_eq!(page.matched, 3);

    let all = search(&store, "cache").await;
    assert_eq!(all.matched, 7);
    let mut walked = Vec::new();
    let mut offset = Some(0);
    while let Some(at) = offset {
        let page = store.search("cache", None, Some(3), Some(at)).await.unwrap();
        assert_eq!(page.matched, 7);
        walked.extend(page.hits.iter().map(|h| h.summary.id.clone()));
        offset = page.next;
    }
    assert_eq!(walked, ids(&all), "the pages, in turn, are the whole answer");
    let past = store.search("cache", None, Some(3), Some(50)).await.unwrap();
    assert!(past.hits.is_empty() && past.next.is_none() && past.matched == 7);
    let capped = store.search("cache", None, Some(1_000), None).await.unwrap();
    assert_eq!(capped.hits.len(), 7);
}

/// Without a filter only the window is ranked; with one, every match. Both
/// give one order: pages of any size, walked by `next`, across the line
/// from the matches as typed to the corrected ones, are the whole answer
/// with or without a filter that admits everything.
#[tokio::test]
async fn windows_cross_from_the_typed_matches_to_the_corrected_ones() {
    let store = Store::new();
    let mut tickets: Vec<(String, String)> = Vec::new();
    for i in 0..3 {
        tickets.push((format!("As typed {i}"), format!("the plcement, {}", "and more ".repeat(i))));
    }
    for i in 0..23 {
        tickets.push((format!("Corrected {i}"), format!("a placement {}", "words ".repeat(i))));
    }
    let refs: Vec<(&str, &str)> = tickets.iter().map(|(t, b)| (t.as_str(), b.as_str())).collect();
    holding(&store, &refs).await;
    let everything = Lens::of(&Filter::parse("(status Ready or not status Ready)").unwrap()).unwrap();
    let whole = store.search("plcement", Some(everything.clone()), Some(100), None).await.unwrap();
    assert_eq!(whole.matched, 26);
    assert!(whole.hits[..3].iter().all(|h| !h.corrected) && whole.hits[3..].iter().all(|h| h.corrected));
    let order: Vec<String> = whole.hits.iter().map(|h| h.summary.id.to_string()).collect();
    for size in [1, 2, 3, 4, 7, 25, 26] {
        for lens in [None, Some(everything.clone())] {
            let (mut walked, mut offset) = (Vec::new(), Some(0));
            while let Some(at) = offset {
                let page = store.search("plcement", lens.clone(), Some(size), Some(at)).await.unwrap();
                assert_eq!(page.matched, 26);
                assert!(page.hits.len() <= size as usize);
                walked.extend(page.hits.iter().map(|h| h.summary.id.clone()));
                offset = page.next;
            }
            assert_eq!(walked, order, "pages of {size}, filtered: {}", lens.is_some());
        }
    }
}

// ---------------------------------------------------------------------
// The index

/// `amend` keeps the index: the old words are gone, the new ones found —
/// a new title as well as a new body.
#[tokio::test]
async fn an_amend_moves_the_index() {
    let store = Store::new();
    holding(&store, &[("Old title", "the stale words")]).await;
    store.amend("SR-2", None, Some("a fresh body".into())).await.unwrap();
    assert!(search(&store, "stale").await.hits.is_empty());
    assert_eq!(ids(&search(&store, "fresh").await), ["SR-2"]);
    assert_eq!(ids(&search(&store, "old title").await), ["SR-2"], "the title stands");
    store.amend("SR-2", Some("New heading".into()), None).await.unwrap();
    assert!(search(&store, "old title").await.hits.is_empty());
    assert_eq!(ids(&search(&store, "heading fresh").await), ["SR-2"]);
    assert_eq!(search(&store, "heading").await.hits[0].summary.title, "New heading");
}

fn index_tables(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE name LIKE 'search%' ORDER BY name").unwrap();
    let names = stmt.query_map([], |r| r.get(0)).unwrap();
    names.collect::<rusqlite::Result<Vec<String>>>().unwrap()
}

/// A file without the index — written before SEARCH-2 — gains it on its
/// first open, every ticket indexed; a later open builds nothing; what
/// was written after survives a restart; and `--verify-derived` holds it
/// to the tickets.
#[tokio::test]
async fn a_first_open_builds_the_index_and_every_later_one_reads_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("search.db");
    {
        let store = Store::open(&path).unwrap();
        holding(&store, &[("Placement", "kept in the cache"), ("Waves", "derived")]).await;
    }
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE search_terms; DROP TABLE search; DROP TABLE search_rows;").unwrap();
    drop(conn);
    assert!(index_tables(&path).is_empty());
    match Store::verify_derived(&path).unwrap() {
        Verdict::Consistent { indexed: None, tickets: 3, .. } => {}
        other => panic!("no index: nothing to compare, the rest consistent: {other:?}"),
    }
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(ids(&search(&store, "cache").await), ["SR-2"], "built by the open");
        store.create(new("SR", Kind::Build, "Born after", "a newborn's words")).await.unwrap();
    }
    assert!(!search::migrate(&mut Connection::open(&path).unwrap()).unwrap(), "built once");
    let store = Store::open(&path).unwrap();
    assert_eq!(ids(&search(&store, "newborn").await), ["SR-4"]);
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { indexed: Some(4), tickets: 4, .. }));
}

/// `--verify-derived` finds an index that is not the tickets': a ticket
/// written around it, a row whose text is not its body's, a row for no
/// ticket.
#[tokio::test]
async fn verify_finds_an_index_that_is_not_the_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify.db");
    {
        let store = Store::open(&path).unwrap();
        holding(&store, &[("Placement", "kept in the cache")]).await;
    }
    let differs = |path: &Path| match Store::verify_derived(path).unwrap() {
        Verdict::Differs(lines) => lines,
        other => panic!("expected differences, got {other:?}"),
    };
    let conn = Connection::open(&path).unwrap();
    conn.execute("UPDATE tickets SET body = 'written around the index' WHERE id = 'SR-2'", []).unwrap();
    assert_eq!(differs(&path), ["search: SR-2's indexed text is not what its body gives now"]);
    conn.execute("UPDATE tickets SET body = 'kept in the cache', title = 'Renamed' WHERE id = 'SR-2'", []).unwrap();
    assert_eq!(differs(&path), ["search: SR-2's indexed title is \"Placement\"; its ticket's is \"Renamed\""]);
    conn.execute("UPDATE tickets SET title = 'Placement' WHERE id = 'SR-2'", []).unwrap();
    conn.execute("INSERT INTO search_rows (id) VALUES ('GHOST-1')", []).unwrap();
    let lines = differs(&path);
    assert!(lines.contains(&"search: GHOST-1 is indexed and is no ticket".to_string()), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("the index holds 2 rows for 3 ids")), "{lines:?}");
    conn.execute("DELETE FROM search_rows WHERE id = 'GHOST-1'", []).unwrap();
    conn.execute("DELETE FROM search_rows WHERE id = 'SR-1'", []).unwrap();
    let lines = differs(&path);
    assert!(lines.contains(&"search: SR-1 is not indexed".to_string()), "{lines:?}");
}

/// A seed's tickets are indexed with them — searchable, and consistent.
#[tokio::test]
async fn a_seed_is_indexed_with_its_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seed.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SeedSpec { count: 600, seed: 7, into_existing: false }).unwrap();
    let page = store.search("disagree", None, Some(5), None).await.unwrap();
    assert!(page.matched > 0 && page.hits.len() == 5, "{}", page.matched);
    let begun = store.search("disagre readers", None, Some(5), None).await.unwrap();
    assert_eq!(begun.matched, page.matched, "a prefix: every ticket that disagrees has its readers");
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { indexed: Some(600), tickets: 600, .. }));
}
