//! `sessions` (TETHER-7, TETHER-5's surface): agent sessions on this
//! machine, each a substrate `claudecode` session tether started for a
//! ticket — `start`, `say`, `stop`, `list`, `watch`. The running of them is
//! [`crate::runner`]'s; this file keeps their vocabulary and the methods.
//!
//! First slice: every signed-in viewer sees every session on the machine;
//! an editor starts, resumes and stops them. A session acting as an
//! AGENT-1 agent (TETHER-5) and a sandboxed one (TETHER-6) come later.

use futures::Stream;
use plexus_client::ClientError;
use plexus_core::plexus::AuthContext;
use plexus_substrate::activations::claudecode::Model;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::runner::{Runner, Start, Stopped};
use crate::vocab::{Archetype, Prompt, SessionName, TicketId, WorkDir};

/// Where a session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// A turn is running.
    Running,
    /// Its last turn completed; another may resume it.
    Finished,
    /// `stop` ended it.
    Stopped,
    /// Its last turn ended in an error (`message` says which).
    Failed,
    /// TETHER-9: tether stopped while its turn ran (a restart finds it so).
    Interrupted,
}

impl SessionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionState::Running => "running",
            SessionState::Finished => "finished",
            SessionState::Stopped => "stopped",
            SessionState::Failed => "failed",
            SessionState::Interrupted => "interrupted",
        }
    }
}

/// TETHER-9: what started a session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// `sessions.start`.
    #[default]
    Start,
    /// `runs.start`: a brief, an archetype, a report.
    Run,
    /// The listener: an input answered that no session of tether's asked.
    Answer,
    /// The listener: a change request or a comment on a queued ticket.
    Reply,
}

/// One agent session on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// tether's name for it — also its `claudecode` session's name.
    pub name: String,
    /// The ticket it works.
    pub ticket: String,
    pub state: SessionState,
    /// The principal that started it (`tether` for the listener's).
    pub started_by: String,
    /// Unix seconds.
    pub started_at: i64,
    /// When its last turn ended.
    pub ended_at: Option<i64>,
    pub working_dir: String,
    pub model: Model,
    /// Its `claudecode` session's id in the substrate inside tether.
    pub substrate_id: String,
    /// Lines in its tail so far.
    pub lines: u64,
    /// Why it failed, when it did.
    pub message: Option<String>,
    /// TETHER-9: what started it.
    #[serde(default)]
    pub origin: Origin,
    /// TETHER-9: its turns so far (a resume is another).
    #[serde(default)]
    pub turns: u32,
    /// TETHER-9: the inputs it asked on its ticket — an answer to one
    /// resumes it.
    #[serde(default)]
    pub asked: Vec<String>,
    /// TETHER-9: a run's archetype.
    #[serde(default)]
    pub archetype: Option<Archetype>,
    /// TETHER-9: the chat its report goes to when it ends.
    #[serde(default)]
    pub report_chat: Option<u64>,
    /// TETHER-9: what its last turn said last (after its last tool call).
    #[serde(default)]
    pub report: Option<String>,
}

/// One line of a session's tail: a `claudecode.chat` event, said short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TailLine {
    /// From 0, in order.
    pub seq: u64,
    /// `start`, `content`, `thinking`, `tool_use`, `tool_result`,
    /// `tool_error`, `complete`, `passthrough` or `error`.
    pub kind: String,
    pub text: String,
}

/// What `sessions` and `runs` answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsEvent {
    /// `start`: the session is running.
    Started { session: Session },
    /// `list`: one session; `stop` of one already ended; `say`: the
    /// session, its turn running or the prompt waiting for the next.
    Session { session: Session },
    /// `stop`: it was running, and is stopped.
    Stopped { session: Session },
    /// `watch`: a line of its tail.
    Line { name: String, line: TailLine },
    /// `watch`: its turn ended with nothing waiting; the watch ends with it.
    Ended { session: Session },
    /// No session of that name.
    NotFound { name: String },
    Error { message: String },
}

