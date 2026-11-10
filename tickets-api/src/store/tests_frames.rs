//! COMP-16: what a body's frame reads — the reader's reach ∩ its body's
//! author's ∩ what it asks (`reach-read:`). The ticket's table, as cases:
//! the bound never wider than the reader, never wider than the author; an
//! amend by another author moves it; nobody known, the ticket alone.
//!
//! ```text
//! PLEXUS-1 ── DESIGN-1 ── DESIGN-2 (the body)
//!          └─ IDENTITY-1 ── IDENTITY-2
//! SECRET-1 ── SECRET-2
//! ```

use super::tests_reach::new;
use super::*;
use crate::access::{Access, Held, VerifiedUser};
use crate::address::OrgId;
use crate::members::Role;

const ADMIN: &str = "idp:a0000000-0000-4000-8000-00000000ad01";
const DESIGNER: &str = "idp:a0000000-0000-4000-8000-00000000de51";
const IDENTITY: &str = "idp:a0000000-0000-4000-8000-0000000001d0";
const NOBODY: &str = "idp:a0000000-0000-4000-8000-000000000000";
const READER: &str = "idp:a0000000-0000-4000-8000-0000000000ee";

fn shortcode(s: &str) -> Held {
    Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(s) }
}

/// `who` holding `grants`.
fn user(who: &str, grants: Vec<(Held, Role)>) -> VerifiedUser {
    VerifiedUser::for_tests(&Access::for_tests(), who.parse().unwrap(), grants)
}

/// Each principal's grants, as the book would hold them: the admin on `*`,
/// the designer an editor of DESIGN alone, IDENTITY's an editor of IDENTITY
/// alone, and NOBODY nothing.
fn grants_of(who: &str) -> Vec<(Held, Role)> {
    match who {
        ADMIN => vec![(Held::Everywhere, Role::Admin)],
        DESIGNER => vec![(shortcode("DESIGN"), Role::Editor)],
        IDENTITY => vec![(shortcode("IDENTITY"), Role::Editor)],
        _ => vec![],
    }
}

fn author(who: &str) -> VerifiedUser {
    user(who, grants_of(who))
}

fn admin() -> VerifiedUser {
    author(ADMIN)
}

fn act(who: &str) -> Act {
    Act { actor: who.into(), ..Act::default() }
}

/// The tree above, every body but DESIGN-2's written by the store; DESIGN-2
/// written by `by`, with `body`.
async fn tracker(by: Option<&str>, body: &str) -> Store {
    let s = Store::new();
    s.create(SYS, new("PLEXUS", Kind::Scope, None, &[], "")).await.unwrap();
    s.create(SYS, new("DESIGN", Kind::Scope, Some("PLEXUS-1"), &[], "")).await.unwrap();
    s.create(SYS, new("IDENTITY", Kind::Scope, Some("PLEXUS-1"), &[], "")).await.unwrap();
    s.create(SYS, new("IDENTITY", Kind::Build, Some("IDENTITY-1"), &[], "")).await.unwrap();
    s.create(SYS, new("SECRET", Kind::Scope, None, &[], "")).await.unwrap();
    s.create(SYS, new("SECRET", Kind::Build, Some("SECRET-1"), &[], "classified")).await.unwrap();
    // A birth's act is its own (`create_by`), as the activation hands it the caller's.
    let by = by.map_or_else(Act::default, act);
    s.create_by(SYS, new("DESIGN", Kind::Build, Some("DESIGN-1"), &[], body), by).await.unwrap();
    s
}

/// The frame of DESIGN-2 as `reader` draws it.
async fn frame(s: &Store, reader: &VerifiedUser) -> (FrameOf, Framed) {
    s.frame(reader, "DESIGN-2", author).await.unwrap()
}

/// What `framed` reads, as `shortcode:count` in the store's order.
fn extent(s: &Store, framed: &Framed) -> Vec<String> {
    s.extent(framed).unwrap().into_iter().map(|(sc, n)| format!("{}:{n}", sc.as_str())).collect()
}

const WIDE: &str = "---\nreach-read: under PLEXUS-1   # FILTER-3's language\n---\nthe body\n";

