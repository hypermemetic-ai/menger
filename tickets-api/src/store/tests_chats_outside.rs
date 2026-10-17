//! CHAT-14: chats outside tickets, as the store answers them — a direct
//! message between two (the same two again: that one), a group with a
//! title, members added and removed by members and by an admin of the
//! organization, reach for its participants alone (no ticket's, no
//! admin's), a chat's images seen by its participants alone, and the names
//! principals are known by.

use super::*;
use crate::access::{Access, AdminOf, Held, VerifiedUser, Viewer};
use crate::address::{OrgId, ShortcodeId};
use crate::chats::{ChatKind, ChatState};
use crate::members::{Role, Scope};
use base64::Engine;

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const RUI: &str = "idp:00000000-0000-4000-8000-000000000041";
const BUILDER: &str = "key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28";
const REVIEWER: &str = "idp:00000000-0000-4000-8000-0000000000e1";
const SAM: &str = "idp:00000000-0000-4000-8000-000000000005";
const BOSS: &str = "idp:00000000-0000-4000-8000-0000000000ff";
const ORG: &str = OrgId::HYPERMEMETIC;

/// `principal`, a viewer of KILN.
fn reader(principal: &str) -> VerifiedUser {
    let kiln = Held::Shortcode { org: OrgId::new(ORG).unwrap(), id: ShortcodeId::new("KILN") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(kiln, Role::Viewer)])
}

/// An admin of everything, in no chat.
fn boss() -> VerifiedUser {
    VerifiedUser::for_tests(&Access::for_tests(), BOSS.parse().unwrap(), vec![(Held::Everywhere, Role::Admin)])
}

/// Who holds a grant in the org: everyone but sam.
fn member(p: &str) -> bool {
    p != SAM
}

async fn kiln() -> Store {
    let store = Store::new();
    store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
    store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    store
}

async fn open(store: &Store, by: &str, with: &[&str], title: Option<&str>) -> Result<(ChatRecord, bool), ChatError> {
    store.open_outside(&reader(by), ORG.into(), with.iter().map(|s| s.to_string()).collect(), title.map(str::to_string), member).await
}

/// `by`'s seat in `chat`.
fn seat(store: &Store, by: &str, chat: u64) -> ChatOn {
    store.chat_seen(&reader(by), chat).unwrap().1.on
}

#[tokio::test]
async fn a_direct_message_is_two_and_opening_it_again_is_that_one() {
    let store = kiln().await;
    let (dm, again) = open(&store, ADA, &[RUI], None).await.unwrap();
    assert!(!again);
    assert_eq!((dm.kind, dm.ticket.as_str(), dm.org.as_str()), (ChatKind::Direct, "", ORG));
    assert_eq!(dm.parts.iter().map(|p| p.principal.as_str()).collect::<Vec<_>>(), [ADA, RUI]);
    let (back, existing) = open(&store, RUI, &[ADA], None).await.unwrap();
    assert!(existing && back.id == dm.id, "rui opening it with ada is the one there is");
    let view = store.chat_view(&reader(RUI), dm.id).unwrap();
    assert_eq!((view.kind, view.ticket), (ChatKind::Direct, None));
    // Closed, a new one opens.
    store.set_chat_state(seat(&store, ADA, dm.id), dm.id, Signer::named(ADA), false).await.unwrap();
    let (fresh, existing) = open(&store, ADA, &[RUI], None).await.unwrap();
    assert!(!existing && fresh.id != dm.id);
    // Nobody named is refused; one outside the org is named.
    assert!(matches!(open(&store, ADA, &[], None).await, Err(ChatError::Refused(r)) if r[0].contains("names someone else")));
    let Err(ChatError::Refused(r)) = open(&store, ADA, &[SAM], None).await else { panic!("sam holds no grant") };
    assert_eq!(r, [format!("{SAM} holds no grant in {ORG}")]);
}

