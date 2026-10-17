//! CHAT-3: CHAT-1's scenarios, as the store answers them — on the synthetic
//! project KILN: ada is a person; builder and reviewer would be agents
//! (AGENT-3 gives them `agent:<name>` principals — here they are plain
//! principals); rui is a reader of KILN in no chat; sam reads nothing.
//!
//! A chat belongs to one ticket (opened there; the same chat again), only
//! participants read it (rui), every participant can read the ticket (sam,
//! a grant revoked), a message is signed by its caller, an agent waits for
//! a message (and a dropped connection loses nothing; who is listening),
//! unread is kept per participant, a chat closes and reopens, the filter
//! cells `chatting` and `unread`, and the chats survive a reopen.

use super::tests::Changes;
use super::*;
use crate::access::{Access, Held, VerifiedUser, Viewer};
use crate::address::OrgId;
use crate::chats::{ChatState, Message};
use crate::members::Role;

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const BUILDER: &str = "idp:00000000-0000-4000-8000-0000000000b1";
const REVIEWER: &str = "idp:00000000-0000-4000-8000-0000000000e1";
const RUI: &str = "idp:00000000-0000-4000-8000-000000000041";
const SAM: &str = "idp:00000000-0000-4000-8000-000000000005";
const BOSS: &str = "idp:00000000-0000-4000-8000-0000000000ff";

/// `principal`, a viewer of KILN.
fn reader(principal: &str) -> VerifiedUser {
    let kiln = Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new("KILN") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(kiln, Role::Viewer)])
}

/// `principal`, holding nothing on KILN (a viewer of OTHER only).
fn outsider(principal: &str) -> VerifiedUser {
    let other = Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new("OTHER") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(other, Role::Viewer)])
}

/// An admin of everything, in no chat.
fn boss() -> VerifiedUser {
    VerifiedUser::for_tests(&Access::for_tests(), BOSS.parse().unwrap(), vec![(Held::Everywhere, Role::Admin)])
}

/// Who can read KILN: everyone but sam.
fn reads(p: &str) -> bool {
    p != SAM
}

/// KILN-1 (a scope) with KILN-2 … KILN-7 under it; OTHER-1 apart.
async fn kiln() -> Store {
    let store = Store::new();
    store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
    for _ in 2..=7 {
        store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    }
    store.create(SYS, super::tests_reach::new("OTHER", Kind::Scope, None, &[], "")).await.unwrap();
    store
}

async fn open(store: &Store, by: &str, ticket: &str, with: &[&str]) -> Result<(ChatRecord, bool), ChatError> {
    store.open_chat(SYS.permit::<Viewer>(ticket), Signer::named(by), with.iter().map(|s| s.to_string()).collect(), None, reads).await
}

async fn post(store: &Store, by: &str, chat: u64, body: &str) -> Result<Message, ChatError> {
    let ticket = store.tickets.read().unwrap().chats.by_id.get(&chat).map(|c| c.ticket.clone()).unwrap_or_else(|| "KILN-7".into());
    store.post_chat(SYS.permit::<Viewer>(&ticket), chat, Signer::named(by), body.into()).await
}

#[tokio::test]
async fn a_chat_is_opened_on_its_ticket_and_the_same_chat_again_is_that_chat() {
    let store = kiln().await;
    let mut rx = store.subscribe_chats();
    let (c, existing) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    assert!(!existing);
    assert_eq!((c.id, c.ticket.as_str(), c.state), (1, "KILN-7", ChatState::Open));
    assert_eq!(c.parts.iter().map(|p| p.principal.as_str()).collect::<Vec<_>>(), [ADA, BUILDER], "the opener first");
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Opened(c.clone()));
    // The same participants on the same ticket, in any order: chat 1, nothing made.
    let (again, existing) = open(&store, BUILDER, "KILN-7", &[ADA, ADA]).await.unwrap();
    assert!(existing);
    assert_eq!(again.id, 1);
    assert!(rx.try_recv().is_err(), "nothing written, nothing said");
    // Another set, or another ticket, is another chat.
    assert_eq!(open(&store, ADA, "KILN-7", &[REVIEWER]).await.unwrap().0.id, 2);
    assert_eq!(open(&store, ADA, "KILN-4", &[BUILDER]).await.unwrap().0.id, 3);
    // As ada lists them on KILN-7's page: hers there, each with her unread.
    let (rows, next) = store.chats(&reader(ADA), &ChatQuery { ticket: Some("kiln-7".into()), ..Default::default() }, None, 50).unwrap();
    assert_eq!(rows.iter().map(|c| c.id.get()).collect::<Vec<_>>(), [2, 1]);
    assert_eq!((next, rows[0].ticket.as_str(), rows[0].unread), (None, "KILN-7", 0));
}

