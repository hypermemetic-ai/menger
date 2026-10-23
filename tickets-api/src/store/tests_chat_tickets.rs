//! CHAT-55: a chat's tickets — every ticket a message names (outside a
//! fence), filed or linked from the chat; read through the chat and the
//! reader's reach, newest first, each in its stage (a status, an open
//! input, a deploy to test moving it); a deleted message's mentions drop out
//! and come back with it; `link` writes only what is new and only for a
//! participant; the cell `in chat N` for a reader in the chat; and a file
//! without the table back-filled from its messages.

use super::history::Clock;
use super::*;
use crate::access::{Access, Held, VerifiedUser, Viewer};
use crate::address::OrgId;
use crate::chat_tickets::{ChatStage, MentionHow};
use crate::deployments::Record;
use crate::members::Role;
use super::chats::Pick;

const ADA: &str = "idp:00000000-0000-4000-8000-0000000000ad";
const BUILDER: &str = "idp:00000000-0000-4000-8000-0000000000b1";
const RUI: &str = "idp:00000000-0000-4000-8000-000000000041";

/// `principal`, a viewer of KILN alone.
fn reader(principal: &str) -> VerifiedUser {
    let kiln = Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new("KILN") };
    VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), vec![(kiln, Role::Viewer)])
}

/// KILN-1 (a scope) with KILN-2 … KILN-7 under it; OTHER-1 apart.
async fn kiln(store: &Store) {
    store.create(SYS, super::tests_reach::new("KILN", Kind::Scope, None, &[], "")).await.unwrap();
    for _ in 2..=7 {
        store.create(SYS, super::tests_reach::new("KILN", Kind::Build, Some("KILN-1"), &[], "")).await.unwrap();
    }
    store.create(SYS, super::tests_reach::new("OTHER", Kind::Scope, None, &[], "")).await.unwrap();
}

/// Chat 1, on KILN-2, between ada and builder.
async fn chat(store: &Store) -> u64 {
    store.open_chat(SYS.permit::<Viewer>("KILN-2"), Signer::named(ADA), vec![BUILDER.into()], None, |_| true).await.unwrap().0.id
}

async fn post(store: &Store, by: &str, chat: u64, body: &str) {
    store.post_chat(SYS.permit::<Viewer>("KILN-2"), chat, Signer::named(by), body.into()).await.unwrap();
}

async fn link(store: &Store, by: &str, chat: u64, ids: &[&str], how: MentionHow) -> Result<Vec<String>, ChatError> {
    store.link_chat(SYS.permit::<Viewer>("KILN-2"), chat, Signer::named(by), ids.iter().map(|i| SYS.permit::<Viewer>(i)).collect(), how).await
}

/// The chat's tickets as `who` reads them: `id stage how`, newest first.
fn rows(store: &Store, who: &str, chat: u64) -> Vec<String> {
    store.chat_tickets(&reader(who), chat).unwrap().into_iter().map(|t| format!("{} {} {}", t.summary.id, t.stage.as_str(), t.how.as_str())).collect()
}

fn of(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| l.to_string()).collect()
}

