//! CHAT-20: a message's inputs, as the store answers them — on the synthetic
//! project KILN: ada asks in a chat on KILN-7 with builder; rui reads KILN
//! and is in no chat. An input in a message is born at `post`, answered by
//! a participant (who, kept), cleared only by who answered, closed by who
//! asked; awaited by `answer_for`; the cell `asked` puts its ticket under
//! Needs you for the others in the chat; a queue entry names it as
//! `<message>/<input>` and its closing takes the entry out; a direct
//! message asks too, outside tickets; and it all survives a reopen.

use super::*;
use crate::access::{Access, Held, VerifiedUser, Viewer};
use crate::address::OrgId;
use crate::chats::ChatsEvent;
use crate::members::Role;
use crate::queue::Urgency;
use crate::types::{InputAction, TransitionSource};

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const BUILDER: &str = "idp:00000000-0000-4000-8000-0000000000b1";
const RUI: &str = "idp:00000000-0000-4000-8000-000000000041";

const ASKS: &str = r#"Before I fire it:

<m-decision data-field="decision" id="undated">
<m-question data-field="question">The undated firings: keep or drop?</m-question>
<m-option data-field="option" name="Keep" value="keep" recommended></m-option>
<m-option data-field="option" name="Drop" value="drop"></m-option>
</m-decision>

<m-input id="notes" kind="text"><m-question data-field="question">Anything else?</m-question></m-input>
"#;

fn reader(principal: &str) -> VerifiedUser {
    let kiln = Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new("KILN") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(kiln, Role::Viewer)])
}

fn who(principal: &str) -> Who {
    Who { source: AnswerSource::App, by: Some(principal.into()), name: None }
}

fn keep() -> ChatInputOp {
    ChatInputOp::Answer { value: InputValue::Choice { option: "keep".into() }, rationale: Some("the log holds them".into()) }
}

async fn kiln() -> Store {
    let store = Store::new();
    store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
    for _ in 2..=7 {
        store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    }
    store
}

/// ada's chat with builder on KILN-7, and her message asking.
async fn asked(store: &Store) -> (u64, u64) {
    let (c, _) = store.open_chat(SYS.permit::<Viewer>("KILN-7"), Signer::named(ADA), vec![BUILDER.into()], None, |_| true).await.unwrap();
    let m = store.post_chat(SYS.permit::<Viewer>("KILN-7"), c.id, Signer::named(ADA), ASKS.into()).await.unwrap();
    (c.id, m.id.get())
}

fn admitted(store: &Store, me: &str, filter: &str) -> Vec<String> {
    let lens = Lens::parse(filter).unwrap().with_clock(Clock { me: me.into(), ..Clock::default() });
    let ids: Vec<String> = (1..=7).map(|n| format!("KILN-{n}")).collect();
    let hit = store.member(SYS, &ids, &lens);
    ids.into_iter().zip(hit).filter(|(_, h)| *h).map(|(i, _)| i).collect()
}

fn changed(w: ChatInputWrite) -> crate::chat_inputs::ChatInput {
    match w {
        ChatInputWrite::Changed(i) => i,
        ChatInputWrite::Unchanged(i) => panic!("nothing moved: {i:?}"),
    }
}

#[tokio::test]
async fn a_message_s_inputs_are_born_at_post_and_listed_with_their_home() {
    let store = kiln().await;
    let (chat, message) = asked(&store).await;
    let born = store.message_inputs(message);
    assert_eq!(born.iter().map(|i| (i.id.as_str(), i.state)).collect::<Vec<_>>(), [("undated", InputState::Open), ("notes", InputState::Open)]);
    let first = &born[0];
    assert_eq!((first.chat.get(), first.message.get(), first.ticket.as_deref(), first.author.as_str()), (chat, message, Some("KILN-7"), ADA));
    assert_eq!(first.name(), format!("{message}/undated"));
    // Listed for a participant; not for rui, in no chat.
    let q = ChatInputQuery { state: Some(InputState::Open), ..Default::default() };
    assert_eq!(store.chat_inputs(&reader(BUILDER), &q, None, 100).unwrap().0.len(), 2);
    assert!(store.chat_inputs(&reader(RUI), &q, None, 100).unwrap().0.is_empty());
    // A malformed input refuses the post, naming why; nothing is kept.
    let bad = r#"<m-input id="x" kind="nope"><m-question data-field="question">?</m-question></m-input>"#;
    assert!(matches!(store.post_chat(SYS.permit::<Viewer>("KILN-7"), chat, Signer::named(ADA), bad.into()).await, Err(ChatError::Refused(_))));
}