/// CHAT-6: a chat may have only you in it — `others` empty, or naming only
/// the caller, is a chat of one; the same again is that chat; someone added
/// later must read the ticket; and waiting in it waits for whoever is added
/// (your own posts are no answer).
#[tokio::test]
async fn a_chat_of_one_is_opened_and_others_join_it_later() {
    let store = kiln().await;
    let mut rx = store.subscribe_chats();
    let (c, existing) = open(&store, ADA, "KILN-7", &[]).await.unwrap();
    assert!(!existing);
    assert_eq!(c.parts.iter().map(|p| p.principal.as_str()).collect::<Vec<_>>(), [ADA], "the caller alone");
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Opened(c.clone()));
    // Naming only yourself, or no one again: the same chat, nothing made.
    for with in [&[][..], &[ADA][..], &["  "][..]] {
        let (again, existing) = open(&store, ADA, "KILN-7", with).await.unwrap();
        assert_eq!((again.id, existing), (c.id, true), "{with:?}");
    }
    assert!(rx.try_recv().is_err(), "nothing written, nothing said");
    // Another person's chat of one on the same ticket is theirs.
    assert_ne!(open(&store, RUI, "KILN-7", &[]).await.unwrap().0.id, c.id);
    // A chat of one is listed on its ticket like any other, and holds the cell.
    let (rows, _) = store.chats(&reader(ADA), &ChatQuery { ticket: Some("KILN-7".into()), ..Default::default() }, None, 50).unwrap();
    assert_eq!(rows.iter().map(|r| r.id.get()).collect::<Vec<_>>(), [c.id]);
    assert_eq!(admitted(&store, ADA, "chatting"), ["KILN-7"]);
    // ada waits in it: her own post is no answer.
    let mine = post(&store, ADA, c.id, "notes to self: the kiln log first").await.unwrap();
    let wait = MessageWait { chat: Some(c.id), after: mine.id.get() - 1, ..Default::default() };
    assert_eq!(store.message_for(&reader(ADA), &wait), None, "your own posts don't count");
    // Adding: as ever, the one added must read the ticket.
    let e = store.add_to_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), SAM.into(), reads).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec![format!("{SAM} can't read KILN-7")]));
    let added = store.add_to_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), BUILDER.into(), reads).await.unwrap();
    assert_eq!(added.parts.len(), 2);
    let answer = post(&store, BUILDER, c.id, "reading it now").await.unwrap();
    assert_eq!(store.message_for(&reader(ADA), &wait), Some(answer), "whoever is added and posts");
    // Now a chat of two: ada's chat of one on KILN-7 is a new one.
    let (fresh, existing) = open(&store, ADA, "KILN-7", &[]).await.unwrap();
    assert!(!existing && fresh.id != c.id);
}

#[tokio::test]
async fn only_participants_read_a_chat() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    post(&store, ADA, c.id, "Is it the limit check or the test?").await.unwrap();
    // rui reads KILN and is not in chat 1: it is not there.
    let rui = reader(RUI);
    assert_eq!(store.chat_messages(&rui, c.id, None, 100).unwrap_err(), ChatError::NotFound(1));
    assert_eq!(store.chat_messages(&rui, 99, None, 100).unwrap_err(), ChatError::NotFound(99), "the same answer as none");
    assert!(store.chats(&rui, &ChatQuery { ticket: Some("KILN-7".into()), ..Default::default() }, None, 50).unwrap().0.is_empty());
    assert!(store.chat_view(&rui, c.id).is_none());
    // rui cannot post there either.
    let s = store.chat_seen(&rui, c.id);
    assert!(s.is_err());
    // The participants read it.
    for who in [ADA, BUILDER] {
        let (msgs, _) = store.chat_messages(&reader(who), c.id, None, 100).unwrap();
        assert_eq!(msgs.len(), 1, "{who}");
    }
    // An admin of the ticket reads it, and lists it on the ticket — but is
    // in it as no participant: no post, no mark.
    let b = boss();
    assert_eq!(store.chat_messages(&b, c.id, None, 100).unwrap().0.len(), 1);
    assert_eq!(store.chats(&b, &ChatQuery { ticket: Some("KILN-7".into()), ..Default::default() }, None, 50).unwrap().0.len(), 1);
    assert!(store.chats(&b, &ChatQuery::default(), None, 50).unwrap().0.is_empty(), "not its own chats");
    let refused = store.post_chat(SYS.permit("KILN-7"), c.id, Signer::named(BOSS), "hello".into()).await.unwrap_err();
    assert!(matches!(&refused, ChatError::Refused(r) if r[0].starts_with("you are not in chat 1")), "{refused:?}");
}