#[tokio::test]
async fn a_group_is_several_with_a_title_and_members_add_and_remove() {
    let store = kiln().await;
    let (g, _) = open(&store, ADA, &[RUI, BUILDER], Some(" the kiln crew ")).await.unwrap();
    assert_eq!((g.kind, g.title.as_deref()), (ChatKind::Group, Some("the kiln crew")));
    let added = store.add_to_chat(seat(&store, RUI, g.id), g.id, Signer::named(RUI), REVIEWER.into(), member).await.unwrap();
    assert!(added.has(REVIEWER), "a member adds");
    let e = store.add_to_chat(seat(&store, RUI, g.id), g.id, Signer::named(RUI), SAM.into(), member).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec![format!("{SAM} holds no grant in {ORG}")]));
    let out = store.remove_from_chat(seat(&store, ADA, g.id), g.id, Signer::named(ADA), BUILDER.into()).await.unwrap();
    assert!(!out.has(BUILDER));
    assert!(store.chat_view(&reader(BUILDER), g.id).is_none(), "the one removed no longer reads it");
    // Leaving: one removes itself.
    store.remove_from_chat(seat(&store, REVIEWER, g.id), g.id, Signer::named(REVIEWER), REVIEWER.into()).await.unwrap();
    // An admin of the org adds and removes without being in it, and reads nothing.
    let admin = || ChatOn::Admin(AdminOf::for_tests(Scope::Org(OrgId::new(ORG).unwrap())));
    let back = store.add_to_chat(admin(), g.id, Signer::named(BOSS), BUILDER.into(), member).await.unwrap();
    assert!(back.has(BUILDER) && !back.has(BOSS));
    store.remove_from_chat(admin(), g.id, Signer::named(BOSS), RUI.into()).await.unwrap();
    assert!(store.chat_view(&boss(), g.id).is_none(), "an admin of the org does not read it");
    let elsewhere = ChatOn::Admin(AdminOf::for_tests(Scope::Org(OrgId::new("acme").unwrap())));
    assert_eq!(store.add_to_chat(elsewhere, g.id, Signer::named(BOSS), RUI.into(), member).await.unwrap_err(), ChatError::NotFound(g.id));
    // Who is in it survives in the file as in the cache: a removed one is gone from both.
    let kept: Vec<String> = store.db.lock().unwrap().prepare("SELECT principal FROM chat_participants WHERE chat = ?1 ORDER BY rowid").unwrap().query_map([g.id as i64], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(kept, [ADA, BUILDER]);
}

#[tokio::test]
async fn a_direct_message_stays_between_two() {
    let store = kiln().await;
    let (dm, _) = open(&store, ADA, &[RUI], None).await.unwrap();
    let Err(ChatError::Refused(r)) = store.add_to_chat(seat(&store, ADA, dm.id), dm.id, Signer::named(ADA), BUILDER.into(), member).await else { panic!() };
    assert!(r[0].contains("direct message"), "{r:?}");
    let Err(ChatError::Refused(r)) = store.remove_from_chat(seat(&store, ADA, dm.id), dm.id, Signer::named(ADA), RUI.into()).await else { panic!() };
    assert!(r[0].contains("close it instead"), "{r:?}");
}

#[tokio::test]
async fn only_its_participants_read_a_chat_outside_tickets() {
    let store = kiln().await;
    let (dm, _) = open(&store, ADA, &[RUI], None).await.unwrap();
    store.post_chat(seat(&store, ADA, dm.id), dm.id, Signer::named(ADA), "just us".into()).await.unwrap();
    assert_eq!(store.chat_messages(&reader(RUI), dm.id, None, 10).unwrap().0.len(), 1);
    for outside in [reader(REVIEWER), boss()] {
        assert_eq!(store.chat_messages(&outside, dm.id, None, 10).unwrap_err(), ChatError::NotFound(dm.id), "no ticket's reach, no admin's");
        assert!(store.chats(&outside, &ChatQuery::default(), None, 50).unwrap().0.is_empty());
    }
    // A ticket's permit is no proof for it.
    let e = store.post_chat(SYS.permit::<Viewer>("KILN-2"), dm.id, Signer::named(ADA), "x".into()).await.unwrap_err();
    assert_eq!(e, ChatError::NotFound(dm.id));
    // The list by kind.
    store.open_chat(SYS.permit::<Viewer>("KILN-2"), Signer::named(ADA), vec![RUI.into()], None, |_| true).await.unwrap();
    let kinds = |k| store.chats(&reader(ADA), &ChatQuery { kind: Some(k), ..ChatQuery::default() }, None, 50).unwrap().0.len();
    assert_eq!((kinds(ChatKind::Direct), kinds(ChatKind::Ticket), kinds(ChatKind::Group)), (1, 1, 0));
    // The filter cells are a ticket's: a chat outside tickets moves none.
    assert_eq!(store.chats(&reader(ADA), &ChatQuery::default(), None, 50).unwrap().0.len(), 2);
}