#[tokio::test]
async fn a_participant_answers_and_who_answered_is_kept_and_only_they_clear_it() {
    let store = kiln().await;
    let (chat, message) = asked(&store).await;
    let mut rx = store.subscribe_chats();
    // rui is in no chat: refused, nothing written.
    let no = store.chat_input_write(&reader(RUI), who(RUI), chat, message, "undated", keep()).await;
    assert!(matches!(no, Err(ChatError::NotFound(_) | ChatError::Refused(_))), "{no:?}");
    let a = changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.unwrap());
    assert_eq!(a.state, InputState::Answered);
    let answer = a.answer.clone().unwrap();
    assert_eq!((answer.by.as_deref(), answer.source, answer.rationale.as_deref()), (Some(BUILDER), AnswerSource::App, Some("the log holds them")));
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Input(a.clone()));
    // A value that does not answer it is refused.
    let wrong = ChatInputOp::Answer { value: InputValue::Choice { option: "burn".into() }, rationale: None };
    assert!(matches!(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", wrong).await, Err(ChatError::Refused(_))));
    // ada did not give it: she does not clear it; builder does.
    assert!(matches!(store.chat_input_write(&reader(ADA), who(ADA), chat, message, "undated", ChatInputOp::Clear).await, Err(ChatError::Refused(_))));
    let c = changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", ChatInputOp::Clear).await.unwrap());
    assert_eq!((c.state, c.answer.is_none()), (InputState::Open, true));
    assert_eq!(c.history.iter().map(|e| (e.action, e.by.as_deref())).collect::<Vec<_>>(), [(InputAction::Answered, Some(BUILDER)), (InputAction::Cleared, Some(BUILDER))]);
}

#[tokio::test]
async fn who_asked_closes_and_reopens_and_a_closed_input_takes_no_answer() {
    let store = kiln().await;
    let (chat, message) = asked(&store).await;
    changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.unwrap());
    assert!(matches!(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", ChatInputOp::Close).await, Err(ChatError::Refused(_))), "builder did not ask");
    let closed = changed(store.chat_input_write(&reader(ADA), who(ADA), chat, message, "undated", ChatInputOp::Close).await.unwrap());
    assert_eq!(closed.state, InputState::Closed);
    assert!(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.is_err(), "closed");
    let again = changed(store.chat_input_write(&reader(ADA), who(ADA), chat, message, "undated", ChatInputOp::Reopen).await.unwrap());
    assert_eq!(again.state, InputState::Answered, "it still holds builder's answer");
}

#[tokio::test]
async fn an_answer_by_someone_else_answers_the_wait_of_who_asked() {
    let store = kiln().await;
    let (chat, message) = asked(&store).await;
    let since = chrono::Utc::now().timestamp() - 1;
    let any = AnswerWait { chat: Some(chat), input: None, since };
    let named = AnswerWait { chat: Some(chat), input: Some((message, "notes".into())), since };
    assert_eq!(store.answer_for(&reader(ADA), &any), None, "nothing answered yet");
    changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.unwrap());
    match store.answer_for(&reader(ADA), &any) {
        Some(ChatsEvent::Answered { input, by, .. }) => assert_eq!((input.as_str(), by.as_deref()), ("undated", Some(BUILDER))),
        other => panic!("{other:?}"),
    }
    // Waiting on `notes` alone: `undated`'s answer is no answer.
    assert_eq!(store.answer_for(&reader(ADA), &named), None);
    // builder waits on nothing of his: ada's questions are not his.
    assert_eq!(store.answer_for(&reader(BUILDER), &any), None);
}

#[tokio::test]
async fn asked_puts_the_ticket_under_needs_you_for_the_others_in_the_chat() {
    let store = kiln().await;
    let (chat, message) = asked(&store).await;
    assert_eq!(admitted(&store, BUILDER, "asked"), ["KILN-7"]);
    assert!(admitted(&store, ADA, "asked").is_empty(), "her own question waits on others");
    assert!(admitted(&store, RUI, "asked").is_empty(), "rui is in no chat");
    assert_eq!(admitted(&store, BUILDER, crate::saved_filters::NEEDS_YOU.1), ["KILN-7"]);
    // Answered, it no longer asks (one input still open: `notes`).
    changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.unwrap());
    assert_eq!(admitted(&store, BUILDER, "asked"), ["KILN-7"]);
    let text = ChatInputOp::Answer { value: InputValue::Text { text: "no".into() }, rationale: None };
    changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "notes", text).await.unwrap());
    assert!(admitted(&store, BUILDER, "asked").is_empty());
}

