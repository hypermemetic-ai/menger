//! TETHER-9: what tether tells a session — how to reach the tracker and how
//! to ask (file it, end the turn: tether resumes it with the answer), then
//! the work: a run's brief, an answer, a change request, a comment.

use menger_tickets_api::{Answer, Comment, CommentIntent, Input, InputValue};

use crate::remote::Remote;

/// The first turn's preamble: who started it, where the tracker is, and
/// how a question comes back.
pub fn preamble(remote: Option<&Remote>, ticket: &str) -> String {
    let reach = match remote {
        Some(r) => format!(
            "The tracker is {url}; reach it with `synapse --url {url} --as {agent} menger …` (read \
             ~/dev/controlflow/hypermemetic/menger-skills/reaching/SKILL.md first).",
            url = r.url(),
            agent = r.agent()
        ),
        None => "Reach the tracker with synapse as menger-skills `reaching` says.".to_string(),
    };
    format!(
        "You are a session menger tether runs on this machine, working ticket {ticket}. {reach} \
         When you need the owner, file the question on the ticket as an input (menger AGENTS.md \
         \"Inputs\"; queue it when it cannot wait), then end your turn — do not wait with \
         await_reply: tether listens for the answer and resumes this session with it.\n\n"
    )
}

fn value(v: &InputValue) -> String {
    match v {
        InputValue::Choice { option } => format!("`{option}`"),
        InputValue::Multi { options } => options.iter().map(|o| format!("`{o}`")).collect::<Vec<_>>().join(", "),
        InputValue::Text { text } => format!("\n\n> {}\n", text.replace('\n', "\n> ")),
    }
}

/// An input answered.
pub fn answered(input: &Input, answer: &Answer, remote: Option<&Remote>) -> String {
    let (ticket, id) = (input.ticket.as_str(), input.id.as_str());
    let question = input.question.as_deref().map(|q| format!(" (\"{q}\")")).unwrap_or_default();
    let why = answer.rationale.as_deref().map(|r| format!("\nTheir rationale: {r}")).unwrap_or_default();
    let close = match remote {
        Some(r) => format!("`synapse --url {} --as {} menger tickets inputs close --ticket {ticket} --input {id}`", r.url(), r.agent()),
        None => format!("`tickets inputs close --ticket {ticket} --input {id}`"),
    };
    format!(
        "The owner answered input `{id}` on {ticket}{question}: {}{why}\n\nAct on the answer, then close the input: {close}.",
        value(&answer.value)
    )
}

/// A comment on a ticket tether cares about: a change request (APP-23) or
/// a reply on a queued ticket.
pub fn replied(ticket: &str, c: &Comment) -> String {
    let body = c.body.replace('\n', "\n> ");
    match c.intent {
        CommentIntent::Changes => format!(
            "The owner requested changes on {ticket} (comment {}): the ticket is out of the queue.\n\n> {body}\n\nMake them.",
            c.id
        ),
        _ => format!("{} commented on {ticket} (comment {}):\n\n> {body}\n\nRead it, and act on it if it asks something of you.", c.author, c.id),
    }
}

/// The work for a session the listener starts itself (no session of
/// tether's asked): another agent may be on it, so it looks first.
pub fn unheld(work: String) -> String {
    format!(
        "No session of tether's asked this, so another agent may have — and may be acting on it now. \
         Read the ticket first (its timeline and status, the input's state); if someone is already on \
         it, end your turn saying so.\n\n{work}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_says_what_and_how_to_close() {
        let input: Input = serde_json::from_value(serde_json::json!({
            "ticket": "KILN-4", "id": "landing", "kind": "choice", "question": "Archive or delete?",
            "options": [], "recommendation": null, "state": "answered", "answer": null, "history": []
        }))
        .unwrap();
        let a: Answer = serde_json::from_value(serde_json::json!({
            "value": { "type": "choice", "option": "archive" }, "rationale": "holds are real", "source": "app", "at_unix": 5
        }))
        .unwrap();
        let p = answered(&input, &a, None);
        assert!(p.contains("`landing` on KILN-4") && p.contains("`archive`") && p.contains("holds are real") && p.contains("inputs close --ticket KILN-4 --input landing"), "{p}");
    }
}
