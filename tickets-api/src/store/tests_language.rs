//! FILTER-4, acceptance 2 in the language: random filters made from
//! FILTER-10's own AST — every accepted field, `in`, nested `and` / `or` /
//! `not`, lists and `not` lists, a display part — sent as their text,
//! read by the language and compiled against the cells, equal the same
//! filter tested on every row by the language's reference evaluator
//! ([`Filter::matches`]): `counts`, every page after random cursors and at
//! random positions, and `facets` against a per-row tally — on stores built
//! by create, by seed and from the fixtures, after hundreds of random
//! writes, and after a reopen. Then the refusals, the empty filter, `ready`
//! as the ready set, and FILTER-3's examples.

use super::cells::{Key, SEVERITIES};
use super::filtered::FACET_FIELDS;
use super::tests_filtered::{assert_lens_is_a_scan, Plain};
use super::tests_ready::{random_write, Rng, Tally};
use super::*;
use crate::filter::{category_of, Category, Display, Facts, Field, Filter, Group, Predicate, Shape};

/// The names a random filter draws on: `in` the shortcodes among them (no
/// `-`), `under` any of them — shortcodes and ticket ids. (FILTER-13: the
/// facts the reference evaluator reads of a row, its whole lineage
/// included, are [`Plain::facts`].)
fn random_field(rng: &mut Rng, field: usize, names: &[String]) -> Field {
    let shortcodes: Vec<&String> = names.iter().filter(|n| !n.contains('-')).collect();
    match field {
        0 => Field::Status(Status::ALL[rng.below(Status::ALL.len())]),
        1 => Field::Category(Category::ALL[rng.below(Category::ALL.len())]),
        2 => Field::Kind(Kind::ALL[rng.below(Kind::ALL.len())]),
        3 => Field::Severity(SEVERITIES[rng.below(SEVERITIES.len())]),
        4 if !shortcodes.is_empty() => Field::In(shortcodes[rng.below(shortcodes.len())].to_ascii_uppercase()),
        4 => Field::In("NONE".into()),
        5 => Field::Blocked,
        6 => Field::Ready,
        // FILTER-13: `under` a shortcode or a ticket.
        8 => Field::Under(names[rng.below(names.len())].to_ascii_uppercase()),
        // FILTER-21: `menger doc`.
        10 => Field::MengerDoc,
        // MENGER-12: `asking`, and `label` — one the walk files, or one it
        // never does.
        11 => Field::Asking,
        12 => Field::Label(["needs-you", "p1", "later", "never"][rng.below(4)].into()),
        _ => Field::HasCommits,
    }
}

/// FILTER-21: `menger` draws `menger doc` among the fields — MENGER-12: and
/// `asking` and `label`; without it the draws are the ones they were
/// (`tests_tree` and `tests_lensed` replay them).
fn random_pred(rng: &mut Rng, names: &[String], menger: bool) -> Predicate {
    let which = rng.below(if menger { 13 } else { 10 });
    let field = random_field(rng, if which == 9 { 8 } else { which }, names);
    Predicate { field, not: rng.below(2) == 0 }
}

/// FILTER-13: what random filters name — the store's shortcodes, one it
/// does not hold, and ticket ids: every parent (the subtrees `under` cuts),
/// some leaves, and one that names nothing.
pub(super) fn filter_names(store: &Store) -> Vec<String> {
    let mut out: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).chain(["NONE".into()]).collect();
    let rows = super::tests_ready::rows(store);
    let parents: std::collections::BTreeSet<&str> = rows.iter().filter_map(|t| t.parent.as_deref()).collect();
    out.extend(parents.iter().take(24).map(|p| p.to_string()));
    out.extend(rows.iter().filter(|t| !parents.contains(t.id.as_str())).step_by(7).take(8).map(|t| t.id.to_string()));
    out.push("NONE-1".into());
    out
}

