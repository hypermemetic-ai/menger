//! The service's typed parameters (ACCESS-8, `typed_params`): a log's name,
//! a field's name, a tag, a message, and the JSON a call carries — a
//! schema, an entry, many entries. Each a transparent text whose
//! `Deserialize` is its one rule, so a bad value is `invalid_params` naming
//! the parameter before the method runs. Whether a log is there, and whether
//! an entry fits its schema, is the service's answer, in band.

/// The longest a log's or a field's name is.
pub const NAME_MAX: usize = 63;
/// The longest a tag is.
pub const TAG_MAX: usize = 64;
/// The longest a message is: 1 KiB.
pub const MESSAGE_MAX: usize = 1024;
/// The largest schema: 64 KiB of JSON.
pub const SCHEMA_MAX: usize = 64 * 1024;
/// The largest entry: 64 KiB of JSON.
pub const ENTRY_MAX: usize = 64 * 1024;
/// The most one `append` carries: 8 MiB of JSON.
pub const ENTRIES_MAX: usize = 8 * 1024 * 1024;

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

/// Whether `n` is a name the store may make a table or a column of: a
/// lower-case letter, then lower-case letters, digits and `_`, at most
/// [`NAME_MAX`] in all. Lower case alone: SQLite reads a name without regard
/// to case, so `Value` and `value` would be one column.
pub fn identifier_ok(n: &str) -> bool {
    (1..=NAME_MAX).contains(&n.len()) && n.as_bytes()[0].is_ascii_lowercase() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn log_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    match identifier_ok(t) {
        true => Ok(t.to_string()),
        false => Err(format!("`{t}` is not a log's name: a lower-case letter, then lower-case letters, digits and _, at most {NAME_MAX} (perf, deploy_sizes)")),
    }
}

text_type!(
    /// A log, by its name: a lower-case letter, then lower-case letters,
    /// digits and `_`, at most 63 (`perf`, `deploy_sizes`).
    LogName,
    log_name
);

fn field_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    match identifier_ok(t) {
        true => Ok(t.to_string()),
        false => Err(format!("`{t}` is not a field's name: a lower-case letter, then lower-case letters, digits and _, at most {NAME_MAX}")),
    }
}

text_type!(
    /// A field of a log's schema, by its name (`machine`, `p99_us`).
    FieldName,
    field_name
);

/// Whether `t` is a tag: 1 to 64 letters, digits, `.`, `_`, `-`, `:` and
/// `/`, never `-` first.
pub fn tag_ok(t: &str) -> bool {
    (1..=TAG_MAX).contains(&t.len()) && !t.starts_with('-') && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-:/".contains(&b))
}

fn tag(text: &str) -> Result<String, String> {
    let t = text.trim();
    match tag_ok(t) {
        true => Ok(t.to_string()),
        false => Err(format!("`{t}` is not a tag: 1 to {TAG_MAX} letters, digits, . _ - : and /, never - first (plexus, websocket, app:menger)")),
    }
}

text_type!(
    /// A tag an entry carries (`plexus`, `websocket`, `app:menger`): 1 to
    /// 64 letters, digits, `.`, `_`, `-`, `:` and `/`, never `-` first.
    Tag,
    tag
);

fn message(text: &str) -> Result<String, String> {
    let t = text.trim();
    match !t.is_empty() && t.len() <= MESSAGE_MAX && !t.chars().any(|c| c.is_control() && c != '\n') {
        true => Ok(t.to_string()),
        false => Err(format!("a message is 1 to {MESSAGE_MAX} bytes of text, no control characters but a newline")),
    }
}

text_type!(
    /// Why, in words: 1 to 1024 bytes of text.
    Message,
    message
);

/// A call's JSON as text: a string holding JSON, or (from a client that
/// sends JSON as itself) the JSON value, written back as text. At most `max`
/// bytes; its shape is the service's to check.
fn json_text(text: &str, what: &str, max: usize) -> Result<String, String> {
    match text.len() <= max {
        true => Ok(text.to_string()),
        false => Err(format!("{what} is at most {max} bytes of JSON; this is {}", text.len())),
    }
}

macro_rules! json_type {
    ($(#[$doc:meta])* $name:ident, $what:expr, $max:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Its text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(text: &str) -> Result<Self, String> {
                json_text(text, $what, $max).map(Self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = match <serde_json::Value as serde::Deserialize>::deserialize(d)? {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                json_text(&text, $what, $max).map(Self).map_err(serde::de::Error::custom)
            }
        }
    };
}

json_type!(
    /// A JSON Schema of an object, as JSON text (synapse: `--schema
    /// @perf.schema.json`); at most 64 KiB.
    SchemaText,
    "a schema",
    SCHEMA_MAX
);

json_type!(
    /// One entry, a JSON object, as JSON text (synapse: `--entry
    /// @result.json`); at most 64 KiB.
    EntryText,
    "an entry",
    ENTRY_MAX
);

json_type!(
    /// Many entries as JSON text: one JSON object a line (JSON lines), or a
    /// JSON array of objects (synapse: `--entries @results.jsonl`); at most
    /// 8 MiB in all.
    EntriesText,
    "entries",
    ENTRIES_MAX
);

#[cfg(test)]
mod tests {
    use super::*;

    fn holds<T: std::str::FromStr<Err = String> + std::fmt::Debug>(yes: &[&str], no: &[&str]) {
        for y in yes {
            assert!(y.parse::<T>().is_ok(), "{y:?} holds");
        }
        for n in no {
            assert!(n.parse::<T>().is_err(), "{n:?} is refused");
        }
    }

    #[test]
    fn names_tags_and_messages_hold_their_spellings() {
        let long = "a".repeat(64);
        holds::<LogName>(&["perf", "deploy_sizes", "a1", &"a".repeat(63)], &["", "Perf", "1perf", "_x", "per-f", "a b", "pe\"rf", "log;drop", &long]);
        holds::<FieldName>(&["value", "p99_us"], &["Value", "", "x-y", "é"]);
        holds::<Tag>(&["plexus", "app:menger", "v2.1", "a/b", "x_y-z"], &["", "-x", "a b", "a,b", &"t".repeat(65)]);
        holds::<Message>(&["replaced by perf2", "two\nlines"], &["", "   ", "a\u{7}b", &"m".repeat(1025)]);
    }

    #[test]
    fn json_comes_as_text_or_as_itself() {
        let s: SchemaText = serde_json::from_str(r#""{\"type\":\"object\"}""#).unwrap();
        assert_eq!(s.as_str(), r#"{"type":"object"}"#);
        let s: SchemaText = serde_json::from_str(r#"{"type":"object"}"#).unwrap();
        assert_eq!(s.as_str(), r#"{"type":"object"}"#);
        let big = format!("\"{}\"", "x".repeat(ENTRY_MAX + 1));
        assert!(serde_json::from_str::<EntryText>(&big).is_err());
    }
}