#[tokio::test]
async fn a_queue_entry_names_a_message_s_input_and_its_closing_unqueues_it() {
    let store = kiln().await;
    let (chat, message) = asked(&store).await;
    let act = Act { actor: ADA.into(), name: None, source: TransitionSource::Api, message: None, cause: None };
    let name = format!("{message}/undated");
    // One that is not in a chat on the ticket is refused.
    assert!(store.queue_by(SYS.permit("KILN-6"), Urgency::Blocking, "idle", Some(&name), act.clone()).await.is_err());
    assert!(store.queue_by(SYS.permit("KILN-7"), Urgency::Blocking, "idle", Some(&format!("{message}/nope")), act.clone()).await.is_err());
    store.queue_by(SYS.permit("KILN-7"), Urgency::Blocking, "idle until it is decided", Some(&name), act).await.unwrap();
    assert_eq!(store.queued(SYS, true, 0).len(), 1);
    changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.unwrap());
    assert_eq!(store.queued(SYS, true, 0).len(), 1, "an answer is not the close");
    changed(store.chat_input_write(&reader(ADA), who(ADA), chat, message, "undated", ChatInputOp::Close).await.unwrap());
    assert!(store.queued(SYS, true, 0).is_empty(), "closing it took the entry out");
}

#[tokio::test]
async fn a_direct_message_asks_outside_tickets() {
    let store = kiln().await;
    let (dm, _) = store.open_outside(&reader(ADA), OrgId::HYPERMEMETIC.into(), vec![RUI.into()], None, |_| true).await.unwrap();
    let on = store.chat_seen(&reader(ADA), dm.id).unwrap().1.on;
    let m = store.post_chat(on, dm.id, Signer::named(ADA), ASKS.into()).await.unwrap();
    let inputs = store.message_inputs(m.id.get());
    assert_eq!((inputs.len(), inputs[0].ticket.as_deref()), (2, None), "a direct message has no ticket");
    let a = changed(store.chat_input_write(&reader(RUI), who(RUI), dm.id, m.id.get(), "undated", keep()).await.unwrap());
    assert_eq!(a.answer.unwrap().by.as_deref(), Some(RUI));
    assert!(admitted(&store, RUI, "asked").is_empty(), "no ticket holds it");
}

#[tokio::test]
async fn a_message_s_inputs_and_their_answers_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let (chat, message) = {
        let store = Store::open(&path).unwrap();
        store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
        for _ in 2..=7 {
            store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
        }
        let (chat, message) = asked(&store).await;
        changed(store.chat_input_write(&reader(BUILDER), who(BUILDER), chat, message, "undated", keep()).await.unwrap());
        (chat, message)
    };
    let store = Store::open(&path).unwrap();
    let kept = store.message_inputs(message);
    assert_eq!(kept.iter().map(|i| (i.id.as_str(), i.state)).collect::<Vec<_>>(), [("undated", InputState::Answered), ("notes", InputState::Open)]);
    assert_eq!((kept[0].chat.get(), kept[0].answer.as_ref().and_then(|a| a.by.as_deref())), (chat, Some(BUILDER)));
    assert_eq!(kept[0].history.len(), 1);
}
