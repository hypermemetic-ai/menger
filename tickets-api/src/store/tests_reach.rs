//! ACCESS-10: every read through a reach — a reader's hidden tickets are
//! neither answered nor named. A test per ACCESS-3 read gap (#1 blockers,
//! #3 tree, #4 components, #6 waves, #8 cursors, #13 ids in visible rows,
//! #15 no unscoped path), and a property: for random trees, grants and
//! reads, nothing answered or named lies outside the reach.

use super::*;
use crate::access::{Access, Held, VerifiedUser};
use crate::address::{OrgId, TicketId};
use crate::members::Role;

/// The reader every test here verifies.
const READER: &str = "idp:c952acd3-60aa-4534-b2e3-83634591fee1";

/// A viewer of `shortcodes` (each with those hosted under it), behind a
/// gate as the activations declare the roles.
pub(super) fn viewer_of(shortcodes: &[&str]) -> VerifiedUser {
    let held = shortcodes.iter().map(|s| (Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(s) }, Role::Viewer)).collect();
    VerifiedUser::for_tests(&Access::for_tests(), READER.parse().unwrap(), held)
}

/// A viewer of the tickets under `id` (it and its descendants).
pub(super) fn viewer_under(id: &str) -> VerifiedUser {
    VerifiedUser::for_tests(&Access::for_tests(), READER.parse().unwrap(), vec![(Held::Under(TicketId::new(id)), Role::Viewer)])
}

/// A reader whose reach no read can narrow to: one ticket alone (ACCESS-21
/// keeps none yet; a ceiling may).
fn single(id: &str) -> VerifiedUser {
    VerifiedUser::for_tests(&Access::for_tests(), READER.parse().unwrap(), vec![(Held::Ticket(TicketId::new(id)), Role::Viewer)])
}

pub(super) fn new(epic: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str], body: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("{epic} {}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(str::to_string),
        path: None,
        body: body.into(),
    }
}

/// SECRET, and OPEN hosted under SECRET-1: OPEN-3 waits on OPEN-2 and on
/// SECRET-2 (whose body is classified, and defines `m-secret`); OPEN-4,
/// under OPEN-3, uses it.
///
/// ```text
/// SECRET-1 ── SECRET-2 (m-secret)
///          └─ OPEN-1 ── OPEN-2
///                   └─ OPEN-3 ⟵ OPEN-2, SECRET-2
///                         └─ OPEN-4 (<m-secret>)
/// ```
pub(super) async fn tracker() -> Store {
    let s = Store::new();
    s.create(new("SECRET", Kind::Scope, None, &[], "")).await.unwrap();
    s.create(new("SECRET", Kind::Build, Some("SECRET-1"), &[], "classified\n\n```component m-secret\n<template>s</template>\n```\n")).await.unwrap();
    s.create(new("OPEN", Kind::Scope, Some("SECRET-1"), &[], "")).await.unwrap();
    s.create(new("OPEN", Kind::Build, Some("OPEN-1"), &[], "")).await.unwrap();
    s.create(new("OPEN", Kind::Build, Some("OPEN-1"), &["OPEN-2", "SECRET-2"], "")).await.unwrap();
    s.create(new("OPEN", Kind::Build, Some("OPEN-3"), &[], "<m-secret></m-secret>")).await.unwrap();
    s
}

fn ids_of<'a>(rows: impl IntoIterator<Item = &'a TicketSummary>) -> Vec<String> {
    rows.into_iter().map(|r| r.id.to_string()).collect()
}

fn blocker_ids(refs: Vec<BlockerRef>) -> Vec<String> {
    refs.into_iter()
        .map(|r| match r {
            BlockerRef::Resolved(t) => t.id.to_string(),
            BlockerRef::Dangling(id) => format!("dangling {id}"),
        })
        .collect()
}

fn tree_ids(page: TreePage) -> Vec<String> {
    page.nodes.into_iter().map(|n| n.summary.id.to_string()).collect()
}

