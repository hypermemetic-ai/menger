//! The service's typed parameters (ACCESS-8, `typed_params`) — what names
//! and carries a document: [`DocId`], [`DocTitle`], [`DocBody`],
//! [`DocQuery`]. Each a transparent text whose `Deserialize` is its one
//! rule, so a bad value is `invalid_params` naming the parameter before the
//! method runs (the media service's `vocab.rs`, the pattern).
//!
//! **A doc's id** is TICKETS-35's uid form: 8 characters of Crockford's
//! base32, lower case (`0-9` and the letters but `i`, `l`, `o` and `u`) —
//! 40 random bits, short, URL-safe (`#/doc/7k3mq2xa`), read case-blind. The
//! store mints one per document ([`mint`]), drawn again until no document
//! of the file has it; it never changes and is never given out again.

/// The alphabet, in digit order: Crockford's base32, lower case (TICKETS-35's).
pub const ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// How many characters an id is.
pub const ID_LEN: usize = 8;

/// The most a title may be, in characters.
pub const TITLE_MAX: usize = 200;

/// The most a body may be, in bytes: 2 MiB — a document travels whole in
/// one WebSocket message (10 MiB).
pub const BODY_MAX: usize = 2 * 1024 * 1024;

/// The most a title search may be, in characters.
pub const QUERY_MAX: usize = 200;

macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $rule:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Its text, as it goes on the wire.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Its text, owned.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(&text).map(Self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(text: &str) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(text).map(Self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::try_from(<String as serde::Deserialize>::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

fn not_a(text: &str, what: &str, shape: &str) -> String {
    format!("`{text}` is not {what}: {shape}")
}

fn id(text: &str) -> Result<String, String> {
    let t = text.trim();
    // The app's route to it is read past: `#/doc/<id>`, `/app/#/doc/<id>`.
    let t = t.rsplit_once("#/doc/").map_or(t, |(_, id)| id).trim_end_matches('/');
    match t.len() == ID_LEN && t.bytes().all(|b| ID_ALPHABET.contains(&b.to_ascii_lowercase())) {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(not_a(t, "a doc's id", "8 characters of Crockford's base32 — digits and letters but i, l, o and u — as 7k3mq2xa")),
    }
}

text_type!(
    /// A document's id: 8 characters of Crockford's base32, lower case
    /// (read case-blind; `#/doc/` before it is read past), as `create`
    /// answered it.
    DocId,
    id
);

fn title(text: &str) -> Result<String, String> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match (t.is_empty(), t.chars().count() > TITLE_MAX) {
        (true, _) => Err("a title is some words: it is empty".to_string()),
        (_, true) => Err(format!("a title is at most {TITLE_MAX} characters; this one is {}", t.chars().count())),
        _ => Ok(t),
    }
}

text_type!(
    /// A document's title: one line of words (whitespace folded to single
    /// spaces), at most 200 characters, never empty.
    DocTitle,
    title
);

fn body(text: &str) -> Result<String, String> {
    match text.len() <= BODY_MAX {
        true => Ok(text.to_string()),
        false => Err(format!("a body is at most {BODY_MAX} bytes; this one is {}", text.len())),
    }
}

text_type!(
    /// A document's body: markdown, kept byte for byte, at most 2 MiB.
    DocBody,
    body
);

fn query(text: &str) -> Result<String, String> {
    let t = text.trim();
    match t.chars().count() <= QUERY_MAX {
        true => Ok(t.to_string()),
        false => Err(format!("a search is at most {QUERY_MAX} characters")),
    }
}

text_type!(
    /// Words a title holds, case-blind (`kiln glaze` finds "The kiln's
    /// glaze"); at most 200 characters.
    DocQuery,
    query
);

/// The most an update may be on the wire (base64): 8 MiB.
pub const UPDATE_MAX: usize = 8 * 1024 * 1024;
/// The most a caret (an awareness update, base64) may be: 64 KiB.
pub const AWARENESS_MAX: usize = 64 * 1024;

fn base64_of(text: &str, max: usize, what: &str) -> Result<String, String> {
    let t = text.trim();
    let ok = t.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b));
    match (ok, t.len() <= max) {
        (false, _) => Err(format!("{what} is standard base64")),
        (_, false) => Err(format!("{what} is at most {max} bytes of base64; this is {}", t.len())),
        _ => Ok(t.to_string()),
    }
}

fn update(text: &str) -> Result<String, String> {
    base64_of(text, UPDATE_MAX, "an update")
}

text_type!(
    /// A Yjs v1 update, standard base64 — what the browser's `Y.encodeStateAsUpdate`
    /// and a doc's `update` event make; at most 8 MiB.
    DocUpdate,
    update
);

