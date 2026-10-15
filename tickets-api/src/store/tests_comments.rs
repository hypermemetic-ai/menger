//! COMMENT-2: the comments store — a test per row of the ticket's Required
//! behavior that the store answers: `add` keeps and returns (the anchor, the
//! ticket's version, the author's default), each refusal keeps nothing,
//! `resolve` / `reopen` flip and say so once, `list` is newest first and
//! pages with `after` + `limit`, every filter and every conjunction of them
//! held to a reference evaluator — `under` to the filter language's own
//! answer — over random comments on a tree that moves, a comment keeps its
//! anchor whatever its ticket goes through, writes are durable, and a file
//! in form 6 gains form 7 once, in place, and answers as before
//! (IDENTITY-21: form 8 now, the same way).

use super::*;
use crate::comments::{Anchor, Comment, CommentState};
use stored::State;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    }

    fn pick<'a>(&mut self, of: &[&'a str]) -> &'a str {
        of[self.below(of.len())]
    }
}

fn ticket(epic: &str, kind: Kind, parent: Option<&str>) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(Into::into),
        path: None,
        body: format!("the body of a {} in {epic}", kind.as_str()),
    }
}

/// The ticket's example anchor, on `ticket`.
fn anchor(ticket: Option<&str>) -> Anchor {
    Anchor {
        route: format!("#/ticket/{}", ticket.unwrap_or("NONE-1")),
        path: "[data-tag=\"ticket DEMAND-1\"]".into(),
        label: "Parent chip".into(),
        excerpt: "DEMAND-1".into(),
        ticket: ticket.map(Into::into),
        version: None,
    }
}

/// A tree three shortcodes deep: ROOT-1 roots ROOT (ROOT-2, ROOT-3, ROOT-4
/// under ROOT-3); SUB-1 roots SUB and hangs under ROOT-3 (SUB-2 under it);
/// DEEP-1 roots DEEP under SUB-2; SOLO stands apart.
async fn tree(store: &Store) {
    for (epic, kind, parent) in [
        ("ROOT", Kind::Scope, None),
        ("ROOT", Kind::Build, None),
        ("ROOT", Kind::Execution, None),
        ("ROOT", Kind::Build, Some("ROOT-3")),
        ("SUB", Kind::Scope, Some("ROOT-3")),
        ("SUB", Kind::Build, None),
        ("DEEP", Kind::Scope, Some("SUB-2")),
        ("DEEP", Kind::Doc, None),
        ("SOLO", Kind::Scope, None),
        ("SOLO", Kind::Build, None),
    ] {
        store.create(ticket(epic, kind, parent)).await.unwrap();
    }
}

fn ids(page: &CommentPage) -> Vec<u64> {
    page.comments.iter().map(|c| c.id).collect()
}

fn all(store: &Store, q: CommentQuery) -> Vec<u64> {
    ids(&store.comments(&q, None, None).unwrap())
}

/// Row 1 and row 2: `add` keeps the comment and returns it — the next id,
/// `open`, the time, `owner` unless given, its anchor and body; the anchor's
/// ticket kept as the ticket's id with its `updated_unix` as `version` (a
/// `version` sent is replaced; none without a ticket); `watch` hears it.
#[tokio::test]
async fn add_keeps_the_comment_and_returns_it() {
    let store = Store::new();
    tree(&store).await;
    let mut rx = store.subscribe_comments();
    let before = chrono::Utc::now().timestamp();
    let sent = Anchor { ticket: Some(" root-2 ".into()), ..anchor(Some("ROOT-2")) };
    let c = store.add_comment(sent, "show the parent's title too".into(), None).await.unwrap();
    let root2 = store.get("ROOT-2").await.unwrap();
    assert_eq!(
        c,
        Comment {
            id: 1,
            state: CommentState::Open,
            author: "owner".into(),
            at: c.at,
            anchor: Anchor { ticket: Some("ROOT-2".into()), version: Some(root2.updated_unix), ..anchor(Some("ROOT-2")) },
            body: "show the parent's title too".into(),
            principal: None,
            intent: crate::comments::CommentIntent::Comment,
        }
    );
    assert!(c.at >= before && c.at <= chrono::Utc::now().timestamp());
    assert_eq!(rx.try_recv().unwrap(), CommentChange::Added(c.clone()));

    let sent = Anchor { version: Some(42), ..anchor(None) };
    let d = store.add_comment(sent, "on the board".into(), Some(" agent-7 ".into())).await.unwrap();
    assert_eq!((d.id, d.author.as_str(), d.anchor.ticket.as_deref(), d.anchor.version), (2, "agent-7", None, None), "a version sent is replaced");
    assert_eq!(store.comment(2), Some(d.clone()));
    assert_eq!(all(&store, CommentQuery::default()), [2, 1]);
    assert_eq!(rx.try_recv().unwrap(), CommentChange::Added(d));
    assert!(rx.try_recv().is_err());
}