#[tokio::test]
async fn every_participant_can_read_the_ticket() {
    let store = kiln().await;
    let mut rx = store.subscribe_chats();
    // sam holds no grant covering KILN-7: refused, nothing made.
    let e = open(&store, ADA, "KILN-7", &[SAM, BUILDER]).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec![format!("{SAM} can't read KILN-7")]));
    assert!(rx.try_recv().is_err());
    assert!(store.tickets.read().unwrap().chats.by_id.is_empty());
    // Nor added later.
    let (c, _) = open(&store, ADA, "KILN-7", &[RUI]).await.unwrap();
    let e = store.add_to_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), SAM.into(), reads).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec![format!("{SAM} can't read KILN-7")]));
    // A grant revoked: rui, a participant, without its reach on KILN, has
    // lost the chat — and has it back when the reach is back.
    let before = reader(RUI);
    assert!(store.chat_view(&before, c.id).is_some());
    let revoked = outsider(RUI);
    assert!(store.chat_view(&revoked, c.id).is_none());
    assert_eq!(store.chat_messages(&revoked, c.id, None, 100).unwrap_err(), ChatError::NotFound(c.id));
    assert!(store.chats(&revoked, &ChatQuery::default(), None, 50).unwrap().0.is_empty());
    assert!(store.chat_view(&reader(RUI), c.id).is_some());
    // An add: ada adds builder; adding builder again is the chat as it is.
    let added = store.add_to_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), BUILDER.into(), reads).await.unwrap();
    assert_eq!(added.parts.len(), 3);
    let _ = rx.try_recv();
    assert!(matches!(rx.try_recv().unwrap(), ChatChange::Added(_)));
    let same = store.add_to_chat(SYS.permit("KILN-7"), c.id, Signer::named(RUI), BUILDER.into(), reads).await.unwrap();
    assert_eq!(same, added);
    assert!(rx.try_recv().is_err());
    // The permit must be for this chat's ticket.
    let wrong = store.add_to_chat(SYS.permit("KILN-4"), c.id, Signer::named(ADA), REVIEWER.into(), reads).await.unwrap_err();
    assert_eq!(wrong, ChatError::NotFound(c.id));
}

#[tokio::test]
async fn a_message_is_signed_by_its_caller() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    let mut rx = store.subscribe_chats();
    let signer = Signer { principal: BUILDER.into(), name: Some("builder".into()) };
    let m = store.post_chat(SYS.permit("KILN-7"), c.id, signer, "The test.".into()).await.unwrap();
    assert_eq!((m.id.get(), m.chat.get(), m.author.as_str(), m.name.as_deref(), m.via.as_deref(), m.body.as_str()), (1, 1, BUILDER, Some("builder"), None, "The test."));
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Posted(m.clone()), "to every participant's watch");
    // The poster has read its own; the other has one unread.
    assert_eq!(store.chat_view(&reader(BUILDER), c.id).unwrap().unread, 0);
    assert_eq!(store.chat_view(&reader(ADA), c.id).unwrap().unread, 1);
    // An empty body, or one past the cap, is refused and keeps nothing.
    let e = post(&store, ADA, c.id, "  \n").await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec!["the message's body is empty".into()]));
    let big = "x".repeat(store.body_max() + 1);
    assert!(matches!(post(&store, ADA, c.id, &big).await.unwrap_err(), ChatError::Refused(r) if r[0].contains("past the")));
    assert_eq!(store.chat_messages(&reader(ADA), c.id, None, 100).unwrap().0, [m]);
}

