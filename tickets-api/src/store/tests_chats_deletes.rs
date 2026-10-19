//! CHAT-44: deleting chat messages — kept and marked, never gone. Its
//! author deletes a message, an admin of the chat's ticket deletes any; all
//! or none, each refusal named; a deleted message is no one's unread, no
//! `await_message` reply and not a chat's last line; `restore` puts it
//! back; a delete survives a reopen.

use super::*;
use crate::access::{Access, Admin, Held, VerifiedUser, Viewer};
use crate::address::OrgId;
use crate::members::Role;

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const BUILDER: &str = "idp:00000000-0000-4000-8000-0000000000b1";
const BOSS: &str = "idp:00000000-0000-4000-8000-0000000000ff";

fn reader(principal: &str) -> VerifiedUser {
    let kiln = Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new("KILN") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(kiln, Role::Viewer)])
}

async fn kiln(path: Option<&std::path::Path>) -> Store {
    let store = match path {
        Some(p) => Store::open(p).unwrap(),
        None => Store::new(),
    };
    store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
    store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    store
}

/// A chat on KILN-2 between ada and builder: builder's report posted five
/// times (2..=6) between ada's 1 and 7.
async fn chat(store: &Store) -> u64 {
    let (c, _) = store.open_chat(SYS.permit::<Viewer>("KILN-2"), Signer::named(ADA), vec![BUILDER.into()], None, |_| true).await.unwrap();
    post(store, ADA, c.id, "the report?").await;
    for _ in 0..5 {
        post(store, BUILDER, c.id, "the report").await;
    }
    post(store, ADA, c.id, "thanks").await;
    c.id
}

async fn post(store: &Store, by: &str, chat: u64, body: &str) {
    store.post_chat(SYS.permit::<Viewer>("KILN-2"), chat, Signer::named(by), body.into()).await.unwrap();
}

async fn delete(store: &Store, by: &str, chat: u64, pick: Pick, restore: bool) -> Result<MessageDeletion, ChatError> {
    store.delete_messages(SYS.permit::<Viewer>("KILN-2"), chat, Signer::named(by), pick, restore).await
}

fn deleted(store: &Store, chat: u64) -> Vec<u64> {
    let (msgs, _) = store.chat_messages(&reader(ADA), chat, None, 100).unwrap();
    msgs.iter().filter(|m| m.deleted.is_some()).map(|m| m.id.get()).collect()
}