fn tq(parent: Option<&str>, epic: Option<&str>, matches_only: bool) -> TreeQuery {
    TreeQuery { parent: parent.map(str::to_string), epic: epic.map(str::to_string), lens: Lens::all(), matches_only, keep: None }
}

/// ACCESS-3 #1: `blockers` resolves only what is in reach — a hidden
/// blocker is not answered (its body never read), nor named as dangling.
#[tokio::test]
async fn blockers_answer_no_hidden_ticket() {
    let s = tracker().await;
    let v = viewer_of(&["OPEN"]);
    assert_eq!(blocker_ids(s.blockers(&v, "OPEN-3").await.unwrap()), ["OPEN-2"]);
    assert_eq!(blocker_ids(s.blockers(SYS, "OPEN-3").await.unwrap()), ["OPEN-2", "SECRET-2"], "the system sees both");
    // A hidden ticket's own blockers: not found, as a ticket that is not there.
    assert!(matches!(s.blockers(&v, "SECRET-2").await, Err(StoreError::NotFound(_))));
    assert!(matches!(s.blockers(&v, "SECRET-99").await, Err(StoreError::NotFound(_))));
}

/// ACCESS-3 #13: a visible row names no hidden ticket — not in
/// `blocked_by`, `unlocks` or `parent` — and says how many it hides; the
/// system's row, and its JSON, are as they were.
#[tokio::test]
async fn a_visible_row_names_no_hidden_ticket() {
    let s = tracker().await;
    let v = viewer_of(&["OPEN"]);
    let t = s.get(&v, "OPEN-3").await.unwrap();
    assert_eq!((t.blocked_by.clone(), t.hidden_blocked_by), (vec!["OPEN-2".to_string()], 1));
    let root = s.get(&v, "OPEN-1").await.unwrap();
    assert_eq!((root.parent.clone(), root.parent_hidden), (None, true), "a hidden parent is none, said");
    let page = s.page(&v, PageQuery { epic: Some("OPEN"), ..Default::default() }).unwrap();
    let row = page.tickets.iter().find(|r| r.id.as_str() == "OPEN-3").unwrap();
    assert_eq!((row.blocked_by.clone(), row.hidden_blocked_by), (vec!["OPEN-2".to_string()], 1), "and on a page of rows");
    // The system's: every id, nothing counted, nothing new on the wire.
    let all = s.get(SYS, "OPEN-3").await.unwrap();
    assert_eq!((all.blocked_by.len(), all.hidden_blocked_by), (2, 0));
    let json = serde_json::to_value(&all).unwrap();
    assert!(json.get("hidden_blocked_by").is_none() && json.get("hidden_unlocks").is_none() && json.get("parent_hidden").is_none(), "{json}");
    let secret = s.get(SYS, "SECRET-2").await.unwrap();
    assert_eq!(secret.unlocks, ["OPEN-3"]);
    // A group: its host out of reach is no host.
    let groups = s.groups(&v);
    assert_eq!(groups.iter().map(|g| g.shortcode.as_str()).collect::<Vec<_>>(), ["OPEN"], "SECRET is never named");
    assert_eq!((groups[0].root.as_deref(), groups[0].host.as_deref()), (Some("OPEN-1"), None));
}

/// IDENTITY-37, ACCESS-1 decided 7: a ticket out of reach and one that is
/// not there are one answer, through every read of one ticket.
#[tokio::test]
async fn hidden_is_not_found() {
    let s = tracker().await;
    let v = viewer_of(&["OPEN"]);
    for id in ["SECRET-2", "SECRET-99"] {
        assert!(matches!(s.get(&v, id).await, Err(StoreError::NotFound(_))), "{id}");
        assert!(matches!(s.history(&v, id), Err(StoreError::NotFound(_))), "{id}");
        assert!(matches!(s.inputs(&v, Some(id), None, None), Err(StoreError::NotFound(_))), "{id}");
        assert!(matches!(s.waves_to(&v, id, &[Kind::Build]), Err(StoreError::NotFound(_))), "{id}");
        assert!(s.tombstone(&v, id).is_none() && s.queue_history(&v, id).is_empty());
    }
    assert_eq!(s.member(&v, &["OPEN-2".into(), "SECRET-2".into(), "SECRET-99".into()], &Lens::all()), [true, false, false]);
    assert_eq!(s.count(&v), 4);
    assert_eq!(s.count(SYS), 6);
}

