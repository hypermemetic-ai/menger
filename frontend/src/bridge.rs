//! COMP-5: **the bridge** — the host's side of a frame's `menger.call`
//! (COMP-1 pin 9: read-only).
//!
//! A frame cannot reach the app's WebSocket (its origin is opaque); it asks
//! the host instead: `menger:call { id, method, params }` over postMessage
//! (src/frame.rs checks the sender and its channel). The host answers over
//! the app's own connection, with the typed client the rest of the app uses
//! — never a raw call: [`parse_call`] turns the method and its params into a
//! [`Call`], one of the read methods with the activation's own
//! parameter names, or refuses it; [`run`] makes it. The answer is the
//! stream's items — every data event, an in-band `error` included, as the
//! wire spells them — or, when the call never got an answer (no
//! connection, a refused request), the reason, which the SDK turns into a
//! rejected promise.
//!
//! Read-only by construction: a method outside [`READ_METHODS`] is refused
//! here, before anything is sent — `set_status`, `amend`, `create`, every
//! other write, `answer` and `clear_answer` included, and `watch` (a frame
//! hears the stream as change notices, not by subscribing).
//!
//! INPUT-4: **the first write.** A frame records an answer to one of its own
//! ticket's inputs — `menger.input.submit(id, value, rationale?)`, sent as
//! `menger:submit { id, params: { input, value, rationale? } }` — and never
//! through `menger.call`. The host makes `tickets.answer` for it, with
//! source `app`, only when every check here passes ([`admit`],
//! [`parse_submit`], [`check_submit`]): a viewer's gesture is under way (the
//! app's own transient activation, which a click or a key in a frame grants
//! its ancestors); the frame renders a ticket's stored body (the host
//! stamped which on the frame — the frame cannot name one); the input is
//! declared there, and neither closed nor removed; the value answers it
//! (`inputs::check`, what the server runs). A refusal is the frame's
//! rejected promise, and nothing is sent; what the server answers
//! ([`recorded`]) is the recorded answer, or its own in-band refusal.
//!
//! INPUT-9: **the second write.** `menger.input.clear(id)`, sent as
//! `menger:clear { id, params: { input } }`, clears the answer recorded on
//! one of the frame's own ticket's inputs: `tickets.clear_answer` with
//! source `app`, under the same checks as a submit but for the value
//! ([`admit`], [`parse_clear`], [`check_clear`]) — a gesture under way, a
//! frame that renders a ticket, the input declared there and neither
//! closed nor removed. The answer ([`cleared`]) is the input as the clear
//! left it, or the server's refusal. Every other write stays refused.

use menger_tickets_api::{inputs, Answer, AnswerSource, Input, InputState, InputValue, TicketsEvent};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::session::Client;

/// What a frame may call (COMP-1 pin 9) — the activation's read methods;
/// INPUT-4: `inputs` among them.
pub(crate) const READ_METHODS: [&str; 10] =
    ["get", "list", "find", "waves", "groups", "extents", "blockers", "health", "components", "inputs"];

/// INPUT-4: the SDK call a frame's answer is made by — named in every
/// refusal, so a component author finds it.
pub(crate) const SUBMIT: &str = "menger.input.submit";

/// INPUT-9: the SDK call a frame's clear is made by — its second write.
pub(crate) const CLEAR: &str = "menger.input.clear";

