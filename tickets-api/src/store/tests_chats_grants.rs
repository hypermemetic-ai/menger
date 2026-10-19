//! CHAT-16: an admin adds anyone to any chat — in it or not — and one who
//! can't read the chat's ticket is added with a grant, in one write: the
//! ticket alone (viewer) or the chat only (`chatter`), which reads its chats
//! and nothing of the ticket. Removing such a seat revokes the grant when
//! asked. (The `needs_grant` answer, an add with no grant named, is the
//! activation's: tests/wire_chats.rs.)

use super::*;
use crate::access::{Access, Admin, AdminOf, Held, VerifiedUser, Viewer};
use crate::address::{OrgId, ShortcodeId, TicketId};
use crate::chats::ChatKind;
use crate::members::{Grant, Role, Scope};
use crate::vocab::{GrantedBy, Grantee};

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const RUI: &str = "idp:00000000-0000-4000-8000-000000000041";
const SAM: &str = "idp:00000000-0000-4000-8000-000000000005";
const BOSS: &str = "idp:00000000-0000-4000-8000-0000000000ff";

fn kiln_reader(principal: &str) -> VerifiedUser {
    let kiln = Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new("KILN") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(kiln, Role::Viewer)])
}

/// `principal` holding `role` on KILN-7 alone.
fn on_seven(principal: &str, role: Role) -> VerifiedUser {
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(Held::Ticket(TicketId::new("KILN-7")), role)])
}

fn reads(p: &str) -> bool {
    p != SAM
}

async fn kiln() -> Store {
    let store = Store::new();
    store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
    for _ in 2..=7 {
        store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    }
    store
}

fn grant(principal: &str, role: Role) -> Grant {
    Grant { principal: Grantee::try_from(principal).unwrap(), scope: Scope::Ticket(TicketId::new("KILN-7")), role, granted_by: GrantedBy::Principal(BOSS.parse().unwrap()), at: 1 }
}

fn seven_admin() -> AdminOf {
    AdminOf::for_tests(Scope::Ticket(TicketId::new("KILN-7")))
}

#[tokio::test]
async fn an_admin_not_in_the_chat_adds_anyone_and_a_non_admin_outside_it_cannot() {
    let store = kiln().await;
    let (c, _) = store.open_chat(SYS.permit::<Viewer>("KILN-7"), Signer::named(ADA), vec![], None, reads).await.unwrap();
    // The admin of KILN-7, in no seat, adds rui (who reads it).
    let added = store.add_to_chat(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), RUI.into(), reads).await.unwrap();
    assert!(added.has(RUI) && !added.has(BOSS));
    // A reader not in it: refused.
    let e = store.add_to_chat(SYS.permit::<Viewer>("KILN-7"), c.id, Signer::named(SAM), RUI.into(), reads).await.unwrap_err();
    assert!(matches!(e, ChatError::Refused(r) if r[0].contains("only its participants add")));
    // One who can't read it, without a grant: refused naming it.
    let e = store.add_to_chat(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), SAM.into(), reads).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec![format!("{SAM} can't read KILN-7")]));
}

#[tokio::test]
async fn an_add_grants_the_ticket_alone_in_the_same_write() {
    let store = kiln().await;
    let (c, _) = store.open_chat(SYS.permit::<Viewer>("KILN-7"), Signer::named(ADA), vec![], None, reads).await.unwrap();
    let added = store.add_granting(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), SAM.into(), seven_admin(), grant(SAM, Role::Viewer)).await.unwrap();
    assert_eq!(added.parts.last().unwrap().granted.as_deref(), Some("KILN-7"));
    let held = store.tickets.read().unwrap().members.of(SAM);
    assert_eq!(held.iter().map(|g| (g.scope.text(), g.role)).collect::<Vec<_>>(), [("KILN-7".to_string(), Role::Viewer)]);
    // Who it was granted to, on the chat as its readers see it.
    assert_eq!(store.chat_view(&kiln_reader(ADA), c.id).unwrap().granted, [SAM]);
    let view = store.chat_view(&on_seven(SAM, Role::Viewer), c.id).unwrap();
    assert_eq!((view.ticket.as_deref(), view.ticket_title.is_some()), (Some("KILN-7"), true));
    // A grant on another scope than the authority's is refused, nothing written.
    let wrong = AdminOf::for_tests(Scope::Ticket(TicketId::new("KILN-6")));
    let e = store.add_granting(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), RUI.into(), wrong, grant(RUI, Role::Viewer)).await.unwrap_err();
    assert!(matches!(e, ChatError::Refused(_)), "{e:?}");
    assert!(store.tickets.read().unwrap().members.of(RUI).is_empty());
}