/// ACCESS-3 #8: a cursor naming a hidden ticket is a cursor into nothing,
/// exactly as one naming no ticket — every paged read.
#[tokio::test]
async fn a_cursor_naming_a_hidden_ticket_is_a_cursor_into_nothing() {
    let s = tracker().await;
    let v = viewer_of(&["OPEN"]);
    for after in ["SECRET-2", "SECRET-99"] {
        let cursor = |r: Result<Page, StoreError>| matches!(r, Err(StoreError::CursorNotFound(_)));
        assert!(cursor(s.page(&v, PageQuery { after: Some(after), ..Default::default() })), "{after}: page");
        assert!(cursor(s.ready_page(&v, None, Some(after), None)), "{after}: ready");
        assert!(cursor(s.find_page(&v, "", Some(after), None)), "{after}: find");
        assert!(cursor(s.find_filtered(&v, "open", &Lens::all(), Some(after), None)), "{after}: find, filtered");
        let q = Filtered { lens: Lens::all(), epic: None, status: None, parent: None, keep: None };
        assert!(cursor(s.filtered_page(&v, &q, Some(after), None, None)), "{after}: filtered");
        assert!(matches!(s.tree_page(&v, &tq(None, Some("OPEN"), false), Some(after), None), Err(StoreError::CursorNotFound(_))), "{after}: tree");
        let mut walk = s.walk(&v, Select::List { epic: None, status: None, parent: None }, Some(after), None);
        assert!(matches!(walk.next(&s).await, Err(StoreError::CursorNotFound(_))), "{after}: walk");
    }
    // The system's cursor is any ticket.
    assert!(s.page(SYS, PageQuery { after: Some("SECRET-2"), ..Default::default() }).is_ok());
}

/// ACCESS-3 #3 and ACCESS-1 decided 10: `tree` draws no row out of reach —
/// a hidden parent answers as one that is not there, a hidden root's
/// shortcode has the granted tickets at its top, and a hidden ticket's
/// hosted roots are never mapped.
#[tokio::test]
async fn tree_draws_no_row_out_of_reach() {
    let s = tracker().await;
    let v = viewer_under("OPEN-3");
    for matches_only in [false, true] {
        let top = s.tree_page(&v, &tq(None, Some("OPEN"), matches_only), None, None).unwrap();
        let node = &top.nodes[0];
        assert_eq!((node.summary.parent.as_ref(), node.summary.parent_hidden), (None, true));
        assert_eq!(tree_ids(top), ["OPEN-3"], "matches_only {matches_only}: the granted ticket at its shortcode's top");
    }
    assert_eq!(tree_ids(s.tree_page(SYS, &tq(None, Some("OPEN"), false), None, None).unwrap()), ["OPEN-1"], "the system: the root");
    let hidden = s.tree_page(&v, &tq(Some("OPEN-1"), None, false), None, None).unwrap();
    let none = s.tree_page(&v, &tq(Some("OPEN-99"), None, false), None, None).unwrap();
    assert_eq!((hidden.nodes.len(), hidden.remaining, hidden.next), (0, 0, None));
    assert_eq!((none.nodes.len(), none.remaining, none.next), (0, 0, None), "a hidden parent is one that is not there");
    assert_eq!(tree_ids(s.tree_page(&v, &tq(Some("OPEN-3"), None, false), None, None).unwrap()), ["OPEN-4"]);
    // A viewer of OPEN, at SECRET's root: the system is shown OPEN's root
    // there; the viewer, nothing.
    let w = viewer_of(&["OPEN"]);
    let sys = tree_ids(s.tree_page(SYS, &tq(Some("SECRET-1"), None, true), None, None).unwrap());
    assert!(sys.contains(&"OPEN-1".to_string()), "{sys:?}");
    assert!(tree_ids(s.tree_page(&w, &tq(Some("SECRET-1"), None, true), None, None).unwrap()).is_empty());
    assert!(tree_ids(s.tree_page(&w, &tq(None, Some("SECRET"), false), None, None).unwrap()).is_empty());
    // The groups agree: OPEN, its root none, its counts its tickets in reach.
    let g = s.groups(&v);
    assert_eq!(g.iter().map(|g| (g.shortcode.as_str(), g.root.as_deref(), g.total)).collect::<Vec<_>>(), [("OPEN", None, 2)]);
}