/// A group of every shape the language has but `text` and `under`.
fn random_group(rng: &mut Rng, epics: &[String], menger: bool) -> Group {
    match rng.below(5) {
        0 | 1 => Group::one(random_pred(rng, epics, menger)),
        2 => {
            // `field in (a, b)`, or `not field in (a, b)` — FILTER-13: of
            // `under` too.
            // MENGER-12: and of `label`, in this module's own walks.
            let field = if menger { [0, 1, 2, 3, 4, 8, 12][rng.below(7)] } else { [0, 1, 2, 3, 4, 8][rng.below(6)] };
            let (incl, n) = (rng.below(2) == 0, 2 + rng.below(2));
            let preds = (0..n).map(|_| Predicate { field: random_field(rng, field, epics), not: !incl }).collect();
            if incl {
                Group::any(preds)
            } else {
                Group::all(preds)
            }
        }
        3 => Group::any((0..2 + rng.below(2)).map(|_| random_pred(rng, epics, menger)).collect()),
        _ => Group::all((0..2 + rng.below(2)).map(|_| random_pred(rng, epics, menger)).collect()),
    }
}

/// A random filter, as the wire carries it: its text, read back.
pub(super) fn random_filter(rng: &mut Rng, epics: &[String]) -> Filter {
    random_filter_of(rng, epics, false)
}

/// FILTER-21: the same, with `menger doc` among its fields — what this
/// module's own walks draw.
fn random_menger_filter(rng: &mut Rng, epics: &[String]) -> Filter {
    random_filter_of(rng, epics, true)
}

fn random_filter_of(rng: &mut Rng, epics: &[String], menger: bool) -> Filter {
    let f = Filter {
        groups: (0..rng.below(4)).map(|_| random_group(rng, epics, menger)).collect(),
        display: match rng.below(4) {
            0 => Display { shape: Shape::Flat, ..Display::default() },
            1 => Display { columns: vec!["id".into(), "title".into()], ..Display::default() },
            _ => Display::default(),
        },
    };
    let read = Filter::parse(&f.format()).unwrap_or_else(|e| panic!("{}: {e}", f.format()));
    assert_eq!(read, f, "parse(format(f)) == f: {}", f.format());
    read
}

/// `facets` by testing every row: for each field, the rows `base` and
/// `filter`-without-the-field's-own-groups pass, tallied by their value.
fn facets_by_rows(plain: &Plain, filter: &Filter, base: &Filter) -> Vec<(String, String, u64)> {
    let mut out = Vec::new();
    for field in FACET_FIELDS {
        let rest = Filter {
            groups: filter.groups.iter().filter(|g| !g.preds.iter().all(|p| p.field.name() == field)).cloned().collect(),
            display: Display::default(),
        };
        let mut tally: Vec<(String, u64)> = Vec::new();
        let mut add = |v: String| match tally.iter_mut().find(|(x, _)| *x == v) {
            Some((_, n)) => *n += 1,
            None => tally.push((v, 1)),
        };
        for (t, k) in plain.rows.iter().zip(&plain.cells) {
            let f = plain.facts(t, *k);
            if !(base.matches(&f) && rest.matches(&f)) {
                continue;
            }
            let yes = |b: bool| if b { "yes" } else { "no" }.to_string();
            // MENGER-12: a ticket counts under each label it holds.
            if field == "label" {
                t.labels.iter().for_each(|l| add(l.to_string()));
                continue;
            }
            add(match field {
                "status" => t.status.as_str().to_string(),
                "category" => category_of(t.status).as_str().to_string(),
                "kind" => t.kind.as_str().to_string(),
                "severity" => t.severity.map_or("none".into(), |s| s.as_str().to_ascii_lowercase()),
                "blocked" => yes(k.blocked()),
                "ready" => yes(category_of(t.status) == Category::Ready && !k.blocked()),
                "has commits" => yes(!t.commits.is_empty()),
                "menger doc" => yes(t.path.as_deref().is_some_and(|p| p.starts_with("_MENGER/"))),
                "asking" => yes(k.asking()),
                _ => t.epic.to_string(),
            });
        }
        // The answer's order: each field's own, written out.
        let order: Vec<String> = match field {
            "status" => Status::ALL.iter().map(|s| s.as_str().to_string()).collect(),
            "category" => Category::ALL.iter().map(|c| c.as_str().to_string()).collect(),
            "kind" => Kind::ALL.iter().map(|k| k.as_str().to_string()).collect(),
            "severity" => ["critical", "high", "medium", "low", "none"].map(String::from).to_vec(),
            "in" | "label" => {
                let mut e: Vec<String> = tally.iter().map(|(v, _)| v.clone()).collect();
                e.sort();
                e
            }
            _ => vec!["yes".into(), "no".into()],
        };
        for v in order {
            if let Some((_, n)) = tally.iter().find(|(x, _)| *x == v) {
                out.push((field.to_string(), v, *n));
            }
        }
    }
    out
}

