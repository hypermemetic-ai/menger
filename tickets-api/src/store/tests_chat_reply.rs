//! CHAT-66: a reply is a `[↩](message:N)` token first in its body — read
//! off it, carried on every message the store answers with the original's
//! quote, refused when it names a message of another chat (or none), the
//! quote saying when the original is deleted; and it survives a reopen.

use super::*;
use crate::access::{Access, Held, VerifiedUser, Viewer};
use crate::address::OrgId;
use crate::members::Role;

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const BUILDER: &str = "idp:00000000-0000-4000-8000-0000000000b1";

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
    store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    store
}

async fn open(store: &Store, ticket: &str) -> u64 {
    store.open_chat(SYS.permit::<Viewer>(ticket), Signer::named(ADA), vec![BUILDER.into()], None, |_| true).await.unwrap().0.id
}

async fn post(store: &Store, ticket: &str, by: &str, chat: u64, body: &str) -> Result<crate::chats::Message, ChatError> {
    store.post_chat(SYS.permit::<Viewer>(ticket), chat, Signer::named(by), body.into()).await
}

#[tokio::test]
async fn a_reply_is_read_off_its_body_and_carries_the_quote() {
    let store = kiln(None).await;
    let c = open(&store, "KILN-2").await;
    let first = post(&store, "KILN-2", BUILDER, c, "Expired events: archive or delete?\n\nmore").await.unwrap();
    assert_eq!((first.reply_to, &first.quote), (None, &None), "not a reply");
    let mut rx = store.subscribe_chats();
    let reply = post(&store, "KILN-2", ADA, c, &format!("[↩](message:{}) archive", first.id.get())).await.unwrap();
    assert_eq!(reply.reply_to, Some(first.id));
    assert_eq!(reply.body, format!("[↩](message:{}) archive", first.id.get()), "the body keeps only the token");
    let q = reply.quote.clone().expect("quoted");
    assert_eq!((q.id, q.author.as_str(), q.snippet.as_str(), q.deleted), (first.id, BUILDER, "Expired events: archive or delete?", false));
    // The watch hears it quoted; `messages` answers it quoted.
    match rx.try_recv().unwrap() {
        ChatChange::Posted(m) => assert_eq!(m.quote, reply.quote),
        other => panic!("{other:?}"),
    }
    let (msgs, _) = store.chat_messages(&reader(ADA), c, None, 100).unwrap();
    assert_eq!(msgs[1].reply_to, Some(first.id));
    assert_eq!(msgs[1].quote, reply.quote);
    // A token later in the body is a link, not a reply.
    let link = post(&store, "KILN-2", ADA, c, &format!("see [↩](message:{})", first.id.get())).await.unwrap();
    assert_eq!(link.reply_to, None);
}

#[tokio::test]
async fn a_reply_to_another_chat_or_to_nothing_is_refused_naming_it() {
    let store = kiln(None).await;
    let here = open(&store, "KILN-2").await;
    let there = open(&store, "KILN-3").await;
    let elsewhere = post(&store, "KILN-3", BUILDER, there, "over there").await.unwrap();
    let err = post(&store, "KILN-2", ADA, here, &format!("[↩](message:{}) hi", elsewhere.id.get())).await.unwrap_err();
    let ChatError::Refused(why) = err else { panic!("{err:?}") };
    assert!(why[0].contains(&format!("message {} is not in chat {here}", elsewhere.id.get())), "{why:?}");
    let err = post(&store, "KILN-2", ADA, here, "[↩](message:999) hi").await.unwrap_err();
    assert!(matches!(err, ChatError::Refused(ref w) if w[0].contains("message 999 is not in chat")), "{err:?}");
    // Nothing was posted.
    assert!(store.chat_messages(&reader(ADA), here, None, 100).unwrap().0.is_empty());
}

#[tokio::test]
async fn a_deleted_original_is_said_and_a_reply_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let c;
    {
        let store = kiln(Some(&path)).await;
        c = open(&store, "KILN-2").await;
        post(&store, "KILN-2", BUILDER, c, "![drawing](media:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)").await.unwrap();
        post(&store, "KILN-2", ADA, c, "[↩](message:1) nice").await.unwrap();
        post(&store, "KILN-2", ADA, c, "[↩](message:2)\nand again").await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let (msgs, _) = store.chat_messages(&reader(ADA), c, None, 100).unwrap();
    assert_eq!(msgs[1].quote.as_ref().map(|q| q.snippet.as_str()), Some("drawing"));
    assert_eq!(msgs[2].quote.as_ref().map(|q| (q.id.get(), q.snippet.as_str())), Some((2, "nice")), "a quote is its words, not its token");
    store.delete_messages(SYS.permit::<Viewer>("KILN-2"), c, Signer::named(BUILDER), Pick::Ids(vec![1]), false).await.unwrap();
    let (msgs, _) = store.chat_messages(&reader(ADA), c, None, 100).unwrap();
    let q = msgs[1].quote.clone().unwrap();
    assert!(q.deleted && q.snippet.is_empty(), "{q:?}");
}