/// ACCESS-3 #4: the components are those owned by a ticket in reach.
#[tokio::test]
async fn components_are_those_owned_in_reach() {
    let s = tracker().await;
    let names = |c: Vec<Component>| c.into_iter().map(|c| c.name).collect::<Vec<_>>();
    assert_eq!(names(s.components(SYS)), ["m-secret"]);
    assert!(names(s.components(&viewer_of(&["OPEN"]))).is_empty(), "owned by SECRET-2, out of reach");
    let both = viewer_of(&["SECRET"]);
    let c = s.components(&both);
    assert_eq!(c[0].references, ["OPEN-4"], "OPEN is hosted under SECRET: in reach");
}

/// ACCESS-3 #6: `waves` places only what is in reach, and names no hidden
/// blocker — which still holds its waiter back (its wave is the store's).
#[tokio::test]
async fn waves_follow_no_hidden_blocker_by_name() {
    let s = tracker().await;
    let v = viewer_of(&["OPEN"]);
    let w = s.waves(&v, &["OPEN".to_string()], &waves::DEFAULT_KINDS);
    let three = w.placed.iter().find(|p| p.summary.id.as_str() == "OPEN-3").unwrap();
    assert_eq!(three.waiting_on, ["OPEN-2"]);
    assert_eq!((three.placement, three.summary.hidden_blocked_by), (Placement::Wave { n: 2 }, 1));
    assert!(s.waves(&v, &["SECRET".to_string()], &waves::DEFAULT_KINDS).placed.is_empty(), "a shortcode out of reach: nothing, not an error");
    let to = s.waves_to(&v, "OPEN-3", &[Kind::Build]).unwrap();
    assert!(to.waves.placed.iter().all(|p| p.summary.id.as_str().starts_with("OPEN-")), "{:?}", to.waves.placed);
    let all = s.waves_to(SYS, "OPEN-3", &[Kind::Build]).unwrap();
    assert!(all.waves.placed.iter().any(|p| p.summary.id.as_str() == "SECRET-2"));
}

/// ACCESS-3 #7: a comment out of reach is answered as no comment — its
/// cursor none, its id not found beside the newest the reader sees.
#[tokio::test]
async fn a_comment_out_of_reach_is_no_comment() {
    let s = tracker().await;
    let anchor = |t: &str| crate::comments::Anchor { route: format!("#/ticket/{t}"), path: "p".into(), label: "l".into(), excerpt: "e".into(), ticket: Some(t.into()), version: None };
    let open = s.add_comment(anchor("OPEN-2"), "seen".into(), None).await.unwrap();
    let secret = s.add_comment(anchor("SECRET-2"), "unseen".into(), None).await.unwrap();
    let v = viewer_of(&["OPEN"]);
    let page = s.comments(&v, &Default::default(), None, None).unwrap();
    assert_eq!(page.comments.iter().map(|c| c.id).collect::<Vec<_>>(), [open.id]);
    assert!(matches!(s.comment(&v, secret.id.get()), Err(StoreError::CommentNotFound { newest: Some(n), .. }) if n == open.id.get()));
    assert!(matches!(s.comments(&v, &Default::default(), Some(secret.id.get()), None), Err(StoreError::CommentCursor(_))));
    assert_eq!(s.comments(SYS, &Default::default(), None, None).unwrap().comments.len(), 2);
}