/// One random filter held to the evaluator: every read, and facets under a
/// random base.
#[track_caller]
fn assert_filter_is_a_scan(store: &Store, plain: &Plain, rng: &mut Rng, epics: &[String], what: &str) {
    let filter = random_menger_filter(rng, epics);
    let lens = Lens::parse(&filter.format()).unwrap();
    let judge = |t: &Row, k: Key| filter.matches(&plain.facts(t, k));
    assert_lens_is_a_scan(store, plain, &lens, &judge, rng, what);
    let base = if rng.below(2) == 0 { Filter::default() } else { random_menger_filter(rng, epics) };
    assert_eq!(
        store.facets(SYS, &filter, &base).unwrap(),
        facets_by_rows(plain, &filter, &base),
        "{what}: facets of `{}` under `{}`",
        filter.format(),
        base.format()
    );
}

async fn walk(store: Store, path: &Path, seed: u64, writes: usize, every: usize, epics: &[&str]) -> Store {
    let mut rng = Rng(seed);
    // FILTER-21: board notes among the docs — MENGER-12: and inputs and
    // labels moving.
    let mut tally = Tally::asking_and_labelling();
    let names = filter_names;
    for step in 0..writes {
        let what = random_write(&store, &mut rng, epics, &mut tally).await;
        if step % every == every - 1 {
            let plain = Plain::of(&store);
            let held = names(&store);
            for _ in 0..3 {
                assert_filter_is_a_scan(&store, &plain, &mut rng, &held, &format!("step {step}: {what}"));
            }
        }
    }
    drop(store);
    let store = Store::open(path).unwrap();
    let (plain, held) = (Plain::of(&store), names(&store));
    for _ in 0..8 {
        assert_filter_is_a_scan(&store, &plain, &mut rng, &held, "the reopen");
    }
    store
}

#[tokio::test]
async fn random_filters_equal_the_evaluator_on_a_store_built_by_create() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("created.db");
    let store = Store::open(&path).unwrap();
    let store = walk(store, &path, 0xF117_E410, 450, 9, &["AA", "MAP", "SUB", "ZED"]).await;
    // FILTER-21: the walk filed menger docs beside ordinary ones; the flag,
    // its negation and the nav's default under another flag are answered as
    // the evaluator answers them — counts, pages at cursors and positions,
    // and facets, whose `menger doc` menu counts both values.
    let plain = Plain::of(&store);
    let menger = |t: &Row| t.path.as_deref().is_some_and(|p| p.starts_with("_MENGER/"));
    let docs = plain.rows.iter().filter(|t| t.kind == Kind::Doc);
    assert!(plain.rows.iter().any(menger) && docs.clone().any(|t| !menger(t)), "board notes and ordinary docs");
    let mut rng = Rng(0xF117_E421);
    for text in ["menger doc", "not menger doc", "not menger doc and kind doc", "(menger doc or has commits) and not blocked"] {
        let filter = Filter::parse(text).unwrap();
        let lens = Lens::parse(text).unwrap();
        let judge = |t: &Row, k: Key| filter.matches(&plain.facts(t, k));
        assert_lens_is_a_scan(&store, &plain, &lens, &judge, &mut rng, text);
        let facets = store.facets(SYS, &filter, &Filter::default()).unwrap();
        assert_eq!(facets, facets_by_rows(&plain, &filter, &Filter::default()), "facets of `{text}`");
    }
    let facets = store.facets(SYS, &Filter::default(), &Filter::default()).unwrap();
    let menu: Vec<&str> = facets.iter().filter(|(f, _, _)| f == "menger doc").map(|(_, v, _)| v.as_str()).collect();
    assert_eq!(menu, ["yes", "no"]);
}

