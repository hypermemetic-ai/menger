//! The typed parameters (question 5): wasm-safe — serde + schemars + std.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// A ticket's id, `SHORT-N` (upper-cased on parse).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct TicketId(String);

impl TicketId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TicketId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let up = s.trim().to_ascii_uppercase();
        let (code, seq) = up
            .rsplit_once('-')
            .ok_or_else(|| format!("`{s}` is not a ticket id (SHORT-N)"))?;
        let code_ok = !code.is_empty() && code.len() <= 32 && code.bytes().all(|b| b.is_ascii_alphanumeric());
        if !code_ok || seq.parse::<u32>().is_err() {
            return Err(format!("`{s}` is not a ticket id (SHORT-N)"));
        }
        Ok(TicketId(up))
    }
}

impl<'de> Deserialize<'de> for TicketId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        TicketId::try_from(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for TicketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A ticket's status — canonical spellings; `active` and `in review` are
/// read too.
///
/// The schema: by default the derived closed `enum` of the canonical
/// spellings; `--features schema-anyof` / `schema-examples` swap in the two
/// open shapes question 5 compares (`run.sh` runs all three).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(not(any(feature = "schema-anyof", feature = "schema-examples")), derive(JsonSchema))]
pub enum Status {
    Pending,
    Ready,
    Coding,
    #[serde(rename = "In Review")]
    InReview,
    Complete,
}

impl Status {
    pub fn parse(s: &str) -> Option<Status> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Status::Pending,
            "ready" => Status::Ready,
            "coding" | "active" => Status::Coding,
            "in review" | "in_review" | "inreview" => Status::InReview,
            "complete" | "done" => Status::Complete,
            _ => return None,
        })
    }
}

impl<'de> Deserialize<'de> for Status {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Status::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "unknown status `{s}`: expected one of Pending, Ready, Coding, In Review, Complete"
            ))
        })
    }
}

#[cfg(any(feature = "schema-anyof", feature = "schema-examples"))]
impl JsonSchema for Status {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Status".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        #[cfg(feature = "schema-anyof")]
        return schemars::json_schema!({
            "description": "A ticket's status — canonical spellings; `active` and `in review` are read too.",
            "anyOf": [
                { "type": "string", "enum": ["Pending", "Ready", "Coding", "In Review", "Complete"] },
                { "type": "string", "description": "another spelling the server reads: `active`, `in review`, any case" }
            ]
        });
        #[cfg(not(feature = "schema-anyof"))]
        return schemars::json_schema!({
            "description": "A ticket's status — canonical spellings; `active` and `in review` are read too.",
            "type": "string",
            "examples": ["Pending", "Ready", "Coding", "In Review", "Complete"]
        });
    }
}

/// What the spike's methods answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpikeEvent {
    /// A ticket, as the caller sees it.
    Ticket { id: TicketId, status: Status, by: String },
    /// An input answered.
    Answered { ticket: TicketId, value: Status, by: String },
    /// In-band refusal.
    Error { message: String },
}
