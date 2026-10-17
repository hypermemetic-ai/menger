//! ACCESS-14: `tickets inputs` — a ticket's inputs (INPUT-3), a child of
//! `tickets` on the frame (README "The frame"): `list` (was `tickets.inputs`),
//! `answer`, `clear` (was `clear_answer`), `close` (was `close_input`),
//! `reopen` (was `reopen_input`). Reached as `tickets.inputs.<method>`; its
//! grants are `inputs.<method>`, listed in its own `roles(...)`; the
//! parent's client reaches it as `client.inputs()` (MACRO-16).
//!
//! ACCESS-3 #12: an answer's and a clear's `source` is the server's to say,
//! from the verified caller ([`source_of`]) — no caller declares it.

use super::*;

/// The inputs child. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct InputsActivation {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

/// ACCESS-14 (ACCESS-3 #12): where an answer or a clear comes from, as the
/// server reads its caller — `app` for a person signed in through a
/// provider (a browser sign-in, or a session minted from one), `api` for
/// an agent's key (or a session minted from one) and for every call that
/// proves no one (login off). Never what the caller says.
#[cfg(feature = "server")]
pub(super) fn source_of(caller: &VerifiedUser) -> AnswerSource {
    match caller.context() {
        Some(ctx) if ctx.is_authenticated() && ctx.get_metadata_string(crate::access::AGENT_KEY).is_none() => AnswerSource::App,
        _ => AnswerSource::Api,
    }
}