/// Row 6: an empty body, a blank author and a malformed anchor are each an
/// error naming the problem — and keep nothing: no row, no id spent, no
/// event.
#[tokio::test]
async fn every_refusal_names_its_problem_and_keeps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("refusals.db");
    let store = Store::open(&path).unwrap();
    tree(&store).await;
    let mut rx = store.subscribe_comments();
    let refused = [
        (anchor(Some("ROOT-2")), "  \n", None, "the comment's body is empty (nothing was kept)"),
        (anchor(Some("ROOT-2")), "x", Some("   "), "author is blank (leave it out for `owner`) (nothing was kept)"),
        (Anchor { route: String::new(), ..anchor(None) }, "x", None, "malformed anchor: route is missing or empty (nothing was kept)"),
        (Anchor { path: " ".into(), ..anchor(None) }, "x", None, "malformed anchor: path is missing or empty (nothing was kept)"),
        (Anchor { label: String::new(), ..anchor(None) }, "x", None, "malformed anchor: label is missing or empty (nothing was kept)"),
        (
            Anchor { route: "ticket/ROOT-2".into(), ..anchor(None) },
            "x",
            None,
            "malformed anchor: route 'ticket/ROOT-2' is not an app page: a route begins #/ (e.g. #/ticket/DEMAND-2, or #/ for the board) (nothing was kept)",
        ),
        (
            anchor(Some("NOPE-9")),
            "x",
            None,
            "malformed anchor: ticket NOPE-9 names no ticket — leave it out when the element belongs to none (nothing was kept)",
        ),
        (Anchor { ticket: Some(String::new()), ..anchor(None) }, "x", None, "malformed anchor: ticket is empty (leave it out when the element belongs to no ticket) (nothing was kept)"),
    ];
    for (a, body, author, message) in refused {
        let e = store.add_comment(a, body.into(), author.map(Into::into)).await.unwrap_err();
        assert_eq!(e.to_string(), message);
    }
    assert!(rx.try_recv().is_err(), "nothing said");
    assert_eq!(all(&store, CommentQuery::default()), Vec::<u64>::new());
    let e = store.resolve_comment(1).await.unwrap_err();
    assert_eq!(e.to_string(), "no comment 1: there are no comments yet (nothing changed)");
    let c = store.add_comment(anchor(None), "the first kept".into(), None).await.unwrap();
    assert_eq!(c.id, 1, "no id was spent on a refusal");
    let e = store.reopen_comment(9).await.unwrap_err();
    assert_eq!(e.to_string(), "no comment 9: comment ids run from 1 to 1 (nothing changed)");
    drop(store);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(comments::count(&conn).unwrap(), 1, "one row: the comment kept");
}

