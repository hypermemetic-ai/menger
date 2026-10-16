//! ACCESS-8: the closed choices a method takes — the vocabulary's enums,
//! read leniently ([`super::lenient!`]), and the few a parameter needed
//! that the vocabulary did not have: where a call comes from, what a
//! severity change is, what becomes of a deleted ticket's children.

use crate::chats::ChatState;
use crate::comments::{CommentIntent, CommentState};
use crate::deployments::DeployState;
use crate::members::Role;
use crate::notes::NoteState;
use crate::queue::Urgency;
use crate::themes::{Takeover, ThemeReach};
use crate::types::{AnswerSource, InputState, Kind, Severity, Status, TransitionSource};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl InputSource {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        AnswerSource::from_input(self).as_str()
    }
}

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

lenient!(Status, Kind, Severity, InputState, AnswerSource, TransitionSource, Role, CommentState, NoteState, DeployState, Urgency, ThemeReach, Takeover);
lenient!(CallerSource, InputSource, DeleteChildren, AmendedField, CommentIntent, ChatState);

closed_schema!(Status, "A ticket's status — the canonical spellings; `active` (Coding) and `in review` are read too, any case", [
    Status::Pending, Status::Ready, Status::Coding, Status::InReview, Status::Blocked, Status::Complete,
    Status::Idea, Status::Epic, Status::Superseded, Status::Canceled, Status::Doc,
]);
closed_schema!(Kind, "What shape of work a ticket is — the owner's five, and doc (any case)", [
    Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike, Kind::Doc,
]);
closed_schema!(Severity, "How bad a gap is (any case)", [Severity::Critical, Severity::High, Severity::Medium, Severity::Low]);
closed_schema!(InputState, "Where an input stands (any case)", [
    InputState::Open, InputState::Answered, InputState::Closed, InputState::Removed, InputState::Withdrawn,
]);
closed_schema!(Role, "A role a grant gives (any case)", [Role::Viewer, Role::Deployer, Role::Editor, Role::Admin]);
closed_schema!(CommentIntent, "What a comment is: a comment, or a change request (any case)", [CommentIntent::Comment, CommentIntent::Changes]);
closed_schema!(CommentState, "Where a comment stands (any case)", [CommentState::Open, CommentState::Resolved]);
closed_schema!(ChatState, "Where a chat stands: open, or closed (read-only) (any case)", [ChatState::Open, ChatState::Closed]);
closed_schema!(NoteState, "Where a note stands: still a note, or made a ticket (any case)", [NoteState::Open, NoteState::Upgraded]);
closed_schema!(DeployState, "What a deployment did (any case; rolled-back too)", [
    DeployState::Started, DeployState::Succeeded, DeployState::Failed, DeployState::RolledBack,
]);
closed_schema!(Urgency, "How urgently the owner should look (any case)", [Urgency::Whenever, Urgency::Today, Urgency::Now, Urgency::Blocking]);
closed_schema!(ThemeReach, "How far a theme's adoption reaches (any case)", [ThemeReach::Frame, ThemeReach::Ticket, ThemeReach::Site]);
closed_schema!(Takeover, "Whether a level takes the themes from above (any case; true/yes, false/no too)", [Takeover::On, Takeover::Off]);
closed_schema!(CallerSource, "Where a write comes from: app, api (default) or pipeline (synapse is api, merge pipeline)", [
    CallerSource::App, CallerSource::Api, CallerSource::Pipeline,
]);
closed_schema!(InputSource, "Who answers or clears an input: app (a viewer's click) or api (default)", [InputSource::App, InputSource::Api]);
closed_schema!(DeleteChildren, "What becomes of a deleted ticket's children: reparent or delete", [DeleteChildren::Reparent, DeleteChildren::Delete]);

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

/// INPUT-3: who answers or clears an input — `app` (a viewer's click) or
/// `api` (the default). `system` is the store's own (a withdrawal, a
/// restore) and no caller's to say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    App,
    #[default]
    Api,
}

impl AnswerSource {
    /// What an [`InputSource`] says, as the store keeps it.
    pub const fn from_input(s: InputSource) -> Self {
        match s {
            InputSource::App => AnswerSource::App,
            InputSource::Api => AnswerSource::Api,
        }
    }
}

impl From<InputSource> for AnswerSource {
    fn from(s: InputSource) -> Self {
        Self::from_input(s)
    }
}

impl fmt::Display for InputSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InputSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.parse::<AnswerSource>()? {
            AnswerSource::App => Ok(Self::App),
            AnswerSource::Api => Ok(Self::Api),
            AnswerSource::System => Err("unknown answer source 'system' (expected app or api: system is the store's own)".into()),
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
