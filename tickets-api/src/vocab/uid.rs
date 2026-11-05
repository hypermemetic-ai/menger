//! TICKETS-35: a ticket's **uid** — the one name it has that nothing ever
//! changes. Owner, 2026-10-30: *"urls should be uuid based not shortcode
//! based"*; its form is TICKETS-11's decision `uid-slug` (owner,
//! 2026-10-20): a short opaque id first in a link, `/t/7k3mq2xa/APP-100-…`.
//!
//! **8 characters of Crockford's base32, lower case** (`0-9` and the letters
//! but `i`, `l`, `o` and `u`): 40 random bits, read case-blind. The store
//! mints one for every ticket at its birth (and, once, for every row a file
//! held before uids), never another ticket's — live or deleted — and never a
//! shortcode's code; it keeps it through every move (`reshortcode`,
//! `rename`, `set_parent`, `delete` and `undelete`, an archive, an epic's
//! move) and never gives it out again.
//!
//! A ticket is named by it bare (`7k3mq2xa`), by the app's path to it
//! (`/t/7k3mq2xa/…`, `#/t/7k3mq2xa/…`) or by the whole link
//! (`https://app.menger.sh/t/7k3mq2xa/PERF-19-…`): [`uid_named`]. Whatever
//! follows the uid in a link is ignored — a slug, a stale one, nothing.
//!
//! Kept in eight bytes, not a `String`: every row of the cache carries one.
//! Its shape on the wire and in the schema is every text type's (the module
//! doc of [`super`]): the JSON is its text, `$defs/Uid { type: string }`,
//! deserializing is the check. The empty uid ([`Uid::default`]) is none — a
//! row from a server before uids, or one a file holds before its first open
//! by this build; it is never minted and never on the wire.

use super::not_a;

/// The alphabet, in digit order: Crockford's base32, lower case.
pub const UID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// How many characters a uid is.
pub const UID_LEN: usize = 8;

/// How many random bits a uid holds: [`UID_LEN`] base32 digits.
pub const UID_BITS: u32 = 40;

/// A ticket's uid (the module doc). `Ord` is its text's.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Uid([u8; UID_LEN]);

/// `text` as a uid's text — trimmed, case-blind, folded to lower case — or
/// why it is not one.
fn uid_rule(text: &str) -> Result<String, String> {
    let t = text.trim();
    let ok = t.len() == UID_LEN && t.bytes().all(|b| UID_ALPHABET.contains(&b.to_ascii_lowercase()));
    match ok {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(not_a(t, "a uid", "8 characters of Crockford's base32 — digits and letters but i, l, o and u — as 7k3mq2xa")),
    }
}

impl Uid {
    /// The uid of these 40 bits (the low ones of `bits`), most significant
    /// digit first — what the store mints from its random draw.
    pub fn from_bits(bits: u64) -> Self {
        let mut out = [0u8; UID_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let shift = 5 * (UID_LEN - 1 - i) as u32;
            *slot = UID_ALPHABET[((bits >> shift) & 31) as usize];
        }
        Self(out)
    }

    /// Its 40 bits — [`Uid::from_bits`] backwards (0 for none).
    pub fn to_bits(&self) -> u64 {
        match self.is_none() {
            true => 0,
            false => self.0.iter().fold(0u64, |acc, b| (acc << 5) | UID_ALPHABET.iter().position(|a| a == b).unwrap_or(0) as u64),
        }
    }

    /// Its text, as it goes on the wire — empty for none.
    pub fn as_str(&self) -> &str {
        match self.is_none() {
            true => "",
            // Only ever bytes of the alphabet ([`uid_rule`], [`Uid::from_bits`]).
            false => std::str::from_utf8(&self.0).unwrap_or_default(),
        }
    }

    /// Its text, owned.
    pub fn into_string(self) -> String {
        self.as_str().to_string()
    }

    /// Whether it is none — a row from before uids ([`Uid::default`]).
    pub fn is_none(&self) -> bool {
        self.0[0] == 0
    }

    /// Whether it is a uid — not none.
    pub fn is_some(&self) -> bool {
        !self.is_none()
    }

    /// A uid the store kept, read back — none when it is not one (a hand
    /// edit's): never text from a caller (that is `parse`).
    pub fn stored(text: &str) -> Self {
        text.parse().unwrap_or_default()
    }
}

/// TICKETS-35: the uid `text` names a ticket by — the uid bare, the app's
/// path to it (`/t/<uid>/…`, `#/t/<uid>/…`, `/app/#/t/<uid>/…`), or the
/// whole link (`https://host/t/<uid>/…`); whatever follows the uid is
/// ignored. `None` for anything else.
pub fn uid_named(text: &str) -> Option<Uid> {
    let t = text.trim();
    if let Ok(u) = t.parse::<Uid>() {
        return Some(u);
    }
    // The whole link: its path, after the scheme and the host.
    let path = match t.strip_prefix("https://").or_else(|| t.strip_prefix("http://")) {
        Some(after) => &after[after.find('/')?..],
        None => t,
    };
    // The app's hash route, and the shell's path.
    let path = ["/app/#", "/app#", "/#", "#"].iter().find_map(|p| path.strip_prefix(p)).unwrap_or(path);
    let rest = path.strip_prefix("/t/")?;
    let uid = rest.split(['/', '?', '#']).next()?;
    uid.parse().ok()
}

impl TryFrom<String> for Uid {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl std::str::FromStr for Uid {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let t = uid_rule(text)?;
        let mut out = [0u8; UID_LEN];
        out.copy_from_slice(t.as_bytes());
        Ok(Self(out))
    }
}

