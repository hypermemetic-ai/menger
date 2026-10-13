//! FILTER-10: the filter language — FILTER-3 §2's examples and manifest's
//! grammar cases, the refusals, the merges and the contradictions as case
//! tables, then the properties over random filters and random facts:
//! `parse(format(f)) == f`, `a.and(b)` matches what both match, and
//! `admits_nothing` is exact over the closed domain and sound on random
//! tickets.

use super::*;

fn parsed(text: &str) -> Filter {
    Filter::parse(text).unwrap_or_else(|e| panic!("{text:?} parses: {e}"))
}

fn refused(text: &str) -> FilterError {
    match Filter::parse(text) {
        Ok(f) => panic!("{text:?} is refused, but parsed as {f:?}"),
        Err(e) => e,
    }
}

fn is(field: Field) -> Predicate {
    Predicate::is(field)
}

fn not(field: Field) -> Predicate {
    Predicate::is_not(field)
}

fn one(p: Predicate) -> Group {
    Group::one(p)
}

fn groups(gs: Vec<Group>) -> Filter {
    Filter { groups: gs, display: Display::default() }
}

fn status(s: Status) -> Field {
    Field::Status(s)
}

fn category(c: Category) -> Field {
    Field::Category(c)
}

fn kind(k: Kind) -> Field {
    Field::Kind(k)
}

fn sev(s: Option<Severity>) -> Field {
    Field::Severity(s)
}

fn within(e: &str) -> Field {
    Field::In(e.into())
}

fn under(x: &str) -> Field {
    Field::Under(x.into())
}

fn text(t: &str) -> Field {
    Field::Text(t.into())
}

// -- FILTER-3 §2: the examples, canonical ------------------------------------------

/// FILTER-3 §2's six examples, each canonical, and what each reads as.
#[test]
fn every_filter_3_example_parses_to_its_groups_and_formats_back_to_itself() {
    use Category::*;
    let cases: Vec<(&str, Filter)> = vec![
        (
            "ready and kind build and in SCALE",
            groups(vec![one(is(Field::Ready)), one(is(kind(Kind::Build))), one(is(within("SCALE")))]),
        ),
        (
            "status in (Pending, Ready) and not severity none",
            groups(vec![
                Group::any(vec![is(status(Status::Pending)), is(status(Status::Ready))]),
                one(not(sev(None))),
            ]),
        ),
        (
            "(blocked or status Blocked) and not category static",
            groups(vec![Group::any(vec![is(Field::Blocked), is(status(Status::Blocked))]), one(not(category(Static)))]),
        ),
        ("category in (active, review)", groups(vec![Group::any(vec![is(category(Active)), is(category(Review))])])),
        (
            "not category in (done, dropped, static) · flat",
            Filter {
                groups: vec![Group::all(vec![not(category(Done)), not(category(Dropped)), not(category(Static))])],
                display: Display { shape: Shape::Flat, ..Display::default() },
            },
        ),
        ("has commits and category review", groups(vec![one(is(Field::HasCommits)), one(is(category(Review)))])),
    ];
    for (text, want) in cases {
        assert_eq!(parsed(text), want, "{text}");
        assert_eq!(want.format(), text, "the canonical form of {text}");
    }
}

/// More canonical texts — every field, the quoted status, lists of every
/// listable field, the display parts — each a fixed point of format ∘ parse.
#[test]
fn canonical_texts_are_fixed_points() {
    for text in [
        "",
        "ready",
        "not blocked",
        "has commits",
        "menger doc",
        "not menger doc",
        "not menger doc and kind build",
        "(menger doc or has commits)",
        "status \"In Review\"",
        "status in (Ready, \"In Review\")",
        "not status in (Complete, Superseded, Doc)",
        "kind in (build, spike, gap)",
        "severity in (critical, high)",
        "not severity none",
        "in (SCALE, MNG)",
        "not in (SCALE, MNG)",
        "not in SCALE",
        "under SCALE-13",
        "under in (SCALE, MNG-2)",
        "not under in (SCALE, MNG-2)",
        "text \"watch\"",
        "text \"a and b\" and text \"(x), y · z\"",
        "(kind build and severity critical)",
        "(in SCALE or not ready or status \"In Review\")",
        "(not status Ready or not kind doc)",
        "ready · flat",
        "· flat",
        "ready · order code",
        "ready · flat · order code · columns id, title",
        "· columns id",
    ] {
        assert_eq!(parsed(text).format(), text, "{text}");
    }
}

/// The FILTER-3 §6.2 and §1 filters the server's measurements and the
/// built-in views name all parse.
#[test]
fn the_filters_filter_3_names_elsewhere_parse() {
    for text in [
        "status Ready",
        "status in (Pending, Ready) and kind build",
        "blocked and severity critical",
        "kind scope and status Idea and severity critical",
        "category done or category dropped",
        "(status Blocked or blocked) and not kind doc",
        "not kind doc",
        "status \"In Review\"",
        "under SCALE",
        "\"watch\"",
        "not category static",
        "not category in (done, dropped, static)",
        "category static",
    ] {
        parsed(text);
    }
}

// -- manifest's grammar ------------------------------------------------------------

/// manifest's cases — lists, `not` lists, parentheses with `or` and with
/// `and`, an unparenthesized `or`, `not` toggling, quoting, case — read as
/// views.ts reads them, with menger's fields.
#[test]
fn manifests_grammar_cases_parse_to_the_same_groups() {
    use Status::*;
    let cases: Vec<(&str, Vec<Group>)> = vec![
        ("status in (Pending, Ready)", vec![Group::any(vec![is(status(Pending)), is(status(Ready))])]),
        ("status in(Pending,Ready)", vec![Group::any(vec![is(status(Pending)), is(status(Ready))])]),
        ("not kind in (doc, spike)", vec![Group::all(vec![not(kind(Kind::Doc)), not(kind(Kind::Spike))])]),
        ("(ready or blocked)", vec![Group::any(vec![is(Field::Ready), is(Field::Blocked)])]),
        ("(ready and blocked)", vec![Group::all(vec![is(Field::Ready), is(Field::Blocked)])]),
        ("ready or blocked", vec![Group::any(vec![is(Field::Ready), is(Field::Blocked)])]),
        ("ready or blocked and kind gap", vec![
            Group::any(vec![is(Field::Ready), is(Field::Blocked)]),
            one(is(kind(Kind::Gap))),
        ]),
        ("(not ready or blocked)", vec![Group::any(vec![not(Field::Ready), is(Field::Blocked)])]),
        ("not not ready", vec![one(is(Field::Ready))]),
        ("not not not ready", vec![one(not(Field::Ready))]),
        ("NOT Ready", vec![one(not(Field::Ready))]),
        ("Status READY", vec![one(is(status(Ready)))]),
        ("status active", vec![one(is(status(Coding)))]),
        ("status in review", vec![one(is(status(InReview)))]),
        ("status \"in review\"", vec![one(is(status(InReview)))]),
        ("status in (ready, in review)", vec![Group::any(vec![is(status(Ready)), is(status(InReview))])]),
        ("status in (Ready)", vec![one(is(status(Ready)))]),
        ("not status in (Ready)", vec![one(not(status(Ready)))]),
        ("kind Build", vec![one(is(kind(Kind::Build)))]),
        ("severity Critical", vec![one(is(sev(Some(Severity::Critical))))]),
        ("category Static", vec![one(is(category(Category::Static)))]),
        ("in scale", vec![one(is(within("SCALE")))]),
        ("in \"scale\"", vec![one(is(within("SCALE")))]),
        ("in (scale, mng)", vec![Group::any(vec![is(within("SCALE")), is(within("MNG"))])]),
        ("not in (scale, mng)", vec![Group::all(vec![not(within("SCALE")), not(within("MNG"))])]),
        ("(in scale or kind build)", vec![Group::any(vec![is(within("SCALE")), is(kind(Kind::Build))])]),
        ("under scale-13", vec![one(is(under("SCALE-13")))]),
        ("\"watch\"", vec![one(is(text("watch")))]),
        ("text \"Watch Me\"", vec![one(is(text("Watch Me")))]),
        ("text watch", vec![one(is(text("watch")))]),
        ("text \"a and b\"", vec![one(is(text("a and b")))]),
        ("\"x (y), z · w\"", vec![one(is(text("x (y), z · w")))]),
        ("(text \"a\")", vec![one(is(text("a")))]),
        ("  ready   and   blocked  ", vec![one(is(Field::Ready)), one(is(Field::Blocked))]),
        ("ready AND blocked", vec![one(is(Field::Ready)), one(is(Field::Blocked))]),
        ("ready and ", vec![one(is(Field::Ready))]),
    ];
    for (text, want) in cases {
        assert_eq!(parsed(text).groups, want, "{text}");
    }
}