#[tokio::test]
async fn the_author_deletes_its_own_and_the_messages_stay_marked() {
    let store = kiln(None).await;
    let c = chat(&store).await;
    let mut rx = store.subscribe_chats();
    let d = delete(&store, BUILDER, c, Pick::Range { from: 3, to: 6 }, false).await.unwrap();
    assert_eq!((d.ids.as_slice(), d.by.as_str()), (&[3, 4, 5, 6][..], BUILDER));
    match rx.try_recv().unwrap() {
        ChatChange::Deleted { chat, ids, by, restored, .. } => assert_eq!((chat, ids, by.as_str(), restored), (c, vec![3, 4, 5, 6], BUILDER, false)),
        other => panic!("{other:?}"),
    }
    // Kept: every message is still answered, the four marked, by whom.
    let (msgs, _) = store.chat_messages(&reader(ADA), c, None, 100).unwrap();
    assert_eq!(msgs.len(), 7);
    assert_eq!(deleted(&store, c), [3, 4, 5, 6]);
    assert_eq!(msgs[2].deleted.as_ref().unwrap().by, BUILDER);
    // Deleting them again names none and says nothing.
    let again = delete(&store, BUILDER, c, Pick::Ids(vec![3, 4]), false).await.unwrap();
    assert!(again.ids.is_empty());
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn all_or_none_naming_each_refusal() {
    let store = kiln(None).await;
    let c = chat(&store).await;
    // ada may not delete builder's; 99 is no message of the chat: nothing written.
    let ChatError::Refused(reasons) = delete(&store, ADA, c, Pick::Ids(vec![1, 2, 99]), false).await.unwrap_err() else { panic!() };
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    assert!(reasons[0].contains("message 2 is") && reasons[0].contains("only its author or an admin deletes it"), "{reasons:?}");
    assert!(reasons[1].contains("no message 99 in chat"), "{reasons:?}");
    assert!(deleted(&store, c).is_empty());
    // A range is every message in it: builder's range over ada's 7 is refused.
    assert!(delete(&store, BUILDER, c, Pick::Range { from: 2, to: 7 }, false).await.is_err());
    assert!(matches!(delete(&store, BUILDER, c, Pick::Range { from: 6, to: 2 }, false).await, Err(ChatError::Refused(_))));
    assert!(matches!(delete(&store, BUILDER, c, Pick::Ids(vec![]), false).await, Err(ChatError::Refused(_))));
    // One not in the chat deletes nothing, even its own id's worth.
    assert!(delete(&store, BOSS, c, Pick::Ids(vec![1]), false).await.is_err());
}

#[tokio::test]
async fn an_admin_of_the_ticket_deletes_anyone_s_and_the_author_restores() {
    let store = kiln(None).await;
    let c = chat(&store).await;
    let admin = ChatOn::TicketAdmin(SYS.permit::<Admin>("KILN-2"));
    let d = store.delete_messages(admin, c, Signer::named(BOSS), Pick::Ids(vec![1, 2, 3]), false).await.unwrap();
    assert_eq!(d.ids, [1, 2, 3]);
    assert_eq!(deleted(&store, c), [1, 2, 3]);
    // Restore: the same rule — builder puts back its own, not ada's.
    assert!(delete(&store, BUILDER, c, Pick::Ids(vec![1, 2]), true).await.is_err());
    let r = delete(&store, BUILDER, c, Pick::Ids(vec![2, 3]), true).await.unwrap();
    assert_eq!(r.ids, [2, 3]);
    assert_eq!(deleted(&store, c), [1]);
}

#[tokio::test]
async fn a_deleted_message_is_no_one_s_unread_no_reply_and_not_the_last_line() {
    let store = kiln(None).await;
    let c = chat(&store).await;
    // ada's thanks (7) moved her mark past builder's five: two more of his.
    post(&store, BUILDER, c, "the report").await;
    post(&store, BUILDER, c, "the report").await;
    assert_eq!(store.chat_view(&reader(ADA), c).unwrap().unread, 2);
    let wait = MessageWait { chat: Some(c), after: 7, ..Default::default() };
    assert_eq!(store.message_for(&reader(ADA), &wait).unwrap().id.get(), 8);
    delete(&store, BUILDER, c, Pick::Ids(vec![8, 9]), false).await.unwrap();
    let view = store.chat_view(&reader(ADA), c).unwrap();
    assert_eq!((view.unread, view.last_message.map(|m| m.get()), view.count), (0, Some(7), 9), "count is every message kept");
    assert_eq!(store.message_for(&reader(ADA), &wait), None, "a deleted message answers no wait");
    // The filter cell `unread` follows.
    let lens = Lens::parse("unread").unwrap().with_clock(Clock { me: ADA.into(), ..Clock::default() });
    assert_eq!(store.member(SYS, &["KILN-2".to_string()], &lens), [false]);
    // Restored, they count again.
    delete(&store, BUILDER, c, Pick::Ids(vec![9]), true).await.unwrap();
    assert_eq!(store.chat_view(&reader(ADA), c).unwrap().unread, 1);
}

#[tokio::test]
async fn a_delete_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = kiln(Some(&path)).await;
        let c = chat(&store).await;
        delete(&store, BUILDER, c, Pick::Range { from: 2, to: 6 }, false).await.unwrap();
        delete(&store, BUILDER, c, Pick::Ids(vec![4]), true).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(deleted(&store, 1), [2, 3, 5, 6]);
    let (msgs, _) = store.chat_messages(&reader(ADA), 1, None, 100).unwrap();
    assert_eq!(msgs[1].deleted.as_ref().unwrap().by, BUILDER);
}