/// Row 5: `resolve` and `reopen` flip the state and return the comment;
/// one already there is returned as it is, written and said nothing.
/// Durable: a store opened on the file reads every comment as it was left.
#[tokio::test]
async fn resolve_and_reopen_flip_the_state_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("states.db");
    let store = Store::open(&path).unwrap();
    tree(&store).await;
    let a = store.add_comment(anchor(Some("SUB-2")), "one".into(), None).await.unwrap();
    let b = store.add_comment(anchor(None), "two".into(), Some("agent".into())).await.unwrap();
    let mut rx = store.subscribe_comments();
    let committed = |store: &Store| store.committing.load(Ordering::SeqCst);
    let resolved = store.resolve_comment(a.id).await.unwrap();
    assert_eq!(resolved, Comment { state: CommentState::Resolved, ..a.clone() });
    assert_eq!(rx.try_recv().unwrap(), CommentChange::Resolved(resolved.clone()));
    let writes = committed(&store);
    assert_eq!(store.resolve_comment(a.id).await.unwrap(), resolved, "resolved already");
    assert_eq!(store.reopen_comment(b.id).await.unwrap(), b, "open already");
    assert_eq!(committed(&store), writes, "nothing written");
    assert!(rx.try_recv().is_err(), "nothing said");
    let reopened = store.reopen_comment(a.id).await.unwrap();
    assert_eq!(reopened, a);
    assert_eq!(rx.try_recv().unwrap(), CommentChange::Reopened(a.clone()));
    store.resolve_comment(b.id).await.unwrap();
    let held = store.comments(&CommentQuery::default(), None, None).unwrap();
    drop(store);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.comments(&CommentQuery::default(), None, None).unwrap(), held, "as stored");
    assert_eq!(store.comment(b.id).unwrap().state, CommentState::Resolved);
    assert_eq!(store.add_comment(anchor(None), "three".into(), None).await.unwrap().id, 3, "ids go on from the file's");
}

/// Row 3: newest first, paged with `after` (exclusive) and `limit`; `next`
/// is the page's last id while more match, else none; an `after` naming no
/// comment is refused; `limit: 0` is the cursor it was cut with.
#[tokio::test]
async fn list_is_newest_first_and_pages_with_after_and_limit() {
    let store = Store::new();
    tree(&store).await;
    for i in 1..=7 {
        let state_resolved = i % 3 == 0;
        let c = store.add_comment(anchor(Some("ROOT-2")), format!("comment {i}"), None).await.unwrap();
        if state_resolved {
            store.resolve_comment(c.id).await.unwrap();
        }
    }
    let q = CommentQuery::default();
    let page = |after: Option<u64>, limit: Option<u32>| store.comments(&q, after, limit).unwrap();
    assert_eq!((ids(&page(None, None)), page(None, None).next), (vec![7, 6, 5, 4, 3, 2, 1], None));
    assert_eq!((ids(&page(None, Some(3))), page(None, Some(3)).next), (vec![7, 6, 5], Some(5)));
    assert_eq!((ids(&page(Some(5), Some(3))), page(Some(5), Some(3)).next), (vec![4, 3, 2], Some(2)));
    assert_eq!((ids(&page(Some(2), Some(3))), page(Some(2), Some(3)).next), (vec![1], None));
    assert_eq!((ids(&page(Some(4), Some(3))), page(Some(4), Some(3)).next), (vec![3, 2, 1], None), "exactly the rest: no next");
    assert_eq!((ids(&page(Some(1), None)), page(Some(1), None).next), (vec![], None));
    assert_eq!((ids(&page(Some(5), Some(0))), page(Some(5), Some(0)).next), (vec![], Some(5)), "limit 0: the cursor");
    let open = CommentQuery { state: Some(CommentState::Open), ..CommentQuery::default() };
    let p = store.comments(&open, None, Some(2)).unwrap();
    assert_eq!((ids(&p), p.next), (vec![7, 5], Some(5)));
    let p = store.comments(&open, Some(5), Some(2)).unwrap();
    assert_eq!((ids(&p), p.next), (vec![4, 2], Some(2)));
    let p = store.comments(&open, Some(2), Some(2)).unwrap();
    assert_eq!((ids(&p), p.next), (vec![1], None));
    let e = store.comments(&q, Some(8), Some(2)).unwrap_err();
    assert_eq!(e.to_string(), "after: no comment 8 to resume after");
}

