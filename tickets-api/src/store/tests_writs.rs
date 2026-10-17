//! ACCESS-11: every write through a permit — the ticket checked is the
//! ticket written. A test per ACCESS-3 write gap the store closes (#1 a
//! birth's blockers, #2 a move's old parent and a root's re-hosting, #11 a
//! grant planted while login is off) and the proofs' own rule: a write
//! given a proof for one thing and asked of another writes nothing.

use super::*;
use crate::access::{Access, AdminOf, Held, Hidden, Permit, VerifiedUser};
use crate::address::{OrgId, TicketId};
use crate::members::{Role, Scope};

const WRITER: &str = "idp:0b0e1d2c-60aa-4534-b2e3-83634591fee2";
/// ACCESS-48: a grant is to a principal — one in its grammar.
const SOMEONE: &str = "idp:0b0e1d2c-60aa-4534-b2e3-83634591fee3";

/// A user holding `role` on each of `shortcodes` (and those hosted there).
fn holding(role: Role, shortcodes: &[&str]) -> VerifiedUser {
    let held = shortcodes.iter().map(|s| (Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(s) }, role)).collect();
    VerifiedUser::for_tests(&Access::for_tests(), WRITER.parse().unwrap(), held)
}

fn t(id: &str) -> TicketId {
    TicketId::new(id)
}

fn new(epic: &str, kind: Kind, parent: Option<&str>, blocked_by: &[&str]) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("{epic} {}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: blocked_by.iter().map(|b| (*b).into()).collect(),
        parent: parent.map(str::to_string),
        path: None,
        body: String::new(),
    }
}

/// SECRET-1 · SECRET-2; OPEN-1 (hosted under SECRET-1) · OPEN-2 · OPEN-3
/// (under OPEN-2); OTHER-1 · OTHER-2.
async fn tracker() -> Store {
    let s = Store::new();
    s.create(SYS, new("SECRET", Kind::Scope, None, &[])).await.unwrap();
    s.create(SYS, new("SECRET", Kind::Build, Some("SECRET-1"), &[])).await.unwrap();
    s.create(SYS, new("OPEN", Kind::Scope, Some("SECRET-1"), &[])).await.unwrap();
    s.create(SYS, new("OPEN", Kind::Build, Some("OPEN-1"), &[])).await.unwrap();
    s.create(SYS, new("OPEN", Kind::Build, Some("OPEN-2"), &[])).await.unwrap();
    s.create(SYS, new("OTHER", Kind::Scope, None, &[])).await.unwrap();
    s.create(SYS, new("OTHER", Kind::Build, Some("OTHER-1"), &[])).await.unwrap();
    s
}

fn written(s: &Store) -> u64 {
    s.tickets.read().unwrap().written
}

fn scope(s: &Store, sc: &str) -> Scope {
    s.scope_of_shortcode(&ShortcodeId::new(sc))
}

fn mismatched(e: &StoreError) -> bool {
    e.to_string().contains("the write was not given the proof it needs")
}

#[tokio::test]
async fn a_permit_writes_its_own_ticket_and_no_hidden_one_is_minted() {
    let s = tracker().await;
    let editor = holding(Role::Editor, &["OPEN"]);
    assert_eq!(editor.permit::<Editor>(&t("SECRET-2"), &s).unwrap_err(), Hidden);
    assert!(holding(Role::Viewer, &["OPEN"]).permit::<Editor>(&t("OPEN-2"), &s).is_err(), "a viewer holds no editor's permit");
    let on = editor.permit::<Editor>(&t("OPEN-2"), &s).unwrap();
    match s.set_status(on, Status::Ready).await.unwrap() {
        Change::StatusChanged { ticket, .. } => assert_eq!(ticket.id, "OPEN-2"),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.get(SYS, "SECRET-2").await.unwrap().status, Status::Pending);
}