/// The display part: manifest's chunks after `·`, menger's subset.
#[test]
fn the_display_part_keeps_tree_flat_columns_and_order_code() {
    let d = |text: &str| parsed(text).display;
    assert_eq!(d("ready"), Display::default());
    assert_eq!(d("ready · tree"), Display::default());
    assert_eq!(d("ready · Flat").shape, Shape::Flat);
    assert_eq!(d("ready · flat · tree").shape, Shape::Tree, "the last one says");
    assert_eq!(d("ready · order code").order, Some(Order::Code));
    assert_eq!(d("ready · columns ID, Title").columns, vec!["id", "title"]);
    assert_eq!(d("· flat"), Display { shape: Shape::Flat, ..Display::default() });
    assert_eq!(parsed("· flat").groups, vec![]);
    assert_eq!(parsed("ready · tree").format(), "ready", "the default display is written as nothing");
    assert_eq!(parsed("ready ·").format(), "ready");
}

// -- strictness ----------------------------------------------------------------------

/// Each refused field — manifest's that menger holds no data for, and the
/// three that need an index — is one bad chunk naming it.
#[test]
fn each_refused_field_is_an_error_naming_it() {
    for (text, field) in [
        ("tag urgent", "tag"),
        ("#urgent", "#tag"),
        ("session open", "session"),
        ("session in (open, active)", "session"),
        ("discussion", "discussion"),
        ("unattested", "unattested"),
        ("past cap", "past cap"),
        ("lowest Ready", "lowest"),
        ("archived", "archived"),
        ("not archived", "archived"),
        ("depth <= 2", "depth"),
        ("created < 2d", "created"),
        ("written < 1w", "written"),
        ("age > 3d", "age"),
    ] {
        let e = refused(text);
        assert_eq!(e.chunks.len(), 1, "{text}: {e}");
        assert_eq!(e.chunks[0].text, text);
        assert!(e.chunks[0].why.contains(&format!("`{field}`")), "{text}: {}", e.chunks[0].why);
    }
}

/// `text` anywhere but as a top-level `and` group of its own.
#[test]
fn text_inside_or_not_or_parentheses_is_an_error() {
    for text in [
        "(text \"a\" or ready)",
        "\"a\" or ready",
        "not text \"a\"",
        "not \"a\"",
        "(text \"a\" and ready)",
        "(text \"a\" and text \"b\")",
        "text in (a, b)",
        "ready and not \"a\"",
    ] {
        let e = refused(text);
        assert_eq!(e.chunks.len(), 1, "{text}: {e}");
        assert!(e.chunks[0].why.contains("tested on rows"), "{text}: {}", e.chunks[0].why);
    }
    parsed("text \"a\" and text \"b\" and ready");
}

/// One error names every bad chunk — filter and display alike — and
/// nothing is parsed.
#[test]
fn one_error_names_every_bad_chunk() {
    let e = refused("ready and tag x and kind build and created < 2d and frobnicate and (text \"a\" or blocked) · flat · board by status");
    let named: Vec<&str> = e.chunks.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(named, ["tag x", "created < 2d", "frobnicate", "(text \"a\" or blocked)", "board by status"]);
    assert!(e.chunks.iter().all(|c| !c.why.is_empty()));
    let shown = e.to_string();
    for c in &e.chunks {
        assert!(shown.contains(&c.text) && shown.contains(&c.why), "{shown}");
    }
}

/// A group with several faults names each, by its predicate.
#[test]
fn a_group_names_each_of_its_faults() {
    let e = refused("(tag x or session open or ready)");
    let why = &e.chunks[0].why;
    assert!(why.contains("`tag x`") && why.contains("`session open`"), "{why}");
}

/// An unknown status lists the known ones; so do an unknown kind,
/// category and severity.
#[test]
fn an_unknown_name_lists_the_known_ones() {
    let e = refused("status Done");
    let why = &e.chunks[0].why;
    for s in Status::ALL {
        assert!(why.contains(s.as_str()), "{s} in {why}");
    }
    let e = refused("status in (Ready, Shipped)");
    assert!(e.chunks[0].why.contains("`Shipped`") && e.chunks[0].why.contains("In Review"), "{e}");
    let e = refused("kind bug");
    for k in Kind::ALL {
        assert!(e.chunks[0].why.contains(k.as_str()), "{e}");
    }
    let e = refused("category closed");
    for c in Category::ALL {
        assert!(e.chunks[0].why.contains(c.as_str()), "{e}");
    }
    let e = refused("severity urgent");
    for s in ["critical", "high", "medium", "low", "none"] {
        assert!(e.chunks[0].why.contains(s), "{e}");
    }
}

/// What the grammar cannot read is an error, never a dropped chunk.
#[test]
fn unreadable_chunks_are_errors() {
    for text in [
        "frobnicate",
        "not",
        "ready and and blocked",
        "ready and",
        "status",
        "blocked yes",
        "has commits 3",
        "menger doc yes",
        "menger doc board",
        "menger doc in (a, b)",
        "menger",
        "menger docs",
        "in",
        "in SC-1",
        "in (SCALE, MNG-2)",
        "under \"a b\"",
        "text \"\"",
        "\"\"",
        "\"unterminated",
        "status \"In Review",
        "status In\"Review",
        "(ready",
        "ready)",
        "()",
        "status in ()",
        "(ready or blocked and kind build)",
        "((ready or blocked) or kind build)",
        "(status in (Ready, Pending) or blocked)",
        "not (ready or blocked)",
        "has",
        "readyx",
        "ready · tree by kind",
        "ready · board",
        "ready · order created",
        "ready · order",
        "ready · columns",
        "ready · columns (a)",
        "ready · wide",
        "flat",
    ] {
        let e = refused(text);
        assert!(!e.chunks.is_empty() && e.chunks.iter().all(|c| !c.why.is_empty()), "{text}: {e:?}");
    }
}

// -- categories ----------------------------------------------------------------------

