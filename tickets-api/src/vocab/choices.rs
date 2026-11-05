//! ACCESS-8: the closed choices a method takes — the vocabulary's enums,
//! read leniently ([`super::lenient!`]), and the few a parameter needed
//! that the vocabulary did not have: where a call comes from, what a
//! severity change is, what becomes of a deleted ticket's children.

use crate::members::Role;
use crate::queue::Urgency;
use crate::types::{AnswerSource, InputState, Kind, Severity, Status, TransitionSource};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl DeleteChildren {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            DeleteChildren::Reparent => "reparent",
            DeleteChildren::Delete => "delete",
        }
    }
}
use std::fmt;
use std::str::FromStr;

lenient!(Status, Kind, Severity, InputState, AnswerSource, TransitionSource, Role, Urgency);
lenient!(CallerSource, DeleteChildren, AmendedField, RevisionField);

closed_schema!(Status, "A ticket's status — the canonical spellings; `active` (Coding) and `in review` are read too, any case", [
    Status::Pending, Status::Ready, Status::Coding, Status::InReview, Status::Blocked, Status::Complete,
    Status::Draft, Status::Epic, Status::Superseded, Status::Canceled, Status::Doc,
]);
closed_schema!(InputState, "Where an input stands (any case)", [
    InputState::Open, InputState::Answered, InputState::Closed, InputState::Removed, InputState::Withdrawn,
]);
closed_schema!(Urgency, "How urgently the owner should look (any case)", [Urgency::Whenever, Urgency::Today, Urgency::Now, Urgency::Blocking]);
closed_schema!(Kind, "What shape of work a ticket is — the owner's five, and doc (any case)", [
    Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike, Kind::Doc,
]);
closed_schema!(Severity, "How bad a gap is (any case)", [Severity::Critical, Severity::High, Severity::Medium, Severity::Low]);
closed_schema!(Role, "A role a grant gives (any case)", [Role::Chatter, Role::Viewer, Role::Deployer, Role::Editor, Role::Admin]);
closed_schema!(CallerSource, "Where a write comes from: app, api (default) or pipeline (synapse is api, merge pipeline)", [
    CallerSource::App, CallerSource::Api, CallerSource::Pipeline,
]);
closed_schema!(DeleteChildren, "What becomes of a deleted ticket's children: reparent or delete", [DeleteChildren::Reparent, DeleteChildren::Delete]);
closed_schema!(RevisionField, "A field a restore puts back: title, body, status, kind, severity, parent, blocked_by or labels", [
    RevisionField::Title, RevisionField::Body, RevisionField::Status, RevisionField::Kind,
    RevisionField::Severity, RevisionField::Parent, RevisionField::BlockedBy, RevisionField::Labels,
]);

/// TICKETS-7: a field `revisions restore` puts back from a revision — every
/// one a revision keeps but the commits, which are linked, never restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionField {
    Title,
    Body,
    Status,
    Kind,
    Severity,
    Parent,
    BlockedBy,
    Labels,
}

impl RevisionField {
    /// The wire spelling — a revision's field name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Body => "body",
            Self::Status => "status",
            Self::Kind => "kind",
            Self::Severity => "severity",
            Self::Parent => "parent",
            Self::BlockedBy => "blocked_by",
            Self::Labels => "labels",
        }
    }
}

impl fmt::Display for RevisionField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RevisionField {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "title" => Ok(Self::Title),
            "body" => Ok(Self::Body),
            "status" => Ok(Self::Status),
            "kind" => Ok(Self::Kind),
            "severity" => Ok(Self::Severity),
            "parent" => Ok(Self::Parent),
            "blocked_by" | "blockers" => Ok(Self::BlockedBy),
            "labels" => Ok(Self::Labels),
            "commits" => Err("commits are linked, never restored".to_string()),
            other => Err(format!("field '{other}': title, body, status, kind, severity, parent, blocked_by or labels")),
        }
    }
}

/// MENGER-13: where a write says it comes from — `app` (a viewer in the
/// app), `api` (synapse, an agent, a script: the default), `pipeline` (the
/// merge). `cascade` is the store's own and no caller's to say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerSource {
    App,
    #[default]
    Api,
    Pipeline,
}

impl CallerSource {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            CallerSource::App => "app",
            CallerSource::Api => "api",
            CallerSource::Pipeline => "pipeline",
        }
    }
}

impl From<CallerSource> for TransitionSource {
    fn from(s: CallerSource) -> Self {
        match s {
            CallerSource::App => TransitionSource::App,
            CallerSource::Api => TransitionSource::Api,
            CallerSource::Pipeline => TransitionSource::Pipeline,
        }
    }
}

