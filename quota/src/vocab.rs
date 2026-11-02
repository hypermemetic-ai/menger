//! The service's vocabulary: what it streams ([`QuotaEvent`]) and its typed
//! parameters (ACCESS-8, `typed_params`) — each a transparent text whose
//! `Deserialize` is its one rule, so a bad value is `invalid_params` naming
//! the parameter before the method runs.

use serde::{Deserialize, Serialize};

/// One source of a principal's usage, as `usage` and `table` carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SourceUse {
    /// The source, e.g. media.bytes.
    pub source: String,
    /// bytes | count | calls.
    pub unit: String,
    /// total | day (a day's counter starts at nothing each UTC day).
    pub window: String,
    /// Committed so far (in this window).
    pub used: u64,
    /// Reserved, not yet committed (an upload in flight).
    pub held: u64,
    /// The cap; none: unlimited.
    pub cap: Option<u64>,
    /// Where the cap came from: own | team:<name> | default | exempt.
    pub from: String,
}

/// What the `quota` service streams. A refusal is in band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuotaEvent {
    /// `reserve`'s answer: held until `expires` (unix seconds) — commit or
    /// release it by `ticket`; `remaining` under the cap after it (none:
    /// unlimited).
    Reserved { ticket: String, remaining: Option<u64>, expires: i64 },
    /// `reserve` refused: `used` (with what is reserved) + `asked` would pass `cap`. Nothing held.
    OverAllowance { source: String, used: u64, cap: u64, asked: u64, message: String },
    /// `reserve` refused: `open` reservations of `source` in flight, at most `max` at once.
    TooManyAtOnce { source: String, open: u32, max: u32, message: String },
    /// `reserve` refused: the data disk is past the full line. Nothing held.
    BoxFull { used_percent: f64, full_percent: f64, message: String },
    /// `commit`'s answer: `amount` counted; `used` after it.
    Committed { ticket: String, principal: String, source: String, amount: u64, used: u64 },
    /// `release`'s answer: nothing counted.
    Released { ticket: String, principal: String, source: String, amount: u64 },
    /// `credit`'s answer: `used` after the bytes came back.
    Credited { principal: String, source: String, used: u64 },
    /// `usage` and `table`: one principal's usage of every source.
    /// `bytes` is the sum of its byte sources counted for good (what it stores).
    Usage { principal: String, exempt: bool, bytes: u64, sources: Vec<SourceUse> },
    /// `table`, first: how full the data disk is against the plan's lines (state ok | warn | full | unknown).
    Box { used_percent: Option<f64>, warn_percent: f64, full_percent: f64, state: crate::disk::BoxState },
    /// `table`, last: the sum per source over every row, and how many rows.
    Total { principals: u32, bytes: u64, sources: Vec<SourceUse> },
    /// `set`'s answer: the allowance as it stands (`holder` principal:<p> | team:<t> | default; cap none: unlimited, or `cleared`).
    Allowance { holder: String, source: String, cap: Option<u64>, cleared: bool },
    /// `recount`: a principal's total of a source was `was`, the store counts `counted`.
    Drift { principal: String, source: String, was: u64, counted: u64 },
    /// `recount`, last: what was counted, what drifted, and the sources no store answered for (and why).
    Recounted { principals: u32, drifted: u32, sources: Vec<String>, not_counted: Vec<String> },
    /// A refusal in words: nothing was changed.
    Error { message: String },
}

impl From<crate::Refusal> for QuotaEvent {
    fn from(r: crate::Refusal) -> Self {
        let message = r.to_string();
        match r {
            crate::Refusal::OverAllowance { source, used, cap, asked } => QuotaEvent::OverAllowance { source, used, cap, asked, message },
            crate::Refusal::TooManyAtOnce { source, open, max } => QuotaEvent::TooManyAtOnce { source, open, max, message },
            crate::Refusal::BoxFull { used_percent, full_percent } => QuotaEvent::BoxFull { used_percent, full_percent, message },
            crate::Refusal::UnknownSource { .. } | crate::Refusal::Failed(_) => QuotaEvent::Error { message },
        }
    }
}

macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $rule:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn into_string(self) -> String {
                self.0
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
                <String as serde::Deserialize>::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

fn source(t: &str) -> Result<String, String> {
    let t = t.trim();
    match crate::sources::source(t) {
        Some(s) => Ok(s.name.to_string()),
        None => Err(format!("`{t}` is no quota source (the sources: {})", crate::sources::names())),
    }
}

text_type!(
    /// A source by name: media.bytes, media.fetch.bytes, media.fetch.calls, media.egress.bytes, docs.bytes, docs.count, tickets.bytes, tickets.count, questions.count.
    SourceName,
    source
);

fn cap(t: &str) -> Result<String, String> {
    let t = t.trim();
    if t.eq_ignore_ascii_case("clear") || t.eq_ignore_ascii_case("inherit") {
        return Ok("clear".into());
    }
    crate::plan::parse_cap(t).map(|_| t.to_string())
}

text_type!(
    /// A cap: unlimited, a number (300), or a number with a unit (2 GB, 200MB, 1.5GiB); `clear` removes the allowance (it falls back).
    CapText,
    cap
);

impl CapText {
    /// The cap; `None` for `clear`.
    pub fn cap(&self) -> Option<crate::plan::Cap> {
        (self.0 != "clear").then(|| crate::plan::parse_cap(&self.0).expect("read when it was made"))
    }
}

fn ticket(t: &str) -> Result<String, String> {
    let t = t.trim();
    match t.len() == 32 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(format!("`{t}` is not a reservation's ticket: 32 hex digits, as `reserve` answered it")),
    }
}

text_type!(
    /// A reservation's ticket: 32 hex digits, as `reserve` answered it.
    HoldTicket,
    ticket
);

fn principal(t: &str) -> Result<String, String> {
    let t = t.trim();
    let ok = !t.is_empty() && t.len() <= 512 && t.contains(':') && !t.chars().any(|c| c.is_whitespace() || c.is_control());
    match ok {
        true => Ok(t.to_string()),
        false => Err(format!("`{t}` is not a principal: provider:subject, e.g. key:4b…, idp:c952…")),
    }
}

text_type!(
    /// A Plexus principal: provider:subject (key:4b…, idp:c952…, oidc:workos/user_01…).
    PrincipalName,
    principal
);

fn word(t: &str) -> Result<String, String> {
    let t = t.trim();
    let ok = !t.is_empty() && t.len() <= 64 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    match ok {
        true => Ok(t.to_string()),
        false => Err(format!("`{t}` is not a name: letters, digits, - _ . (at most 64)")),
    }
}

text_type!(
    /// A team's name, e.g. release-crew.
    TeamRef,
    word
);

text_type!(
    /// An organization, e.g. acme.
    OrgRef,
    word
);

fn sort(t: &str) -> Result<String, String> {
    let t = t.trim();
    if matches!(t, "bytes" | "principal") || source(t).is_ok() {
        return Ok(t.to_string());
    }
    Err(format!("`{t}` is not a sort: bytes (the default, most first), principal, or a source's name (its use, most first)"))
}

text_type!(
    /// How `table` sorts: bytes (stored, most first; the default), principal, or a source's name (its use, most first).
    SortKey,
    sort
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spellings_hold_and_refuse() {
        assert!("media.bytes".parse::<SourceName>().is_ok());
        assert!("media.nothing".parse::<SourceName>().is_err());
        assert_eq!("2 GB".parse::<CapText>().unwrap().cap(), Some(crate::plan::Cap::At(2_000_000_000)));
        assert_eq!("clear".parse::<CapText>().unwrap().cap(), None);
        assert!("lots".parse::<CapText>().is_err());
        assert!("ab".repeat(16).parse::<HoldTicket>().is_ok());
        assert!("abc".parse::<HoldTicket>().is_err());
        assert!("key:4b".parse::<PrincipalName>().is_ok());
        assert!("nobody".parse::<PrincipalName>().is_err());
        assert!("tickets.bytes".parse::<SortKey>().is_ok() && "principal".parse::<SortKey>().is_ok());
        assert!("size".parse::<SortKey>().is_err());
        assert!(serde_json::from_str::<SourceName>("\"x\"").is_err(), "deserializing is the check");
    }

    #[test]
    fn a_refusal_is_its_typed_event() {
        let ev: QuotaEvent = crate::Refusal::OverAllowance { source: "media.bytes".into(), used: 10, cap: 12, asked: 5 }.into();
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!((v["type"].as_str(), v["used"].as_u64(), v["cap"].as_u64(), v["asked"].as_u64()), (Some("over_allowance"), Some(10), Some(12), Some(5)), "{v}");
        let v = serde_json::to_value(QuotaEvent::from(crate::Refusal::BoxFull { used_percent: 90.0, full_percent: 85.0 })).unwrap();
        assert_eq!(v["type"], "box_full");
    }
}

impl plexus_client::FromClientError for QuotaEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        QuotaEvent::Error { message: error.to_string() }
    }
}