// The attribute is NOT `#[cfg]`-gated: it has to run in the client-only
// build too, or there would be no `InputsClient`.
/// A ticket's inputs: asked of the owner inside a ticket, answered, closed (INPUT-3).
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(viewer = ["list"], editor = [viewer, "answer", "clear", "close", "reopen"])
)]
impl InputsActivation {
    /// Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket.
    ///
    /// INPUT-3: the inputs store. Yields one `Input` per input of `ticket`
    /// (every ticket's, without one) whose ticket is in the shortcode `epic`
    /// and which is in `state` — its ticket, id, kind (`choice`, `multi`,
    /// `text`), question, options (`{ value, label, recommended }`, the
    /// body's order), recommendation, state (`open`, `answered`, `closed`,
    /// `removed`), current answer (`{ value, rationale, source, at_unix }`,
    /// `null` with none) and whole history, oldest first — in the store's
    /// order of tickets, each ticket's inputs in its body's order and the
    /// removed after. Nothing trails them; the stream's own `done` is the
    /// whole answer. `NotFound` when `ticket` names no ticket. From the
    /// cache; `tickets.watch` keeps a client's copy current with
    /// `InputChanged`.
    ///
    /// FILTER-15: `filter` (the language's text, FILTER-10) keeps the inputs
    /// whose ticket the filter admits — each such ticket one lookup in the
    /// filter's admitted cells; every other parameter as without it. A
    /// filter not taken, or one that asks `under` or `text`, is one in-band
    /// `error`; one that narrows nothing (empty, a display part alone) is no
    /// filter.
    #[plexus_macros::method(
        group = "inputs",
        values(state = ["open", "answered", "closed", "removed", "withdrawn"]),
        example(note = "everything waiting on the owner", params = r#"{"state": "open"}"#),
        example(note = "one ticket's inputs, answers and history", params = r#"{"ticket": "RETURNS-13"}"#)
    )]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only this ticket's inputs, e.g. INPUT-9 (optional).
        ticket: Option<TicketRef>,
        /// Only inputs of tickets in this shortcode (optional).
        epic: Option<ShortcodeRef>,
        /// Only inputs in this state: open | answered | closed | removed | withdrawn (optional; withdrawn: its ticket is canceled).
        state: Option<InputState>,
        /// A filter on the input's ticket, e.g. 'kind build' (FILTER-3's language; optional): only inputs of tickets it admits.
        filter: Option<FilterText>,
        /// The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC).
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(inputs_events(&self.store, &user, ticket, epic, state, filter, tz))
    }

    /// Answer an input: record a value (checked against its kind and options) with a rationale, the time, and where it came from (said by the server); yields input_changed.
    ///
    /// INPUT-3: answers an input — records `value` against `ticket`'s input
    /// `input`, with its `rationale` and the time. ACCESS-14: its `source`
    /// is the server's — `app` for a person signed in through a provider,
    /// `api` for an agent's key and for a call that proves no one. The value
    /// is typed by the input's kind and checked against the options its
    /// body declares: `{"type": "choice", "option": …}` names one,
    /// `{"type": "multi", "options": […]}` a set (kept in the body's order,
    /// each once; empty: none ticked), `{"type": "text", "text": …}` a
    /// reply, not blank. Every answer is a history entry — a changed one,
    /// and the same one again. Yields exactly one event: `InputChanged` (the
    /// input as it is now, its answer and history included), `NotFound` for
    /// a ticket that is none, or an in-band `Error` — the ticket declares no
    /// such input, the value does not answer it, it is closed, or it was
    /// removed from its body — and nothing is written. Never touches the
    /// body. `tickets.watch` carries the `InputChanged`.
    #[plexus_macros::method(
        group = "inputs",
        target = "ticket",
        example(note = "pick an option, saying why", params = r#"{"ticket": "RETURNS-13", "input": "landing", "value": {"type": "choice", "option": "archive"}, "rationale": "holds are real"}"#),
        example(note = "reply in text", params = r#"{"ticket": "RETURNS-13", "input": "notes", "value": {"type": "text", "text": "ship it"}}"#),
        example(note = "tick several (a list inside --value goes whole, with -p)", params = r#"{"ticket": "RETURNS-13", "input": "checks", "value": {"type": "multi", "options": ["unit", "wire"]}}"#)
    )]
    pub async fn answer(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. INPUT-9.
        ticket: TicketRef,
        /// The input's id in that ticket's body, e.g. landing.
        input: InputId,
        /// Typed by the input's kind: {"type": "choice", "option": "archive"} | {"type": "multi", "options": ["a", "b"]} | {"type": "text", "text": "a reply"}; through synapse, --value.type choice --value.option archive.
        value: InputValue,
        /// Why (optional).
        rationale: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(ticket.to_string()) => {
            let on = permit::<Editor>(&store, &user, &ticket)?;
            Ok(input_event(store.answer(on, &input, value, rationale.and_then(Message::given), source_of(&user)).await, ticket.into_string()))
        })
    }

    /// Clear an input's answer (open again; kept in its history, with where it came from, said by the server); yields input_changed.
    ///
    /// INPUT-3: clears an input's answer — `open` again, the clear in its
    /// history, with its source (ACCESS-14: the server's, as for `answer`).
    /// Yields exactly one event: `InputChanged`; `Input` when it held no
    /// answer (nothing written); `NotFound`; or an in-band `Error` — no
    /// such input, closed, removed.
    #[plexus_macros::method(group = "inputs", target = "ticket")]
    pub async fn clear(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. INPUT-9.
        ticket: TicketRef,
        /// The input's id in that ticket's body.
        input: InputId,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(ticket.to_string()) => {
            let on = permit::<Editor>(&store, &user, &ticket)?;
            Ok(input_event(store.clear_answer(on, &input, source_of(&user)).await, ticket.into_string()))
        })
    }

    /// Close an input: freeze its answer (or its lack of one); yields input_changed.
    ///
    /// INPUT-3: closes an input — frozen (a decision's DECIDED), with its
    /// answer or without one; an answer or a clear is refused until it is
    /// reopened. Yields exactly one event: `InputChanged`; `Input` when it
    /// was closed already; `NotFound`; or an in-band `Error` — no such
    /// input, removed.
    #[plexus_macros::method(group = "inputs", target = "ticket")]
    pub async fn close(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. INPUT-9.
        ticket: TicketRef,
        /// The input's id in that ticket's body.
        input: InputId,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(ticket.to_string()) => {
            let on = permit::<Editor>(&store, &user, &ticket)?;
            Ok(input_event(store.close_input(on, &input).await, ticket.into_string()))
        })
    }

    /// Reopen a closed input; yields input_changed.
    ///
    /// INPUT-3: reopens a closed input — `answered` again when it holds an
    /// answer, else `open`. Yields exactly one event: `InputChanged`;
    /// `Input` when it was not closed; `NotFound`; or an in-band `Error` —
    /// no such input, removed.
    #[plexus_macros::method(group = "inputs", target = "ticket")]
    pub async fn reopen(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. INPUT-9.
        ticket: TicketRef,
        /// The input's id in that ticket's body.
        input: InputId,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(ticket.to_string()) => {
            let on = permit::<Editor>(&store, &user, &ticket)?;
            Ok(input_event(store.reopen_input(on, &input).await, ticket.into_string()))
        })
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use plexus_core::plexus::AuthContext;
    use serde_json::json;

    const ALICE: &str = "idp:c952acd3-60aa-4534-b2e3-83634591fee1";

    /// A caller signed in with `meta`, verified as the frame's resolver does.
    fn signed_in(meta: serde_json::Value) -> VerifiedUser {
        let access = Access::for_tests();
        access.attach(Arc::new(Store::new()));
        access.verify_now(&AuthContext::new(ALICE.into(), "s".into(), vec![], meta)).expect("signed in")
    }

    /// ACCESS-14 (ACCESS-3 #12): the server says where an answer comes
    /// from — a person signed in through a provider is `app`; an agent's
    /// key, and a call proving no one (login off), `api`.
    #[test]
    fn the_source_is_the_server_s_from_the_caller() {
        let anyone = Access::open().verify_now(&AuthContext::anonymous()).expect("login off admits anyone");
        assert_eq!(source_of(&anyone), AnswerSource::Api, "login off: no one proved");
        let person = signed_in(json!({ "principal": ALICE, "provider": "idp", "name": "alice" }));
        assert_eq!(source_of(&person), AnswerSource::App, "a person through a provider");
        let agent = signed_in(json!({ "principal": ALICE, "provider": "key", "agent": "builder" }));
        assert_eq!(source_of(&agent), AnswerSource::Api, "an agent's key");
    }
}