/// One read call, with the activation's parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Get { id: String },
    List { epic: Option<String>, status: Option<String>, parent: Option<String>, after: Option<String>, limit: Option<u32>, summary: Option<bool> },
    Find { query: String, after: Option<String>, limit: Option<u32> },
    Waves { epics: Vec<String>, kinds: Option<Vec<String>> },
    Groups,
    Extents,
    Blockers { id: String },
    Health,
    Components,
    /// INPUT-4.
    Inputs { ticket: Option<String>, epic: Option<String>, state: Option<String> },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    epic: Option<String>,
    status: Option<String>,
    parent: Option<String>,
    after: Option<String>,
    limit: Option<u32>,
    summary: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindParams {
    query: String,
    after: Option<String>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WavesParams {
    epics: Vec<String>,
    kinds: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputsParams {
    ticket: Option<String>,
    epic: Option<String>,
    state: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

/// The refusal for anything but a read — which names the two writes a
/// frame may make, and how (INPUT-4, INPUT-9).
pub(crate) fn read_only(method: &str) -> String {
    format!(
        "read-only: a frame may call only {} — not `{method}`; its only writes are `answer` and `clear_answer`, \
         made by {SUBMIT} and {CLEAR} on a viewer's click",
        READ_METHODS.join(", ")
    )
}

/// `method` with `params` (JSON — the frame's object as `JSON.stringify`
/// wrote it; `null` or nothing for none) as a [`Call`], or why not: a
/// method that is not a read ([`read_only`]), or params the method does not
/// take — an unknown name, a missing one, a wrong type, each named.
pub(crate) fn parse_call(method: &str, params: &str) -> Result<Call, String> {
    if !READ_METHODS.contains(&method) {
        return Err(read_only(method));
    }
    let params = match params.trim() {
        "" | "null" => "{}",
        p => p,
    };
    // The activation's params are named: an object, never a list or a value.
    if !params.starts_with('{') {
        return Err(format!("{method}: params must be an object, e.g. {{ id: \"MNG-2\" }}"));
    }
    fn take<T: DeserializeOwned>(method: &str, params: &str) -> Result<T, String> {
        serde_json::from_str(params).map_err(|e| format!("{method}: {e}"))
    }
    Ok(match method {
        "get" => Call::Get { id: take::<IdParams>(method, params)?.id },
        "list" => {
            let p: ListParams = take(method, params)?;
            Call::List { epic: p.epic, status: p.status, parent: p.parent, after: p.after, limit: p.limit, summary: p.summary }
        }
        "find" => {
            let p: FindParams = take(method, params)?;
            Call::Find { query: p.query, after: p.after, limit: p.limit }
        }
        "waves" => {
            let p: WavesParams = take(method, params)?;
            Call::Waves { epics: p.epics, kinds: p.kinds }
        }
        "blockers" => Call::Blockers { id: take::<IdParams>(method, params)?.id },
        "inputs" => {
            let p: InputsParams = take(method, params)?;
            Call::Inputs { ticket: p.ticket, epic: p.epic, state: p.state }
        }
        other => {
            take::<NoParams>(other, params)?;
            match other {
                "groups" => Call::Groups,
                "extents" => Call::Extents,
                "health" => Call::Health,
                _ => Call::Components,
            }
        }
    })
}

/// Make the call over the app's connection: the stream's items, or why
/// there are none.
pub(crate) async fn run(client: &Client, call: Call) -> Result<Vec<TicketsEvent>, String> {
    let answer = match call {
        Call::Get { id } => client.get_all(id).await,
        Call::List { epic, status, parent, after, limit, summary } => {
            client.list_all(epic, status, parent, after, limit, summary, None, None).await
        }
        Call::Find { query, after, limit } => client.find_all(query, after, limit).await,
        Call::Waves { epics, kinds } => client.waves_all(epics, kinds).await,
        Call::Groups => client.groups_all().await,
        Call::Extents => client.extents_all().await,
        Call::Blockers { id } => client.blockers_all(id).await,
        Call::Health => client.health_all().await,
        Call::Components => client.components_all().await,
        Call::Inputs { ticket, epic, state } => client.inputs_all(ticket, epic, state).await,
    };
    answer.map_err(|e| crate::session::describe(&e))
}

// -- INPUT-4: the first write -----------------------------------------------

/// `menger.input.submit(id, value, rationale?)` as the host reads it. No
/// ticket: a frame answers only the ticket whose body it renders, which the
/// host knows and the frame cannot name.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Submit {
    /// The input's id in the ticket's body.
    pub(crate) input: String,
    pub(crate) value: InputValue,
    #[serde(default)]
    pub(crate) rationale: Option<String>,
}

/// Whether a write — `sdk`, [`SUBMIT`] or (INPUT-9) [`CLEAR`], which every
/// refusal names — may go on at all, before its params are read: a viewer's
/// gesture is under way — `activated`, the app window's transient user
/// activation (`None`: this browser cannot say, and the SDK's own check
/// stands alone) — and the frame renders a ticket's stored body (`ticket`,
/// the one the host stamped on the frame; none for an editor's preview or a
/// help card). The ticket, or why not.
pub(crate) fn admit(sdk: &str, ticket: Option<&str>, activated: Option<bool>) -> Result<String, String> {
    if activated == Some(false) {
        return Err(format!("{sdk}: only inside a viewer's click — the app saw no click or key just now (nothing sent)"));
    }
    match ticket.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => Ok(t.to_string()),
        None => Err(format!(
            "{sdk}: this frame shows no ticket's stored body (an editor's preview, a help card) — it has no input to answer (nothing sent)"
        )),
    }
}

/// The frame's params (JSON, as for a call) as a [`Submit`], or why not —
/// an unknown field (`ticket` included), a missing one, a value that is not
/// an [`InputValue`], each named.
pub(crate) fn parse_submit(params: &str) -> Result<Submit, String> {
    serde_json::from_str(params.trim()).map_err(|e| {
        format!(
            "{SUBMIT}: {e} — it takes (id, value, rationale?), a value {{type: \"choice\", option}} | \
             {{type: \"multi\", options}} | {{type: \"text\", text}} (nothing sent)"
        )
    })
}

/// An `inputs { ticket }` answer, read for `sdk`'s write: the ticket's
/// inputs, or what the server said instead.
pub(crate) fn inputs_of(sdk: &str, events: Vec<TicketsEvent>) -> Result<Vec<Input>, String> {
    let mut held = Vec::new();
    for ev in events {
        match ev {
            TicketsEvent::Input { input } => held.push(input),
            TicketsEvent::NotFound { id } => return Err(format!("{sdk}: {id} is not a ticket (nothing sent)")),
            TicketsEvent::Error { message } => return Err(format!("{sdk}: {message} (nothing sent)")),
            _ => {}
        }
    }
    Ok(held)
}

/// The checks both writes share, against `held` — the inputs the store
/// holds for `ticket`, the frame's: the input `id` is declared in its body
/// (one another ticket declares is not this frame's to write), it is not
/// removed from it, and not closed. The input, or why not, in the server's
/// words, naming `sdk`.
fn open_input<'a>(sdk: &str, ticket: &str, id: &str, held: &'a [Input]) -> Result<&'a Input, String> {
    let mine: Vec<&Input> = held.iter().filter(|i| i.ticket == ticket).collect();
    let Some(input) = mine.iter().find(|i| i.id == id) else {
        let declared: Vec<String> =
            mine.iter().filter(|i| i.state != InputState::Removed).map(|i| format!("'{}'", i.id)).collect();
        return Err(if declared.is_empty() {
            format!("{sdk}: {ticket} declares no input '{id}' — its body holds none (nothing sent)")
        } else {
            format!("{sdk}: {ticket} declares no input '{id}' — its inputs are {} (nothing sent)", declared.join(", "))
        });
    };
    match input.state {
        InputState::Removed => Err(format!("{sdk}: {ticket}'s '{id}' was removed from its body (nothing sent)")),
        InputState::Closed => {
            Err(format!("{sdk}: {ticket}'s '{id}' is closed: its answer is frozen until it is reopened (nothing sent)"))
        }
        InputState::Open | InputState::Answered => Ok(input),
    }
}

