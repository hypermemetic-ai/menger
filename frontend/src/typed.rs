//! ACCESS-8: the app's text as the parameter types the clients take —
//! `TicketRef`, `FilterText`, `Kind` — parsed once, here, at the app's edge
//! to the wire. The emitted clients take nothing else (MFE-11): a call site
//! that holds an id as text says how it becomes one, and a value the type
//! refuses is the call's refusal before anything is sent.

use std::str::FromStr;

/// `text` as `T`, or why it is not one.
pub(crate) fn typed<T: FromStr<Err = String>>(text: &str) -> Result<T, String> {
    text.parse()
}

/// TICKETS-36: the ticket keyed `key`, as a call names it — by its uid when
/// the app has met it. A key is no name: the original of two namesakes is
/// keyed `PERF-19`, which as a name fits both (TICKETS-35: ambiguous), so a
/// page's own calls name its ticket by uid.
pub(crate) fn ticket(key: &str) -> Result<menger_tickets_api::vocab::TicketRef, String> {
    match crate::cache::uid_of(key) {
        Some(uid) => uid.parse(),
        None => key.parse(),
    }
}

/// TICKETS-36: [`ticket`] for each of `keys`.
pub(crate) fn tickets<S: AsRef<str>>(keys: impl IntoIterator<Item = S>) -> Result<Vec<menger_tickets_api::vocab::TicketRef>, String> {
    keys.into_iter().map(|k| ticket(k.as_ref())).collect()
}

/// TICKETS-36: an optional ticket, by its key — as [`ticket`]; blank is none.
pub(crate) fn ticket_opt<S: AsRef<str>>(key: Option<S>) -> Result<Option<menger_tickets_api::vocab::TicketRef>, String> {
    match key.as_ref().map(AsRef::as_ref).map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(k) => ticket(k).map(Some),
    }
}

/// An optional value: `None`, and blank text, are none (as the server read
/// a blank optional parameter before ACCESS-8); anything else must be a `T`.
pub(crate) fn opt<T: FromStr<Err = String>, S: AsRef<str>>(text: Option<S>) -> Result<Option<T>, String> {
    match text.as_ref().map(AsRef::as_ref).map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) => t.parse().map(Some),
    }
}

/// Every one of `texts` as a `T`, or the first that is not one.
pub(crate) fn many<T: FromStr<Err = String>, S: AsRef<str>>(texts: impl IntoIterator<Item = S>) -> Result<Vec<T>, String> {
    texts.into_iter().map(|t| t.as_ref().parse()).collect()
}

/// An optional list, each one a `T`.
pub(crate) fn opt_many<T: FromStr<Err = String>, S: AsRef<str>>(texts: Option<impl IntoIterator<Item = S>>) -> Result<Option<Vec<T>>, String> {
    texts.map(many).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TICKETS-36: a ticket the app holds by its key is named on the wire by
    /// its uid once the app knows it — the original of two namesakes is
    /// keyed `PERF-2`, a name that fits both; one it does not know, by its key.
    #[test]
    fn a_ticket_known_by_its_key_is_named_by_its_uid() {
        assert_eq!(&*ticket("PERF-2").unwrap(), "PERF-2", "not met yet: its key");
        crate::cache::learn_pair("0kv0kwhs".parse().unwrap(), "PERF-2", "Compression on the WebSocket");
        assert_eq!(ticket("PERF-2").unwrap().uid(), "0kv0kwhs".parse().ok());
        let both: Vec<String> = tickets(["PERF-2", "OPS-1"]).unwrap().iter().map(|t| t.to_string()).collect();
        assert_eq!(both, ["0kv0kwhs", "OPS-1"]);
        assert_eq!(ticket_opt(Some(" ")).unwrap(), None);
        assert_eq!(ticket_opt(None::<&str>).unwrap(), None);
        assert_eq!(ticket_opt(Some("PERF-2")).unwrap().and_then(|t| t.uid()), "0kv0kwhs".parse().ok());
    }
}