/// An admin opening a DESIGN-only editor's body that asks `under PLEXUS-1`
/// reads nothing outside DESIGN: the author bounds it.
#[tokio::test]
async fn an_editor_of_one_shortcode_bounds_an_admin_s_frame_to_it() {
    let s = tracker(Some(DESIGNER), WIDE).await;
    let (f, framed) = frame(&s, &admin()).await;
    assert_eq!(f.author.as_deref(), Some(DESIGNER));
    assert_eq!(f.asked_text(), "under PLEXUS-1");
    assert_eq!(f.alone(), None);
    assert_eq!(extent(&s, &framed), ["DESIGN:2"]);
    assert!(matches!(s.get(&framed, "IDENTITY-2").await, Err(StoreError::NotFound(_))), "outside the author's reach");
    assert!(matches!(s.get(&framed, "SECRET-2").await, Err(StoreError::NotFound(_))));
    assert_eq!(s.get(&framed, "DESIGN-1").await.unwrap().id.as_str(), "DESIGN-1");
    // The same reads through a page: DESIGN's two, nothing else.
    let page = s.page(&framed, PageQuery::default()).unwrap();
    let ids: Vec<String> = page.tickets.iter().map(|t| t.id.to_string()).collect();
    assert_eq!(ids, ["DESIGN-1", "DESIGN-2"]);
}

/// The same body by an admin reads what it asks: `under PLEXUS-1`.
#[tokio::test]
async fn an_admin_s_body_reads_what_it_asks() {
    let s = tracker(Some(ADMIN), WIDE).await;
    let (f, framed) = frame(&s, &admin()).await;
    assert_eq!(f.author.as_deref(), Some(ADMIN));
    assert_eq!(extent(&s, &framed), ["DESIGN:2", "IDENTITY:2", "PLEXUS:1"]);
    assert!(matches!(s.get(&framed, "SECRET-1").await, Err(StoreError::NotFound(_))), "outside what it asks");
}

/// A reader narrower than the author reads its own reach, never more.
#[tokio::test]
async fn a_narrower_reader_reads_its_own_reach() {
    let s = tracker(Some(ADMIN), WIDE).await;
    let reader = user(READER, vec![(shortcode("DESIGN"), Role::Viewer), (shortcode("IDENTITY"), Role::Viewer)]);
    let (_, framed) = frame(&s, &reader).await;
    assert_eq!(extent(&s, &framed), ["DESIGN:2", "IDENTITY:2"]);
    assert!(matches!(s.get(&framed, "PLEXUS-1").await, Err(StoreError::NotFound(_))), "the author's and the ask's, but not the reader's");
}

/// An amend by a narrower author narrows the bound — and an amend back by
/// the wider one widens it again: read afresh at each call.
#[tokio::test]
async fn an_amend_by_another_author_moves_the_bound() {
    let s = tracker(Some(ADMIN), WIDE).await;
    assert_eq!(extent(&s, &frame(&s, &admin()).await.1), ["DESIGN:2", "IDENTITY:2", "PLEXUS:1"]);
    let amended = format!("{WIDE}\nan editor's line\n");
    revisions::acting(act(DESIGNER), s.amend(SYS.permit("DESIGN-2"), None, Some(amended))).await.unwrap();
    let (f, framed) = frame(&s, &admin()).await;
    assert_eq!(f.author.as_deref(), Some(DESIGNER));
    assert_eq!(extent(&s, &framed), ["DESIGN:2"]);
    // A write of another field (the title) by someone else leaves the author.
    revisions::acting(act(IDENTITY), s.amend(SYS.permit("DESIGN-2"), Some("retitled".into()), None)).await.unwrap();
    assert_eq!(frame(&s, &admin()).await.0.author.as_deref(), Some(DESIGNER), "a title is no code");
    // The admin's restore of its own revision is the admin's write.
    s.shift_revision_clock(crate::store::revisions::COALESCE_WINDOW + 1);
    revisions::acting(act(ADMIN), s.amend(SYS.permit("DESIGN-2"), None, Some(WIDE.into()))).await.unwrap();
    assert_eq!(extent(&s, &frame(&s, &admin()).await.1), ["DESIGN:2", "IDENTITY:2", "PLEXUS:1"]);
}