impl plexus_client::FromClientError for SessionsEvent {
    fn from_client_error(error: ClientError) -> Self {
        SessionsEvent::Error { message: error.to_string() }
    }
}

/// The principal behind a call.
pub fn principal(auth: &AuthContext) -> String {
    auth.principal().map(|p| p.to_string()).unwrap_or_else(|| auth.user_id.clone())
}

/// `stop`'s answer.
pub fn stopped(name: String, s: Stopped) -> SessionsEvent {
    match s {
        Stopped::Now(session) => SessionsEvent::Stopped { session },
        Stopped::Already(session) => SessionsEvent::Session { session },
        Stopped::NotFound => SessionsEvent::NotFound { name },
    }
}

/// The `sessions` activation.
#[derive(Clone)]
pub struct SessionsActivation {
    runner: Runner,
}

impl SessionsActivation {
    pub fn new(runner: Runner) -> Self {
        Self { runner }
    }
}

#[plexus_macros::activation(
    version = "0.2.0",
    description = "Agent sessions on this machine, run by the substrate inside tether",
    roles(viewer = ["list", "watch"], editor = [viewer, "start", "say", "stop"]),
    typed_params,
)]
impl SessionsActivation {
    /// Start an agent session working a ticket: a claudecode session in
    /// the substrate inside tether, its first turn running.
    #[plexus_macros::method]
    pub async fn start(
        &self,
        auth: &AuthContext,
        /// The ticket the session works (e.g. MENGER-12).
        ticket: TicketId,
        /// The first prompt (default: work the ticket).
        prompt: Option<Prompt>,
        /// The directory it works in, absolute (default: tether's --work-dir).
        working_dir: Option<WorkDir>,
        /// opus, sonnet or haiku (default: opus).
        model: Option<Model>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let (runner, started_by) = (self.runner.clone(), principal(auth));
        let prompt = prompt.map(Prompt::into_string).unwrap_or_else(|| format!("Work ticket {ticket}."));
        let start = Start { ticket, prompt, working_dir: working_dir.map(WorkDir::into_string), model: model.unwrap_or(Model::Opus), origin: crate::sessions::Origin::Start, started_by, archetype: None, report_chat: None };
        futures::stream::once(async move {
            match runner.start(start).await {
                Ok(session) => SessionsEvent::Started { session },
                Err(message) => SessionsEvent::Error { message },
            }
        })
    }

    /// TETHER-9: say something to a session — another turn now, or after
    /// the turn that runs.
    #[plexus_macros::method]
    pub async fn say(
        &self,
        /// The session's name.
        name: SessionName,
        /// What to tell it.
        prompt: Prompt,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let name = name.into_string();
        let ev = match self.runner.resume(&name, prompt.into_string()) {
            Some(session) => SessionsEvent::Session { session },
            None => SessionsEvent::NotFound { name },
        };
        futures::stream::iter([ev])
    }

    /// Stop a running session: its turn and its Claude Code end.
    #[plexus_macros::method]
    pub async fn stop(
        &self,
        /// The session's name.
        name: SessionName,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let name = name.into_string();
        futures::stream::iter([stopped(name.clone(), self.runner.stop(&name))])
    }

    /// Every session on this machine, oldest first.
    #[plexus_macros::method]
    pub async fn list(
        &self,
        /// Only this ticket's.
        ticket: Option<TicketId>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let all = self.runner.list(ticket.as_ref().map(TicketId::as_str), None);
        futures::stream::iter(all.into_iter().map(|session| SessionsEvent::Session { session }))
    }

    /// A session's tail: its lines from `since`, then each as it comes,
    /// then `ended` when its turn ends with nothing waiting.
    #[plexus_macros::method(streaming)]
    pub async fn watch(
        &self,
        /// The session's name.
        name: SessionName,
        /// The first line wanted (default 0: the whole tail kept).
        since: Option<u64>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        self.runner.watch(name.into_string(), since)
    }
}