/// The one table.
#[test]
fn every_status_has_its_category() {
    use Category::*;
    let want = [
        (Status::Pending, Open),
        (Status::Ready, Ready),
        (Status::Coding, Active),
        (Status::InReview, Review),
        (Status::Blocked, Open),
        (Status::Complete, Done),
        (Status::Idea, Open),
        (Status::Epic, Open),
        (Status::Superseded, Dropped),
        (Status::Canceled, Dropped),
        (Status::Doc, Static),
    ];
    assert_eq!(want.len(), Status::ALL.len());
    for (s, c) in want {
        assert_eq!(category_of(s), c, "{s}");
        assert_eq!(c.as_str().parse::<Category>(), Ok(c));
    }
}

// -- the evaluator --------------------------------------------------------------------

fn facts(id: &str, title: &str, status: Status, kind: Kind) -> Facts {
    let epic = id.rsplit_once('-').map_or(id, |(e, _)| e).to_string();
    Facts {
        asking: false,
        labels: Vec::new(),
        id: id.into(),
        lineage: vec![id.into(), epic.clone()],
        history: Vec::new(),
        epic,
        title: title.into(),
        status,
        kind,
        severity: None,
        blocked: false,
        has_commits: false,
        menger_doc: false,
    }
}

/// Every predicate, on hand-built facts.
#[test]
fn the_evaluator_reads_every_predicate() {
    let mut t = facts("SCALE-13", "Watch the bounded store", Status::Ready, Kind::Build);
    t.lineage = vec!["SCALE-13".into(), "SCALE-1".into(), "SCALE".into(), "PLEXUS".into()];
    let m = |text: &str, t: &Facts| parsed(text).matches(t);
    assert!(m("", &t));
    assert!(m("status Ready and category ready and kind build and severity none", &t));
    assert!(m("ready and not blocked and not has commits", &t));
    assert!(m("in SCALE and in scale and not in MNG", &t));
    assert!(m("under SCALE-13 and under SCALE-1 and under SCALE and under plexus and not under SCALE-2", &t));
    assert!(m("text \"watch\" and \"BOUNDED\" and \"scale-1\" and \"  store \" and text \" \"", &t));
    assert!(!m("text \"watched\"", &t));
    t.blocked = true;
    assert!(m("blocked and not ready and category ready", &t), "blocked leaves the ready set, not its category");
    t.severity = Some(Severity::High);
    t.has_commits = true;
    assert!(m("severity high and not severity none and has commits", &t));
    assert!(m("(severity critical or severity high) and severity in (low, high)", &t));
    assert!(!m("(severity critical and severity high)", &t));
    let d = facts("DOCS-2", "A finding", Status::Doc, Kind::Doc);
    assert!(m("category static and kind doc and status Doc", &d));
    assert!(!m("not category static", &d));
    let empty = Group { conj: Conj::Any, preds: vec![] };
    assert!(empty.matches(&d), "an empty group passes, as manifest's");
}

/// FILTER-21: `menger doc` is a flag like `ready` and `has commits` — read
/// whatever its case and however many `not`s, written back canonical, never
/// with a value, in a list or alone as `menger` — and it admits exactly the
/// tickets whose path starts with `_MENGER/`; `not menger doc` the rest.
#[test]
fn menger_doc_is_a_flag_that_round_trips_and_admits_menger_paths() {
    let cases: Vec<(&str, Vec<Group>, &str)> = vec![
        ("menger doc", vec![one(is(Field::MengerDoc))], "menger doc"),
        ("not menger doc", vec![one(not(Field::MengerDoc))], "not menger doc"),
        ("NOT Menger DOC", vec![one(not(Field::MengerDoc))], "not menger doc"),
        ("not not menger doc", vec![one(is(Field::MengerDoc))], "menger doc"),
        ("(menger doc or ready)", vec![Group::any(vec![is(Field::MengerDoc), is(Field::Ready)])], "(menger doc or ready)"),
        ("menger doc or has commits", vec![Group::any(vec![is(Field::MengerDoc), is(Field::HasCommits)])], "(menger doc or has commits)"),
        ("not menger doc and in PLEXUS", vec![one(not(Field::MengerDoc)), one(is(within("PLEXUS")))], "not menger doc and in PLEXUS"),
    ];
    for (text, want, canonical) in cases {
        let f = parsed(text);
        assert_eq!(f.groups, want, "{text}");
        assert_eq!(f.format(), canonical, "{text}");
        assert_eq!(parsed(canonical), f, "{canonical} is a fixed point");
    }
    assert_eq!(Field::MengerDoc.name(), "menger doc");
    assert_eq!(Field::MengerDoc.value(), None, "a flag has no value, as `ready`'s");
    assert_eq!(Field::Ready.value(), Field::MengerDoc.value());
    for (text, why) in [("menger doc yes", "takes no value"), ("menger doc in (a, b)", "takes no value"), ("menger", "not understood")] {
        let e = refused(text);
        assert_eq!(e.chunks.len(), 1, "{text}: {e}");
        assert!(e.chunks[0].why.contains(why), "{text}: {}", e.chunks[0].why);
    }
    assert!(refused("frobnicate").chunks[0].why.contains("`menger doc`"), "the predicates listed name it");

    // The evaluator: a board note, an ordinary doc, a build.
    let mut board = facts("PLEXUS-2", "board: PLEXUS", Status::Doc, Kind::Doc);
    board.menger_doc = true;
    let plain = facts("PLEXUS-3", "A finding", Status::Doc, Kind::Doc);
    let work = facts("PLEXUS-4", "Build it", Status::Ready, Kind::Build);
    let m = |text: &str, t: &Facts| parsed(text).matches(t);
    assert!(m("menger doc", &board) && !m("menger doc", &plain) && !m("menger doc", &work));
    assert!(!m("not menger doc", &board) && m("not menger doc", &plain) && m("not menger doc", &work));
    assert!(m("menger doc and kind doc and category static and in PLEXUS", &board));
    assert!(m("(menger doc or ready)", &board) && m("(menger doc or ready)", &work) && !m("(menger doc or ready)", &plain));
    assert!(m("not menger doc and kind doc", &plain) && !m("not menger doc and kind doc", &board));
}

