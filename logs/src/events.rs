//! What the `logs` service streams. Every method returns
//! `impl Stream<Item = LogsEvent>`; a refusal is in band — `refused {
//! reason, message }`, naming the place in a schema, the entry's index and
//! its field where there is one — and so is a log or a ticket not there or
//! not the caller's (`not_found`: one answer for both, ACCESS-10).

use crate::schema::Field;
use serde::{Deserialize, Serialize};

/// One column of a log's table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Column {
    /// The column's (and the field's) name.
    pub name: String,
    /// What it holds: `text`, `integer`, `real`, `boolean` or `json`.
    pub column: String,
    /// The field's JSON Schema type (`string`, `integer`, `number`,
    /// `boolean`, `array`, `object`); an envelope column's own.
    #[serde(rename = "type")]
    pub json_type: String,
    /// Whether every entry carries it.
    pub required: bool,
    /// The server writes it: `seq`, `at`, `by`, `tags`, `ticket`.
    pub envelope: bool,
    /// The values a string field takes, when its schema names them.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "enum")]
    pub values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Closed to new entries, and why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
}

fn envelope(name: &str, column: &str, json_type: &str, required: bool, description: &str) -> Column {
    Column { name: name.into(), column: column.into(), json_type: json_type.into(), required, envelope: true, values: None, description: Some(description.into()), deprecated: None }
}

/// The envelope's columns, first in every log's table.
pub fn envelope_columns() -> Vec<Column> {
    vec![
        envelope("seq", "integer", "integer", true, "the entry's number in its log, from 1"),
        envelope("at", "integer", "integer", true, "when it was appended, unix milliseconds"),
        envelope("by", "text", "string", true, "the principal who appended it"),
        envelope("tags", "json", "array", true, "its tags, a list of strings"),
        envelope("ticket", "text", "string", false, "a ticket it names, by uid"),
    ]
}

impl From<&Field> for Column {
    fn from(f: &Field) -> Self {
        Column {
            name: f.name.clone(),
            column: f.kind.column().into(),
            json_type: f.kind.as_str().into(),
            required: f.required,
            envelope: false,
            values: f.values.clone(),
            description: f.description.clone(),
            deprecated: f.deprecated.clone(),
        }
    }
}

/// Every column of a log whose fields are `fields`: the envelope's, then each field's.
pub fn columns(fields: &[Field]) -> Vec<Column> {
    envelope_columns().into_iter().chain(fields.iter().map(Column::from)).collect()
}

/// What the `logs` service streams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogsEvent {
    /// `define`'s answer: the log is made, tied to the ticket (its id now,
    /// and its uid, which the log keeps), with these columns.
    Defined { log: String, ticket: String, uid: String, columns: Vec<Column> },
    /// `append`'s answer: the entries are kept, numbered `seq` to `last`
    /// (one entry: `last` is `seq`); `bytes` counted against the caller
    /// (`logs.bytes`).
    Appended { log: String, seq: u64, last: u64, count: u64, bytes: u64 },
    /// `describe`'s answer: the log, its ticket's id now, its schema as
    /// last defined or extended, its columns, how many entries it holds,
    /// and whether (and why) it is deprecated.
    Described {
        log: String,
        ticket: String,
        uid: String,
        schema: serde_json::Value,
        columns: Vec<Column>,
        entries: u64,
        created_by: String,
        created_unix_ms: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<String>,
    },
    /// `list`: one log the caller may see.
    Log {
        log: String,
        ticket: String,
        uid: String,
        fields: u64,
        entries: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<String>,
        /// The fields deprecated in it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        deprecated_fields: Vec<String>,
    },
    /// `extend`'s answer: the fields added (optional, each), and every column now.
    Extended { log: String, added: Vec<Column>, columns: Vec<Column> },
    /// `deprecate`'s answer: the log (or its `field`) takes no new entry, and why.
    Deprecated {
        log: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
        message: String,
    },
    /// LOGS-3: `read` and `watch`: one entry — its log, its envelope (`seq`,
    /// `at` in unix ms, `by`, `tags`, `ticket` a uid when it names one), and
    /// the entry as JSON (a field it does not carry is absent).
    Entry {
        log: String,
        seq: u64,
        at: i64,
        by: String,
        tags: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticket: Option<String>,
        entry: serde_json::Value,
    },
    /// LOGS-3: `read`'s last event: how many entries it answered, and —
    /// when there may be more — `next`, given back as `after` for the page
    /// after this one.
    Page {
        count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// LOGS-3: `watch`'s first event: it is open, for these.
    Watching {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
    },
    /// LOGS-3: `watch` fell behind and `missed` entries went by unsaid —
    /// `read` has them.
    Lagged { missed: u64 },
    /// No such log or ticket in the caller's reach — not there and not
    /// theirs, one answer (ACCESS-10).
    NotFound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticket: Option<String>,
    },
    /// Refused, nothing kept: `bad_schema` (`place`, a JSON pointer into
    /// it), `name_taken`, `bad_entry` (`index` of the entry, from 0, and
    /// its `field`), `deprecated` (the log's or the `field`'s), `not_grown`
    /// (`field`), `forbidden`, `too_many`, and the quota service's
    /// (`over_allowance`, …); LOGS-3: `bad_filter` (a `where` its columns
    /// refuse, naming them), `bad_cursor`.
    Refused {
        reason: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },
    /// The service itself failed, in words.
    Error { message: String },
}

impl LogsEvent {
    /// `refused { reason, message }`, nothing else said.
    pub fn refused(reason: &str, message: impl Into<String>) -> Self {
        LogsEvent::Refused { reason: reason.into(), message: message.into(), place: None, index: None, field: None }
    }

    /// `not_found { log }`.
    pub fn no_log(log: &str) -> Self {
        LogsEvent::NotFound { log: Some(log.into()), ticket: None }
    }

    /// `not_found { ticket }`.
    pub fn no_ticket(ticket: &str) -> Self {
        LogsEvent::NotFound { log: None, ticket: Some(ticket.into()) }
    }
}

impl plexus_rpc::client::FromClientError for LogsEvent {
    fn from_client_error(error: plexus_rpc::client::ClientError) -> Self {
        LogsEvent::Error { message: error.to_string() }
    }
}