/// Row 4, by name: each filter alone, and a blank one as none.
#[tokio::test]
async fn each_filter_admits_what_it_names() {
    let store = Store::new();
    tree(&store).await;
    let add = |ticket: Option<&'static str>, route: &'static str, path: &'static str, label: &'static str, excerpt: &'static str, body: &'static str, author: Option<&'static str>| {
        let store = &store;
        async move {
            let a = Anchor { route: route.into(), path: path.into(), label: label.into(), excerpt: excerpt.into(), ticket: ticket.map(Into::into), version: None };
            store.add_comment(a, body.into(), author.map(Into::into)).await.unwrap().id
        }
    };
    let c1 = add(Some("ROOT-4"), "#/ticket/ROOT-4", "main > h1", "Title", "a build", "rename the title", None).await;
    let c2 = add(Some("SUB-2"), "#/ticket/SUB-2", "main > h1 > span", "Status pill", "Pending", "should be Ready", Some("agent-1")).await;
    let c3 = add(Some("DEEP-2"), "#/ticket/DEEP-2", "[data-tag=\"m-wave\"]", "Wave summary", "WAVE 2", "numbers look off", None).await;
    let c4 = add(Some("SOLO-2"), "#/ticket/SOLO-2", "main > h1", "Title", "a TITLE here", "fine", Some("agent-1")).await;
    let c5 = add(None, "#/", "nav > ul", "Nav", "", "the nav is slow", None).await;
    store.resolve_comment(c2).await.unwrap();
    let q = |f: fn(&mut CommentQuery)| {
        let mut q = CommentQuery::default();
        f(&mut q);
        all(&store, q)
    };
    assert_eq!(q(|_| {}), [c5, c4, c3, c2, c1]);
    assert_eq!(q(|q| q.ticket = Some("sub-2".into())), [c2], "a ticket whatever its case");
    assert_eq!(q(|q| q.ticket = Some("ROOT-3".into())), Vec::<u64>::new(), "the ticket itself, not its subtree");
    assert_eq!(q(|q| q.under = Some("ROOT-3".into())), [c3, c2, c1], "ROOT-3's subtree: ROOT-4, and SUB and DEEP hosted under it");
    assert_eq!(q(|q| q.under = Some("ROOT".into())), [c3, c2, c1], "a shortcode, and those it hosts");
    assert_eq!(q(|q| q.under = Some("sub".into())), [c3, c2]);
    assert_eq!(q(|q| q.under = Some("SUB-2".into())), [c3, c2], "itself and DEEP, hosted under it");
    assert_eq!(q(|q| q.under = Some("DEEP-2".into())), [c3]);
    assert_eq!(q(|q| q.under = Some("ROOT-2".into())), Vec::<u64>::new());
    assert_eq!(q(|q| q.under = Some("NOWHERE".into())), Vec::<u64>::new(), "under nothing: nothing");
    assert_eq!(q(|q| q.route = Some("#/ticket/SOLO-2".into())), [c4]);
    assert_eq!(q(|q| q.route = Some("#/ticket".into())), Vec::<u64>::new(), "a route exactly");
    assert_eq!(q(|q| q.path = Some("main > h1".into())), [c4, c2, c1], "a path prefix");
    assert_eq!(q(|q| q.path = Some("main > h1 > span".into())), [c2]);
    assert_eq!(q(|q| q.state = Some(CommentState::Resolved)), [c2]);
    assert_eq!(q(|q| q.state = Some(CommentState::Open)), [c5, c4, c3, c1]);
    assert_eq!(q(|q| q.author = Some("agent-1".into())), [c4, c2]);
    assert_eq!(q(|q| q.author = Some("owner".into())), [c5, c3, c1]);
    assert_eq!(q(|q| q.text = Some("TITLE".into())), [c4, c1], "the body (c1) or the label (c1, c4) or the excerpt (c4)");
    assert_eq!(q(|q| q.text = Some("wave".into())), [c3], "the label and the excerpt");
    assert_eq!(q(|q| q.text = Some("ready".into())), [c2], "the body");
    // Every filter at once, as the ticket's last example asks.
    assert_eq!(
        q(|q| {
            q.under = Some("ROOT".into());
            q.state = Some(CommentState::Open);
            q.text = Some("title".into());
        }),
        [c1]
    );
    // Blank is none.
    assert_eq!(
        q(|q| {
            q.ticket = Some(" ".into());
            q.under = Some(String::new());
            q.route = Some("".into());
            q.path = Some("  ".into());
            q.author = Some(" ".into());
            q.text = Some("   ".into());
        }),
        [c5, c4, c3, c2, c1]
    );
}