/// MENGER-12: `asking` is a flag like `menger doc`, and `label <name>` a
/// value — read whatever their case (a label folded to lower case), written
/// back canonical, `label` listable (`label in (a, b)`); `asking` admits the
/// tickets with an open input, `label x` those holding `x`; a bad label is
/// refused, naming why.
#[test]
fn asking_and_label_round_trip_and_admit() {
    let label = |l: &str| Field::Label(l.into());
    let cases: Vec<(&str, Vec<Group>, &str)> = vec![
        ("asking", vec![one(is(Field::Asking))], "asking"),
        ("NOT Asking", vec![one(not(Field::Asking))], "not asking"),
        ("label needs-you", vec![one(is(label("needs-you")))], "label needs-you"),
        ("label Needs-You", vec![one(is(label("needs-you")))], "label needs-you"),
        ("label \"needs-you\"", vec![one(is(label("needs-you")))], "label needs-you"),
        ("not label p1", vec![one(not(label("p1")))], "not label p1"),
        ("asking or label needs-you", vec![Group::any(vec![is(Field::Asking), is(label("needs-you"))])], "(asking or label needs-you)"),
        ("label in (a, b)", vec![Group::any(vec![is(label("a")), is(label("b"))])], "label in (a, b)"),
        ("not label in (a, b)", vec![Group::all(vec![not(label("a")), not(label("b"))])], "not label in (a, b)"),
        ("asking and in MENGER", vec![one(is(Field::Asking)), one(is(within("MENGER")))], "asking and in MENGER"),
    ];
    for (text, want, canonical) in cases {
        let f = parsed(text);
        assert_eq!(f.groups, want, "{text}");
        assert_eq!(f.format(), canonical, "{text}");
        assert_eq!(parsed(canonical), f, "{canonical} is a fixed point");
    }
    assert_eq!((Field::Asking.name(), Field::Asking.value()), ("asking", None));
    assert_eq!((label("x").name(), label("x").value()), ("label", Some("x".to_string())));
    for (text, why) in [
        ("asking yes", "takes no value"),
        ("label", "needs a value"),
        ("label \"two words\"", "not a label"),
        ("label a_b", "not a label"),
        ("tag needs-you", "no data"),
    ] {
        let e = refused(text);
        assert_eq!(e.chunks.len(), 1, "{text}: {e}");
        assert!(e.chunks[0].why.contains(why), "{text}: {}", e.chunks[0].why);
    }
    assert!(refused("frobnicate").chunks[0].why.contains("`asking`, `label`"), "the predicates listed name them");

    // The evaluator: one asking, one labelled, one both, one neither.
    let mut asks = facts("MENGER-2", "asks", Status::Coding, Kind::Build);
    asks.asking = true;
    let mut tagged = facts("MENGER-3", "tagged", Status::Ready, Kind::Build);
    tagged.labels = vec!["needs-you".into(), "p1".into()];
    let mut both = facts("MENGER-4", "both", Status::Doc, Kind::Doc);
    (both.asking, both.labels) = (true, vec!["needs-you".into()]);
    let quiet = facts("MENGER-5", "quiet", Status::Pending, Kind::Build);
    let m = |text: &str, t: &Facts| parsed(text).matches(t);
    let who = |text: &str| -> Vec<&str> {
        [&asks, &tagged, &both, &quiet].into_iter().filter(|t| m(text, t)).map(|t| t.id.as_str()).collect()
    };
    assert_eq!(who("asking"), ["MENGER-2", "MENGER-4"]);
    assert_eq!(who("not asking"), ["MENGER-3", "MENGER-5"]);
    assert_eq!(who("label needs-you"), ["MENGER-3", "MENGER-4"]);
    assert_eq!(who("asking or label needs-you"), ["MENGER-2", "MENGER-3", "MENGER-4"]);
    assert_eq!(who("label in (p1, later)"), ["MENGER-3"]);
    assert_eq!(who("not label in (p1, needs-you)"), ["MENGER-2", "MENGER-5"]);
    assert_eq!(who("label needs-you and label p1"), ["MENGER-3"], "two labels of one ticket: both hold");

    // Merges: a ticket holds many labels, so two inclusions never
    // intersect (as `under`'s), and an inclusion is not cut by an exclusion.
    let a = parsed("label in (a, b)");
    assert_eq!(a.and(&parsed("label in (b, c)")).format(), "label in (a, b) and label in (b, c)");
    assert_eq!(a.and(&parsed("not label a")).format(), "label in (a, b) and not label a");
    assert_eq!(parsed("not label a").and(&parsed("not label b")).format(), "not label in (a, b)");

    // Contradictions: the flag against itself; a label against itself; two
    // different labels never clash.
    assert!(parsed("asking and not asking").admits_nothing().is_some());
    assert!(parsed("label a and not label a").admits_nothing().is_some());
    assert!(parsed("label a and not label b").admits_nothing().is_none());
    assert!(parsed("asking and kind doc and status Doc").admits_nothing().is_none(), "a doc may ask");
}

/// `palette_match` from `frontend/src/keys.rs` — find's rule, which the
/// store's `Needle` is held to (SCALE-6) — as the oracle for `text`.
fn palette_match(code: &str, title: &str, query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || palette_contains(code, q) || palette_contains(title, q)
}