/// ACCESS-3 #1: a birth's blockers are each a permit — a blocked_by the
/// proofs do not name is refused, and nothing is born.
#[tokio::test]
async fn a_birth_records_only_the_blockers_its_permits_name() {
    let s = tracker().await;
    let editor = holding(Role::Editor, &["OPEN"]);
    assert_eq!(editor.permit::<Viewer>(&t("SECRET-2"), &s).unwrap_err(), Hidden, "no viewer's permit for a hidden blocker");
    assert!(editor.forward("SECRET-99", &s).is_err(), "nor a forward reference where it would not see the ticket");
    assert!(editor.forward("OPEN-99", &s).is_ok(), "one in a shortcode it holds whole stands");
    let before = (written(&s), s.extents_text());
    let parent = editor.permit::<Editor>(&t("OPEN-1"), &s).unwrap();
    let seen = editor.permit::<Viewer>(&t("OPEN-2"), &s).unwrap();
    let at = Filing::new(Under::Parent(parent), vec![Blocker::Seen(seen)]);
    let e = s.create(at, new("OPEN", Kind::Build, Some("OPEN-1"), &["OPEN-2", "SECRET-2"])).await.unwrap_err();
    assert!(mismatched(&e), "{e}");
    assert_eq!((written(&s), s.extents_text()), before, "nothing born, no number spent");
    // Its permits named exactly: born, waiting on what they name.
    let at = Filing::new(Under::Parent(editor.permit(&t("OPEN-1"), &s).unwrap()), vec![Blocker::Seen(editor.permit(&t("OPEN-2"), &s).unwrap())]);
    let born = s.create(at, new("OPEN", Kind::Build, Some("OPEN-1"), &["OPEN-2"])).await.unwrap();
    assert_eq!(born.blocked_by, ["OPEN-2"]);
    // Filed under another parent than its permit's: refused.
    let at = Filing::new(Under::Parent(editor.permit(&t("OPEN-2"), &s).unwrap()), vec![]);
    assert!(mismatched(&s.create(at, new("OPEN", Kind::Build, Some("OPEN-1"), &[])).await.unwrap_err()));
    // A new shortcode: editor on `*` (or its organization), not on a shortcode.
    assert!(editor.on::<Editor>(&Scope::Everywhere, &s).is_err());
    let at = Filing::new(Under::Founding(crate::access::On::for_tests(scope(&s, "OPEN"))), vec![]);
    assert!(mismatched(&s.create(at, new("FRESH", Kind::Scope, None, &[])).await.unwrap_err()));
    let at = Filing::new(Under::Founding(crate::access::On::for_tests(Scope::Everywhere)), vec![]);
    assert_eq!(s.create(at, new("FRESH", Kind::Scope, None, &[])).await.unwrap().id, "FRESH-1");
}

/// ACCESS-3 #2: a move takes the parent the child has now — checked under
/// the write lock: one that moved since refuses.
#[tokio::test]
async fn a_move_checks_the_parent_it_has_under_the_lock() {
    let s = tracker().await;
    let editor = holding(Role::Editor, &["OPEN"]);
    let p = |id: &str| editor.permit::<Editor>(&t(id), &s).unwrap();
    // OPEN-3 hangs under OPEN-2; a proof that says OPEN-1 is stale.
    let stale = Move::new(p("OPEN-3"), Parenting::new(Some(p("OPEN-1")), p("OPEN-1"), None));
    let e = s.set_parent(stale).await.unwrap_err();
    assert!(e.to_string().contains("moved since it was asked"), "{e}");
    assert_eq!(s.get(SYS, "OPEN-3").await.unwrap().parent.unwrap(), "OPEN-2");
    let fresh = Move::new(p("OPEN-3"), Parenting::new(Some(p("OPEN-2")), p("OPEN-1"), None));
    s.set_parent(fresh).await.unwrap();
    assert_eq!(s.get(SYS, "OPEN-3").await.unwrap().parent.unwrap(), "OPEN-1");
    // The parent it goes to is the one its permit names.
    let elsewhere = Move::new(p("OPEN-3"), Parenting::new(Some(p("OPEN-1")), p("OPEN-2"), None));
    assert!(s.set_parent(elsewhere).await.is_ok());
}