#[tokio::test]
async fn an_agent_waits_for_a_message_and_a_dropped_connection_loses_nothing() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    let (other, _) = open(&store, ADA, "KILN-4", &[BUILDER]).await.unwrap();
    let asked = post(&store, BUILDER, c.id, "Is 1240 allowed?").await.unwrap();
    let builder = reader(BUILDER);
    // builder waits from its own message: its own is no answer.
    let wait = MessageWait { chat: Some(c.id), since: Some(asked.at), ..Default::default() };
    assert_eq!(store.message_for(&builder, &wait), None);
    // A wait without `since` sets aside what was there.
    let fresh = MessageWait { after: store.newest_message(), ..Default::default() };
    let answer = post(&store, ADA, c.id, "1240 is allowed.").await.unwrap();
    assert_eq!(store.message_for(&builder, &wait), Some(answer.clone()));
    assert_eq!(store.message_for(&builder, &fresh), Some(answer.clone()), "any of builder's chats");
    // Dropped and back with the same `since`: the message is there at once.
    assert_eq!(store.message_for(&builder, &wait), Some(answer.clone()));
    // Narrowed by ticket: KILN-4's chat has nothing for builder yet.
    let on_four = MessageWait { ticket: Some("KILN-4".into()), after: 0, ..Default::default() };
    assert_eq!(store.message_for(&builder, &on_four), None);
    let there = post(&store, ADA, other.id, "and the shelf map?").await.unwrap();
    assert_eq!(store.message_for(&builder, &on_four), Some(there));
    // Not a participant: nothing, whatever it asks.
    assert_eq!(store.message_for(&reader(RUI), &MessageWait::default()), None);
    assert_eq!(store.message_for(&reader(RUI), &MessageWait { chat: Some(c.id), ..Default::default() }), None);
}

#[tokio::test]
async fn who_is_listening_is_shown_until_the_wait_ends() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    let mut rx = store.subscribe_chats();
    store.listen(&[c.id], BUILDER, true);
    store.listen(&[c.id], BUILDER, true);
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Listening { chat: c.id, principal: BUILDER.into(), waiting: true });
    assert!(rx.try_recv().is_err(), "a second wait of the same is no change");
    assert_eq!(store.chat_view(&reader(ADA), c.id).unwrap().waiting, [BUILDER]);
    store.listen(&[c.id], BUILDER, false);
    assert!(rx.try_recv().is_err());
    store.listen(&[c.id], BUILDER, false);
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Listening { chat: c.id, principal: BUILDER.into(), waiting: false });
    assert!(store.chat_view(&reader(ADA), c.id).unwrap().waiting.is_empty());
}

#[tokio::test]
async fn unread_is_kept_per_participant() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[REVIEWER]).await.unwrap();
    for body in ["one", "two", "three"] {
        post(&store, REVIEWER, c.id, body).await.unwrap();
    }
    let ada = reader(ADA);
    assert_eq!(store.chat_view(&ada, c.id).unwrap().unread, 3);
    let (unread, _) = store.chats(&ada, &ChatQuery { unread: Some(true), ..Default::default() }, None, 50).unwrap();
    assert_eq!(unread.len(), 1);
    let mut rx = store.subscribe_chats();
    // ada reads up to the second: one left.
    assert_eq!(store.read_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), 2).await.unwrap(), (2, 1));
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Read { chat: c.id, principal: ADA.into(), upto: 2 });
    // A mark never moves back, and past the last it is the last.
    assert_eq!(store.read_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), 1).await.unwrap(), (2, 1));
    assert!(rx.try_recv().is_err(), "unmoved: nothing written");
    assert_eq!(store.read_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), 99).await.unwrap(), (3, 0));
    assert!(store.chats(&ada, &ChatQuery { unread: Some(true), ..Default::default() }, None, 50).unwrap().0.is_empty());
    // reviewer's own count is its own.
    assert_eq!(store.chat_view(&reader(REVIEWER), c.id).unwrap().unread, 0);
    // An admin of the ticket has no mark.
    let e = store.read_chat(SYS.permit("KILN-7"), c.id, Signer::named(BOSS), 3).await.unwrap_err();
    assert!(matches!(e, ChatError::Refused(_)), "{e:?}");
}