#[tokio::test]
async fn a_chat_s_image_is_its_participants_alone() {
    let store = kiln().await;
    let png = {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&4u32.to_be_bytes());
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0]);
        base64::engine::general_purpose::STANDARD.encode(b)
    };
    let (dm, _) = open(&store, ADA, &[RUI], None).await.unwrap();
    let (s, _) = (store.chat_seen(&reader(ADA), dm.id).unwrap().1, ());
    let put = store.put_media(OnMedia::chat(dm.id, &s), "image/png", png.clone(), Some(ADA.into())).await.unwrap();
    let hash = put.info.hash;
    assert!(store.media_info(&reader(RUI), &hash).is_ok(), "a participant sees it");
    for outside in [reader(REVIEWER), boss()] {
        assert!(matches!(store.media_info(&outside, &hash), Err(StoreError::MediaNotFound(_))), "not found to one not in it, an admin too");
    }
    assert!(store.media_info(SYS, &hash).is_ok());
    // A ticket's chat: its participants — not the ticket's other readers.
    let (tc, _) = store.open_chat(SYS.permit::<Viewer>("KILN-2"), Signer::named(ADA), vec![RUI.into()], None, |_| true).await.unwrap();
    let s = store.chat_seen(&reader(ADA), tc.id).unwrap().1;
    let mut other = base64::engine::general_purpose::STANDARD.decode(&png).unwrap();
    other.push(1);
    let h2 = store.put_media(OnMedia::chat(tc.id, &s), "image/png", base64::engine::general_purpose::STANDARD.encode(other), Some(ADA.into())).await.unwrap().info.hash;
    assert!(store.media_info(&reader(RUI), &h2).is_ok());
    assert!(matches!(store.media_info(&reader(REVIEWER), &h2), Err(StoreError::MediaNotFound(_))), "reviewer reads KILN-2 and is not in the chat");
    // One not in the chat cannot put for it.
    let e = store.put_media(OnMedia::chat(tc.id, &s), "image/png", png, Some(REVIEWER.into())).await.unwrap_err();
    assert!(matches!(e, StoreError::NotFound(_)), "{e:?}");
}

#[tokio::test]
async fn a_principal_is_known_by_the_name_it_signed_in_with() {
    let store = kiln().await;
    assert_eq!(store.name_of(BUILDER), None);
    let (dm, _) = open(&store, ADA, &[BUILDER], None).await.unwrap();
    let mut by = Signer::named(BUILDER);
    by.name = Some("agent-old".into());
    let on = store.chat_seen(&reader(BUILDER), dm.id).unwrap().1.on;
    store.post_chat(on, dm.id, by, "hi".into()).await.unwrap();
    assert_eq!(store.name_of(BUILDER).as_deref(), Some("agent-old"), "before a sign-in, the name its messages carry");
    assert!(store.note_name(SYS, BUILDER, " builder ").await.unwrap());
    assert!(!store.note_name(SYS, BUILDER, "builder").await.unwrap(), "the same name writes nothing");
    assert_eq!(store.name_of(BUILDER).as_deref(), Some("builder"));
    assert!(store.knows_name(BUILDER, "builder") && !store.knows_name(BUILDER, "other"));
    let kept: String = store.db.lock().unwrap().query_row("SELECT name FROM principal_names WHERE principal = ?1", [BUILDER], |r| r.get(0)).unwrap();
    assert_eq!(kept, "builder");
    let _ = ChatState::Open;
}

/// CHAT-13 beside CHAT-14: the standing chat members join every direct
/// message and group; a direct message is still known by its two people.
#[tokio::test]
async fn the_standing_members_join_direct_messages_and_groups() {
    let store = kiln().await;
    store.set_chat_members(vec![BUILDER.into()]);
    let (dm, _) = open(&store, ADA, &[RUI], None).await.unwrap();
    assert_eq!(dm.parts.iter().map(|p| p.principal.as_str()).collect::<Vec<_>>(), [ADA, RUI, BUILDER]);
    assert_eq!(dm.kind, ChatKind::Direct, "builder beside them does not make it a group");
    assert!(open(&store, RUI, &[ADA], None).await.unwrap().1, "the same two: that one");
    let (with_builder, existing) = open(&store, ADA, &[BUILDER], None).await.unwrap();
    assert!(!existing && with_builder.id != dm.id, "ada and builder is another direct message");
    assert_eq!(with_builder.parts.len(), 2);
    assert!(open(&store, BUILDER, &[ADA], None).await.unwrap().1);
    let (g, _) = open(&store, ADA, &[RUI, REVIEWER], Some("crew")).await.unwrap();
    assert!(g.has(BUILDER));
    assert_eq!(store.chat_view(&reader(ADA), g.id).unwrap().agents, [BUILDER]);
    // The startup back-fill reaches a chat outside tickets that lacks them.
    store.set_chat_members(vec![BUILDER.into(), BOSS.into()]);
    let seated = store.seat_chat_members(SYS, |_, _| false).await.unwrap();
    assert!(seated.contains(&dm.id) && seated.contains(&g.id));
}
