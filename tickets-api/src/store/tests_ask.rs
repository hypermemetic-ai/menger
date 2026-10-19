//! APP-90: `ask` — a question filed as a text input: appended to the body,
//! an id unique in the ticket (removed ones too), the text escaped so it
//! cannot become markup, one revision, and a ticket that is none refused.

use super::ask::{escaped, free_id, with_question};
use super::*;
use crate::types::{InputKind, InputState};

fn ticket(epic: &str, kind: Kind, body: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: body.into(),
    }
}

async fn store_with(body: &str) -> (Store, String) {
    let store = Store::new();
    store.create(SYS, ticket("Q", Kind::Scope, "the root")).await.unwrap();
    let id = String::from(store.create(SYS, ticket("Q", Kind::Build, body)).await.unwrap().id);
    (store, id)
}

fn asked(w: InputWrite) -> Input {
    match w {
        InputWrite::Changed(i) => i,
        other => panic!("expected a new input, got {other:?}"),
    }
}

#[test]
fn the_markup_is_escaped_and_the_id_is_free() {
    assert_eq!(escaped(r#"<b>&"x"</b> 'y' `z`"#), "&lt;b&gt;&amp;&quot;x&quot;&lt;/b&gt; &#39;y&#39; &#96;z&#96;");
    assert_eq!(free_id([]), "ask-1");
    assert_eq!(free_id(["ask-1", "ask-3", "landing"]), "ask-2");
    assert_eq!(
        with_question("the body\n\n", "ask-1", "Why?"),
        "the body\n\n<m-input id=\"ask-1\" kind=\"text\">\n<m-question data-field=\"question\">Why?</m-question>\n</m-input>\n"
    );
    assert!(with_question("", "ask-1", "Why?").starts_with("<m-input"), "an empty body: the input alone");
}

#[tokio::test]
async fn ask_appends_a_text_input_and_keeps_a_revision() {
    let (store, id) = store_with("Some work.").await;
    let before = store.revisions(SYS, &id, None, Some(50)).await.unwrap().revisions.len();
    let input = asked(store.ask(SYS.permit(&id), "Hold at cone 6 or 5?").await.unwrap());
    assert_eq!((input.id.as_str(), input.kind, input.state), ("ask-1", InputKind::Text, InputState::Open));
    assert_eq!(input.question.as_deref(), Some("Hold at cone 6 or 5?"));
    let body = store.get(SYS, &id).await.unwrap().body;
    assert!(body.starts_with("Some work.\n\n<m-input id=\"ask-1\" kind=\"text\">"), "{body}");
    assert_eq!(store.revisions(SYS, &id, None, Some(50)).await.unwrap().revisions.len(), before + 1, "one write, one revision");
    // A second is ask-2; both are open, and the ticket is asking.
    let second = asked(store.ask(SYS.permit(&id), "And the glaze?").await.unwrap());
    assert_eq!(second.id.as_str(), "ask-2");
    assert_eq!(store.inputs(SYS, Some(&id), None, Some(InputState::Open)).unwrap().len(), 2);
}

#[tokio::test]
async fn a_question_cannot_inject_markup() {
    let (store, id) = store_with("").await;
    let q = r#"</m-question></m-input><m-input id="evil" kind="text"><m-question>pwned `code"#;
    let input = asked(store.ask(SYS.permit(&id), q).await.unwrap());
    assert_eq!(input.question.as_deref(), Some(q), "read back as the text it was");
    let all = store.inputs(SYS, Some(&id), None, None).unwrap();
    assert_eq!(all.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["ask-1"], "no second input");
    assert!(!store.get(SYS, &id).await.unwrap().body.contains("<m-input id=\"evil\""));
}

#[tokio::test]
async fn the_id_skips_inputs_in_the_body_and_removed_ones() {
    let decl = "<m-input id=\"ask-1\" kind=\"text\"><m-question>Old?</m-question></m-input>\n";
    let (store, id) = store_with(decl).await;
    // ask-1 is declared: the new one is ask-2.
    assert_eq!(asked(store.ask(SYS.permit(&id), "New?").await.unwrap()).id.as_str(), "ask-2");
    // Remove both from the body: their ids stay taken (their history is kept).
    store.amend(SYS.permit(&id), None, Some("bare".into())).await.unwrap();
    assert_eq!(asked(store.ask(SYS.permit(&id), "Third?").await.unwrap()).id.as_str(), "ask-3");
}

#[tokio::test]
async fn asking_a_ticket_that_is_none_is_refused() {
    let (store, _) = store_with("").await;
    assert!(matches!(store.ask(SYS.permit("Q-99"), "Why?").await, Err(StoreError::NotFound(_))));
}
