//! LOGS-2: **the logs service** — a log is a typed table made from a JSON
//! Schema, appended to and read by name, as a service of v2's own beside
//! `tickets`, `media`, `docs`, `quota` and `code` (V2-1). The owner,
//! 2026-11-16: "we give it a json schema and it builds a table for it? then
//! we can reference those logs dynamically and submit to them."
//!
//! **A log is made from a schema** (`logs.define`): each property a column —
//! typed for a string, an integer, a number or a boolean, JSON for an array
//! or an object ([`schema`]) — in a table of its own (owner: "table per
//! schema"), every row beside it carrying the envelope the server writes:
//! `seq`, `at`, `by`, `tags`, `ticket` ([`store`]). Its file is `logs.db`
//! ([`LOGS_DB_FILE`]; `--logs-db-path`).
//!
//! **A log is tied to a ticket by its uid** (owner: "log tied to a ticket
//! uuid") and takes that ticket's reach: appending is an editor's there,
//! reading a viewer's, defining, extending and deprecating the admin's —
//! each asked of the tickets service as the caller through the seam
//! ([`seam`]), never read from its file.
//!
//! **An entry is checked, then kept** (`logs.append`), all of a call or
//! none, each reserved for the caller through the quota seam (`logs.bytes`,
//! [`Allowance`]). **A schema grows by adding only** (`logs.extend`), and **a
//! log or a field can be deprecated** (`logs.deprecate`): no new entry
//! enters, what was written stays.
//!
//! | module | what |
//! |---|---|
//! | [`vocab`] | the typed parameters: [`LogName`], [`FieldName`], [`Tag`], [`Message`], the JSON texts |
//! | [`schema`] | a JSON Schema read into fields; an entry checked; a schema grown |
//! | [`events`] | [`LogsEvent`], what every method streams; [`Column`] |
//! | [`store`] | the file: the logs, a table a log, the tags |
//! | [`seam`] | [`Reach`], the tickets service asked as the caller; [`Seam`] |
//! | [`allowance`] | [`Allowance`], the one call site for the quota service; [`Unlimited`] |
//! | [`service`] | [`LogsService`], what each method does |
//! | [`activation`] | `logs.*`, the one declaration |

pub mod vocab;
pub use vocab::{EntriesText, EntryText, FieldName, LogName, Message, SchemaText, Tag};
pub mod schema;
pub mod events;
pub use events::{Column, LogsEvent};
pub mod allowance;
pub use allowance::{Allowance, Denied, Unlimited};
pub mod store;
pub use store::LogsStore;
pub mod seam;
pub use seam::{Answer, Reach, Seam, Seen};
mod caller;
pub use caller::{Caller, LogsRequest};
pub mod service;
pub use service::{Clock, LogsService};
pub mod activation;
pub use activation::{LogsActivation, LogsClient};

/// The service's file when `--logs-db-path` names none: `logs.db` beside
/// the tickets file, never the same file.
pub const LOGS_DB_FILE: &str = "logs.db";