/// A body nobody known wrote (the store's, or one older than revisions)
/// reads the ticket alone, and says so.
#[tokio::test]
async fn no_author_known_reads_the_ticket_alone() {
    let s = tracker(None, WIDE).await;
    let (f, framed) = frame(&s, &admin()).await;
    assert_eq!(f.author, None);
    assert!(f.alone().is_some_and(|why| why.contains("nobody known")), "{:?}", f.alone());
    assert_eq!(extent(&s, &framed), ["DESIGN:1"]);
    assert!(matches!(s.get(&framed, "DESIGN-1").await, Err(StoreError::NotFound(_))), "not even its parent");
    assert_eq!(s.get(&framed, "DESIGN-2").await.unwrap().id.as_str(), "DESIGN-2");
}

/// A body that asks nothing reads its own shortcode's subtree, as
/// IDENTITY-36 bounded it; `all` reads as wide as the reader and author.
#[tokio::test]
async fn nothing_asked_is_its_shortcode_and_all_is_as_wide_as_allowed() {
    let s = tracker(Some(ADMIN), "no frontmatter\n").await;
    let (f, framed) = frame(&s, &admin()).await;
    assert_eq!((f.asked.clone(), f.asked_text()), (ReachAsked::Own, "under DESIGN".to_string()));
    assert_eq!(extent(&s, &framed), ["DESIGN:2"]);
    let s = tracker(Some(ADMIN), "---\nreach-read: all\n---\n").await;
    assert_eq!(extent(&s, &frame(&s, &admin()).await.1), ["DESIGN:2", "IDENTITY:2", "PLEXUS:1", "SECRET:2"]);
    // `all` by the designer is still the designer's.
    let s = tracker(Some(DESIGNER), "---\nreach-read: \"all\"\n---\n").await;
    assert_eq!(extent(&s, &frame(&s, &admin()).await.1), ["DESIGN:2"]);
}

/// An ask the language does not take — or one that asks `text` — reads the
/// ticket alone, and says why.
#[tokio::test]
async fn an_unreadable_ask_reads_the_ticket_alone() {
    for asked in ["under PLEXUS-1 and", "text \"secret\""] {
        let s = tracker(Some(ADMIN), &format!("---\nreach-read: {asked}\n---\n")).await;
        let (f, framed) = frame(&s, &admin()).await;
        assert!(matches!(f.asked, ReachAsked::Unreadable(..)), "{asked}: {:?}", f.asked);
        assert!(f.alone().is_some_and(|why| why.contains("reach-read")), "{asked}: {:?}", f.alone());
        assert_eq!(extent(&s, &framed), ["DESIGN:1"], "{asked}");
    }
}

/// An author who holds nothing any more reads nothing: never wider than
/// the author.
#[tokio::test]
async fn an_author_holding_nothing_reads_nothing() {
    let s = tracker(Some(NOBODY), WIDE).await;
    let (f, framed) = frame(&s, &admin()).await;
    assert_eq!(f.author.as_deref(), Some(NOBODY));
    assert!(extent(&s, &framed).is_empty(), "{:?}", extent(&s, &framed));
    assert!(matches!(s.get(&framed, "DESIGN-2").await, Err(StoreError::NotFound(_))));
}

/// The frame of a ticket the reader cannot see is not found, as `get`.
#[tokio::test]
async fn a_hidden_ticket_has_no_frame() {
    let s = tracker(Some(ADMIN), WIDE).await;
    let reader = user(READER, vec![(shortcode("IDENTITY"), Role::Viewer)]);
    assert!(matches!(s.frame(&reader, "DESIGN-2", author).await, Err(StoreError::NotFound(_))));
}

/// The bound is never wider than the reader, whatever the parts: a plain
/// `Framed` is the reader, and every part only narrows.
#[tokio::test]
async fn the_bound_is_never_wider_than_the_reader() {
    let s = tracker(Some(ADMIN), "---\nreach-read: all\n---\n").await;
    let reader = user(READER, vec![(shortcode("SECRET"), Role::Viewer)]);
    assert_eq!(extent(&s, &Framed::plain(&reader)), ["SECRET:2"]);
    assert_eq!(extent(&s, &Framed::over(&reader, vec![Ok(None)])), ["SECRET:2"], "a part of everything narrows nothing");
    // The reader's own refusal stays its own.
    let held_nowhere = user(READER, vec![]);
    assert!(s.extent(&Framed::plain(&held_nowhere)).is_err());
}