fn vector(text: &str) -> Result<String, String> {
    base64_of(text, AWARENESS_MAX, "a state vector")
}

text_type!(
    /// A Yjs state vector, standard base64 — `Y.encodeStateVector(ydoc)`.
    DocVector,
    vector
);

fn awareness(text: &str) -> Result<String, String> {
    base64_of(text, AWARENESS_MAX, "a caret")
}

text_type!(
    /// A caret and selection: a Yjs awareness update, standard base64
    /// (`encodeAwarenessUpdate`); at most 64 KiB.
    DocAwareness,
    awareness
);

fn session(text: &str) -> Result<String, String> {
    let t = text.trim();
    match !t.is_empty() && t.len() <= 64 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a session", "1 to 64 letters, digits, - and _ — the page's own id, e.g. s-3f9a1c")),
    }
}

text_type!(
    /// A connection's own id on a document — the page makes one, e.g. s-3f9a1c.
    DocSession,
    session
);

text_type!(
    /// V2-30: a thread's id: 8 characters of Crockford's base32, as `ask` answered it.
    ThreadId,
    id
);

/// The most a question or an answer may be, in bytes: 256 KiB.
pub const THREAD_TEXT_MAX: usize = 256 * 1024;

fn thread_text(text: &str) -> Result<String, String> {
    match (text.trim().is_empty(), text.len() <= THREAD_TEXT_MAX) {
        (true, _) => Err("a question or an answer says something: it is empty".to_string()),
        (_, false) => Err(format!("a question or an answer is at most {THREAD_TEXT_MAX} bytes; this one is {}", text.len())),
        _ => Ok(text.trim_end().to_string()),
    }
}

text_type!(
    /// V2-30: a question or an answer: markdown — words, links, images as a
    /// body shows them; at most 256 KiB, never empty.
    ThreadText,
    thread_text
);

fn agent_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    match !t.is_empty() && t.len() <= 64 && !t.chars().any(char::is_whitespace) {
        true => Ok(t.to_string()),
        false => Err(not_a(t, "a name", "an agent's name or a principal, one word, e.g. builder (or agent, for any)")),
    }
}

text_type!(
    /// V2-30: whom a question is asked of: an agent's name (`builder`), a
    /// principal, or `agent` for any agent.
    AgentName,
    agent_name
);

/// A fresh id from `draw`'s low 40 bits, most significant digit first.
pub fn id_of_bits(bits: u64) -> String {
    (0..ID_LEN).map(|i| ID_ALPHABET[((bits >> (5 * (ID_LEN - 1 - i))) & 31) as usize] as char).collect()
}

/// 40 bits from the system's randomness — a v4 uuid's first five bytes,
/// which no version or variant bit touches (TICKETS-35's draw).
pub fn draw() -> u64 {
    let b = uuid::Uuid::new_v4().into_bytes();
    u64::from_be_bytes([0, 0, 0, b[0], b[1], b[2], b[3], b[4]])
}

/// An id `taken` does not refuse, drawn again on a clash.
pub fn mint(taken: impl Fn(&str) -> bool) -> String {
    loop {
        let id = id_of_bits(draw());
        if !taken(&id) {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spellings_hold_and_refuse() {
        assert_eq!("7K3MQ2XA".parse::<DocId>().unwrap().as_str(), "7k3mq2xa");
        assert_eq!("/app/#/doc/7k3mq2xa".parse::<DocId>().unwrap().as_str(), "7k3mq2xa");
        for bad in ["", "7k3mq2x", "7k3mq2xab", "7k3mq2xi", "7k3mq-xa"] {
            assert!(bad.parse::<DocId>().is_err(), "{bad:?}");
        }
        assert_eq!("  The   kiln\n".parse::<DocTitle>().unwrap().as_str(), "The kiln");
        assert!("   ".parse::<DocTitle>().is_err());
        assert!("x".repeat(TITLE_MAX + 1).parse::<DocTitle>().is_err());
        assert!("x".repeat(BODY_MAX + 1).parse::<DocBody>().is_err());
        assert_eq!("".parse::<DocBody>().unwrap().as_str(), "");
        assert!(serde_json::from_str::<DocId>("\"abc\"").is_err(), "deserializing is the check");
    }

    #[test]
    fn an_id_is_eight_digits_of_the_alphabet_and_minted_apart() {
        assert_eq!(id_of_bits(0), "00000000");
        assert_eq!(id_of_bits((1 << 40) - 1), "zzzzzzzz");
        for _ in 0..200 {
            let id = id_of_bits(draw());
            assert!(id.parse::<DocId>().is_ok(), "{id}");
        }
        let first = id_of_bits(draw());
        let again = mint(|t| t == first);
        assert_ne!(again, first);
    }
}
