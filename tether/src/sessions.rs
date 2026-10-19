//! `sessions` (TETHER-7, TETHER-5's surface): agent sessions on this
//! machine, each a substrate `claudecode` session tether started for a
//! ticket — `start`, `stop`, `list`, `watch`.
//!
//! A session's turn runs in a task of tether's holding the `claudecode.chat`
//! stream (through [`Engine`], in memory); its events become the session's
//! tail, kept here and fanned out to every `watch`. `stop` aborts the task:
//! the client's stream goes, the hub's with it (LOCAL-6), and substrate's
//! executor kills Claude Code when its stream is dropped.
//!
//! First slice: every signed-in viewer sees every session on the machine;
//! an editor starts and stops them. A session acting as an AGENT-1 agent
//! (TETHER-5) and a sandboxed one (TETHER-6) come later.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_stream::stream;
use futures::{Stream, StreamExt};
use plexus_client::{ClientError, ClientEvent};
use plexus_core::plexus::AuthContext;
use plexus_substrate::activations::claudecode::{ChatEvent, CreateResult, Model};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::engine::Engine;
use crate::vocab::{name_for, Prompt, SessionName, TicketId, WorkDir};

/// Where a session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Its turn is running.
    Running,
    /// Its turn completed.
    Finished,
    /// `stop` ended it.
    Stopped,
    /// Its turn ended in an error (`message` says which).
    Failed,
}

/// One agent session on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// tether's name for it — also its `claudecode` session's name.
    pub name: String,
    /// The ticket it works.
    pub ticket: String,
    pub state: SessionState,
    /// The principal that started it.
    pub started_by: String,
    /// Unix seconds.
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub working_dir: String,
    pub model: Model,
    /// Its `claudecode` session's id in the substrate inside tether.
    pub substrate_id: String,
    /// Lines in its tail so far.
    pub lines: u64,
    /// Why it failed, when it did.
    pub message: Option<String>,
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

/// What `sessions` answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionsEvent {
    /// `start`: the session is running.
    Started { session: Session },
    /// `list`: one session; `stop` of one already ended.
    Session { session: Session },
    /// `stop`: it was running, and is stopped.
    Stopped { session: Session },
    /// `watch`: a line of its tail.
    Line { name: String, line: TailLine },
    /// `watch`: the session ended; the watch ends with it.
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

/// The most of a tail kept per session; a watch after more starts later.
const TAIL_MAX: usize = 10_000;
/// The longest text kept on one line.
const LINE_MAX: usize = 4_000;

#[derive(Clone)]
enum Tail {
    Line(TailLine),
    End(Session),
}

struct Entry {
    session: Session,
    tail: Vec<TailLine>,
    tx: broadcast::Sender<Tail>,
    task: Option<tokio::task::AbortHandle>,
}

