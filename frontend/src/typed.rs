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