/// The host's own checks for a submit ([`open_input`]), and `submit.value`
/// answers the input — the value to send (a `multi`'s options in the
/// declared order), or why not, in the server's words.
pub(crate) fn check_submit(ticket: &str, submit: &Submit, held: &[Input]) -> Result<InputValue, String> {
    let id = submit.input.as_str();
    let input = open_input(SUBMIT, ticket, id, held)?;
    inputs::check(input.kind, &input.options, submit.value.clone())
        .map_err(|why| format!("{SUBMIT}: not an answer to {ticket}'s '{id}': {why} (nothing sent)"))
}

/// `tickets.answer`'s reply: the answer it recorded, or its refusal.
pub(crate) fn recorded(events: Vec<TicketsEvent>) -> Result<Answer, String> {
    for ev in events {
        match ev {
            TicketsEvent::InputChanged { input } | TicketsEvent::Input { input } => {
                return input.answer.ok_or_else(|| format!("{SUBMIT}: {}'s '{}' holds no answer", input.ticket, input.id))
            }
            TicketsEvent::NotFound { id } => return Err(format!("{SUBMIT}: {id} is not a ticket")),
            TicketsEvent::Error { message } => return Err(format!("{SUBMIT}: {message}")),
            _ => {}
        }
    }
    Err(format!("{SUBMIT}: the server recorded no answer"))
}

/// Answer `submit` against `ticket`'s inputs over the app's connection: the
/// store's inputs for it, the checks ([`check_submit`]), then
/// `tickets.answer` with source `app` — the recorded answer, or why not.
pub(crate) async fn submit(client: &Client, ticket: String, submit: Submit) -> Result<Answer, String> {
    let described = |e: plexus_client::ClientError| format!("{SUBMIT}: {}", crate::session::describe(&e));
    let held = inputs_of(SUBMIT, client.inputs_all(Some(ticket.clone()), None, None).await.map_err(described)?)?;
    let value = check_submit(&ticket, &submit, &held)?;
    let source = Some(AnswerSource::App.as_str().to_string());
    recorded(client.answer_all(ticket, submit.input, value, submit.rationale, source).await.map_err(described)?)
}

// -- INPUT-9: the second write ----------------------------------------------