/// Every session tether started, while it runs.
#[derive(Default)]
struct Registry(Mutex<BTreeMap<String, Entry>>);

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn short(mut s: String) -> String {
    if s.len() > LINE_MAX {
        let mut cut = LINE_MAX;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

/// A chat event as a tail line's kind and text.
fn said(ev: ChatEvent) -> (&'static str, String) {
    match ev {
        ChatEvent::Start { id, .. } => ("start", id.to_string()),
        ChatEvent::Content { text } => ("content", text),
        ChatEvent::Thinking { thinking } => ("thinking", thinking),
        ChatEvent::ToolUse { tool_name, input, .. } => ("tool_use", format!("{tool_name} {input}")),
        ChatEvent::ToolResult { output, is_error, .. } => (if is_error { "tool_error" } else { "tool_result" }, output),
        ChatEvent::Complete { claude_session_id, .. } => ("complete", claude_session_id),
        ChatEvent::Passthrough { event_type, .. } => ("passthrough", event_type),
        ChatEvent::Err { message } => ("error", message),
    }
}

impl Registry {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Entry>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn add(&self, session: Session) {
        let (tx, _) = broadcast::channel(1024);
        self.lock().insert(session.name.clone(), Entry { session, tail: Vec::new(), tx, task: None });
    }

    fn hold(&self, name: &str, task: tokio::task::AbortHandle) {
        if let Some(e) = self.lock().get_mut(name) {
            e.task = Some(task);
        }
    }

    fn get(&self, name: &str) -> Option<Session> {
        self.lock().get(name).map(|e| e.session.clone())
    }

    fn push(&self, name: &str, kind: &str, text: String) {
        let mut all = self.lock();
        let Some(e) = all.get_mut(name) else { return };
        let line = TailLine { seq: e.session.lines, kind: kind.into(), text: short(text) };
        e.session.lines += 1;
        if e.tail.len() >= TAIL_MAX {
            e.tail.remove(0);
        }
        e.tail.push(line.clone());
        let _ = e.tx.send(Tail::Line(line));
    }

    /// End a running session as `state`; the session as it ends, or `None`
    /// when it had already ended (or is not here).
    fn end(&self, name: &str, state: SessionState, message: Option<String>) -> Option<Session> {
        let mut all = self.lock();
        let e = all.get_mut(name)?;
        if e.session.state != SessionState::Running {
            return None;
        }
        e.session.state = state;
        e.session.ended_at = Some(now());
        e.session.message = message;
        if let Some(task) = e.task.take() {
            task.abort();
        }
        let _ = e.tx.send(Tail::End(e.session.clone()));
        Some(e.session.clone())
    }

    fn list(&self, ticket: Option<&str>) -> Vec<Session> {
        let mut out: Vec<Session> =
            self.lock().values().map(|e| e.session.clone()).filter(|s| ticket.is_none_or(|t| s.ticket == t)).collect();
        out.sort_by(|a, b| (a.started_at, &a.name).cmp(&(b.started_at, &b.name)));
        out
    }

    /// A watch's start: its feed, the tail from `since`, and the session —
    /// read under one lock, so no line falls between them.
    fn follow(&self, name: &str, since: u64) -> Option<(broadcast::Receiver<Tail>, Vec<TailLine>, Session)> {
        let all = self.lock();
        let e = all.get(name)?;
        let backlog = e.tail.iter().filter(|l| l.seq >= since).cloned().collect();
        Some((e.tx.subscribe(), backlog, e.session.clone()))
    }
}

/// The `sessions` activation.
#[derive(Clone)]
pub struct SessionsActivation {
    engine: Engine,
    registry: Arc<Registry>,
    work_dir: PathBuf,
}

impl SessionsActivation {
    /// Sessions run by `engine`, in `work_dir` unless a start names one.
    pub fn new(engine: Engine, work_dir: PathBuf) -> Self {
        Self { engine, registry: Arc::default(), work_dir }
    }

    /// The turn: `claudecode.chat` through the engine, its events the tail.
    fn run(&self, name: String, prompt: String) -> tokio::task::AbortHandle {
        let engine = self.engine.clone();
        let registry = self.registry.clone();
        tokio::spawn(async move {
            let client = engine.claudecode();
            let mut chat = client.chat(name.clone(), prompt, None, None);
            let mut outcome = (SessionState::Failed, Some("the turn ended without completing".to_string()));
            while let Some(ev) = chat.next().await {
                let ev = match ev {
                    Ok(ClientEvent::Data(ev)) => ev,
                    Ok(_) => continue,
                    Err(e) => ChatEvent::Err { message: e.to_string() },
                };
                match &ev {
                    ChatEvent::Complete { .. } => outcome = (SessionState::Finished, None),
                    ChatEvent::Err { message } => outcome = (SessionState::Failed, Some(message.clone())),
                    _ => {}
                }
                let (kind, text) = said(ev);
                registry.push(&name, kind, text);
            }
            registry.end(&name, outcome.0, outcome.1);
        })
        .abort_handle()
    }
}

fn principal(auth: &AuthContext) -> String {
    auth.principal().map(|p| p.to_string()).unwrap_or_else(|| auth.user_id.clone())
}

#[plexus_macros::activation(
    version = "0.1.0",
    description = "Agent sessions on this machine, run by the substrate inside tether",
    roles(viewer = ["list", "watch"], editor = [viewer, "start", "stop"]),
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
        let me = self.clone();
        let started_by = principal(auth);
        stream! {
            let name = name_for(&ticket).into_string();
            let working_dir = working_dir.map(WorkDir::into_string).unwrap_or_else(|| me.work_dir.to_string_lossy().into_owned());
            let model = model.unwrap_or(Model::Opus);
            let prompt = prompt.map(Prompt::into_string).unwrap_or_else(|| format!("Work ticket {ticket}."));
            let created = me.engine.claudecode().create_all(name.clone(), working_dir.clone(), model, None, None, None).await;
            let substrate_id = match created.as_deref() {
                Ok([CreateResult::Ok { id, .. }, ..]) => id.to_string(),
                Ok([CreateResult::Err { message }, ..]) => {
                    yield SessionsEvent::Error { message: format!("substrate refused the session: {message}") };
                    return;
                }
                Ok(_) => {
                    yield SessionsEvent::Error { message: "substrate answered nothing".into() };
                    return;
                }
                Err(e) => {
                    yield SessionsEvent::Error { message: format!("substrate: {e}") };
                    return;
                }
            };
            let session = Session {
                name: name.clone(),
                ticket: ticket.into_string(),
                state: SessionState::Running,
                started_by,
                started_at: now(),
                ended_at: None,
                working_dir,
                model,
                substrate_id,
                lines: 0,
                message: None,
            };
            me.registry.add(session.clone());
            let task = me.run(name.clone(), prompt);
            me.registry.hold(&name, task);
            yield SessionsEvent::Started { session };
        }
    }

    /// Stop a running session: its turn and its Claude Code end.
    #[plexus_macros::method]
    pub async fn stop(
        &self,
        /// The session's name.
        name: SessionName,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let name = name.into_string();
        let ev = match self.registry.end(&name, SessionState::Stopped, None) {
            Some(session) => SessionsEvent::Stopped { session },
            None => match self.registry.get(&name) {
                Some(session) => SessionsEvent::Session { session },
                None => SessionsEvent::NotFound { name },
            },
        };
        futures::stream::iter([ev])
    }

    /// Every session on this machine, oldest first.
    #[plexus_macros::method]
    pub async fn list(
        &self,
        /// Only this ticket's.
        ticket: Option<TicketId>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let all = self.registry.list(ticket.as_ref().map(TicketId::as_str));
        futures::stream::iter(all.into_iter().map(|session| SessionsEvent::Session { session }))
    }

    /// A session's tail: its lines from `since`, then each as it comes,
    /// then `ended` when the session ends.
    #[plexus_macros::method(streaming)]
    pub async fn watch(
        &self,
        /// The session's name.
        name: SessionName,
        /// The first line wanted (default 0: the whole tail kept).
        since: Option<u64>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let name = name.into_string();
        let followed = self.registry.follow(&name, since.unwrap_or(0));
        stream! {
            let Some((mut rx, backlog, session)) = followed else {
                yield SessionsEvent::NotFound { name };
                return;
            };
            let mut next = since.unwrap_or(0);
            for line in backlog {
                next = line.seq + 1;
                yield SessionsEvent::Line { name: name.clone(), line };
            }
            if session.state != SessionState::Running {
                yield SessionsEvent::Ended { session };
                return;
            }
            loop {
                match rx.recv().await {
                    Ok(Tail::Line(line)) if line.seq >= next => {
                        next = line.seq + 1;
                        yield SessionsEvent::Line { name: name.clone(), line };
                    }
                    Ok(Tail::Line(_)) => {}
                    Ok(Tail::End(session)) => {
                        yield SessionsEvent::Ended { session };
                        return;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        yield SessionsEvent::Error { message: format!("the watch fell {n} lines behind; watch again with since") };
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}