/// What a comment is, as the reference evaluator reads it.
#[derive(Clone, Debug)]
struct Want {
    ticket: Option<String>,
    under: Option<String>,
    route: Option<String>,
    path: Option<String>,
    state: Option<CommentState>,
    author: Option<String>,
    text: Option<String>,
}

/// The reference: every filter given holds, by testing the comment — and
/// `under` by asking the filter language itself whether it admits the
/// comment's ticket (`under X`, [`Store::member`]).
fn reference(store: &Store, comments: &[Comment], w: &Want) -> Vec<u64> {
    let blank = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let (ticket, under, route, path, author, text) = (blank(&w.ticket), blank(&w.under), blank(&w.route), blank(&w.path), blank(&w.author), blank(&w.text));
    let lens = under.as_ref().map(|u| Lens::parse(&format!("under {u}")).unwrap());
    let mut out: Vec<u64> = comments
        .iter()
        .filter(|c| w.state.is_none_or(|s| c.state == s))
        .filter(|c| ticket.as_ref().is_none_or(|t| c.anchor.ticket.as_ref().is_some_and(|ct| ct.eq_ignore_ascii_case(t))))
        .filter(|c| route.as_ref().is_none_or(|r| &c.anchor.route == r))
        .filter(|c| path.as_ref().is_none_or(|p| c.anchor.path.starts_with(p.as_str())))
        .filter(|c| author.as_ref().is_none_or(|a| &c.author == a))
        .filter(|c| {
            text.as_ref().is_none_or(|t| {
                let t = t.to_ascii_lowercase();
                [&c.body, &c.anchor.label, &c.anchor.excerpt].iter().any(|h| h.to_ascii_lowercase().contains(&t))
            })
        })
        .filter(|c| {
            lens.as_ref().is_none_or(|lens| c.anchor.ticket.as_ref().is_some_and(|t| store.member(std::slice::from_ref(t), lens)[0]))
        })
        .map(|c| c.id)
        .collect();
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

/// Row 4, at random: comments on a tree that moves under them, and queries
/// that draw every filter — alone, together, blank, in other cases, naming
/// nothing — each answer held to the reference, and every walk of its pages
/// (a random `limit`) to the whole answer, `next` included.
#[tokio::test]
async fn every_filter_and_every_conjunction_is_the_reference() {
    let store = Store::new();
    tree(&store).await;
    let mut rng = Rng(0xC0_4D_E2);
    let tickets = ["ROOT-1", "ROOT-2", "ROOT-3", "ROOT-4", "SUB-1", "SUB-2", "DEEP-1", "DEEP-2", "SOLO-1", "SOLO-2"];
    let names = [
        "ROOT", "SUB", "DEEP", "SOLO", "root", "Sub-2", "ROOT-1", "ROOT-2", "ROOT-3", "ROOT-4", "SUB-1", "SUB-2", "DEEP-1", "DEEP-2", "SOLO-1",
        "SOLO-2", "NOPE", "NOPE-1",
    ];
    let routes = ["#/", "#/ticket/ROOT-2", "#/waves/ROOT", "#/inputs"];
    let paths = ["main > h1", "main > h1 > span", "[data-tag=\"m-wave\"]", "nav > ul > li"];
    let labels = ["Title", "Parent chip", "Status pill", "Wave summary"];
    let excerpts = ["", "DEMAND-1", "the Title", "WAVE 2"];
    let words = ["title", "Title", "slow", "parent", "wave", "ready", "off"];
    let authors = [None, Some("agent-1"), Some("Owner")];
    let mut held: Vec<Comment> = Vec::new();
    // How many `under` answers held some comments, and how many none.
    let (mut under_some, mut under_none) = (0, 0);
    // Moves that keep the rules (a parent in the shortcode; a root hosted
    // anywhere but under itself): the lineage `under` reads changes.
    let moves = [("ROOT-4", "ROOT-2"), ("ROOT-4", "ROOT-3"), ("SUB-1", "ROOT-2"), ("SUB-1", "ROOT-4"), ("DEEP-1", "SUB-1"), ("DEEP-1", "SUB-2"), ("SOLO-1", "ROOT-2")];
    for step in 0..400 {
        match rng.below(10) {
            0..=5 => {
                let t = (rng.below(5) > 0).then(|| rng.pick(&tickets));
                let a = Anchor {
                    route: rng.pick(&routes).into(),
                    path: rng.pick(&paths).into(),
                    label: rng.pick(&labels).into(),
                    excerpt: rng.pick(&excerpts).into(),
                    ticket: t.map(|t| if rng.below(4) == 0 { t.to_ascii_lowercase() } else { t.to_string() }),
                    version: None,
                };
                let body = format!("{} {}", rng.pick(&words), rng.pick(&words));
                let c = store.add_comment(a, body, authors[rng.below(authors.len())].map(Into::into)).await.unwrap();
                held.push(c);
            }
            6 if !held.is_empty() => {
                let id = held[rng.below(held.len())].id;
                let c = if rng.below(2) == 0 { store.resolve_comment(id).await } else { store.reopen_comment(id).await }.unwrap();
                held[id as usize - 1] = c;
            }
            7 => {
                let (child, parent) = moves[rng.below(moves.len())];
                store.set_parent(child, parent).await.unwrap();
            }
            _ => {}
        }
        if step % 4 != 0 {
            continue;
        }
        for _ in 0..6 {
            let maybe = |rng: &mut Rng, of: &[&str]| -> Option<String> {
                match rng.below(8) {
                    0 | 1 => Some(rng.pick(of).to_string()),
                    2 => Some("  ".into()),
                    _ => None,
                }
            };
            let w = Want {
                ticket: maybe(&mut rng, &tickets),
                under: maybe(&mut rng, &names),
                route: maybe(&mut rng, &routes),
                path: maybe(&mut rng, &["main", "main > h1", "[data-tag", "nav > ul > li", "x"]),
                state: [None, None, Some(CommentState::Open), Some(CommentState::Resolved)][rng.below(4)],
                author: maybe(&mut rng, &["agent-1", "Owner", "owner"]),
                text: maybe(&mut rng, &["TITLE", "wave", "demand", "SLOW", "zzz"]),
            };
            let q = CommentQuery {
                ticket: w.ticket.clone(),
                under: w.under.clone(),
                route: w.route.clone(),
                path: w.path.clone(),
                state: w.state,
                author: w.author.clone(),
                text: w.text.clone(),
                intent: None,
            };
            let want = reference(&store, &held, &w);
            assert_eq!(all(&store, q.clone()), want, "step {step}: {w:?}");
            if w.under.as_deref().is_some_and(|u| !u.trim().is_empty()) {
                if want.is_empty() {
                    under_none += 1;
                } else {
                    under_some += 1;
                }
            }
            let limit = 1 + rng.below(4) as u32;
            let (mut walked, mut after) = (Vec::new(), None);
            loop {
                let page = store.comments(&q, after, Some(limit)).unwrap();
                assert!(page.comments.len() <= limit as usize);
                walked.extend(ids(&page));
                match page.next {
                    Some(n) => {
                        assert_eq!(Some(n), walked.last().copied(), "next is the page's last id");
                        after = Some(n);
                    }
                    None => break,
                }
            }
            assert_eq!(walked, want, "step {step}: walked by {limit}: {w:?}");
        }
    }
    assert!(held.len() > 150, "the walk left comments: {}", held.len());
    assert!(under_some > 20 && under_none > 20, "`under` both admitted and refused: {under_some} / {under_none}");
}

/// Row 9: the ticket a comment names is amended (title and body), moved,
/// set to another status and kind, given commits — and every comment stays
/// exactly as it was left, `version` included; `under` follows the ticket
/// to where it is now.
#[tokio::test]
async fn a_comment_keeps_its_anchor_whatever_its_ticket_goes_through() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anchors.db");
    let store = Store::open(&path).unwrap();
    tree(&store).await;
    let c = store.add_comment(anchor(Some("ROOT-4")), "keep me".into(), None).await.unwrap();
    let born = store.get("ROOT-4").await.unwrap().updated_unix;
    assert_eq!(c.anchor.version, Some(born));
    assert_eq!(all(&store, CommentQuery { under: Some("ROOT-3".into()), ..CommentQuery::default() }), [c.id]);

    // The clock moves on, so every write below moves `updated_unix`.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    store.amend("ROOT-4", Some("a new title".into()), Some("a new body, the element gone".into())).await.unwrap();
    store.set_parent("ROOT-4", "ROOT-2").await.unwrap();
    store.set_status("ROOT-4", Status::Ready).await.unwrap();
    store.set_kind("ROOT-4", Kind::Gap, None).await.unwrap();
    store.link_commits("ROOT-4", vec!["77df9a0".into()]).await.unwrap();
    let now = store.get("ROOT-4").await.unwrap();
    assert!(now.updated_unix > born, "the ticket moved on");
    assert_eq!(store.comment(c.id), Some(c.clone()), "the comment did not");
    assert_eq!(all(&store, CommentQuery { under: Some("ROOT-3".into()), ..CommentQuery::default() }), Vec::<u64>::new(), "no longer under ROOT-3");
    assert_eq!(all(&store, CommentQuery { under: Some("ROOT-2".into()), ..CommentQuery::default() }), [c.id], "under where it is now");
    assert_eq!(all(&store, CommentQuery { ticket: Some("ROOT-4".into()), ..CommentQuery::default() }), [c.id]);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.comment(c.id), Some(c), "and as stored");
}