fn palette_contains(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// `text` is find's rule, case for case.
#[test]
fn text_is_finds_rule() {
    let keys = include_str!("../../frontend/src/keys.rs");
    assert!(keys.contains("q.is_empty() || contains_ignore_ascii_case(code, q) || contains_ignore_ascii_case(title, q)"));
    let titles = ["Watch the store", "naïve CAFÉ", "", "x", "SCALE-13 again", "watch·dot"];
    let queries = ["watch", "WATCH", " the ", "", "   ", "café", "CAFÉ", "naï", "scale-1", "13", "·", "zzz", "x"];
    for title in titles {
        for q in queries {
            assert_eq!(text_matches(q, "SCALE-13", title), palette_match("SCALE-13", title, q), "{q:?} in {title:?}");
        }
    }
}

// -- `and` --------------------------------------------------------------------------

/// The merge cases: two inclusions, two exclusions, inclusion with
/// exclusion (either way round), an empty merge kept as written, identical
/// groups once, different fields untouched — `under` intersecting nothing.
#[test]
fn merges() {
    let cases = [
        // two inclusions intersect
        ("status in (Pending, Ready, Blocked)", "status in (Ready, Blocked, Coding)", "status in (Ready, Blocked)"),
        ("kind in (build, gap)", "kind gap", "kind gap"),
        ("in (SCALE, MNG)", "in (MNG, FILTER)", "in MNG"),
        ("severity in (critical, none)", "severity in (none, high)", "severity none"),
        // two exclusions unite
        ("not kind in (doc, spike)", "not kind gap", "not kind in (doc, spike, gap)"),
        ("not in SCALE", "not in (MNG, SCALE)", "not in (SCALE, MNG)"),
        ("not under SCALE", "not under MNG-2", "not under in (SCALE, MNG-2)"),
        // an inclusion and an exclusion subtract, either way round
        ("category in (open, ready, active)", "not category open", "category in (ready, active)"),
        ("not category open", "category in (open, ready)", "category ready"),
        ("status in (Pending, Ready)", "not status in (Complete, Superseded)", "status in (Pending, Ready)"),
        // a merge that would leave nothing keeps both as written
        ("status Ready", "status Pending", "status Ready and status Pending"),
        ("in SCALE", "not in SCALE", "in SCALE and not in SCALE"),
        ("kind in (build, gap)", "not kind in (gap, build)", "kind in (build, gap) and not kind in (gap, build)"),
        // identical groups once
        ("ready and kind build", "kind build and blocked", "ready and kind build and blocked"),
        ("(ready or blocked)", "(ready or blocked)", "(ready or blocked)"),
        ("status in (Ready, Pending)", "status in (Pending, Ready)", "status in (Ready, Pending)"),
        // groups of different fields, and groups that list nothing, untouched
        ("status Ready", "category ready", "status Ready and category ready"),
        ("(ready or status Blocked)", "status Blocked", "(ready or status Blocked) and status Blocked"),
        ("(status Ready and kind build)", "status Ready", "(status Ready and kind build) and status Ready"),
        ("text \"a\"", "text \"b\"", "text \"a\" and text \"b\""),
        // FILTER-21: a flag, as `ready` — once when both say it, else kept
        ("not menger doc", "not menger doc and kind build", "not menger doc and kind build"),
        ("not menger doc", "menger doc", "not menger doc and menger doc"),
        // under: a lineage holds many names, so inclusions never intersect
        ("under SCALE", "under SCALE-13", "under SCALE and under SCALE-13"),
        ("under in (SCALE, MNG)", "not under MNG", "under in (SCALE, MNG) and not under MNG"),
        // the empty filter on either side
        ("", "ready", "ready"),
        ("ready", "", "ready"),
        // the ticket's intended usage
        (
            "not category static",
            "status in (Pending, Ready) and in SCALE",
            "not category static and status in (Pending, Ready) and in SCALE",
        ),
        // a three-way merge, and a merged group that meets one kept as written
        ("status in (Pending, Ready, Coding) and in SCALE", "not status Coding and status in (Ready, Coding)", "status Ready and in SCALE"),
        ("status Ready and not status Ready", "status in (Ready, Pending)", "status Ready and not status Ready"),
    ];
    for (a, b, want) in cases {
        let got = parsed(a).and(&parsed(b));
        assert_eq!(got.format(), want, "{a:?} and {b:?}");
        assert_eq!(Filter::parse(&got.format()).as_ref(), Ok(&got), "the merge is canonical");
    }
}

/// The display is the right side's wherever it sets one.
#[test]
fn and_takes_the_displays_right_side_where_it_is_set() {
    let both = |a: &str, b: &str| parsed(a).and(&parsed(b)).format();
    assert_eq!(both("ready · flat", "blocked"), "ready and blocked · flat");
    assert_eq!(both("ready · flat · columns id", "blocked · columns title"), "ready and blocked · flat · columns title");
    assert_eq!(both("ready", "· order code"), "ready · order code");
}

// -- admits_nothing ------------------------------------------------------------------

/// The contradictions: which, and which groups are named.
#[test]
fn admits_nothing_names_the_clashing_groups() {
    let all_statuses = format!("not status in ({})", Status::ALL.map(|s| quoted(s.as_str())).join(", "));
    let cases: Vec<(&str, Option<Vec<usize>>)> = vec![
        ("", None),
        ("ready", None),
        ("ready and kind build and in SCALE and under SCALE-1 and text \"x\"", None),
        ("not in (SCALE, MNG)", None),
        ("in SCALE and under MNG", None),
        ("text \"a\" and text \"b\"", None),
        ("blocked and kind doc", None),
        ("status Ready and status Pending", Some(vec![0, 1])),
        ("kind build and status Ready and has commits and status Pending", Some(vec![1, 3])),
        ("ready and blocked", Some(vec![0, 1])),
        ("ready and status Coding", Some(vec![0, 1])),
        ("kind doc and status Pending", Some(vec![0, 1])),
        ("status Doc and not kind doc", Some(vec![0, 1])),
        ("category static and kind build", Some(vec![0, 1])),
        ("kind doc and ready", Some(vec![0, 1])),
        ("in SCALE and in MNG", Some(vec![0, 1])),
        ("in SCALE and not in SCALE", Some(vec![0, 1])),
        ("in (SCALE, MNG) and not in SCALE and not in MNG", Some(vec![0, 1, 2])),
        ("under SCALE and not under SCALE", Some(vec![0, 1])),
        ("severity none and not severity none", Some(vec![0, 1])),
        ("has commits and not has commits", Some(vec![0, 1])),
        // FILTER-21: only a doc is a menger doc.
        ("menger doc and kind doc and has commits and blocked", None),
        ("not menger doc and kind build", None),
        ("menger doc and not menger doc", Some(vec![0, 1])),
        ("menger doc and kind build", Some(vec![0, 1])),
        ("menger doc and ready", Some(vec![0, 1])),
        ("has commits and menger doc and not category static", Some(vec![1, 2])),
        (&all_statuses, Some(vec![0])),
        ("ready and not severity in (critical, high, medium, low, none)", Some(vec![1])),
        ("(status Ready and status Pending)", Some(vec![0])),
        (
            "kind build and status in (Pending, Ready) and status in (Ready, Coding) and not status Ready",
            Some(vec![0, 1, 2, 3]),
        ),
        ("severity high and (ready or blocked) and not ready and not blocked", Some(vec![1, 2, 3])),
        ("in SCALE and has commits and (ready or blocked) and not ready and not blocked", Some(vec![2, 3, 4])),
    ];
    for (text, want) in cases {
        let f = parsed(text);
        let got = f.admits_nothing();
        assert_eq!(got.as_ref().map(|c| c.at.clone()), want, "{text}: {got:?}");
        if let Some(c) = got {
            assert_eq!(c.groups, c.at.iter().map(|&i| f.groups[i].clone()).collect::<Vec<_>>());
            assert!(!c.to_string().is_empty());
        }
    }
    let empty = Group { conj: Conj::Any, preds: vec![] };
    assert_eq!(groups(vec![empty]).admits_nothing(), None, "an empty group passes");
    // Built, not parsed (`not text` is refused): the evaluator still reads them.
    assert_eq!(groups(vec![one(not(text("  ")))]).admits_nothing().map(|c| c.at), Some(vec![0]));
    let folded = groups(vec![one(is(Field::Ready)), one(is(text("a"))), one(not(text("A ")))]);
    assert_eq!(folded.admits_nothing().map(|c| c.at), Some(vec![1, 2]), "a text is folded and trimmed, as find's");
}

/// The ticket's intended usage: the global and a board's filter meet, and
/// the clash is named in the bar.
#[test]
fn a_merge_left_as_written_is_the_contradiction_named() {
    let effective = parsed("status Ready").and(&parsed("in SCALE and status in (Pending, Coding)"));
    assert_eq!(effective.format(), "status Ready and in SCALE and status in (Pending, Coding)");
    let c = effective.admits_nothing().expect("Ready is neither Pending nor Coding");
    assert_eq!(c.at, vec![0, 2]);
    assert_eq!(c.to_string(), "`status Ready` and `status in (Pending, Coding)` admit no ticket together");
}

// -- properties ---------------------------------------------------------------------

/// SplitMix64 — the store's seed generator's kind of generator, kept here
/// so the properties need no dependency and replay from their seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }

    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())].clone()
    }
}

/// The values the random filters and random facts draw from — few, so
/// groups meet, merge and clash often.
struct Pools {
    epics: &'static [&'static str],
    unders: &'static [&'static str],
    texts: &'static [&'static str],
    /// MENGER-12: the labels a filter names and a ticket holds.
    labels: &'static [&'static str],
}

const WIDE: Pools = Pools {
    epics: &["SCALE", "MNG", "FILTER"],
    unders: &["SCALE", "SCALE-13", "MNG", "MNG-2", "FILTER-9"],
    // quoting's hard cases: spaces, commas, parentheses, `·`, the joiners,
    // a leading `not`, a field's name, padding, a blank
    texts: &["watch", "In Review", "a, b", "x (y)", "not this", "a and b", "c or d", "dot · dot", "  pad  ", "status", " "],
    labels: &["needs-you", "p1", "later"],
};

/// For the exhaustive checks: texts no one of which holds another, none in
/// an id, so a title holds exactly the texts chosen; one blank.
const NARROW: Pools = Pools {
    epics: &["SCALE", "MNG", "FILTER"],
    unders: &["SCALE", "SCALE-13"],
    texts: &["alpha", "ALPHA ", "bravo", "  "],
    labels: &["needs-you", "p1"],
};

