//! The `logs` activation — **the one declaration**, as `tickets`, `media`
//! and `code` are: the `Activation` impl, dispatch, the schema and the
//! methods' bodies from this `impl`, and `LogsClient<T>` for a client of it.
//! Each method is its attribute, its caller and a call into [`LogsService`]
//! ([`crate::service`]), whose answer it streams.
//!
//! **Who may** (LOGS-1 › reach): at the gate, `describe`, `list`, `read`
//! and `watch` a viewer's, `append` an editor's, and `define`, `extend` and `deprecate`
//! the admin's (listed in no role: only `admin` holds them) — each held
//! anywhere; then the log's ticket decides, asked of the tickets service as
//! the caller: the same role on that ticket, or the call is refused
//! (`forbidden`) or the log is not found (a ticket the caller cannot see).

use crate::caller::{Caller, Callers, LogsRequest};
use crate::events::LogsEvent;
use crate::service::{entries_of, LogsService, ReadArgs};
use crate::vocab::{Cursor, EntriesText, EntryText, FieldName, LogName, Message, Principal, SchemaText, Tag, When, WhereText};
use futures::{Stream, StreamExt};
use menger_tickets_api::vocab::TicketRef;
use std::sync::Arc;

/// The activation: the service, and the resolver every caller is verified by.
#[derive(Clone)]
pub struct LogsActivation {
    logs: Arc<LogsService>,
    callers: Callers,
}

impl LogsActivation {
    /// Over `logs`.
    pub fn new(logs: Arc<LogsService>) -> Self {
        Self { logs, callers: Callers }
    }

    /// The service it answers from.
    pub fn logs(&self) -> &Arc<LogsService> {
        &self.logs
    }
}