/// Where a file's stored derived state stands, read without opening a store.
fn read_state(path: &Path) -> State {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    stored::state(&conn).unwrap()
}

fn has_comments_table(path: &Path) -> bool {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'comments'", [], |r| r.get::<_, i64>(0))
        .unwrap()
        == 1
}

/// What the store answers, everything a reader asks of the tickets.
async fn answers(store: &Store) -> (Vec<Ticket>, Vec<Group>, Vec<(String, u32)>, Page) {
    let q = Filtered { lens: Lens::parse("not kind doc").unwrap(), epic: None, status: None, parent: None, keep: None };
    (store.list(None, None, None).await.unwrap(), store.groups(), store.extents_text(), store.filtered_page(&q, None, None, Some(50)).unwrap())
}

/// Row 8 (the upgrade), in process: a file as a form-6 build leaves it — no
/// comments table, the derived state saying form 6. `--verify-derived` says
/// it is not this build's form and what the open will do; the first open
/// gains the table, empty, and answers as before; the next open reads
/// (nothing to upgrade), and a comment added between them is kept. A
/// form-7 file an older build stored form 6 into again keeps its comments.
/// MENGER-12: form 10 moves the cells, so a form-6 file is built again
/// rather than upgraded in place — one stored number altered by hand is
/// answered as built, not as stored.
#[tokio::test]
async fn a_form_6_file_gains_form_7_once_in_place_and_answers_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form6.db");
    let before = {
        let store = Store::open(&path).unwrap();
        tree(&store).await;
        store.set_status("ROOT-2", Status::Complete).await.unwrap();
        store.link_commits("SOLO-2", vec!["77df9a0".into()]).await.unwrap();
        answers(&store).await
    };
    assert_eq!(read_state(&path), State::Current);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE comments; UPDATE derived_state SET form = 6;").unwrap();
    drop(conn);
    assert_eq!(read_state(&path), State::Older(6));
    assert!(!has_comments_table(&path));
    match Store::verify_derived(&path).unwrap() {
        Verdict::NotStored(why) => assert!(why.contains("stored in form 6; this build reads 11") && why.contains("builds it again"), "{why}"),
        other => panic!("a form-6 file is not this build's: {other:?}"),
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(read_state(&path), State::Current, "form 10, committed by the first open");
    assert!(has_comments_table(&path));
    assert_eq!(all(&store, CommentQuery::default()), Vec::<u64>::new(), "the table starts empty");
    assert_eq!(answers(&store).await, before, "answers as before");
    let c = store.add_comment(anchor(Some("ROOT-4")), "after the upgrade".into(), None).await.unwrap();
    drop(store);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { tickets: 10, .. }));

    // MENGER-12: a form-6 file is built again — a number stored by hand in
    // it is answered as built.
    let conn = Connection::open(&path).unwrap();
    assert!(!stored::upgrade(&mut Connection::open(&path).unwrap(), 11).unwrap(), "form 11: nothing to upgrade");
    assert!(!stored::upgrade(&mut Connection::open(&path).unwrap(), 6).unwrap(), "form 6: built again, not upgraded");
    conn.execute_batch("UPDATE rollup_groups SET total = total + 100 WHERE epic = 'SOLO'; UPDATE derived_state SET form = 6;").unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    let solo = |g: &[Group]| g.iter().find(|g| g.epic == "SOLO").unwrap().total;
    assert_eq!(solo(&store.groups()), solo(&before.1), "built again, not read as stored");
    assert_eq!(store.comment(c.id), Some(c.clone()), "a form-7 file stored as form 6 again keeps its comments");
    assert_eq!(all(&store, CommentQuery::default()), [c.id]);
    drop(store);
    assert_eq!(read_state(&path), State::Current);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(conn.query_row("SELECT form FROM derived_state", [], |r| r.get::<_, i64>(0)).unwrap(), 11);
    drop(conn);
    assert!(matches!(Store::verify_derived(&path).unwrap(), Verdict::Consistent { .. }));

    // Form 5 and older are built again, and gain the table with the rest.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE comments; UPDATE derived_state SET form = 5;").unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_eq!(answers(&store).await, before);
    assert_eq!(all(&store, CommentQuery::default()), Vec::<u64>::new());
    drop(store);
    assert!(has_comments_table(&path) && read_state(&path) == State::Current);
}

/// IDENTITY-21: a form-7 file — comments, no members, no signers — gains
/// form 8 in place on its first open, keeps its comments, and builds
/// nothing again.
#[tokio::test]
async fn a_form_7_file_gains_form_8_in_place_and_keeps_its_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form7.db");
    let c = {
        let store = Store::open(&path).unwrap();
        tree(&store).await;
        store.add_comment(anchor(Some("ROOT-4")), "before".into(), None).await.unwrap()
    };
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TABLE members; ALTER TABLE comments DROP COLUMN principal; UPDATE derived_state SET form = 7;",
    )
    .unwrap();
    drop(conn);
    assert_eq!(read_state(&path), State::Older(7));
    let store = Store::open(&path).unwrap();
    assert_eq!(read_state(&path), State::Current);
    assert_eq!(store.comment(c.id), Some(c), "the comment kept, unsigned");
    let signed = store
        .add_comment_as(anchor(Some("ROOT-4")), "after".into(), Some("alice".into()), Some("idp:alice".into()))
        .await
        .unwrap();
    assert_eq!(signed.principal.as_deref(), Some("idp:alice"));
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.comment(signed.id).unwrap().principal.as_deref(), Some("idp:alice"), "the signer is stored");
}