#[tokio::test]
async fn random_filters_equal_the_evaluator_on_a_seeded_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seeded.db");
    let store = Store::open(&path).unwrap();
    store.seed_synthetic(SYS, SeedSpec { count: 3_000, seed: 410, into_existing: false }).unwrap();
    let epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).take(5).chain(["NEW".into()]).collect();
    let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
    walk(store, &path, 0x5EED_F410, 250, 25, &epics).await;
}

#[tokio::test]
async fn random_filters_equal_the_evaluator_on_each_fixture() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures");
    for (i, name) in ["pre-ags13.db", "pre-ags19.db", "pre-scale4.db", "pre-sdlc7.db"].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(format!("{fixtures}/{name}"), &path).unwrap();
        let store = Store::open(&path).unwrap();
        let mut epics: Vec<String> = store.extents_text().into_iter().map(|(e, _)| e).collect();
        epics.push("NEW".into());
        let epics: Vec<&str> = epics.iter().map(String::as_str).collect();
        walk(store, &path, 0xF410 + i as u64, 120, 12, &epics).await;
    }
}

/// FILTER-3 §2's examples and §6.2's nine filters: each read by the
/// language and answered, equal to the evaluator.
#[tokio::test]
async fn every_filter_3_filter_is_answered_as_the_evaluator_answers() {
    let store = Store::new();
    store.seed_synthetic(SYS, SeedSpec { count: 4_000, seed: 1, into_existing: false }).unwrap();
    let big = store.extents_text().into_iter().max_by_key(|(_, n)| *n).unwrap().0;
    let plain = Plain::of(&store);
    let mut rng = Rng(0x3_62);
    for text in [
        "ready and kind build and in SCALE".to_string(),
        "status in (Pending, Ready) and not severity none".into(),
        "(blocked or status Blocked) and not category static".into(),
        "category in (active, review)".into(),
        "not category in (done, dropped, static) · flat".into(),
        "has commits and category review".into(),
        "status Ready".into(),
        "status in (Pending, Ready) and kind build".into(),
        "ready".into(),
        "blocked and severity critical".into(),
        "kind scope and status Idea and severity critical".into(),
        // §6.2's `category done or category dropped`, as the language
        // writes it.
        "category in (done, dropped)".into(),
        "(status Blocked or blocked) and not kind doc".into(),
        "not kind doc".into(),
        format!("ready and in {big}"),
        String::new(),
    ] {
        let filter = Filter::parse(&text).unwrap();
        assert_eq!(filter.format(), text, "canonical");
        let lens = Lens::parse(&text).unwrap();
        let judge = |t: &Row, k: Key| filter.matches(&plain.facts(t, k));
        assert_lens_is_a_scan(&store, &plain, &lens, &judge, &mut rng, &text);
        assert_eq!(store.facets(SYS, &filter, &Filter::default()).unwrap(), facets_by_rows(&plain, &filter, &Filter::default()), "{text}");
    }
}