/// ACCESS-3 #15: there is no unscoped path to fall to — a reach no read can
/// narrow to reads nothing, never everything.
#[tokio::test]
async fn a_reach_no_read_narrows_to_reads_nothing() {
    let s = tracker().await;
    let only = single("OPEN-2");
    assert!(s.groups(&only).is_empty() && s.extents(&only).is_empty() && s.components(&only).is_empty());
    assert_eq!(s.count(&only), 0);
    assert!(s.get(&only, "OPEN-2").await.is_err());
    assert!(s.page(&only, PageQuery::default()).is_err());
    assert!(s.find_page(&only, "", None, None).is_err());
    assert!(s.tree_page(&only, &tq(None, Some("OPEN"), false), None, None).is_err());
    assert_eq!(s.member(&only, &["OPEN-2".into()], &Lens::all()), [false]);
    assert!(s.waves(&only, &["OPEN".to_string()], &waves::DEFAULT_KINDS).placed.is_empty());
    let mut walk = s.walk(&only, Select::Ready { epic: None }, None, None);
    assert!(walk.next(&s).await.is_err());
}

/// A small, seeded generator — the property's trees, grants and reads.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// ACCESS-10's property: for random trees (shortcodes hosted under random
/// tickets, builds under random tickets, blockers anywhere), random grants
/// (shortcodes, subtrees) and every read, nothing answered or named lies
/// outside the reach — the capability's own question, ticket by ticket
/// ([`VerifiedUser::permit`]), is the oracle — and everything in reach is
/// answered (the tree from its tops, every page).
#[tokio::test]
async fn nothing_answered_or_named_lies_outside_the_reach() {
    use crate::access::Viewer;
    const CODES: [&str; 4] = ["PA", "PB", "PC", "PD"];
    let mut partial = 0;
    for round in 0..24u64 {
        let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ (round + 1).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let s = Store::new();
        let mut ids: Vec<String> = Vec::new();
        for (i, code) in CODES.iter().enumerate() {
            let host = (i > 0 && r.below(3) > 0).then(|| ids[r.below(ids.len())].clone());
            s.create(new(code, Kind::Scope, host.as_deref(), &[], "")).await.unwrap();
            let first = ids.len();
            ids.push(format!("{code}-1"));
            for _ in 0..3 + r.below(4) {
                let parent = ids[first + r.below(ids.len() - first)].clone();
                let mut blockers: Vec<String> = (0..r.below(3)).map(|_| ids[r.below(ids.len())].clone()).filter(|b| !b.ends_with("-1")).collect();
                blockers.dedup();
                let b: Vec<&str> = blockers.iter().map(String::as_str).collect();
                let t = s.create(new(code, Kind::Build, Some(&parent), &b, "")).await.unwrap();
                ids.push(t.id.to_string());
            }
        }
        let mut held = Vec::new();
        for _ in 0..1 + r.below(2) {
            held.push(match r.below(2) {
                0 => (Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(CODES[r.below(4)]) }, Role::Viewer),
                _ => (Held::Under(TicketId::new(&ids[r.below(ids.len())])), Role::Viewer),
            });
        }
        let v = VerifiedUser::for_tests(&Access::for_tests(), READER.parse().unwrap(), held.clone());
        let visible: HashSet<String> = ids.iter().filter(|id| v.permit::<Viewer>(&TicketId::new(id.as_str()), &s).is_ok()).cloned().collect();
        let why = format!("round {round}, {held:?}");
        partial += usize::from(!visible.is_empty() && visible.len() < ids.len());
        let inside = |id: &str| visible.contains(id);
        let named = |r: &TicketSummary| {
            r.blocked_by.iter().chain(&r.unlocks).all(|b| inside(b)) && r.parent.as_deref().is_none_or(inside) && inside(r.id.as_str())
        };

        // Every page: exactly what is in reach, each row naming only it.
        let page = s.page(&v, PageQuery::default()).unwrap();
        assert_eq!(ids_of(&page.tickets).into_iter().collect::<HashSet<_>>(), visible, "{why}: page");
        assert!(page.tickets.iter().all(named), "{why}: a row names a hidden ticket");
        for row in &page.tickets {
            let all = s.get(SYS, row.id.as_str()).await.unwrap();
            let hidden = all.blocked_by.iter().filter(|b| !inside(b)).count() as u32;
            assert_eq!(row.hidden_blocked_by, hidden, "{why}: {} counts its hidden blockers", row.id);
            assert_eq!(row.parent_hidden, all.parent.as_deref().is_some_and(|p| !inside(p)), "{why}: {}", row.id);
            assert_eq!(row.child_count, all.child_count, "{why}: a reach holds a visible ticket's children");
        }
        let find = s.find_page(&v, "", None, None).unwrap();
        assert_eq!(ids_of(&find.tickets).into_iter().collect::<HashSet<_>>(), visible, "{why}: find");
        assert_eq!(s.count(&v), visible.len(), "{why}: count");
        assert_eq!(s.counts(&v, &Lens::all()).into_iter().map(|(_, n)| n as usize).sum::<usize>(), visible.len(), "{why}: counts");
        assert_eq!(s.member(&v, &ids, &Lens::all()), ids.iter().map(|id| inside(id)).collect::<Vec<_>>(), "{why}: member");

        // One ticket at a time: answered exactly when in reach.
        for id in &ids {
            let got = s.get(&v, id).await;
            assert_eq!(got.is_ok(), inside(id), "{why}: get {id}");
            if let Ok(t) = got {
                assert!(named(&TicketSummary::from(t)), "{why}: get {id} names a hidden ticket");
                assert!(blocker_ids(s.blockers(&v, id).await.unwrap()).iter().all(|b| inside(b)), "{why}: blockers of {id}");
                assert!(s.tree_page(&v, &tq(Some(id), None, false), None, None).unwrap().nodes.iter().all(|n| named(&n.summary)), "{why}: tree at {id}");
                assert!(s.tree_page(&v, &tq(Some(id), None, true), None, None).unwrap().nodes.iter().all(|n| named(&n.summary)), "{why}: matches at {id}");
            }
            assert_eq!(s.history(&v, id).is_ok(), inside(id), "{why}: history {id}");
        }

        // The nav: the groups' tops, and down from them, reach exactly
        // what is in reach, drawing nothing else.
        let groups = s.groups(&v);
        let mut reached: HashSet<String> = HashSet::new();
        for g in &groups {
            assert!(g.root.as_deref().is_none_or(inside) && g.host.as_deref().is_none_or(inside), "{why}: group {g:?}");
            let mut stack: Vec<String> = Vec::new();
            for m in [false, true] {
                for n in s.tree_page(&v, &tq(None, Some(g.shortcode.as_str()), m), None, None).unwrap().nodes {
                    assert!(named(&n.summary), "{why}: top of {}", g.shortcode);
                    if !m {
                        stack.push(n.summary.id.to_string());
                    }
                }
            }
            while let Some(p) = stack.pop() {
                if !reached.insert(p.clone()) {
                    continue;
                }
                for n in s.tree_page(&v, &tq(Some(&p), None, false), None, None).unwrap().nodes {
                    stack.push(n.summary.id.to_string());
                }
            }
        }
        assert_eq!(reached, visible, "{why}: the tree reaches what is in reach");
        assert!(s.extents(&v).iter().all(|(sc, _)| groups.iter().any(|g| g.shortcode == *sc)), "{why}: extents");

        // The boards: placed and waited on, only what is in reach.
        let codes: Vec<String> = CODES.iter().map(|c| c.to_string()).collect();
        let w = s.waves(&v, &codes, &[Kind::Scope, Kind::Build]);
        assert!(w.placed.iter().all(|p| named(&p.summary) && p.waiting_on.iter().all(|b| inside(b))), "{why}: waves");
        assert_eq!(w.placed.len(), visible.len(), "{why}: every ticket in reach placed");
    }
    assert!(partial >= 12, "the rounds hold something back: {partial} of 24");
}
