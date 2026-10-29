//! ACCESS-8: the vocabulary — every domain value a method takes, as a type.
//!
//! The owner, 2026-10-12: *"we need to make sure that our activations use
//! strong types for all the parameters. if we don't have the types we
//! construct them. no more raw string passing in."* Every parameter of
//! every `#[plexus_macros::activation]` in menger is one of these (or an
//! enum of the vocabulary, or a number), and each activation is compiled
//! with `typed_params` (MACRO-12), so a bare `String` parameter does not
//! compile.
//!
//! Each text type is the same shape (ACCESS-6's finding, ACCESS-43):
//!
//! - `#[serde(transparent)]` over its text, so the JSON on the wire is the
//!   string it always was — not a byte changes for the app, the SDK or
//!   synapse;
//! - `Deserialize` through `TryFrom<String>`: deserializing *is* the check,
//!   so a bad value is `InvalidParams` naming the parameter (MACRO-15) at
//!   dispatch, before the method runs, and a value inside the method is
//!   proof that it passed;
//! - `$defs/<Name> { type: string }` behind a `$ref` in the schema, its
//!   `///` the description — synapse shows `--id <string>`, hub-codegen
//!   emits `type TicketRef = string`;
//! - `Display`, `as_str()`, no public field, and **one rule**: the
//!   function its `TryFrom` calls, which answers the canonical text or why
//!   the text is not one.
//!
//! The enums a method takes (`Status`, `Kind`, `Role`, …) keep their
//! derived schema — the closed `enum` of their canonical spellings — and
//! read every spelling their `FromStr` reads ([`lenient!`]): `active` is
//! `Coding`, `in review` is `In Review`, any case.
//!
//! Wasm-safe (serde, schemars, std): the browser app builds every call with
//! these, so a caller that holds a ticket's id as text parses it once, at
//! its edge.

/// The shape every text type takes (the module's list): a transparent
/// `String` newtype whose `Deserialize` goes through `TryFrom<String>` and
/// whose one rule is `$rule: fn(&str) -> Result<String, String>` — the
/// canonical text, or why `text` is not one.
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

            /// A value the store kept, read back — checked by its rule when
            /// it was kept, as [`crate::Code::stored`] is; never text from a
            /// caller (that is `parse`).
            #[allow(dead_code)]
            pub(crate) fn stored(text: impl Into<String>) -> Self {
                Self(text.into())
            }
        }

        /// Kept as its text.
        #[cfg(feature = "server")]
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(&text).map(Self)
            }
        }

        /// From borrowed text: `"MNG-1".parse::<TicketRef>()`.
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

        /// Its text, read as text (`&id` where the store takes `&str`), as
        /// [`crate::Code`] is — never a way to make one.
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        /// ACCESS-48: keyed by its text — a map of these is asked with `&str`.
        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        /// Its text against written text — `field == "status"`.
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.0
            }
        }

        /// Test-only: a literal as one (`"MNG-1".into()`), checked — a
        /// test that means a bad value asks the rule, never this. Never in
        /// a build that serves: there, text becomes one only through its
        /// rule.
        #[cfg(test)]
        impl From<&str> for $name {
            fn from(text: &str) -> Self {
                text.parse().unwrap_or_else(|e| panic!("a test's literal: {e}"))
            }
        }
    };
}

/// An enum of the vocabulary read the way its `FromStr` reads it — every
/// alias it has ever taken (`active`, `in review`, any case) — while its
/// derived `Serialize` and `JsonSchema` keep the canonical spellings: the
/// schema is the closed `enum` (ACCESS-6's `lenient-enum-schema`, on its
/// recommended answer), so synapse lists the values and refuses an alias
/// as a flag before sending; `-p`, the app and REST still send one.
macro_rules! lenient {
    ($($name:ty),+ $(,)?) => {
        $(
            impl<'de> serde::Deserialize<'de> for $name {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let text = <String as serde::Deserialize>::deserialize(d)?;
                    text.parse::<$name>().map_err(serde::de::Error::custom)
                }
            }

            /// Test-only: a literal as one (`"Ready".into()`), checked.
            #[cfg(test)]
            impl From<&str> for $name {
                fn from(text: &str) -> Self {
                    text.parse().unwrap_or_else(|e| panic!("a test's literal: {e}"))
                }
            }
        )+
    };
}

/// Serialize and read back a vocabulary enum as its text (`Display` and
/// `TryFrom<&str>`), with a string schema of its own name.
macro_rules! as_text {
    ($name:ident, $doc:literal) => {
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::try_from(<String as serde::Deserialize>::deserialize(d)?.as_str()).map_err(serde::de::Error::custom)
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                Self::try_from(text.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(text: &str) -> Result<Self, String> {
                Self::try_from(text)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({ "type": "string", "description": $doc })
            }
        }
    };
}

/// The schema an enum a method takes is given (ACCESS-6's
/// `lenient-enum-schema`, its recommended answer): the closed `enum` of
/// its canonical spellings — `$defs/<Name> { type: string, enum: [...] }`,
/// which synapse lists as the flag's values and hub-codegen emits as a
/// union. Written by hand, not derived: a derived schema of an enum whose
/// variants carry docs is a `oneOf` of `const`s, which synapse reads as a
/// tagged union (`need --status.type`).
macro_rules! closed_schema {
    ($name:ident, $doc:literal, [$($v:expr),+ $(,)?]) => {
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                let values: Vec<&'static str> = vec![$($v.as_str()),+];
                schemars::json_schema!({ "type": "string", "description": $doc, "enum": values })
            }
        }
    };
}

mod choices;
mod names;
mod prose;
mod who;

pub use choices::*;
pub use names::*;
pub use prose::*;
pub use who::*;

#[cfg(test)]
mod tests;

/// The refusal every rule words the same way: `` `text` is not a <what>: <shape> ``.
fn not_a(text: &str, what: &str, shape: &str) -> String {
    format!("`{}` is not {what}: {shape}", text.escape_debug())
}

/// Text past `max` bytes, refused in these words.
fn too_long(what: &str, bytes: usize, max: usize) -> String {
    format!("{what} is {bytes} bytes; at most {max}")
}

/// Shared by the rules that take one word: not blank, no whitespace or
/// control character, at most `max` bytes.
fn word<'a>(text: &'a str, what: &str, shape: &str, max: usize) -> Result<&'a str, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(not_a(t, what, shape));
    }
    if t.len() > max {
        return Err(too_long(what, t.len(), max));
    }
    Ok(t)
}