/// `ready` is exactly the ready set: per shortcode, its count is the ready
/// set's share, and its pages are the ready set's pages.
#[tokio::test]
async fn ready_is_the_ready_set() {
    let store = Store::new();
    store.seed_synthetic(SYS, SeedSpec { count: 3_000, seed: 2, into_existing: false }).unwrap();
    let lens = Lens::parse("ready").unwrap();
    for (epic, held) in store.counts_text(&lens) {
        assert_eq!(held, store.ready_page(SYS, Some(&epic), None, Some(0)).unwrap().remaining, "{epic}");
    }
    let q = Filtered { lens, epic: None, status: None, parent: None, keep: None };
    let (mut after, mut walked) = (None::<String>, 0);
    loop {
        let filtered = store.filtered_page(SYS, &q, after.as_deref(), None, Some(150)).unwrap();
        let ready = store.ready_page(SYS, None, after.as_deref(), Some(150)).unwrap();
        assert_eq!((&filtered.tickets, &filtered.next, filtered.remaining), (&ready.tickets, &ready.next, ready.remaining));
        assert_eq!(filtered.first, Some(walked));
        walked += filtered.tickets.len() as u64;
        match filtered.next {
            Some(n) => after = Some(n),
            None => break,
        }
    }
    assert!(walked > 100);
}

/// The empty filter is every ticket: `counts {}` is each shortcode's
/// tickets.
#[tokio::test]
async fn the_empty_filter_counts_every_ticket_of_every_shortcode() {
    let store = Store::new();
    store.seed_synthetic(SYS, SeedSpec { count: 2_000, seed: 3, into_existing: false }).unwrap();
    let extents: Vec<(String, u64)> = store.extents_text().into_iter().map(|(e, n)| (e, u64::from(n))).collect();
    assert_eq!(store.counts_text(&Lens::all()), extents);
    assert_eq!(store.counts_text(&Lens::parse("").unwrap()), extents);
    assert_eq!(store.counts_text(&Lens::parse("· flat · order code").unwrap()), extents, "the display is ignored");
}