/// `menger.input.clear(id)` as the host reads it — the input only; no
/// ticket, as for a submit.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Clear {
    /// The input's id in the ticket's body.
    pub(crate) input: String,
}

/// The frame's params (JSON) as a [`Clear`], or why not — an unknown field
/// (`ticket` included), a missing or mistyped `input`, each named.
pub(crate) fn parse_clear(params: &str) -> Result<Clear, String> {
    serde_json::from_str(params.trim()).map_err(|e| format!("{CLEAR}: {e} — it takes (id) (nothing sent)"))
}

/// The host's own checks for a clear: [`open_input`] — declared in the
/// frame's ticket, neither removed nor closed. There is no value to check.
pub(crate) fn check_clear(ticket: &str, clear: &Clear, held: &[Input]) -> Result<(), String> {
    open_input(CLEAR, ticket, &clear.input, held).map(|_| ())
}

/// `tickets.clear_answer`'s reply: the input as the clear left it (`open`,
/// the clear in its history; as it was when it held no answer), or the
/// server's refusal.
pub(crate) fn cleared(events: Vec<TicketsEvent>) -> Result<Input, String> {
    for ev in events {
        match ev {
            TicketsEvent::InputChanged { input } | TicketsEvent::Input { input } => return Ok(input),
            TicketsEvent::NotFound { id } => return Err(format!("{CLEAR}: {id} is not a ticket")),
            TicketsEvent::Error { message } => return Err(format!("{CLEAR}: {message}")),
            _ => {}
        }
    }
    Err(format!("{CLEAR}: the server cleared nothing"))
}

