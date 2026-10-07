//! COMP-5: **the bridge** — the host's side of a frame's `menger.call`
//! (COMP-1 pin 9: read-only).
//!
//! A frame cannot reach the app's WebSocket (its origin is opaque); it asks
//! the host instead: `menger:call { id, method, params }` over postMessage
//! (src/frame.rs checks the sender and its channel). The host answers over
//! the app's own connection, with the typed client the rest of the app uses
//! — never a raw call: [`parse_call`] turns the method and its params into a
//! [`Call`], one of the nine read methods with the activation's own
//! parameter names, or refuses it; [`run`] makes it. The answer is the
//! stream's items — every data event, an in-band `error` included, as the
//! wire spells them — or, when the call never got an answer (no
//! connection, a refused request), the reason, which the SDK turns into a
//! rejected promise.
//!
//! Read-only by construction: a method outside [`READ_METHODS`] is refused
//! here, before anything is sent — `set_status`, `amend`, `create`, every
//! other write, and `watch` (a frame hears the stream as change notices,
//! not by subscribing).

use menger_tickets_api::TicketsEvent;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::session::Client;

/// What a frame may call (COMP-1 pin 9) — the activation's read methods.
pub(crate) const READ_METHODS: [&str; 9] =
    ["get", "list", "find", "waves", "groups", "extents", "blockers", "health", "components"];

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
struct NoParams {}

/// The refusal for anything but a read.
pub(crate) fn read_only(method: &str) -> String {
    format!("read-only: a frame may call only {} — not `{method}`", READ_METHODS.join(", "))
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
            client.list_all(epic, status, parent, after, limit, summary).await
        }
        Call::Find { query, after, limit } => client.find_all(query, after, limit).await,
        Call::Waves { epics, kinds } => client.waves_all(epics, kinds).await,
        Call::Groups => client.groups_all().await,
        Call::Extents => client.extents_all().await,
        Call::Blockers { id } => client.blockers_all(id).await,
        Call::Health => client.health_all().await,
        Call::Components => client.components_all().await,
    };
    answer.map_err(|e| crate::session::describe(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COMP-1 pin 9: the nine reads, each with the activation's parameter
    /// names — and nothing else.
    #[test]
    fn a_frame_may_make_the_nine_reads() {
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

    /// Every write — and `watch`, `ready_set`, a name that is no method —
    /// is refused before anything is sent, saying "read-only" and what is
    /// allowed.
    #[test]
    fn anything_else_is_read_only() {
        for m in [
            "set_status", "amend", "create", "set_parent", "link_commits", "unlink_commits", "watch", "ready_set", "", "GET",
            "menger.tickets.get",
        ] {
            let err = parse_call(m, "{}").unwrap_err();
            assert!(err.starts_with("read-only: "), "{m}: {err}");
            assert!(err.contains("get, list, find, waves, groups, extents, blockers, health, components"), "{err}");
        }
        assert_eq!(
            parse_call("set_status", r#"{"id":"A-1","status":"Complete"}"#),
            Err("read-only: a frame may call only get, list, find, waves, groups, extents, blockers, health, components — not `set_status`".into())
        );
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
    }
}