/// `text` is in the language and answered elsewhere: every filtered read
/// refuses it in-band, naming each chunk that holds one and where it lives;
/// a chunk the language cannot read is its error, naming every one.
/// FILTER-13: `under` is answered — by the same reads, with no refusal.
#[test]
fn text_is_refused_naming_where_it_lives_and_under_is_answered() {
    let refused = Lens::parse(r#"ready and under SCALE-13 and text "watch" and (in SCALE or under MNG)"#).unwrap_err().to_string();
    assert!(refused.contains("`text \"watch\"` — text is the palette's — ask `find`"), "{refused}");
    assert!(!refused.contains("under"), "under is not refused: {refused}");
    assert!(!refused.contains("`ready` —"), "{refused}");
    assert!(Lens::parse("ready and under SCALE-13 and (in SCALE or under MNG)").is_ok());
    let store = Store::new();
    let under = Filter::parse("under SCALE").unwrap();
    assert!(store.facets(SYS, &under, &Filter::default()).is_ok());
    assert!(store.facets(SYS, &Filter::default(), &under).is_ok(), "the base too");
    let text = Filter::parse("text \"x\"").unwrap();
    assert!(store.facets(SYS, &text, &Filter::default()).is_err());
    let bad = Lens::parse("tag x and ready and written today and nonsense").unwrap_err().to_string();
    for chunk in ["`tag x`", "`written today`", "`nonsense`"] {
        assert!(bad.contains(chunk), "{bad}");
    }
    assert!(!bad.contains("`ready` —"), "a chunk it took is not named: {bad}");
    let unknown = Lens::parse("status Bogus").unwrap_err().to_string();
    assert!(unknown.contains("Pending") && unknown.contains("In Review"), "an unknown status lists the known ones: {unknown}");
    let many: Vec<String> = (0..=UNDERS_MAX).map(|i| format!("under X-{i}")).collect();
    let too_many = Lens::parse(&many.join(" and ")).unwrap_err().to_string();
    assert!(too_many.contains("at most 64 names"), "{too_many}");
}

/// A group is a field's own — and set aside for that field's facet — only
/// when every predicate in it names that field: `status Ready` and
/// `status in (A, B)` are status's own; `(status Ready or blocked)` is no
/// field's.
#[tokio::test]
async fn a_facet_sets_aside_only_its_fields_own_groups() {
    let store = Store::new();
    store.seed_synthetic(SYS, SeedSpec { count: 1_500, seed: 5, into_existing: false }).unwrap();
    let plain = Plain::of(&store);
    let all = store.facets(SYS, &Filter::default(), &Filter::default()).unwrap();
    let by_status: Vec<&(String, String, u64)> = all.iter().filter(|(f, _, _)| f == "status").collect();
    let f = Filter::parse("status Ready and kind build").unwrap();
    let got = store.facets(SYS, &f, &Filter::default()).unwrap();
    // The status menu counts builds of every status; the kind menu, Ready
    // tickets of every kind.
    let total = |field: &str, v: &[(String, String, u64)]| -> u64 { v.iter().filter(|(x, _, _)| x == field).map(|(_, _, n)| n).sum() };
    assert_eq!(total("status", &got), plain.rows.iter().filter(|t| t.kind == Kind::Build).count() as u64);
    assert_eq!(total("kind", &got), plain.rows.iter().filter(|t| t.status == Status::Ready).count() as u64);
    assert!(by_status.len() >= 5);
    let mixed = Filter::parse("(status Ready or blocked)").unwrap();
    let got = store.facets(SYS, &mixed, &Filter::default()).unwrap();
    assert_eq!(got, facets_by_rows(&plain, &mixed, &Filter::default()), "no field's own: kept for every field");
    let blocked_not_ready = plain.rows.iter().zip(&plain.cells).filter(|(t, k)| k.blocked() && t.status != Status::Ready).count();
    assert!(blocked_not_ready > 0 && got.iter().any(|(f, v, _)| f == "status" && v != "Ready"), "the status menu still holds the blocked");
    // Every facet value is canonical.
    for (field, value, _) in &all {
        match field.as_str() {
            "status" => assert!(Status::ALL.iter().any(|s| s.as_str() == value), "{value}"),
            "blocked" | "ready" | "has commits" => assert!(value == "yes" || value == "no"),
            _ => {}
        }
    }
}

/// What only a hand edit makes — a lower-case shortcode, holes, a sequence
/// 0: `in` reads a shortcode as the language does, whatever its case, and
/// `list { epic }` stays exact, as it always was; every read still equals
/// the evaluator.
#[tokio::test]
async fn hand_edited_shortcodes_are_read_as_the_language_reads_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("holes.db");
    super::tests_paging::write_runs(&path, &[("HOLE", 1), ("HOLE", 2), ("HOLE", 5), ("ZERO", 0), ("ZERO", 1), ("low", 1), ("low", 2)]);
    Connection::open(&path)
        .unwrap()
        .execute_batch("UPDATE tickets SET status = 'Ready'; UPDATE tickets SET blocked_by = '[\"HOLE-3\"]' WHERE id = 'low-2';")
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.counts_text(&Lens::parse("in LOW").unwrap()), [("low".to_string(), 2)]);
    assert_eq!(store.counts_text(&Lens::parse("ready and in low").unwrap()), [("low".to_string(), 1)]);
    let plain = Plain::of(&store);
    let mut rng = Rng(0x10E);
    let epics = vec!["low".to_string(), "HOLE".to_string(), "ZERO".to_string()];
    for _ in 0..40 {
        let filter = random_filter(&mut rng, &epics);
        let lens = Lens::parse(&filter.format()).unwrap();
        let judge = |t: &Row, k: Key| filter.matches(&plain.facts(t, k));
        assert_lens_is_a_scan(&store, &plain, &lens, &judge, &mut rng, "hand-edited shortcodes");
    }
}