#[tokio::test]
async fn a_chat_s_tickets_are_what_its_messages_name_and_what_is_filed_or_linked() {
    let store = Store::new();
    kiln(&store).await;
    let c = chat(&store).await;
    post(&store, ADA, c, "KILN-3 first, then KILN-4 (and OTHER-1).\n```\nKILN-5 failed\n```").await;
    post(&store, BUILDER, c, "KILN-4 again, and KILN-99, which is no ticket").await;
    // KILN-5 is only in the fence; OTHER-1 is out of ada's reach; KILN-99 is none.
    assert_eq!(rows(&store, ADA, c), of(&["KILN-3 waits_on_you mentioned", "KILN-4 waits_on_you mentioned"]));
    assert_eq!(store.chat_tickets(&reader(ADA), c).unwrap()[1].message.map(|m| m.get()), Some(1), "its first mention");
    // Filed and linked: new to it, newest first; one already there is not written again.
    let mut rx = store.subscribe_chats();
    assert_eq!(link(&store, BUILDER, c, &["KILN-6"], MentionHow::Filed).await.unwrap(), ["KILN-6"]);
    assert_eq!(rx.try_recv().unwrap(), ChatChange::Linked { chat: c, tickets: vec!["KILN-6".into()], how: MentionHow::Filed });
    assert_eq!(link(&store, ADA, c, &["KILN-3", "KILN-7", "KILN-7"], MentionHow::Linked).await.unwrap(), ["KILN-7"]);
    assert!(link(&store, ADA, c, &["KILN-3"], MentionHow::Linked).await.unwrap().is_empty());
    assert_eq!(rows(&store, ADA, c)[..2].to_vec(), of(&["KILN-7 waits_on_you linked", "KILN-6 waits_on_you filed"]));
    // Only a participant links; a chat the reader is not in is not found.
    assert!(matches!(link(&store, RUI, c, &["KILN-5"], MentionHow::Linked).await, Err(ChatError::Refused(_))));
    assert!(matches!(store.chat_tickets(&reader(RUI), c), Err(ChatError::NotFound(_))));
}

#[tokio::test]
async fn the_stage_moves_with_the_status_an_input_and_a_deploy() {
    let store = Store::new();
    kiln(&store).await;
    let c = chat(&store).await;
    post(&store, ADA, c, "KILN-3 KILN-4 KILN-5 KILN-6 KILN-7").await;
    store.set_status(SYS.permit("KILN-3"), Status::Ready).await.unwrap();
    store.set_status(SYS.permit("KILN-4"), Status::Coding).await.unwrap();
    store.set_status(SYS.permit("KILN-5"), Status::InReview).await.unwrap();
    store.set_status(SYS.permit("KILN-6"), Status::Coding).await.unwrap();
    let decision = "<m-decision data-field=\"decision\" id=\"go\">\n\n<m-question data-field=\"question\">Go?</m-question>\n\n<m-option data-field=\"option\" name=\"Yes\" value=\"yes\"></m-option>\n\n</m-decision>\n";
    store.amend(SYS.permit("KILN-6"), None, Some(decision.into())).await.unwrap();
    store.set_status(SYS.permit("KILN-7"), Status::Canceled).await.unwrap();
    let stage = |id: &str| store.chat_tickets(&reader(ADA), c).unwrap().into_iter().find(|t| t.summary.id.as_str() == id).map(|t| (t.stage, t.asking));
    assert_eq!(stage("KILN-3"), Some((ChatStage::Ready, 0)));
    assert_eq!(stage("KILN-4"), Some((ChatStage::InProgress, 0)));
    assert_eq!(stage("KILN-5"), Some((ChatStage::InReview, 0)));
    assert_eq!(stage("KILN-6"), Some((ChatStage::WaitsOnYou, 1)), "an open input waits on you");
    assert_eq!(stage("KILN-7"), Some((ChatStage::Closed, 0)), "kept, drawn by no pill");
    // KILN-5's commit goes to test: on test; then to prod, Complete: shipped.
    store.link_commits(SYS.permit("KILN-5"), vec!["5555555".into()]).await.unwrap();
    let rec = |env: &str| Record { project: "kiln".into(), environment: env.into(), state: "succeeded".into(), revision: "5555555".into(), commits: vec!["5555555".into()], from: (env == "prod").then(|| "test".to_string()), ..Record::default() };
    store.set_promotes(SYS, "kiln", "prod", Some("test"), "owner".into()).await.unwrap();
    store.record_deployment(SYS, rec("test"), "ci".into()).await.unwrap();
    assert_eq!(stage("KILN-5"), Some((ChatStage::OnTest, 0)));
    store.set_status(SYS.permit("KILN-5"), Status::Complete).await.unwrap();
    assert_eq!(stage("KILN-5"), Some((ChatStage::OnTest, 0)), "ready for prod");
    store.record_deployment(SYS, rec("prod"), "ci".into()).await.unwrap();
    assert_eq!(stage("KILN-5"), Some((ChatStage::Shipped, 0)));
}