/// Logs: a typed table made from a JSON Schema, tied to a ticket and taking its reach; entries appended by name, each checked and kept with its envelope.
#[plexus_rpc::macros::activation(
    auth = "required",
    request = LogsRequest,
    typed_params,
    roles(
        viewer = ["describe", "list", "read", "watch"],
        editor = [viewer, "append"],
    )
)]
impl LogsActivation {
    /// Make a log from a JSON Schema of an object, tied to a ticket; yields defined { log, ticket, uid, columns }.
    ///
    /// Each property becomes a column: `string`, `integer`, `number` and
    /// `boolean` typed, `array` and `object` JSON; `required`, `enum` (on a
    /// string), `title` and `description` are taken, and so is
    /// `additionalProperties: false`. Anything else — `oneOf`, `anyOf`,
    /// `allOf`, `$ref`, `pattern`, bounds, `items`, a nested `properties`, a
    /// root that is not an object, a field named like the envelope (`seq`,
    /// `at`, `by`, `tags`, `ticket`) or not a lower-case identifier — is
    /// refused `bad_schema`, its `place` a JSON pointer into the schema, and
    /// nothing is made. The log keeps the ticket's uid, so it follows the
    /// ticket through every move, and takes its reach: you must be admin on
    /// that ticket (`forbidden` otherwise; `not_found { ticket }` for one you
    /// cannot see). A name taken is refused `name_taken`. Admin.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "the perf log", params = r#"{"name": "perf", "on": "LAYERS-9", "schema": "{\"type\": \"object\", \"required\": [\"app\", \"value\"], \"properties\": {\"app\": {\"type\": \"string\"}, \"value\": {\"type\": \"number\"}}}"}"#)
    )]
    pub async fn define(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log's name: a lower-case letter, then lower-case letters, digits and _, e.g. perf.
        name: LogName,
        /// The ticket it is tied to, any name of it, e.g. LAYERS-9 or its uid.
        on: TicketRef,
        /// The JSON Schema, as JSON text, e.g. --schema @perf.schema.json.
        schema: SchemaText,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        futures::stream::once(async move { logs.define(&user, name.as_str(), on.as_str(), schema.as_str()).await })
    }

    /// Append entries to a log, each checked against its schema; yields appended { log, seq, last, count, bytes }.
    ///
    /// One entry (`entry`, a JSON object) or many (`entries`: JSON lines, or
    /// a JSON array — at most 10,000), each kept with the envelope the
    /// server writes: `seq` (its number in the log), `at` (unix ms), `by`
    /// (you), `tags` (at most 32) and `ticket` (a ticket's uid, given by any
    /// name). Every entry is checked — each field's type, `required`,
    /// `enum`, no field the schema does not name, no deprecated field — and
    /// one that does not fit refuses the whole call `bad_entry`, naming its
    /// `index` (from 0) and its `field`; nothing of it is kept. A deprecated
    /// log refuses every append (`deprecated`, with its message). The bytes
    /// count against your allowance (`logs.bytes`). You must be editor on the
    /// log's ticket. Editor.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "one perf result", params = r#"{"log": "perf", "entry": "{\"app\": \"plexus\", \"value\": 812000}", "tags": ["plexus", "websocket"]}"#)
    )]
    pub async fn append(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log, by name, e.g. perf.
        log: LogName,
        /// One entry, a JSON object as text, e.g. --entry @result.json (optional: or entries).
        entry: Option<EntryText>,
        /// Many entries, JSON lines or a JSON array, e.g. --entries @results.jsonl (optional: or entry).
        entries: Option<EntriesText>,
        /// Tags every entry of this call carries, e.g. plexus,websocket (optional).
        tags: Option<Vec<Tag>>,
        /// A ticket the entries are about, any name of it, kept by its uid (optional).
        ticket: Option<TicketRef>,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        let tags: Vec<String> = tags.unwrap_or_default().into_iter().map(Tag::into_string).collect();
        futures::stream::once(async move {
            match entries_of(entry.as_ref().map(EntryText::as_str), entries.as_ref().map(EntriesText::as_str)) {
                Ok(all) => logs.append(&user, log.as_str(), all, tags, ticket.as_ref().map(TicketRef::as_str)).await,
                Err(ev) => ev,
            }
        })
    }

    /// A log's schema, columns and count; yields described { log, ticket, uid, schema, columns, entries, created_by, created_unix_ms, deprecated? }.
    ///
    /// `ticket` is the log's ticket's id now (a ticket moved is still its
    /// ticket); `columns` the envelope's, then each field's, each marked
    /// `deprecated` with its message when it is; `deprecated` the log's own.
    /// A log on a ticket you cannot see is `not_found { log }`, as one that
    /// is none. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the perf log", params = r#"{"log": "perf"}"#))]
    pub async fn describe(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log, by name, e.g. perf.
        log: LogName,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        futures::stream::once(async move { logs.describe(&user, log.as_str()).await })
    }

    /// Every log whose ticket you can see, by name; yields log { log, ticket, uid, fields, entries, deprecated?, deprecated_fields? } per log.
    ///
    /// A log on a ticket you cannot see is not named. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "what logs there are", params = r#"{}"#))]
    pub async fn list(&self, #[from_auth(self.callers.verify)] user: Caller) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        futures::stream::once(async move { logs.list(&user).await }).flat_map(futures::stream::iter)
    }

    /// Grow a log's schema by adding optional fields; yields extended { log, added, columns }.
    ///
    /// The new schema names every field the log has, each of its type, none
    /// newly required and no `enum` narrowed; each field it adds is optional
    /// — an entry before it reads it as null. A field dropped, renamed,
    /// retyped or newly required is refused `not_grown`, naming it, and
    /// nothing is changed; a schema not taken is `bad_schema`. You must be
    /// admin on the log's ticket. Admin.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "an optional p99", params = r#"{"log": "perf", "schema": "{\"type\": \"object\", \"required\": [\"app\", \"value\"], \"properties\": {\"app\": {\"type\": \"string\"}, \"value\": {\"type\": \"number\"}, \"p99\": {\"type\": \"number\"}}}"}"#)
    )]
    pub async fn extend(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log, by name, e.g. perf.
        log: LogName,
        /// The grown JSON Schema, as JSON text, e.g. --schema @perf2.schema.json.
        schema: SchemaText,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        futures::stream::once(async move { logs.extend(&user, log.as_str(), schema.as_str()).await })
    }

    /// Close a log, or one field of it, to new entries; yields deprecated { log, field?, message }.
    ///
    /// A deprecated log refuses every append, saying so and why; a
    /// deprecated field refuses an entry that carries it, and an entry
    /// without it is kept. What was written stays readable; `list` and
    /// `describe` mark both. No method reopens either. One deprecated
    /// already is refused `deprecated`, with the message it has. You must be
    /// admin on the log's ticket. Admin.
    #[plexus_rpc::macros::method(group = "write", example(note = "a log replaced", params = r#"{"log": "perf", "message": "replaced by perf2"}"#))]
    pub async fn deprecate(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log, by name, e.g. perf.
        log: LogName,
        /// One field of it, e.g. machine (optional: the whole log).
        field: Option<FieldName>,
        /// Why, in words, e.g. replaced by perf2.
        message: Message,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        futures::stream::once(async move { logs.deprecate(&user, log.as_str(), field.as_ref().map(FieldName::as_str), message.as_str()).await })
    }

    /// Read a log's entries newest first, filtered on its fields and its envelope — or every log's you can see; yields entry { log, seq, at, by, tags, ticket?, entry } per entry, then page { count, next? }.
    ///
    /// With `log`, its entries newest first; `filter` filters on its fields
    /// and the envelope's `seq`, `at`, `by` and `ticket` (a uid) — a field,
    /// `=` `!=` `<` `<=` `>` `>=` or `in (…)`, a quoted string, a number,
    /// `true`, `false` or `null`, joined by `and`, `or`, `not` and
    /// parentheses; a field the log has not is refused `bad_filter`, naming
    /// those it has. With no `log`, every log whose ticket you can see,
    /// newest first by `at`, each entry with its log's name — `filter` then on
    /// the envelope alone. `tag`, `since` and `until` (an age, 7d; a date,
    /// 2026-11-16; unix ms), `by` and `ticket` narrow either. A page is
    /// `limit` entries (100; at most 1,000); `next`, given back as `after`,
    /// is the page after. A log on a ticket you cannot see is `not_found`.
    /// Viewer.
    #[plexus_rpc::macros::method(
        group = "read",
        example(note = "one scenario over time", params = r#"{"log": "perf", "filter": "app = \"plexus\" and scenario = \"stream.10k.64B\" and transport = \"websocket\"", "limit": 20}"#),
        example(note = "across logs, by tag, this week", params = r#"{"tag": "plexus", "since": "7d"}"#)
    )]
    #[allow(clippy::too_many_arguments)]
    pub async fn read(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log, by name, e.g. perf (optional: every log you can see).
        log: Option<LogName>,
        /// A filter on its fields and the envelope, e.g. app = "plexus" and value > 1000 (optional). LOGS-1's `--where`: `where` is a word Rust keeps.
        filter: Option<WhereText>,
        /// Only entries carrying this tag, e.g. plexus (optional).
        tag: Option<Tag>,
        /// Only entries from this moment on: an age (7d), a date (2026-11-16) or unix ms (optional).
        since: Option<When>,
        /// Only entries before this moment, as since (optional).
        until: Option<When>,
        /// Only entries this principal appended, e.g. key:54a9… (optional).
        by: Option<Principal>,
        /// Only entries naming this ticket, any name of it (optional).
        ticket: Option<TicketRef>,
        /// How many entries a page holds (optional: 100; at most 1,000).
        limit: Option<u32>,
        /// Where to go on from: the `next` a page answered (optional).
        after: Option<Cursor>,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        let logs = self.logs.clone();
        let args = ReadArgs { log: log.map(LogName::into_string), filter: filter.map(WhereText::into_string), tag: tag.map(Tag::into_string), since: since.map(When::into_string), until: until.map(When::into_string), by: by.map(Principal::into_string), ticket: ticket.map(TicketRef::into_string), limit, after: after.map(Cursor::into_string) };
        futures::stream::once(async move { logs.read(&user, args).await }).flat_map(futures::stream::iter)
    }

    /// Each entry as it is appended, to a log or to every log you can see; yields watching { log?, tag? }, then entry { log, seq, at, by, tags, ticket?, entry } per entry.
    ///
    /// Open until you close it. With `log`, its entries alone (`not_found`
    /// for a log you cannot see); with `tag`, those carrying it. An entry is
    /// told you only while you can see its log's ticket, asked of the
    /// tickets service again at most every 5 seconds a log. `lagged {
    /// missed }` when you fell more than 1,024 appends behind: `read` has
    /// what went by. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "the perf log, live", params = r#"{"log": "perf"}"#))]
    pub async fn watch(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The log, by name, e.g. perf (optional: every log you can see).
        log: Option<LogName>,
        /// Only entries carrying this tag, e.g. plexus (optional).
        tag: Option<Tag>,
    ) -> impl Stream<Item = LogsEvent> + Send + 'static {
        self.logs.clone().watch(user, log.map(LogName::into_string), tag.map(Tag::into_string))
    }
}