/// ACCESS-3 #2: a shortcode's root re-hosts its shortcode — admin on the
/// host it leaves and on the one it joins, as the tree stands; admin on the
/// shortcode alone is not enough to take it from its host.
#[tokio::test]
async fn a_root_moves_only_with_admin_on_the_host_it_leaves_and_the_one_it_joins() {
    let s = tracker().await;
    let admin = holding(Role::Admin, &["OPEN", "OTHER"]);
    let p = |id: &str| admin.permit::<Editor>(&t(id), &s).unwrap();
    // Admin on OPEN and on OTHER, not on SECRET (OPEN's host): no AdminOf SECRET.
    assert!(admin.admin_of(&scope(&s, "SECRET"), &s).is_err());
    let bare = Move::new(p("OPEN-1"), Parenting::new(None, p("OTHER-2"), None));
    let e = s.set_parent(bare).await.unwrap_err();
    assert!(e.to_string().contains("moved since") || e.to_string().contains("re-hosts"), "{e}");
    // OPEN-1's parent is SECRET-1 — a permit the admin of OPEN cannot mint.
    assert_eq!(admin.permit::<Editor>(&t("SECRET-1"), &s).unwrap_err(), Hidden);
    // Proofs for the wrong hosts: refused.
    let wrong = Rehost { leaving: admin.admin_of(&scope(&s, "OPEN"), &s).unwrap(), joining: admin.admin_of(&scope(&s, "OTHER"), &s).unwrap() };
    let mv = Move::new(p("OPEN-1"), Parenting::new(Some(SYS.permit("SECRET-1")), p("OTHER-2"), Some(wrong)));
    assert!(mismatched(&s.set_parent(mv).await.unwrap_err()));
    assert_eq!(s.get(SYS, "OPEN-1").await.unwrap().parent.unwrap(), "SECRET-1", "still hosted where it was");
    // Admin on both hosts: it moves.
    let both = Rehost { leaving: AdminOf::for_tests(scope(&s, "SECRET")), joining: AdminOf::for_tests(scope(&s, "OTHER")) };
    let mv = Move::new(p("OPEN-1"), Parenting::new(Some(SYS.permit("SECRET-1")), p("OTHER-2"), Some(both)));
    s.set_parent(mv).await.unwrap();
    assert_eq!(s.get(SYS, "OPEN-1").await.unwrap().parent.unwrap(), "OTHER-2");
}

/// An update's parent and blockers are proved as `set_parent`'s and
/// `create`'s are.
#[tokio::test]
async fn an_update_takes_the_parent_and_the_blockers_it_names() {
    let s = tracker().await;
    let editor = holding(Role::Editor, &["OPEN"]);
    let p = |id: &str| editor.permit::<Editor>(&t(id), &s).unwrap();
    let to_parent = Update { parent: Some("OPEN-1".into()), ..Update::default() };
    assert!(mismatched(&s.update(Updating::new(p("OPEN-3"), None, vec![]), to_parent.clone()).await.unwrap_err()));
    let block = Update { block: vec!["SECRET-2".into()], ..Update::default() };
    assert!(mismatched(&s.update(Updating::new(p("OPEN-3"), None, vec![Blocker::Seen(p("OPEN-2").viewer())]), block).await.unwrap_err()));
    assert!(s.get(SYS, "OPEN-3").await.unwrap().blocked_by.is_empty(), "nothing written");
    let proved = Updating::new(p("OPEN-3"), Some(Parenting::new(Some(p("OPEN-2")), p("OPEN-1"), None)), vec![]);
    assert_eq!(s.update(proved, to_parent).await.unwrap().ticket.parent.unwrap(), "OPEN-1");
}