#[tokio::test]
async fn a_chat_closes_read_only_and_reopens() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    let closed = store.set_chat_state(SYS.permit("KILN-7"), c.id, Signer::named(ADA), false).await.unwrap();
    assert_eq!(closed.state, ChatState::Closed);
    assert_eq!(post(&store, BUILDER, c.id, "one more thing").await.unwrap_err(), ChatError::Refused(vec!["chat 1 is closed".into()]));
    let e = store.add_to_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), RUI.into(), reads).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec!["chat 1 is closed".into()]));
    // Kept on its ticket, in the closed list.
    let (rows, _) = store.chats(&reader(ADA), &ChatQuery { ticket: Some("KILN-7".into()), state: Some(ChatState::Closed), ..Default::default() }, None, 50).unwrap();
    assert_eq!(rows.len(), 1);
    // A closed chat is not the open chat the same set finds: a new one opens.
    assert_eq!(open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap(), (store.tickets.read().unwrap().chats.by_id[&2].clone(), false));
    // Any participant reopens it; then it takes posts again.
    let reopened = store.set_chat_state(SYS.permit("KILN-7"), c.id, Signer::named(BUILDER), true).await.unwrap();
    assert_eq!(reopened.state, ChatState::Open);
    assert!(post(&store, BUILDER, c.id, "one more thing").await.is_ok());
    // Not a participant: refused.
    let e = store.set_chat_state(SYS.permit("KILN-7"), c.id, Signer::named(RUI), false).await.unwrap_err();
    assert!(matches!(e, ChatError::Refused(_)));
}

/// The tickets `filter` admits, as `me` reads it.
fn admitted(store: &Store, me: &str, filter: &str) -> Vec<String> {
    let lens = Lens::parse(filter).unwrap().with_clock(Clock { me: me.into(), ..Clock::default() });
    let ids: Vec<String> = (1..=7).map(|n| format!("KILN-{n}")).collect();
    let hit = store.member(SYS, &ids, &lens);
    ids.into_iter().zip(hit).filter(|(_, h)| *h).map(|(i, _)| i).collect()
}