fn random_value(rng: &mut Rng, field: usize, pools: &Pools) -> Field {
    match field {
        0 => status(rng.pick(&Status::ALL)),
        1 => category(rng.pick(&Category::ALL)),
        2 => kind(rng.pick(&Kind::ALL)),
        3 => sev(rng.pick(&SEVERITIES)),
        4 => within(rng.pick(pools.epics)),
        5 => under(rng.pick(pools.unders)),
        6 => Field::Blocked,
        7 => Field::Ready,
        8 => Field::HasCommits,
        9 => Field::MengerDoc,
        // MENGER-12.
        11 => Field::Asking,
        12 => Field::Label(rng.pick(pools.labels).to_string()),
        _ => text(rng.pick(pools.texts)),
    }
}

/// Any predicate but `text`.
fn random_pred(rng: &mut Rng, pools: &Pools) -> Predicate {
    let field = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12][rng.below(12)];
    Predicate { field: random_value(rng, field, pools), not: rng.coin() }
}

/// A group of any shape the language has.
fn random_group(rng: &mut Rng, pools: &Pools) -> Group {
    match rng.below(6) {
        0 | 1 => one(random_pred(rng, pools)),
        2 => {
            let (field, incl, n) = ([0, 1, 2, 3, 4, 5, 12][rng.below(7)], rng.coin(), 2 + rng.below(2));
            let preds = (0..n).map(|_| Predicate { field: random_value(rng, field, pools), not: !incl }).collect();
            match incl {
                true => Group::any(preds),
                false => Group::all(preds),
            }
        }
        3 => Group::any((0..2 + rng.below(2)).map(|_| random_pred(rng, pools)).collect()),
        4 => Group::all((0..2 + rng.below(2)).map(|_| random_pred(rng, pools)).collect()),
        _ => one(is(random_value(rng, 10, pools))),
    }
}

fn random_display(rng: &mut Rng) -> Display {
    let columns = ["id", "title", "status", "parent chip"];
    Display {
        shape: if rng.coin() { Shape::Flat } else { Shape::Tree },
        order: rng.coin().then_some(Order::Code),
        columns: (0..rng.below(3)).map(|_| rng.pick(&columns).to_string()).collect(),
    }
}

fn random_filter(rng: &mut Rng, pools: &Pools, display: bool) -> Filter {
    Filter {
        groups: (0..rng.below(5)).map(|_| random_group(rng, pools)).collect(),
        display: if display { random_display(rng) } else { Display::default() },
    }
}

/// A ticket as the store holds one: a doc in `Doc` and only a doc, a menger
/// doc only a doc (FILTER-21), its lineage its own id, its shortcode and
/// some names above it.
fn random_facts(rng: &mut Rng, pools: &Pools) -> Facts {
    let kind = rng.pick(&Kind::ALL);
    let status = match kind {
        Kind::Doc => Status::Doc,
        _ => rng.pick(&Status::LIFECYCLE),
    };
    let epic = if rng.below(4) == 0 { "OTHER".to_string() } else { rng.pick(pools.epics).to_string() };
    let id = format!("{epic}-{}", 1 + rng.below(20));
    let words: Vec<&str> = (0..rng.below(3)).map(|_| rng.pick(pools.texts)).collect();
    let mut lineage = vec![id.clone(), epic.clone()];
    lineage.extend(pools.unders.iter().filter(|_| rng.below(3) == 0).map(|u| u.to_string()));
    Facts {
        asking: rng.coin(),
        labels: pools.labels.iter().filter(|_| rng.coin()).map(|l| l.to_string()).collect(),
        id,
        epic,
        title: words.join(" "),
        status,
        kind,
        severity: rng.pick(&SEVERITIES),
        blocked: kind != Kind::Doc && rng.coin(),
        has_commits: rng.coin(),
        menger_doc: kind == Kind::Doc && rng.coin(),
        lineage,
        history: Vec::new(),
    }
}

/// `parse(format(f)) == f` for random filters of every shape — every
/// field, `or` and `and` groups, `not`, lists, quoting, the display — and
/// so `format` is a fixed point on its own output.
#[test]
fn parse_inverts_format() {
    let mut rng = Rng(0xF117_E210);
    for _ in 0..5000 {
        let f = random_filter(&mut rng, &WIDE, true);
        let text = f.format();
        assert_eq!(Filter::parse(&text).as_ref(), Ok(&f), "{text}");
        assert_eq!(parsed(&text).format(), text);
    }
}

/// `a.and(b)` matches exactly what both match, on random tickets — and is
/// itself canonical.
#[test]
fn and_matches_what_both_match() {
    let mut rng = Rng(0xA4D_0001);
    let mut merged = 0;
    for _ in 0..3000 {
        let (a, b) = (random_filter(&mut rng, &WIDE, false), random_filter(&mut rng, &WIDE, false));
        let both = a.and(&b);
        merged += usize::from(both.groups.len() < a.groups.len() + b.groups.len());
        assert_eq!(Filter::parse(&both.format()).as_ref(), Ok(&both), "{} and {}", a.format(), b.format());
        for _ in 0..100 {
            let x = random_facts(&mut rng, &WIDE);
            assert_eq!(
                both.matches(&x),
                a.matches(&x) && b.matches(&x),
                "{} AND {} = {} on {x:?}",
                a.format(),
                b.format(),
                both.format()
            );
        }
    }
    assert!(merged > 300, "the pools make merges common: {merged}");
}

