//! MENGER-12: a ticket's **labels** — free-form tags (`needs-you`, …), a
//! set on the row, written by `tickets.label { id, add, remove }` and asked
//! by the filter cell `label <name>`.
//!
//! A label is a name: ASCII letters, digits and `-`, 1 to [`MAX_LEN`]
//! characters, stored lower case — `Needs-You` is `needs-you`; anything
//! else is refused, naming it. A ticket holds at most [`MAX_PER_TICKET`],
//! kept sorted, each once, so two tickets with the same labels hold the same
//! list. Wasm-safe: the app checks a label as it is typed with
//! [`normalize`], and the filter language reads `label <name>` with it.

/// The longest a label may be.
pub const MAX_LEN: usize = 32;

/// At most this many labels on one ticket — SCALE-4's lesson: a list on a
/// row is bounded.
pub const MAX_PER_TICKET: usize = 16;

/// The label that puts a ticket in front of the owner: Home's "Needs you"
/// panel shows every ticket that holds it (beside every ticket `asking`).
pub const NEEDS_YOU: &str = "needs-you";

/// `raw` as the label it names — trimmed, lower-cased — or why it is not
/// one.
pub fn normalize(raw: &str) -> Result<String, String> {
    let label = raw.trim().to_ascii_lowercase();
    if label.is_empty() {
        return Err("a label is not empty".into());
    }
    if label.len() > MAX_LEN {
        return Err(format!("`{raw}` is longer than {MAX_LEN} characters — a label is at most {MAX_LEN}"));
    }
    if !label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err(format!("`{raw}` is not a label: letters, digits and `-` only, as `needs-you`"));
    }
    Ok(label)
}

/// `labels` with `add` added and `remove` taken off — each normalized,
/// sorted, each once — and what moved: `(after, added, removed)`, `added`
/// and `removed` in the order given, each once. A label named in both is
/// added (the `remove` loses). Refused whole, naming every entry that is not
/// a label, or a result past [`MAX_PER_TICKET`].
pub fn apply(labels: &[String], add: &[String], remove: &[String]) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let mut bad = Vec::new();
    let mut read = |raw: &[String]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in raw {
            match normalize(r) {
                Ok(l) if !out.contains(&l) => out.push(l),
                Ok(_) => {}
                Err(why) => bad.push(why),
            }
        }
        out
    };
    let (add, remove) = (read(add), read(remove));
    if !bad.is_empty() {
        return Err(bad.join("; "));
    }
    let mut after: Vec<String> = labels.to_vec();
    let removed: Vec<String> = remove.into_iter().filter(|l| !add.contains(l) && after.contains(l)).collect();
    after.retain(|l| !removed.contains(l));
    let added: Vec<String> = add.into_iter().filter(|l| !after.contains(l)).collect();
    after.extend(added.iter().cloned());
    after.sort();
    if after.len() > MAX_PER_TICKET {
        return Err(format!(
            "a ticket holds at most {MAX_PER_TICKET} labels; this would leave {}",
            after.len()
        ));
    }
    Ok((after, added, removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_label_is_lower_case_letters_digits_and_dashes() {
        assert_eq!(normalize(" Needs-You ").unwrap(), "needs-you");
        assert_eq!(normalize("p1").unwrap(), "p1");
        assert!(normalize("").is_err());
        assert!(normalize("two words").unwrap_err().contains("`two words`"));
        assert!(normalize("a_b").is_err());
        assert!(normalize("é").is_err());
        assert!(normalize(&"x".repeat(MAX_LEN)).is_ok());
        assert!(normalize(&"x".repeat(MAX_LEN + 1)).unwrap_err().contains("32"));
    }

    #[test]
    fn apply_adds_removes_sorts_and_names_what_moved() {
        let (after, added, removed) = apply(&v(&["b"]), &v(&["Needs-You", "a", "a", "b"]), &v(&["b", "zz"])).unwrap();
        // `b` is in both: added wins, and it was already there.
        assert_eq!(after, v(&["a", "b", "needs-you"]));
        assert_eq!(added, v(&["needs-you", "a"]));
        assert!(removed.is_empty());
        let (after, added, removed) = apply(&after, &[], &v(&["NEEDS-YOU"])).unwrap();
        assert_eq!((after, added, removed), (v(&["a", "b"]), vec![], v(&["needs-you"])));
        // Nothing moves: nothing named.
        let (after, added, removed) = apply(&v(&["a"]), &v(&["a"]), &v(&["c"])).unwrap();
        assert_eq!((after, added, removed), (v(&["a"]), vec![], vec![]));
    }

    #[test]
    fn apply_refuses_a_bad_entry_or_too_many_whole() {
        let err = apply(&[], &v(&["ok", "not ok", "nor_this"]), &[]).unwrap_err();
        assert!(err.contains("`not ok`") && err.contains("`nor_this`"), "{err}");
        let many: Vec<String> = (0..MAX_PER_TICKET).map(|i| format!("l{i}")).collect();
        assert!(apply(&many, &v(&["one-more"]), &[]).unwrap_err().contains("at most 16"));
        assert!(apply(&many, &v(&["one-more"]), &v(&["l0"])).is_ok());
    }
}