#[tokio::test]
async fn the_filter_cells_find_tickets_by_the_reader_s_chats() {
    let store = kiln().await;
    let (seven, _) = open(&store, ADA, "KILN-7", &[REVIEWER]).await.unwrap();
    let (four, _) = open(&store, BUILDER, "KILN-4", &[ADA, RUI]).await.unwrap();
    let (two, _) = open(&store, ADA, "KILN-2", &[BUILDER]).await.unwrap();
    post(&store, ADA, seven.id, "Before this merges: does the cooling rate match?").await.unwrap();
    post(&store, BUILDER, four.id, "which shelf is cone 5?").await.unwrap();
    post(&store, BUILDER, two.id, "fixed").await.unwrap();
    post(&store, ADA, two.id, "thanks").await.unwrap();
    store.set_chat_state(SYS.permit("KILN-2"), two.id, Signer::named(ADA), false).await.unwrap();
    // What is waiting on ada: KILN-4 alone (her own on KILN-7 is read).
    assert_eq!(admitted(&store, ADA, "unread"), ["KILN-4"]);
    assert_eq!(admitted(&store, ADA, "chatting"), ["KILN-4", "KILN-7"], "KILN-2's chat is closed");
    // reviewer has KILN-7's question unread; rui KILN-4's; sam nothing.
    assert_eq!(admitted(&store, REVIEWER, "unread"), ["KILN-7"]);
    assert_eq!(admitted(&store, RUI, "unread and chatting"), ["KILN-4"]);
    assert!(admitted(&store, SAM, "unread or chatting").is_empty());
    assert_eq!(admitted(&store, ADA, "not unread and kind build").len(), 5);
    // The same mark everywhere it is read: ada reads KILN-4's, it leaves.
    let mut filtered = store.subscribe_filtered();
    let lens = Lens::parse("unread").unwrap().with_clock(Clock { me: ADA.into(), ..Clock::default() });
    let watcher = store.watcher(lens);
    store.read_chat(SYS.permit("KILN-4"), four.id, Signer::named(ADA), 99).await.unwrap();
    assert!(admitted(&store, ADA, "unread").is_empty());
    let mut heard = Vec::new();
    while let Ok(c) = filtered.try_change() {
        if let Change::UnderMoved { moves } = c {
            heard.extend(watcher.under_membership(&moves));
        }
    }
    assert_eq!(heard, [filtered::Membership::Left { id: "KILN-4".into(), shortcode: "KILN".into(), seq: 4 }]);
    // The words: a cell, alone, written back as read.
    assert_eq!(crate::filter::Filter::parse("Unread and CHATTING").unwrap().format(), "unread and chatting");
    for bad in ["unread today", "chatting by me", "unread 3"] {
        assert!(Lens::parse(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn the_chats_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
        store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
        let (c, _) = store.open_chat(SYS.permit("KILN-2"), Signer::named(ADA), vec![BUILDER.into()], Some(" the gate ".into()), reads).await.unwrap();
        store.post_chat(SYS.permit("KILN-2"), c.id, Signer::named(BUILDER), "one".into()).await.unwrap();
        store.post_chat(SYS.permit("KILN-2"), c.id, Signer::named(BUILDER), "two".into()).await.unwrap();
        store.read_chat(SYS.permit("KILN-2"), c.id, Signer::named(ADA), 1).await.unwrap();
        store.add_to_chat(SYS.permit("KILN-2"), c.id, Signer::named(ADA), RUI.into(), reads).await.unwrap();
        store.set_chat_state(SYS.permit("KILN-2"), c.id, Signer::named(ADA), false).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let view = store.chat_view(&reader(ADA), 1).unwrap();
    assert_eq!((view.title.as_deref(), view.state, view.unread, view.last_message.map(|m| m.get())), (Some("the gate"), ChatState::Closed, 1, Some(2)));
    assert_eq!(view.participants, [ADA, BUILDER, RUI]);
    assert_eq!(store.chat_view(&reader(RUI), 1).unwrap().unread, 2);
    let (msgs, next) = store.chat_messages(&reader(ADA), 1, Some(1), 100).unwrap();
    assert_eq!((msgs.len(), msgs[0].body.as_str(), next), (1, "two", None));
    // Ids go on from the file's.
    store.set_chat_state(SYS.permit("KILN-2"), 1, Signer::named(ADA), true).await.unwrap();
    assert_eq!(store.post_chat(SYS.permit("KILN-2"), 1, Signer::named(ADA), "three".into()).await.unwrap().id.get(), 3);
    assert_eq!(store.open_chat(SYS.permit("KILN-2"), Signer::named(ADA), vec![REVIEWER.into()], None, reads).await.unwrap().0.id, 2);
}

#[tokio::test]
async fn messages_and_chats_page() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    for n in 0..5 {
        post(&store, if n % 2 == 0 { ADA } else { BUILDER }, c.id, &format!("m{n}")).await.unwrap();
    }
    let ada = reader(ADA);
    let (first, next) = store.chat_messages(&ada, c.id, None, 2).unwrap();
    assert_eq!((first.iter().map(|m| m.body.as_str()).collect::<Vec<_>>(), next), (vec!["m0", "m1"], Some(2)));
    let (rest, next) = store.chat_messages(&ada, c.id, next, 10).unwrap();
    assert_eq!((rest.len(), next), (3, None));
    for t in ["KILN-2", "KILN-3", "KILN-4"] {
        open(&store, ADA, t, &[BUILDER]).await.unwrap();
    }
    // By last activity, newest first: chat 1 had its messages first.
    let (page, next) = store.chats(&ada, &ChatQuery::default(), None, 2).unwrap();
    assert_eq!(page.len(), 2);
    let (more, last) = store.chats(&ada, &ChatQuery::default(), next, 10).unwrap();
    assert_eq!((page.len() + more.len(), last), (4, None));
    assert!(matches!(store.chats(&ada, &ChatQuery::default(), Some(99), 10), Err(ChatError::Refused(_))));
}

/// CHAT-7: `/title` — a participant renames a chat; blank clears it; the
/// same title writes nothing; one outside it is refused.
#[tokio::test]
async fn a_participant_renames_a_chat() {
    let store = kiln().await;
    let (c, _) = open(&store, ADA, "KILN-7", &[BUILDER]).await.unwrap();
    let mut rx = store.subscribe_chats();
    let named = store.rename_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), Some("  The hold at 1240 °C ".into())).await.unwrap();
    assert_eq!(named.title.as_deref(), Some("The hold at 1240 °C"));
    assert!(matches!(rx.try_recv().unwrap(), ChatChange::Renamed(r) if r.id == c.id));
    let same = store.rename_chat(SYS.permit("KILN-7"), c.id, Signer::named(BUILDER), Some("The hold at 1240 °C".into())).await.unwrap();
    assert_eq!(same.title.as_deref(), Some("The hold at 1240 °C"));
    assert!(rx.try_recv().is_err(), "the same title: nothing written");
    let e = store.rename_chat(SYS.permit("KILN-7"), c.id, Signer::named(RUI), Some("mine".into())).await.unwrap_err();
    assert_eq!(e, ChatError::Refused(vec![format!("you are not in chat {}: only its participants rename it", c.id)]));
    let cleared = store.rename_chat(SYS.permit("KILN-7"), c.id, Signer::named(ADA), Some(" ".into())).await.unwrap();
    assert_eq!(cleared.title, None);
    assert_eq!(store.chat_view(&reader(ADA), c.id).unwrap().title, None);
}