/// Whether any world of the closed domain — every cell, each shortcode the
/// filter names and one it does not, every set of its `under` names and of
/// its texts — passes `f`, asked of the evaluator itself.
fn some_world_matches(f: &Filter) -> bool {
    let (mut epics, mut unders, mut texts, mut labels) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for p in f.groups.iter().flat_map(|g| &g.preds) {
        match &p.field {
            Field::In(e) if !epics.contains(e) => epics.push(e.clone()),
            // MENGER-12: each label named, held or not.
            Field::Label(l) if !labels.contains(l) => labels.push(l.clone()),
            Field::Under(u) if !unders.contains(u) => unders.push(u.clone()),
            Field::Text(t) if !t.trim().is_empty() && !texts.contains(&t.trim().to_ascii_lowercase()) => {
                texts.push(t.trim().to_ascii_lowercase())
            }
            _ => {}
        }
    }
    epics.push("ELSEWHERE".into());
    // The domain, written out here rather than borrowed from the module: a
    // doc is born in `Doc` and never leaves it, and no other kind holds it;
    // only a doc carries a path, so only a doc is a menger doc (FILTER-21).
    let mut cells = Vec::new();
    for kind in Kind::ALL {
        let statuses: &[Status] = if kind == Kind::Doc { &[Status::Doc] } else { &Status::LIFECYCLE };
        let mengers: &[bool] = if kind == Kind::Doc { &[false, true] } else { &[false] };
        for &status in statuses {
            for severity in [None, Some(Severity::Critical), Some(Severity::High), Some(Severity::Medium), Some(Severity::Low)] {
                for (blocked, commits) in [(false, false), (false, true), (true, false), (true, true)] {
                    for &menger in mengers {
                        // MENGER-12: any ticket may ask.
                        for asking in [false, true] {
                            cells.push(Cell { asking, status, kind, severity, blocked, commits, menger });
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cells.len(), 2080);
    for epic in &epics {
        for u in 0..1usize << unders.len() {
            for t in 0..1usize << texts.len() {
              for l in 0..1usize << labels.len() {
                let chosen = |xs: &[String], mask: usize| -> Vec<String> {
                    xs.iter().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, x)| x.clone()).collect()
                };
                let mut x = Facts {
                    asking: false,
                    labels: chosen(&labels, l),
                    id: "QQ-1".into(),
                    epic: epic.clone(),
                    title: chosen(&texts, t).join("|"),
                    status: Status::Pending,
                    kind: Kind::Build,
                    severity: None,
                    blocked: false,
                    has_commits: false,
                    menger_doc: false,
                    lineage: chosen(&unders, u),
                    history: Vec::new(),
                };
                for c in &cells {
                    (x.status, x.kind, x.severity, x.blocked, x.has_commits, x.menger_doc, x.asking) =
                        (c.status, c.kind, c.severity, c.blocked, c.commits, c.menger, c.asking);
                    if f.matches(&x) {
                        return true;
                    }
                }
              }
            }
        }
    }
    false
}

fn sub(f: &Filter, at: &[usize]) -> Filter {
    groups(at.iter().map(|&i| f.groups[i].clone()).collect())
}

/// Over the closed domain `admits_nothing` is exact — `Some` exactly when
/// no world passes — and what it names admits nothing by itself, with no
/// smaller set of the sizes it tries doing so.
#[test]
fn admits_nothing_is_exact_over_the_domain() {
    let mut rng = Rng(0xC0_47AD);
    let mut found = 0;
    for round in 0..1200 {
        // `and` of two random filters: lists merged, and the clashes a lens
        // meets — about a third of these admit nothing.
        let f = match round % 2 {
            0 => random_filter(&mut rng, &NARROW, false),
            _ => random_filter(&mut rng, &NARROW, false).and(&random_filter(&mut rng, &NARROW, false)),
        };
        let none = !some_world_matches(&f);
        let got = f.admits_nothing();
        assert_eq!(got.is_some(), none, "{}: {got:?}", f.format());
        let Some(c) = got else { continue };
        found += 1;
        assert!(!some_world_matches(&sub(&f, &c.at)), "{} names {:?}", f.format(), c.at);
        let n = f.groups.len();
        if c.at.len() >= 2 {
            assert!((0..n).all(|i| some_world_matches(&sub(&f, &[i]))), "{}: a single group clashes", f.format());
        }
        if c.at.len() >= 3 {
            for i in 0..n {
                for j in i + 1..n {
                    assert!(some_world_matches(&sub(&f, &[i, j])), "{}: {i} and {j} clash", f.format());
                }
            }
        }
    }
    assert!(found > 200, "the rounds hold contradictions: {found}");
}

/// Sound on tickets as the store holds them: a filter that admits nothing
/// matches no random ticket.
#[test]
fn admits_nothing_is_sound_on_random_tickets() {
    let mut rng = Rng(0x50_0D);
    let mut found = 0;
    for _ in 0..3000 {
        let f = random_filter(&mut rng, &WIDE, false).and(&random_filter(&mut rng, &WIDE, false));
        if f.admits_nothing().is_none() {
            continue;
        }
        found += 1;
        for _ in 0..200 {
            let x = random_facts(&mut rng, &WIDE);
            assert!(!f.matches(&x), "{} admits nothing, yet matches {x:?}", f.format());
        }
    }
    assert!(found > 300, "the rounds hold contradictions: {found}");
}

// -- MENGER-13: history ---------------------------------------------------------------

/// The history predicates read and write canonically — the ticket's
/// examples among them — with their windows, `by`, and `between … and …`
/// kept whole by the `and` that joins groups.
#[test]
fn history_predicates_read_and_write_canonically() {
    for (text, canonical) in [
        ("status \"In Review\" and entered \"In Review\" within 12h", "status \"In Review\" and entered \"In Review\" within 12h"),
        ("entered Coding yesterday", "entered Coding yesterday"),
        ("changed within 1h and under SCAFFOLD-1", "changed within 1h and under SCAFFOLD-1"),
        ("entered Complete this week and kind build", "entered Complete this week and kind build"),
        ("entered Canceled within 1d by system", "entered Canceled within 1d by system"),
        ("entered complete since 2026-10-10", "entered Complete since 2026-10-10"),
        ("entered in review within 12 hours", "entered \"In Review\" within 12h"),
        ("left Pending before 2026-10-10 08:30", "left Pending before 2026-10-10T08:30"),
        ("changed between 2026-10-01 and 2026-10-05 and kind gap", "changed between 2026-10-01 and 2026-10-05 and kind gap"),
        ("by me", "changed by me"),
        ("by system today", "changed today by system"),
        ("changed by idp:ada", "changed by idp:ada"),
        ("not entered Ready within 30m", "not entered Ready within 30m"),
        ("(entered Ready within 2w or left Coding within 1d)", "(entered Ready within 2w or left Coding within 1d)"),
        ("changed", "changed"),
    ] {
        let f = parsed(text);
        assert_eq!(f.format(), canonical, "{text}");
        assert_eq!(parsed(canonical), f, "{canonical} reads back");
    }
    let f = parsed("changed between 2026-10-01 and 2026-10-05 and kind gap");
    assert_eq!(f.groups.len(), 2, "the window's `and` is not a group's");
    match &f.groups[0].preds[0].field {
        Field::History(h) => {
            assert_eq!(h.event, Event::Changed);
            assert!(matches!(h.window, Some(Window::Between(When::At(a), When::At(b))) if a.day == 1 && b.day == 5));
            assert!(!h.relative(), "fixed moments do not age");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&parsed("entered Coding within 1h").groups[0].preds[0].field, Field::History(h) if h.relative()));
}

/// What a history predicate cannot be is named, and nothing is parsed.
#[test]
fn malformed_history_predicates_are_refused() {
    for (text, says) in [
        ("entered", "names a status"),
        ("entered Shipped", "unknown status `Shipped`"),
        ("entered Coding within", "takes a length"),
        ("entered Coding within 0h", "a length of 1 or more"),
        ("entered Coding within 3", "needs a unit"),
        ("entered Coding within 3y", "needs a unit"),
        ("changed since 2026-13-01", "is not a date"),
        ("changed since 2026-02-30", "is not a date"),
        ("changed between 2026-10-01", "between <date> and <date>"),
        ("changed this month", "`this` is `this week`"),
        ("changed lately", "is not understood here"),
        ("by", "names who"),
        ("changed today by", "names who"),
    ] {
        let e = refused(text).to_string();
        assert!(e.contains(says), "{text}: {e}");
    }
}

/// A history predicate is a free truth: `admits_nothing` sees a predicate
/// and its negation clash, and nothing else.
#[test]
fn a_history_predicate_is_a_free_truth() {
    assert!(parsed("entered Coding within 1h and not entered Coding within 1h").admits_nothing().is_some());
    assert!(parsed("entered Coding within 1h and not entered Coding within 2h").admits_nothing().is_none());
    assert!(parsed("changed today and status Doc").admits_nothing().is_none());
    let mut x = facts("H-1", "", Status::Coding, Kind::Build);
    let f = parsed("status Coding and entered Coding within 12h");
    assert!(!f.matches(&x));
    x.history = vec!["entered Coding within 12h".into()];
    assert!(f.matches(&x));
}

// -- FILTER-27: when — created, for, ago --------------------------------------------

/// Every form of FILTER-27's language block reads and writes canonically —
/// `created <window>`, `[status S | category C] for over|under D` (the sugar
/// printed as sugar), and `N ago` wherever a moment goes — and the `and` of
/// a `between … ago and … ago` stays the window's.
#[test]
fn when_predicates_read_and_write_canonically() {
    for (text, canonical) in [
        ("created today", "created today"),
        ("created within 3d", "created within 3d"),
        ("created this week", "created this week"),
        ("created before 2w ago", "created before 2w ago"),
        ("created between 2026-10-01 and 2026-10-07", "created between 2026-10-01 and 2026-10-07"),
        ("entered Pending today", "entered Pending today"),
        ("left Pending this week", "left Pending this week"),
        ("entered Ready within 2d by owner", "entered Ready within 2d by owner"),
        ("status Pending for over 2w", "status Pending for over 2w"),
        ("status \"In Review\" for over 3d", "status \"In Review\" for over 3d"),
        ("category active for under 1h", "category active for under 1h"),
        ("for over 2w", "for over 2w"),
        ("changed before 1w ago", "changed before 1w ago"),
        ("changed since 12h ago", "changed since 12h ago"),
        ("changed since 3 days ago", "changed since 3d ago"),
        ("created between 3d ago and 1d ago and kind build", "created between 3d ago and 1d ago and kind build"),
        ("created between 2026-10-01T08:00 and 2w ago", "created between 2026-10-01T08:00 and 2w ago"),
        ("status in (Pending, Ready) for over 1w", "status in (Pending, Ready) for over 1w"),
        ("category (ready, active, review) for over 1w", "category in (ready, active, review) for over 1w"),
        ("category in (ready, active, review) for over 1w", "category in (ready, active, review) for over 1w"),
        ("not status Pending for over 2 weeks", "not status Pending for over 2w"),
        ("for under 1mo and in MENGER", "for under 1mo and in MENGER"),
        ("created within 2 months", "created within 2mo"),
        ("(for over 2w or blocked)", "(for over 2w or blocked)"),
        ("status in review for over 3d", "status \"In Review\" for over 3d"),
    ] {
        let f = parsed(text);
        assert_eq!(f.format(), canonical, "{text}");
        assert_eq!(parsed(canonical), f, "{canonical} reads back");
    }
    let held = |t: &str| match &parsed(t).groups[0].preds[0].field {
        Field::History(History { event: Event::Held(h), .. }) => h.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(held("status Pending for over 2w"), Held { of: HeldOf::Statuses(vec![Status::Pending]), over: true, n: 2, unit: Unit::Week });
    assert_eq!(held("for under 1mo").of, HeldOf::Any);
    assert_eq!(held("category active for under 1h").of, HeldOf::Categories(vec![Category::Active]));
    let f = parsed("created between 3d ago and 1d ago and kind build");
    assert_eq!(f.groups.len(), 2);
    match &f.groups[0].preds[0].field {
        Field::History(h) => {
            assert_eq!(h.window, Some(Window::Between(When::Ago(3, Unit::Day), When::Ago(1, Unit::Day))));
            assert!(h.relative(), "an `ago` moves with the clock");
        }
        other => panic!("{other:?}"),
    }
    assert!(parsed("for over 2w").groups[0].preds[0].field.name() == "for");
    assert!(matches!(&parsed("created since 2026-10-01").groups[0].preds[0].field, Field::History(h) if !h.relative()));
    assert!(matches!(&parsed("status Ready for over 1d").groups[0].preds[0].field, Field::History(h) if h.relative()));
}

/// What a FILTER-27 form cannot be is named, and nothing is parsed.
#[test]
fn malformed_when_predicates_are_refused() {
    for (text, says) in [
        ("created", "takes a window"),
        ("created today by me", "takes no `by`"),
        ("for", "`for over <length>`"),
        ("for 2w", "`for over <length>`"),
        ("for over", "takes a length"),
        ("for over 2", "needs a unit"),
        ("for over 2w today", "takes no window"),
        ("kind build for over 2w", "before `for` stands"),
        ("status Shipped for over 2w", "unknown status"),
        ("category hot for over 2w", "unknown category"),
        ("changed before 2y ago", "is not a date"),
        ("changed since ago", "is not a date"),
        ("written today", "needs an index of its own"),
    ] {
        let e = refused(text).to_string();
        assert!(e.contains(says), "{text}: {e}");
    }
}

/// The sugar is `status S and for over D`: a ticket passes it exactly when
/// it passes both — the store says the `for` truth of a ticket, here its
/// facts' history; and a `for` is a free truth beside its status.
#[test]
fn the_sugar_is_a_status_and_a_length() {
    let f = parsed("status Pending for over 2w");
    let mut x = facts("H-1", "", Status::Pending, Kind::Build);
    assert!(!f.matches(&x));
    x.history = vec!["status Pending for over 2w".into()];
    assert!(f.matches(&x));
    assert!(parsed("status Pending for over 2w and not status Pending for over 2w").admits_nothing().is_some());
    assert!(parsed("for over 2w and for under 2w").admits_nothing().is_none(), "free truths: never a false contradiction");
}

// -- FILTER-30: touched — "hasn't changed" ------------------------------------------

/// `touched` takes every window FILTER-27's clauses take, negated or not,
/// and reads back as written; it is its own event, apart from `changed`.
#[test]
fn touched_reads_and_writes_with_every_window() {
    let windows = [
        "today",
        "yesterday",
        "this week",
        "within 1d",
        "within 3d",
        "within 1w",
        "within 2w",
        "within 1mo",
        "within 12h",
        "within 30m",
        "since 2026-10-01",
        "since 2026-10-01T08:30",
        "since 2w ago",
        "before 2026-10-01",
        "before 3d ago",
        "between 2026-10-01 and 2026-10-07",
        "between 3d ago and 1d ago",
        "between 2026-10-01T08:00 and 2w ago",
    ];
    for w in windows {
        for not in ["", "not "] {
            let text = format!("{not}touched {w}");
            let f = parsed(&text);
            assert_eq!(f.format(), text, "{text}");
            assert_eq!(parsed(&f.format()), f, "{text} reads back");
            let p = &f.groups[0].preds[0];
            assert_eq!(p.not, !not.is_empty(), "{text}");
            match &p.field {
                Field::History(h) => {
                    assert_eq!(h.event, Event::Touched, "{text}");
                    assert_eq!(h.event.name(), "touched");
                    assert!(h.by.is_none());
                }
                other => panic!("{text}: {other:?}"),
            }
        }
    }
    for (text, canonical) in [
        ("touched within 2 weeks", "touched within 2w"),
        ("not touched within 2w and not category done and not kind doc", "not touched within 2w and not category done and not kind doc"),
        ("not touched since 2026-10-01 and under THEME", "not touched since 2026-10-01 and under THEME"),
        ("(touched today or created today)", "(touched today or created today)"),
    ] {
        assert_eq!(parsed(text).format(), canonical, "{text}");
    }
    assert!(matches!(&parsed("touched within 2w").groups[0].preds[0].field, Field::History(h) if h.relative()));
    assert!(matches!(&parsed("touched since 2026-10-01").groups[0].preds[0].field, Field::History(h) if !h.relative()));
}

/// What `touched` cannot be is named.
#[test]
fn malformed_touched_is_refused() {
    for (text, says) in [
        ("touched", "`touched` takes a window"),
        ("touched today by me", "`touched` takes no `by`"),
        ("touched within", "takes a length"),
        ("touched lately", "not understood"),
    ] {
        let e = refused(text).to_string();
        assert!(e.contains(says), "{text}: {e}");
    }
}