/// A write over many tickets takes a permit for each, and writes those.
#[tokio::test]
async fn a_write_over_many_takes_a_permit_per_ticket() {
    let s = tracker().await;
    let editor = holding(Role::Editor, &["OPEN"]);
    let p = |id: &str| editor.permit::<Editor>(&t(id), &s).unwrap();
    let ready = Update { status: Some(Status::Ready), ..Update::default() };
    // Proofs for one of the two selected: refused, nothing written.
    let e = s.update_many_by(Selection::Ids(vec!["OPEN-2".into(), "OPEN-3".into()]), ready.clone(), Act::default(), |_| Ok(vec![Updating::new(p("OPEN-2"), None, vec![])])).await.unwrap_err();
    assert!(mismatched(&e), "{e}");
    assert_eq!(s.get(SYS, "OPEN-2").await.unwrap().status, Status::Pending);
    // `label` over ids labels exactly the tickets its permits are for.
    let changes = s.label_many(vec![p("OPEN-2"), p("OPEN-3")], vec!["x".into()], vec![]).await.unwrap();
    assert_eq!(changes.len(), 2);
    assert!(s.get(SYS, "SECRET-2").await.unwrap().labels.is_empty());
    // A delete takes an admin's permit per ticket that goes.
    let e = s.delete_by(Selection::Ids(vec!["OPEN-3".into()]), Deletion::default(), Act::default(), |_| Ok(Vec::new())).await.unwrap_err();
    assert!(mismatched(&e), "{e}");
    assert!(s.get(SYS, "OPEN-3").await.is_ok());
    assert!(editor.permit::<crate::access::Admin>(&t("OPEN-3"), &s).is_err(), "an editor deletes nothing");
}

/// ACCESS-3 #11: a grant takes admin on exactly its scope, minted for a
/// signed-in user (ACCESS-24: there is no other kind).
#[tokio::test]
async fn a_grant_takes_admin_on_its_scope() {
    let s = tracker().await;
    let g = |scope: &Scope| crate::members::Grant { principal: SOMEONE.parse().unwrap(), scope: scope.clone(), role: Role::Editor, granted_by: crate::vocab::GrantedBy::AuthGrant, at: 0 };
    let open = scope(&s, "OPEN");
    assert!(Access::for_tests().verify_call(None, &plexus_core::plexus::Reach::everywhere()).is_err(), "no one is no admin");
    // Admin on another scope: refused; on this one: kept.
    assert!(mismatched(&s.grant(AdminOf::for_tests(scope(&s, "OTHER")), g(&open)).await.unwrap_err()));
    let admin = holding(Role::Admin, &["OPEN"]).admin_of(&open, &s).unwrap();
    s.grant(admin, g(&open)).await.unwrap();
    assert_eq!(s.grants_of(SYS, SOMEONE).len(), 1);
}

/// A comment write takes a permit on the comment's ticket.
#[tokio::test]
async fn a_comment_write_takes_a_permit_on_its_ticket() {
    let s = tracker().await;
    let anchor = crate::comments::Anchor { route: "#/ticket/OPEN-3".into(), path: "p".into(), label: "l".into(), excerpt: String::new(), ticket: Some("OPEN-3".into()), version: None };
    let viewer = holding(Role::Viewer, &["OPEN"]);
    let wrong = OnComment::new(vec![viewer.permit(&t("OPEN-2"), &s).unwrap()]);
    assert!(mismatched(&s.add_comment(wrong, anchor.clone(), "hi".into(), None).await.unwrap_err()));
    let c = s.add_comment(OnComment::new(vec![viewer.permit(&t("OPEN-3"), &s).unwrap()]), anchor, "hi".into(), None).await.unwrap();
    let editor = holding(Role::Editor, &["OPEN"]);
    let other: Vec<Permit<Editor>> = vec![editor.permit(&t("OPEN-2"), &s).unwrap()];
    assert!(mismatched(&s.resolve_comment(OnComment::new(other), c.id.get()).await.unwrap_err()));
    let on = OnComment::new(vec![editor.permit(&t("OPEN-3"), &s).unwrap()]);
    assert_eq!(s.resolve_comment(on, c.id.get()).await.unwrap().state, crate::comments::CommentState::Resolved);
}