/// Clear `clear` against `ticket`'s inputs over the app's connection: the
/// store's inputs for it, the checks ([`check_clear`]), then
/// `tickets.clear_answer` with source `app` — the input, or why not.
pub(crate) async fn clear(client: &Client, ticket: String, clear: Clear) -> Result<Input, String> {
    let described = |e: plexus_client::ClientError| format!("{CLEAR}: {}", crate::session::describe(&e));
    let held = inputs_of(CLEAR, client.inputs_all(Some(ticket.clone()), None, None).await.map_err(described)?)?;
    check_clear(&ticket, &clear, &held)?;
    let source = Some(AnswerSource::App.as_str().to_string());
    cleared(client.clear_answer_all(ticket, clear.input, source).await.map_err(described)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COMP-1 pin 9: the reads — INPUT-4: ten, `inputs` among them — each
    /// with the activation's parameter names, and nothing else.
    #[test]
    fn a_frame_may_make_the_ten_reads() {
        assert_eq!(parse_call("get", r#"{"id":"MNG-2"}"#), Ok(Call::Get { id: "MNG-2".into() }));
        assert_eq!(
            parse_call("list", r#"{"epic":"MNG","limit":5,"summary":true}"#),
            Ok(Call::List { epic: Some("MNG".into()), status: None, parent: None, after: None, limit: Some(5), summary: Some(true) })
        );
        assert_eq!(
            parse_call("list", "null"),
            Ok(Call::List { epic: None, status: None, parent: None, after: None, limit: None, summary: None })
        );
        assert_eq!(
            parse_call("find", r#"{"query":"wave"}"#),
            Ok(Call::Find { query: "wave".into(), after: None, limit: None })
        );
        assert_eq!(
            parse_call("waves", r#"{"epics":["PLEXUS"]}"#),
            Ok(Call::Waves { epics: vec!["PLEXUS".into()], kinds: None })
        );
        assert_eq!(
            parse_call("waves", r#"{"epics":["A","B"],"kinds":["build"]}"#),
            Ok(Call::Waves { epics: vec!["A".into(), "B".into()], kinds: Some(vec!["build".into()]) })
        );
        assert_eq!(parse_call("blockers", r#"{"id":"MNG-4"}"#), Ok(Call::Blockers { id: "MNG-4".into() }));
        assert_eq!(parse_call("groups", "{}"), Ok(Call::Groups));
        assert_eq!(parse_call("extents", ""), Ok(Call::Extents));
        assert_eq!(parse_call("health", "null"), Ok(Call::Health));
        assert_eq!(parse_call("components", "{}"), Ok(Call::Components));
        assert_eq!(parse_call("inputs", "null"), Ok(Call::Inputs { ticket: None, epic: None, state: None }));
        assert_eq!(
            parse_call("inputs", r#"{"ticket":"INPUT-9","epic":"INPUT","state":"open"}"#),
            Ok(Call::Inputs { ticket: Some("INPUT-9".into()), epic: Some("INPUT".into()), state: Some("open".into()) })
        );
        for m in READ_METHODS {
            let params = match m {
                "get" | "blockers" => r#"{"id":"X-1"}"#,
                "find" => r#"{"query":""}"#,
                "waves" => r#"{"epics":["X"]}"#,
                _ => "{}",
            };
            assert!(parse_call(m, params).is_ok(), "{m}");
        }
    }

    /// Every write — `answer` and the other input writes included: a frame
    /// answers only through `menger.input.submit`, and (INPUT-9) clears
    /// only through `menger.input.clear` — and `watch`, `ready_set`, a name
    /// that is no method, is refused before anything is sent, saying
    /// "read-only", what is allowed, and the two writes and how they are
    /// made.
    #[test]
    fn anything_else_is_read_only() {
        for m in [
            "set_status", "amend", "create", "set_parent", "link_commits", "unlink_commits", "watch", "ready_set", "answer",
            "clear_answer", "close_input", "reopen_input", "", "GET", "menger.tickets.get", "submit", "menger.input.submit",
            "clear", "menger.input.clear",
        ] {
            let err = parse_call(m, "{}").unwrap_err();
            assert!(err.starts_with("read-only: "), "{m}: {err}");
            assert!(err.contains("get, list, find, waves, groups, extents, blockers, health, components, inputs"), "{err}");
            assert!(
                err.contains(
                    "its only writes are `answer` and `clear_answer`, made by menger.input.submit and menger.input.clear \
                     on a viewer's click"
                ),
                "{err}"
            );
        }
        assert_eq!(
            parse_call("set_status", r#"{"id":"A-1","status":"Complete"}"#),
            Err("read-only: a frame may call only get, list, find, waves, groups, extents, blockers, health, components, inputs \
                 — not `set_status`; its only writes are `answer` and `clear_answer`, made by menger.input.submit and \
                 menger.input.clear on a viewer's click"
                .into())
        );
        let err = parse_call("answer", r#"{"ticket":"A-1","input":"d1","value":{"type":"choice","option":"a"}}"#).unwrap_err();
        assert!(err.starts_with("read-only: ") && err.contains("not `answer`"), "{err}");
        // INPUT-9: `clear_answer` too — a frame clears through menger.input.clear only.
        let err = parse_call("clear_answer", r#"{"ticket":"A-1","input":"d1"}"#).unwrap_err();
        assert!(err.starts_with("read-only: ") && err.contains("not `clear_answer`"), "{err}");
    }

    /// Params the method does not take are named, never dropped silently.
    #[test]
    fn wrong_params_are_named() {
        let err = parse_call("get", "{}").unwrap_err();
        assert!(err.starts_with("get: ") && err.contains("missing field `id`"), "{err}");
        let err = parse_call("waves", r#"{"epic":"PLEXUS"}"#).unwrap_err();
        assert!(err.contains("unknown field `epic`"), "{err}");
        let err = parse_call("waves", r#"{"epics":"PLEXUS"}"#).unwrap_err();
        assert!(err.contains("invalid type"), "{err}");
        let err = parse_call("health", r#"{"x":1}"#).unwrap_err();
        assert!(err.starts_with("health: ") && err.contains("unknown field `x`"), "{err}");
        let err = parse_call("list", r#"{"limit":-1}"#).unwrap_err();
        assert!(err.starts_with("list: "), "{err}");
        for not_an_object in ["[]", "[\"MNG-2\"]", "\"MNG-2\"", "5", "true"] {
            let err = parse_call("groups", not_an_object).unwrap_err();
            assert!(err.starts_with("groups: params must be an object"), "{not_an_object}: {err}");
        }
        // INPUT-4: `inputs`, with its own names.
        let err = parse_call("inputs", r#"{"id":"INPUT-9"}"#).unwrap_err();
        assert!(err.starts_with("inputs: ") && err.contains("unknown field `id`"), "{err}");
        let err = parse_call("inputs", r#"{"ticket":9}"#).unwrap_err();
        assert!(err.starts_with("inputs: ") && err.contains("invalid type"), "{err}");
        let err = parse_call("inputs", r#"{"state":["open"]}"#).unwrap_err();
        assert!(err.starts_with("inputs: ") && err.contains("invalid type"), "{err}");
    }

    // -- INPUT-4: the first write ----------------------------------------------

    use menger_tickets_api::{InputAction, InputHistoryEntry, InputKind, InputOption};

    fn option(value: &str) -> InputOption {
        InputOption { value: value.into(), label: value.to_uppercase(), recommended: false }
    }

    fn input(ticket: &str, id: &str, kind: InputKind, options: &[&str], state: InputState) -> Input {
        Input {
            ticket: ticket.into(),
            id: id.into(),
            kind,
            question: None,
            options: options.iter().map(|o| option(o)).collect(),
            recommendation: None,
            state,
            answer: None,
            history: Vec::<InputHistoryEntry>::new(),
        }
    }

    fn choice(option: &str) -> InputValue {
        InputValue::Choice { option: option.into() }
    }

    fn submitted(input: &str, value: InputValue) -> Submit {
        Submit { input: input.into(), value, rationale: None }
    }

    /// What `inputs { ticket: "DEMO-1" }` holds in these tests: a choice, a
    /// multi, a text, one closed and one removed from the body — and what
    /// another ticket declares, which a DEMO-1 frame must never answer.
    fn held() -> Vec<Input> {
        vec![
            input("DEMO-1", "d1", InputKind::Choice, &["a", "b"], InputState::Open),
            input("DEMO-1", "checks", InputKind::Multi, &["unit", "wire", "e2e"], InputState::Answered),
            input("DEMO-1", "notes", InputKind::Text, &[], InputState::Open),
            input("DEMO-1", "frozen", InputKind::Choice, &["a", "b"], InputState::Closed),
            input("DEMO-1", "gone", InputKind::Choice, &["a", "b"], InputState::Removed),
            input("DEMO-2", "other", InputKind::Choice, &["x", "y"], InputState::Open),
        ]
    }

    /// Accepted with a click: the app's activation on (or a browser that
    /// cannot say), a frame that renders a ticket, params the SDK sends, an
    /// input its ticket declares, a value that answers it — the value to
    /// send.
    #[test]
    fn a_submit_inside_a_click_for_this_tickets_input_is_sent() {
        assert_eq!(admit(SUBMIT, Some("DEMO-1"), Some(true)), Ok("DEMO-1".into()));
        assert_eq!(admit(SUBMIT, Some(" DEMO-1 "), None), Ok("DEMO-1".into()), "no userActivation API: the SDK's own check stands");
        let s = parse_submit(r#"{"input":"d1","value":{"type":"choice","option":"b"},"rationale":"holds are real"}"#).unwrap();
        assert_eq!(s, Submit { input: "d1".into(), value: choice("b"), rationale: Some("holds are real".into()) });
        assert_eq!(check_submit("DEMO-1", &s, &held()), Ok(choice("b")));
        let s = parse_submit(r#"{"input":"d1","value":{"type":"choice","option":"a"}}"#).unwrap();
        assert_eq!(s.rationale, None);
        // A multi's options go in the declared order, each once; an answered
        // input takes a new answer.
        let s = parse_submit(r#"{"input":"checks","value":{"type":"multi","options":["e2e","unit","e2e"]}}"#).unwrap();
        assert_eq!(check_submit("DEMO-1", &s, &held()), Ok(InputValue::Multi { options: vec!["unit".into(), "e2e".into()] }));
        let s = submitted("notes", InputValue::Text { text: "ship it, today".into() });
        assert_eq!(check_submit("DEMO-1", &s, &held()), Ok(InputValue::Text { text: "ship it, today".into() }));
    }

    /// Refused without a click, and in a frame that renders no ticket's
    /// stored body — before the params are read.
    #[test]
    fn a_submit_with_no_click_or_no_ticket_is_refused() {
        let err = admit(SUBMIT, Some("DEMO-1"), Some(false)).unwrap_err();
        assert!(err.starts_with("menger.input.submit: only inside a viewer's click"), "{err}");
        assert!(err.ends_with("(nothing sent)"), "{err}");
        for none in [None, Some(""), Some("  ")] {
            let err = admit(SUBMIT, none, Some(true)).unwrap_err();
            assert!(err.contains("this frame shows no ticket's stored body"), "{none:?}: {err}");
        }
        // No gesture outranks no ticket: the first thing said is the click.
        assert!(admit(SUBMIT, None, Some(false)).unwrap_err().contains("only inside a viewer's click"));
    }

    /// A frame cannot name a ticket — the field is refused — so another
    /// ticket's input is one its own ticket does not declare.
    #[test]
    fn another_tickets_input_is_refused() {
        let err = parse_submit(r#"{"ticket":"DEMO-2","input":"other","value":{"type":"choice","option":"x"}}"#).unwrap_err();
        assert!(err.contains("unknown field `ticket`"), "{err}");
        let s = submitted("other", choice("x"));
        assert_eq!(
            check_submit("DEMO-1", &s, &held()),
            Err("menger.input.submit: DEMO-1 declares no input 'other' — its inputs are 'd1', 'checks', 'notes', 'frozen' \
                 (nothing sent)"
                .into())
        );
        // A ticket whose body declares none.
        let err = check_submit("DEMO-3", &submitted("d1", choice("a")), &held()).unwrap_err();
        assert!(err.contains("DEMO-3 declares no input 'd1' — its body holds none"), "{err}");
        // DEMO-2's own frame answers it.
        assert_eq!(check_submit("DEMO-2", &s, &held()), Ok(choice("x")));
    }

    /// Refused for a value that does not answer the input — not one of its
    /// options, another kind, a blank reply — and for a closed or removed
    /// input, whatever the value.
    #[test]
    fn a_bad_value_or_a_closed_or_removed_input_is_refused() {
        let err = check_submit("DEMO-1", &submitted("d1", choice("zzz")), &held()).unwrap_err();
        assert!(err.starts_with("menger.input.submit: not an answer to DEMO-1's 'd1': 'zzz' is not one of its options ('a', 'b')"), "{err}");
        let err = check_submit("DEMO-1", &submitted("d1", InputValue::Text { text: "a".into() }), &held()).unwrap_err();
        assert!(err.contains("the input is a choice and the value a text"), "{err}");
        let err = check_submit("DEMO-1", &submitted("checks", InputValue::Multi { options: vec!["unit".into(), "smoke".into()] }), &held())
            .unwrap_err();
        assert!(err.contains("'smoke' not among its options"), "{err}");
        let err = check_submit("DEMO-1", &submitted("notes", InputValue::Text { text: "  ".into() }), &held()).unwrap_err();
        assert!(err.contains("a reply is not blank"), "{err}");
        let err = check_submit("DEMO-1", &submitted("frozen", choice("a")), &held()).unwrap_err();
        assert_eq!(err, "menger.input.submit: DEMO-1's 'frozen' is closed: its answer is frozen until it is reopened (nothing sent)");
        let err = check_submit("DEMO-1", &submitted("gone", choice("a")), &held()).unwrap_err();
        assert_eq!(err, "menger.input.submit: DEMO-1's 'gone' was removed from its body (nothing sent)");
        // Malformed values are named before any check.
        for bad in [
            r#"{"input":"d1","value":"a"}"#,
            r#"{"input":"d1","value":{"type":"bogus","option":"a"}}"#,
            r#"{"input":"d1","value":{"type":"choice"}}"#,
            r#"{"input":"d1"}"#,
            r#"{"value":{"type":"choice","option":"a"}}"#,
            r#"{"input":5,"value":{"type":"choice","option":"a"}}"#,
            r#"{"input":"d1","value":{"type":"choice","option":"a"},"source":"api"}"#,
            "null",
            "",
        ] {
            let err = parse_submit(bad).unwrap_err();
            assert!(err.starts_with("menger.input.submit: ") && err.contains("it takes (id, value, rationale?)"), "{bad}: {err}");
        }
    }

    /// The server's reply: the answer it recorded, or its own refusal.
    #[test]
    fn the_reply_is_the_recorded_answer_or_the_servers_refusal() {
        let mut changed = input("DEMO-1", "d1", InputKind::Choice, &["a", "b"], InputState::Answered);
        let answer = Answer { value: choice("b"), rationale: Some("why".into()), source: AnswerSource::App, at_unix: 1_791_400_000 };
        changed.answer = Some(answer.clone());
        assert_eq!(recorded(vec![TicketsEvent::InputChanged { input: changed }]), Ok(answer));
        assert_eq!(
            recorded(vec![TicketsEvent::Error { message: "DEMO-1's 'd1' is closed".into() }]),
            Err("menger.input.submit: DEMO-1's 'd1' is closed".into())
        );
        assert_eq!(recorded(vec![TicketsEvent::NotFound { id: "DEMO-1".into() }]), Err("menger.input.submit: DEMO-1 is not a ticket".into()));
        assert!(recorded(vec![]).is_err());
        // `inputs { ticket }`: its inputs, or the server's refusal.
        let held = inputs_of(SUBMIT, held().into_iter().map(|input| TicketsEvent::Input { input }).collect()).unwrap();
        assert_eq!(held.len(), 6);
        assert!(inputs_of(SUBMIT, vec![TicketsEvent::NotFound { id: "NOPE-1".into() }]).unwrap_err().contains("NOPE-1 is not a ticket"));
    }

    // -- INPUT-9: the second write -----------------------------------------------

    fn clearing(input: &str) -> Clear {
        Clear { input: input.into() }
    }

    /// Accepted on a click for an open input of the frame's ticket — one
    /// holding an answer, or none (the server then writes nothing) — with
    /// the params the SDK sends.
    #[test]
    fn a_clear_inside_a_click_for_this_tickets_open_input_is_sent() {
        assert_eq!(admit(CLEAR, Some("DEMO-1"), Some(true)), Ok("DEMO-1".into()));
        assert_eq!(admit(CLEAR, Some("DEMO-1"), None), Ok("DEMO-1".into()), "no userActivation API: the SDK's own check stands");
        assert_eq!(parse_clear(r#"{"input":"checks"}"#), Ok(clearing("checks")));
        for id in ["d1", "checks", "notes"] {
            assert_eq!(check_clear("DEMO-1", &clearing(id), &held()), Ok(()), "{id}");
        }
        assert_eq!(check_clear("DEMO-2", &clearing("other"), &held()), Ok(()), "DEMO-2's own frame clears it");
    }

    /// Refused without a click, in a frame that renders no ticket, for
    /// another ticket's input (the frame cannot name one), and for a closed
    /// or removed input — each naming `menger.input.clear`.
    #[test]
    fn a_clear_with_no_click_for_another_ticket_or_a_closed_input_is_refused() {
        let err = admit(CLEAR, Some("DEMO-1"), Some(false)).unwrap_err();
        assert_eq!(err, "menger.input.clear: only inside a viewer's click — the app saw no click or key just now (nothing sent)");
        let err = admit(CLEAR, None, Some(true)).unwrap_err();
        assert!(err.starts_with("menger.input.clear: this frame shows no ticket's stored body"), "{err}");
        let err = parse_clear(r#"{"ticket":"DEMO-2","input":"other"}"#).unwrap_err();
        assert!(err.starts_with("menger.input.clear: ") && err.contains("unknown field `ticket`"), "{err}");
        assert_eq!(
            check_clear("DEMO-1", &clearing("other"), &held()),
            Err("menger.input.clear: DEMO-1 declares no input 'other' — its inputs are 'd1', 'checks', 'notes', 'frozen' \
                 (nothing sent)"
                .into())
        );
        assert_eq!(
            check_clear("DEMO-1", &clearing("frozen"), &held()),
            Err("menger.input.clear: DEMO-1's 'frozen' is closed: its answer is frozen until it is reopened (nothing sent)".into())
        );
        assert_eq!(
            check_clear("DEMO-1", &clearing("gone"), &held()),
            Err("menger.input.clear: DEMO-1's 'gone' was removed from its body (nothing sent)".into())
        );
        let err = check_clear("DEMO-3", &clearing("d1"), &held()).unwrap_err();
        assert!(err.contains("DEMO-3 declares no input 'd1' — its body holds none"), "{err}");
        // A value, a source, or anything else the SDK does not send is named.
        for bad in [
            r#"{"input":"d1","value":{"type":"choice","option":"a"}}"#,
            r#"{"input":"d1","source":"api"}"#,
            r#"{"input":5}"#,
            "{}",
            "null",
            "",
        ] {
            let err = parse_clear(bad).unwrap_err();
            assert!(err.starts_with("menger.input.clear: ") && err.ends_with("it takes (id) (nothing sent)"), "{bad}: {err}");
        }
    }

    /// The server's reply: the input as the clear left it (or as it was,
    /// holding no answer), or its own refusal.
    #[test]
    fn the_clears_reply_is_the_input_or_the_servers_refusal() {
        let mut open = input("DEMO-1", "d1", InputKind::Choice, &["a", "b"], InputState::Open);
        open.history = vec![
            InputHistoryEntry { action: InputAction::Answered, value: Some(choice("a")), rationale: None, source: AnswerSource::App, at_unix: 1 },
            InputHistoryEntry { action: InputAction::Cleared, value: None, rationale: None, source: AnswerSource::App, at_unix: 2 },
        ];
        assert_eq!(cleared(vec![TicketsEvent::InputChanged { input: open.clone() }]), Ok(open.clone()));
        assert_eq!(cleared(vec![TicketsEvent::Input { input: open.clone() }]), Ok(open), "held no answer: nothing written");
        assert_eq!(
            cleared(vec![TicketsEvent::Error { message: "DEMO-1's 'd1' is closed".into() }]),
            Err("menger.input.clear: DEMO-1's 'd1' is closed".into())
        );
        assert_eq!(cleared(vec![TicketsEvent::NotFound { id: "DEMO-1".into() }]), Err("menger.input.clear: DEMO-1 is not a ticket".into()));
        assert_eq!(cleared(vec![]), Err("menger.input.clear: the server cleared nothing".into()));
        assert!(inputs_of(CLEAR, vec![TicketsEvent::Error { message: "x".into() }]).unwrap_err().starts_with("menger.input.clear: x"));
    }
}