impl fmt::Display for CallerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// [`TransitionSource`]'s spellings (`synapse` is `api`, `merge` is
/// `pipeline`), less the store's own.
impl FromStr for CallerSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.parse::<TransitionSource>()? {
            TransitionSource::App => Ok(Self::App),
            TransitionSource::Api => Ok(Self::Api),
            TransitionSource::Pipeline => Ok(Self::Pipeline),
            TransitionSource::Cascade => Err("source: `cascade` is the store's own — a call is app, api or pipeline".into()),
        }
    }
}

/// MENGER-30: what `update` does to a severity — sets one, or clears it
/// (`none` on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeverityChange {
    Set(Severity),
    Clear,
}

impl SeverityChange {
    /// What the ticket's severity becomes.
    pub const fn severity(self) -> Option<Severity> {
        match self {
            SeverityChange::Set(s) => Some(s),
            SeverityChange::Clear => None,
        }
    }

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            SeverityChange::Set(s) => s.as_str(),
            SeverityChange::Clear => "none",
        }
    }
}

impl fmt::Display for SeverityChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SeverityChange {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim() {
            t if t.eq_ignore_ascii_case("none") => Ok(Self::Clear),
            t => t.parse::<Severity>().map(Self::Set).map_err(|e| format!("{e} — or none, to clear it")),
        }
    }
}

impl Serialize for SeverityChange {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SeverityChange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for SeverityChange {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SeverityChange".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A severity to set, or none to clear it",
            "enum": ["Critical", "High", "Medium", "Low", "none"]
        })
    }
}

/// ACT-5: what becomes of a deleted ticket's children — each moves to the
/// nearest ancestor that stays (`reparent`), or the whole subtree goes
/// with it (`delete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteChildren {
    Reparent,
    Delete,
}

impl FromStr for DeleteChildren {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "reparent" => Ok(Self::Reparent),
            "delete" => Ok(Self::Delete),
            other => Err(format!("children '{other}': reparent (they move to its parent) or delete (they go too)")),
        }
    }
}

/// MNG-16: what an `amend` moved — its title, its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AmendedField {
    Title,
    Body,
}

impl AmendedField {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            AmendedField::Title => "title",
            AmendedField::Body => "body",
        }
    }
}

/// Its wire spelling against written text — `changed == ["title"]`.
impl PartialEq<str> for AmendedField {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for AmendedField {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for AmendedField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AmendedField {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "title" => Ok(Self::Title),
            "body" => Ok(Self::Body),
            other => Err(format!("unknown amended field '{other}' (expected title or body)")),
        }
    }
}

/// PERF-15: a field of the summary row (`TicketSummary`) a page may be thinned
/// to — `list`, `work.ready` and `find` take `fields` (`id,title,kind,status`:
/// the board's four), and each row then carries those and its `id`, nothing
/// else. Its wire spelling is the row's own key.
macro_rules! summary_fields {
    ($($v:ident => $s:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum SummaryField { $($v),+ }

        impl SummaryField {
            /// Every field, in the row's order.
            pub const ALL: &'static [SummaryField] = &[$(SummaryField::$v),+];

            /// The wire spelling — the summary row's key.
            pub const fn as_str(self) -> &'static str {
                match self { $(SummaryField::$v => $s),+ }
            }
        }

        impl FromStr for SummaryField {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, String> {
                match s.trim().to_ascii_lowercase().as_str() {
                    $($s => Ok(SummaryField::$v),)+
                    other => Err(format!("field '{other}': a summary row's field — {}", [$($s),+].join(", "))),
                }
            }
        }
    };
}

summary_fields!(
    Uid => "uid", UidOf => "uid_of", Id => "id", Epic => "epic", Shortcode => "shortcode", Address => "address", Seq => "seq", Title => "title",
    Status => "status", Kind => "kind", Severity => "severity", Parent => "parent", ParentHidden => "parent_hidden",
    ChildCount => "child_count", BlockedBy => "blocked_by", Unlocks => "unlocks", HiddenBlockedBy => "hidden_blocked_by",
    HiddenUnlocks => "hidden_unlocks", Commits => "commits", Path => "path", Placement => "placement", Labels => "labels",
    Archived => "archived", Frozen => "frozen", Hidden => "hidden", CreatedUnix => "created_unix", UpdatedUnix => "updated_unix",
);

lenient!(SummaryField);

impl fmt::Display for SummaryField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl JsonSchema for SummaryField {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SummaryField".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let values: Vec<&'static str> = SummaryField::ALL.iter().map(|f| f.as_str()).collect();
        schemars::json_schema!({ "type": "string", "description": "A field of the summary row (TicketSummary's keys)", "enum": values })
    }
}