impl serde::Serialize for Uid {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Uid {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::try_from(<String as serde::Deserialize>::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Uid {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Uid".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "TICKETS-35: a ticket's uid — 8 characters of Crockford's base32, lower case (7k3mq2xa), read case-blind; minted at its birth, kept through every move, never another's"
        })
    }
}

impl std::fmt::Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Uid({:?})", self.as_str())
    }
}

/// Hashed as its text, so a map of uids is asked with `&str` ([`Borrow`]).
impl std::hash::Hash for Uid {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.as_str().hash(h)
    }
}

impl AsRef<str> for Uid {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Its text, read as text, as every text type's is — never a way to make one.
impl std::ops::Deref for Uid {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for Uid {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<str> for Uid {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Uid {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for Uid {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl From<Uid> for String {
    fn from(u: Uid) -> String {
        u.into_string()
    }
}

/// Kept as its text; none as `NULL`.
#[cfg(feature = "server")]
impl rusqlite::ToSql for Uid {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self.is_none() {
            true => Ok(rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Null)),
            false => Ok(rusqlite::types::ToSqlOutput::Borrowed(rusqlite::types::ValueRef::Text(&self.0))),
        }
    }
}

/// Read back as the store kept it: `NULL` (a row from before uids) and
/// anything that is not one are none.
#[cfg(feature = "server")]
impl rusqlite::types::FromSql for Uid {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match v {
            rusqlite::types::ValueRef::Null => Ok(Self::default()),
            v => String::column_result(v).map(|t| Self::stored(&t)),
        }
    }
}

/// Test-only: a literal as one (`"7k3mq2xa".into()`), checked.
#[cfg(test)]
impl From<&str> for Uid {
    fn from(text: &str) -> Self {
        text.parse().unwrap_or_else(|e| panic!("a test's literal: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_uid_is_eight_crockford_characters_read_case_blind_kept_lower() {
        let u: Uid = "7K3MQ2XA".parse().unwrap();
        assert_eq!((u.as_str(), u.to_string()), ("7k3mq2xa", "7k3mq2xa".to_string()));
        assert_eq!(serde_json::to_value(u).unwrap(), json!("7k3mq2xa"));
        assert_eq!(serde_json::from_value::<Uid>(json!(" 7k3mq2xa ")).unwrap(), u);
        for bad in ["", "7k3mq2x", "7k3mq2xab", "7k3mq2xi", "7k3mq2xl", "7k3mq2xo", "7k3mq2xu", "7k3m q2x", "7k3mq-xa", "PERF-19"] {
            assert!(bad.parse::<Uid>().is_err(), "{bad:?} accepted");
            assert!(serde_json::from_value::<Uid>(json!(bad)).is_err(), "{bad:?} accepted through serde");
        }
        assert!(Uid::default().is_none() && Uid::default().as_str().is_empty());
        assert_eq!(Uid::stored("not one"), Uid::default());
    }

    #[test]
    fn its_bits_are_its_digits_most_significant_first() {
        assert_eq!(Uid::from_bits(0).as_str(), "00000000");
        assert_eq!(Uid::from_bits(31).as_str(), "0000000z");
        assert_eq!(Uid::from_bits((1 << UID_BITS) - 1).as_str(), "zzzzzzzz");
        assert_eq!(Uid::from_bits(1 << 35).as_str(), "10000000");
        assert_eq!(Uid::from_bits(u64::MAX).as_str(), "zzzzzzzz", "only the low 40 bits");
        for bits in [0u64, 1, 31, 32, 1 << 35, 0xAB_CDEF_0123, (1 << UID_BITS) - 1] {
            assert_eq!(Uid::from_bits(bits).to_bits(), bits);
        }
    }

    #[test]
    fn a_link_names_its_uid_and_ignores_what_follows() {
        let u = Some(Uid::from("7k3mq2xa"));
        for named in [
            "7k3mq2xa",
            " 7K3MQ2XA ",
            "/t/7k3mq2xa",
            "/t/7k3mq2xa/",
            "/t/7k3mq2xa/PERF-19-compression-on-the-websocket",
            "/t/7k3mq2xa?comment=4",
            "#/t/7k3mq2xa/PERF-19-x",
            "/app/#/t/7k3mq2xa/PERF-19-x",
            "https://app.menger.sh/t/7k3mq2xa/PERF-19-compression",
            "https://app.menger.sh/app/#/t/7k3mq2xa/x",
            "http://127.0.0.1:4461/t/7K3MQ2XA",
        ] {
            assert_eq!(uid_named(named), u, "{named:?}");
        }
        for not in ["", "t/7k3mq2xa", "/ticket/7k3mq2xa", "/t/7k3mq2x/x", "https://app.menger.sh/7k3mq2xa", "PERF-19", "hypermemetic:PERF-19", "/t/", "https://app.menger.sh"] {
            assert_eq!(uid_named(not), None, "{not:?}");
        }
    }

    #[test]
    fn a_map_of_uids_is_asked_with_text() {
        let mut m = std::collections::HashMap::new();
        m.insert(Uid::from("7k3mq2xa"), 1);
        assert_eq!(m.get("7k3mq2xa"), Some(&1));
        let mut b = std::collections::BTreeMap::new();
        b.insert(Uid::from("7k3mq2xa"), 1);
        b.insert(Uid::from("00000001"), 2);
        assert_eq!(b.get("7k3mq2xa"), Some(&1));
        assert_eq!(b.keys().map(|u| u.as_str()).collect::<Vec<_>>(), ["00000001", "7k3mq2xa"]);
    }
}