#[tokio::test]
async fn a_deleted_message_s_tickets_drop_out_and_come_back_with_it() {
    let store = Store::new();
    kiln(&store).await;
    let c = chat(&store).await;
    post(&store, ADA, c, "KILN-3").await;
    post(&store, BUILDER, c, "KILN-4, and KILN-3").await;
    let del = |restore| store.delete_messages(SYS.permit::<Viewer>("KILN-2"), c, Signer::named(BUILDER), Pick::Ids(vec![2]), restore);
    del(false).await.unwrap();
    assert_eq!(rows(&store, ADA, c), of(&["KILN-3 waits_on_you mentioned"]), "KILN-3 is still named by ada's");
    del(true).await.unwrap();
    assert_eq!(rows(&store, ADA, c), of(&["KILN-4 waits_on_you mentioned", "KILN-3 waits_on_you mentioned"]));
}

#[tokio::test]
async fn the_cell_in_chat_n_answers_a_chat_s_tickets_for_a_reader_in_it() {
    let store = Store::new();
    kiln(&store).await;
    let c = chat(&store).await;
    post(&store, ADA, c, "KILN-3 and KILN-5").await;
    link(&store, ADA, c, &["KILN-7"], MentionHow::Linked).await.unwrap();
    let admitted = |me: &str, filter: &str| {
        let lens = Lens::parse(filter).unwrap().with_clock(Clock { me: me.into(), ..Clock::default() });
        let ids: Vec<String> = (1..=7).map(|n| format!("KILN-{n}")).collect();
        let hit = store.member(SYS, &ids, &lens);
        ids.into_iter().zip(hit).filter(|(_, h)| *h).map(|(i, _)| i).collect::<Vec<_>>()
    };
    assert_eq!(admitted(ADA, "in chat 1"), ["KILN-3", "KILN-5", "KILN-7"]);
    store.set_status(SYS.permit("KILN-5"), Status::Ready).await.unwrap();
    assert_eq!(admitted(BUILDER, "in chat 1 and not status Draft"), ["KILN-5"]);
    assert!(admitted(RUI, "in chat 1").is_empty(), "rui is not in it");
    assert!(admitted(ADA, "in chat 2").is_empty());
    assert_eq!(admitted(ADA, "in KILN and not in chat 1").len(), 4);
    // The words, written back as read; a bad number refused (`in chat` alone is the shortcode CHAT).
    assert_eq!(crate::filter::Filter::parse("In Chat 1 and in KILN").unwrap().format(), "in chat 1 and in KILN");
    for bad in ["in chat 0", "in chat x", "in chat 1 today"] {
        assert!(Lens::parse(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn a_file_without_the_table_is_back_filled_from_its_messages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        kiln(&store).await;
        let c = chat(&store).await;
        post(&store, ADA, c, "KILN-3 then KILN-4").await;
        link(&store, ADA, c, &["KILN-6"], MentionHow::Filed).await.unwrap();
    }
    // Kept across a reopen.
    assert_eq!(rows(&Store::open(&path).unwrap(), ADA, 1), of(&["KILN-6 waits_on_you filed", "KILN-3 waits_on_you mentioned", "KILN-4 waits_on_you mentioned"]));
    // A file from before CHAT-55: the messages' mentions come back, a link does not (it was never written).
    rusqlite::Connection::open(&path).unwrap().execute_batch("DROP TABLE chat_mentions").unwrap();
    assert_eq!(rows(&Store::open(&path).unwrap(), ADA, 1), of(&["KILN-3 waits_on_you mentioned", "KILN-4 waits_on_you mentioned"]));
}