#[tokio::test]
async fn a_chat_only_seat_reads_its_chat_and_nothing_of_the_ticket() {
    let store = kiln().await;
    let (c, _) = store.open_chat(SYS.permit::<Viewer>("KILN-7"), Signer::named(ADA), vec![RUI.into()], None, reads).await.unwrap();
    let (other, _) = store.open_chat(SYS.permit::<Viewer>("KILN-7"), Signer::named(ADA), vec![], Some("ada's notes".into()), reads).await.unwrap();
    store.add_granting(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), SAM.into(), seven_admin(), grant(SAM, Role::Chatter)).await.unwrap();
    let sam = on_seven(SAM, Role::Chatter);
    // The ticket: hidden — no viewer's permit, so no body, no children.
    assert!(sam.permit::<Viewer>(&TicketId::new("KILN-7"), &store).is_err());
    // Its chat: read and post.
    let (s, seat) = store.chat_seen(&sam, c.id).unwrap();
    assert!(seat.participant && s.kind == ChatKind::Ticket);
    let m = store.post_chat(seat.on, c.id, Signer::named(SAM), "the shelf cracked".into()).await.unwrap();
    assert_eq!(store.chat_messages(&sam, c.id, None, 10).unwrap().0.last().map(|x| x.id), Some(m.id));
    // Its id and title, beside the chat.
    let view = store.chat_view(&sam, c.id).unwrap();
    assert_eq!(view.ticket.as_deref(), Some("KILN-7"));
    assert!(view.ticket_title.is_some());
    // Another chat on the ticket: not found.
    assert_eq!(store.chat_messages(&sam, other.id, None, 10).unwrap_err(), ChatError::NotFound(other.id));
    assert_eq!(store.chats(&sam, &ChatQuery::default(), None, 50).unwrap().0.iter().map(|c| c.id.get()).collect::<Vec<_>>(), [c.id]);
}

#[tokio::test]
async fn removing_a_granted_seat_revokes_its_grant_when_asked() {
    let store = kiln().await;
    let (c, _) = store.open_chat(SYS.permit::<Viewer>("KILN-7"), Signer::named(ADA), vec![RUI.into()], None, reads).await.unwrap();
    store.add_granting(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), SAM.into(), seven_admin(), grant(SAM, Role::Chatter)).await.unwrap();
    // Without revoke: out of the chat, the grant kept.
    let rui_on = store.chat_seen(&kiln_reader(RUI), c.id).unwrap().1.on;
    let (_, revoked) = store.remove_revoking(rui_on, c.id, Signer::named(RUI), SAM.into(), None).await.unwrap();
    assert!(!revoked);
    assert_eq!(store.tickets.read().unwrap().members.of(SAM).len(), 1);
    // Added again with it, then removed revoking: both gone, in one write.
    store.add_granting(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), SAM.into(), seven_admin(), grant(SAM, Role::Chatter)).await.unwrap();
    let (out, revoked) = store.remove_revoking(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), SAM.into(), Some(seven_admin())).await.unwrap();
    assert!(revoked && !out.has(SAM));
    assert!(store.tickets.read().unwrap().members.of(SAM).is_empty());
    // A seat that made no grant: refused, nothing written.
    let e = store.remove_revoking(ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-7")), c.id, Signer::named(BOSS), RUI.into(), Some(seven_admin())).await.unwrap_err();
    assert!(matches!(e, ChatError::Refused(r) if r[0].contains("made no grant")));
    assert!(store.tickets.read().unwrap().chats.by_id[&c.id].has(RUI));
}

#[tokio::test]
async fn a_group_s_chat_only_grant_is_on_its_organization() {
    let store = kiln().await;
    let (g, _) = store.open_outside(&kiln_reader(ADA), OrgId::HYPERMEMETIC.into(), vec![RUI.into(), BOSS.into()], Some("crew".into()), |_| true).await.unwrap();
    let org = Scope::Org(OrgId::new(OrgId::HYPERMEMETIC).unwrap());
    let gr = Grant { principal: Grantee::try_from(SAM).unwrap(), scope: org.clone(), role: Role::Chatter, granted_by: GrantedBy::Principal(BOSS.parse().unwrap()), at: 1 };
    let added = store.add_granting(ChatOn::Admin(AdminOf::for_tests(org.clone())), g.id, Signer::named(BOSS), SAM.into(), AdminOf::for_tests(org), gr).await.unwrap();
    assert_eq!(added.parts.last().unwrap().granted.as_deref(), Some("hypermemetic:"));
}
