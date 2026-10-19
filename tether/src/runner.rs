//! The sessions tether runs (TETHER-7, TETHER-9): each a substrate
//! `claudecode` session working a ticket, any number of turns long.
//!
//! A turn runs in a task of tether's holding the `claudecode.chat` stream
//! (through [`Engine`], in memory); its events become the session's tail,
//! kept here and fanned out to every watch. `stop` aborts the task: the
//! client's stream goes, the hub's with it (LOCAL-6), and substrate's
//! executor kills Claude Code when its stream is dropped.
//!
//! TETHER-9: a session is resumed — `resume` runs another turn in the same
//! claudecode session (substrate resumes Claude Code's own), or, while a
//! turn runs, keeps the prompt for the turn after. A session records the
//! inputs it asked (an input that appears open on its ticket while its turn
//! runs, [`Runner::asked`]), so the listener resumes the one that asked.
//! Everything but the tail is kept in tether's state ([`crate::state`]): a
//! restart finds its sessions — a turn it cut short `interrupted` — and
//! runs what was waiting.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_stream::stream;
use futures::{Stream, StreamExt};
use plexus_client::ClientEvent;
use plexus_substrate::activations::claudecode::{ChatEvent, CreateResult, Model};
use tokio::sync::broadcast;

use crate::engine::Engine;
use crate::remote::{now, Remote};
use crate::sessions::{Origin, Session, SessionState, SessionsEvent, TailLine};
use crate::state::{KeptSession, Store};
use crate::vocab::{name_for, Archetype, TicketId};

/// The most of a tail kept per session; a watch after more starts later.
const TAIL_MAX: usize = 10_000;
/// The longest text kept on one line.
const LINE_MAX: usize = 4_000;
/// The longest report kept and posted.
const REPORT_MAX: usize = 30_000;

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
    pending: Vec<String>,
}

impl Entry {
    fn new(session: Session, pending: Vec<String>) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { session, tail: Vec::new(), tx, task: None, pending }
    }
}

/// What a session is started with.
pub struct Start {
    pub ticket: TicketId,
    /// The work: a brief, an answer, a reply (the preamble goes before it).
    pub prompt: String,
    pub working_dir: Option<String>,
    pub model: Model,
    pub origin: Origin,
    pub started_by: String,
    pub archetype: Option<Archetype>,
    /// Post the report here when it ends (a run's `--chat`).
    pub report_chat: Option<u64>,
}

/// What `stop` found.
pub enum Stopped {
    Now(Session),
    Already(Session),
    NotFound,
}

struct Inner {
    engine: Engine,
    work_dir: PathBuf,
    remote: Option<Remote>,
    store: Option<Arc<Store>>,
    all: Mutex<BTreeMap<String, Entry>>,
}

/// Every session tether started.
#[derive(Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

fn short(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut cut = max;
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

/// Two spellings of one ticket: the same last segment (`MENGER/ORG-3`,
/// `hypermemetic:MENGER/TICKETS/ORG-3`, `org-3`).
pub fn same_ticket(a: &str, b: &str) -> bool {
    let last = |s: &str| s.rsplit(['/', ':']).next().unwrap_or(s).to_ascii_uppercase();
    last(a) == last(b)
}

impl Runner {
    /// The sessions run by `engine`, in `work_dir` unless a start names
    /// one; kept in `store` (and restored from it) when there is one; a
    /// report posted on `remote`.
    pub fn new(engine: Engine, work_dir: PathBuf, remote: Option<Remote>, store: Option<Arc<Store>>) -> Self {
        let runner = Self { inner: Arc::new(Inner { engine, work_dir, remote, store, all: Mutex::default() }) };
        runner.restore();
        runner
    }

    pub fn remote(&self) -> Option<&Remote> {
        self.inner.remote.as_ref()
    }

    /// What the store kept: a turn cut short is `interrupted`; a session
    /// with prompts waiting runs them now.
    fn restore(&self) {
        let Some(store) = &self.inner.store else { return };
        let kept = store.kept().sessions;
        let mut all = self.inner.lock();
        for KeptSession { mut session, pending } in kept {
            if session.state == SessionState::Running {
                session.state = SessionState::Interrupted;
                session.ended_at = Some(now());
                session.message = Some("tether stopped while its turn ran".into());
            }
            all.insert(session.name.clone(), Entry::new(session, pending));
        }
        let waiting: Vec<String> = all.iter().filter(|(_, e)| !e.pending.is_empty()).map(|(n, _)| n.clone()).collect();
        for name in waiting {
            let e = all.get_mut(&name).expect("listed");
            let prompt = e.pending.drain(..).collect::<Vec<_>>().join("\n\n---\n\n");
            self.inner.begin(e, prompt);
        }
        self.inner.save(&all);
    }

    /// Start a session: a claudecode session in the substrate, its first
    /// turn running.
    pub async fn start(&self, s: Start) -> Result<Session, String> {
        let name = name_for(&s.ticket).into_string();
        let working_dir = s.working_dir.unwrap_or_else(|| self.inner.work_dir.to_string_lossy().into_owned());
        let created = self.inner.engine.claudecode().create_all(name.clone(), working_dir.clone(), s.model, None, None, None).await;
        let substrate_id = match created.as_deref() {
            Ok([CreateResult::Ok { id, .. }, ..]) => id.to_string(),
            Ok([CreateResult::Err { message }, ..]) => return Err(format!("substrate refused the session: {message}")),
            Ok(_) => return Err("substrate answered nothing".into()),
            Err(e) => return Err(format!("substrate: {e}")),
        };
        let ticket = s.ticket.into_string();
        let prompt = format!("{}{}", crate::prompts::preamble(self.inner.remote.as_ref(), &ticket), s.prompt);
        let session = Session {
            name: name.clone(),
            ticket,
            state: SessionState::Running,
            started_by: s.started_by,
            started_at: now(),
            ended_at: None,
            working_dir,
            model: s.model,
            substrate_id,
            lines: 0,
            message: None,
            origin: s.origin,
            turns: 0,
            asked: Vec::new(),
            archetype: s.archetype,
            report_chat: s.report_chat,
            report: None,
        };
        let mut all = self.inner.lock();
        let e = all.entry(name.clone()).or_insert(Entry::new(session, Vec::new()));
        self.inner.begin(e, prompt);
        let session = e.session.clone();
        self.inner.save(&all);
        Ok(session)
    }

    /// Another turn of `name` with `prompt` — now, or after the turn that
    /// runs. `None`: no such session.
    pub fn resume(&self, name: &str, prompt: String) -> Option<Session> {
        let mut all = self.inner.lock();
        let e = all.get_mut(name)?;
        if e.session.state == SessionState::Running {
            e.pending.push(prompt);
        } else {
            self.inner.begin(e, prompt);
        }
        let session = e.session.clone();
        self.inner.save(&all);
        Some(session)
    }

    /// Stop a running session: its turn and its Claude Code end, and what
    /// waited for its next turn is dropped.
    pub fn stop(&self, name: &str) -> Stopped {
        let mut all = self.inner.lock();
        let Some(e) = all.get_mut(name) else { return Stopped::NotFound };
        if e.session.state != SessionState::Running {
            return Stopped::Already(e.session.clone());
        }
        e.pending.clear();
        Inner::end(e, SessionState::Stopped, None);
        let session = e.session.clone();
        self.inner.save(&all);
        Stopped::Now(session)
    }

    pub fn get(&self, name: &str) -> Option<Session> {
        self.inner.lock().get(name).map(|e| e.session.clone())
    }

    /// Oldest first; only `ticket`'s, only those of `origin`.
    pub fn list(&self, ticket: Option<&str>, origin: Option<Origin>) -> Vec<Session> {
        let mut out: Vec<Session> = self
            .inner
            .lock()
            .values()
            .map(|e| e.session.clone())
            .filter(|s| ticket.is_none_or(|t| same_ticket(&s.ticket, t)) && origin.is_none_or(|o| s.origin == o))
            .collect();
        out.sort_by(|a, b| (a.started_at, &a.name).cmp(&(b.started_at, &b.name)));
        out
    }

    /// An input that appeared open on `ticket`: asked by the session whose
    /// turn runs there (the newest, if several). Its name, when one did.
    pub fn asked(&self, ticket: &str, input: &str) -> Option<String> {
        let mut all = self.inner.lock();
        let name = all
            .values()
            .filter(|e| e.session.state == SessionState::Running && same_ticket(&e.session.ticket, ticket))
            .max_by_key(|e| e.session.started_at)
            .map(|e| e.session.name.clone())?;
        let e = all.get_mut(&name).expect("found");
        if !e.session.asked.iter().any(|a| a == input) {
            e.session.asked.push(input.to_string());
            self.inner.save(&all);
        }
        Some(name)
    }

    /// The session that asked `input` on `ticket` (the newest).
    pub fn holder_of(&self, ticket: &str, input: &str) -> Option<String> {
        self.newest(|s| same_ticket(&s.ticket, ticket) && s.asked.iter().any(|a| a == input))
    }

    /// The newest session on `ticket` that asked anything there.
    pub fn holder_on(&self, ticket: &str) -> Option<String> {
        self.newest(|s| same_ticket(&s.ticket, ticket) && !s.asked.is_empty())
    }

    fn newest(&self, f: impl Fn(&Session) -> bool) -> Option<String> {
        self.inner.lock().values().filter(|e| f(&e.session)).max_by_key(|e| e.session.started_at).map(|e| e.session.name.clone())
    }

    /// A session's tail: its lines from `since`, then each as it comes,
    /// then `ended` when its turn ends with nothing waiting.
    pub fn watch(&self, name: String, since: Option<u64>) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let followed = {
            let all = self.inner.lock();
            all.get(&name).map(|e| {
                let backlog: Vec<TailLine> = e.tail.iter().filter(|l| l.seq >= since.unwrap_or(0)).cloned().collect();
                (e.tx.subscribe(), backlog, e.session.clone())
            })
        };
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

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Entry>> {
        self.all.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Keep every session (under the sessions' lock, so writes keep order).
    fn save(&self, all: &BTreeMap<String, Entry>) {
        if let Some(store) = &self.store {
            let sessions = all.values().map(|e| KeptSession { session: e.session.clone(), pending: e.pending.clone() }).collect();
            store.update(|k| k.sessions = sessions);
        }
    }

    /// A turn of `e` with `prompt`, running.
    fn begin(self: &Arc<Self>, e: &mut Entry, prompt: String) {
        e.session.state = SessionState::Running;
        e.session.turns += 1;
        e.session.ended_at = None;
        e.session.message = None;
        let (inner, name) = (self.clone(), e.session.name.clone());
        let task = tokio::spawn(async move { inner.turn(name, prompt).await });
        e.task = Some(task.abort_handle());
    }

    /// The turn: `claudecode.chat` through the engine, its events the tail;
    /// the text after its last tool call is its report.
    async fn turn(self: Arc<Self>, name: String, prompt: String) {
        let client = self.engine.claudecode();
        let mut chat = client.chat(name.clone(), prompt, None, None);
        let mut outcome = (SessionState::Failed, Some("the turn ended without completing".to_string()));
        let mut report = String::new();
        while let Some(ev) = chat.next().await {
            let ev = match ev {
                Ok(ClientEvent::Data(ev)) => ev,
                Ok(_) => continue,
                Err(e) => ChatEvent::Err { message: e.to_string() },
            };
            match &ev {
                ChatEvent::Content { text } => report.push_str(text),
                ChatEvent::ToolUse { .. } | ChatEvent::ToolResult { .. } => report.clear(),
                ChatEvent::Complete { .. } => outcome = (SessionState::Finished, None),
                ChatEvent::Err { message } => outcome = (SessionState::Failed, Some(message.clone())),
                _ => {}
            }
            let (kind, text) = said(ev);
            self.push(&name, kind, text);
        }
        self.turn_ended(&name, outcome, report);
    }

    fn push(&self, name: &str, kind: &str, text: String) {
        let mut all = self.lock();
        let Some(e) = all.get_mut(name) else { return };
        let line = TailLine { seq: e.session.lines, kind: kind.into(), text: short(text, LINE_MAX) };
        e.session.lines += 1;
        if e.tail.len() >= TAIL_MAX {
            e.tail.remove(0);
        }
        e.tail.push(line.clone());
        let _ = e.tx.send(Tail::Line(line));
    }

    /// End `e`'s turn as `state`, saying so to its watchers.
    fn end(e: &mut Entry, state: SessionState, message: Option<String>) {
        e.session.state = state;
        e.session.ended_at = Some(now());
        e.session.message = message;
        if let Some(task) = e.task.take() {
            task.abort();
        }
        let _ = e.tx.send(Tail::End(e.session.clone()));
    }

    /// A turn ended by itself: the next waiting prompt runs, or the session
    /// ends its turn and a run posts its report.
    fn turn_ended(self: &Arc<Self>, name: &str, (state, message): (SessionState, Option<String>), report: String) {
        let mut all = self.lock();
        let Some(e) = all.get_mut(name) else { return };
        if e.session.state != SessionState::Running {
            return; // stopped while it ran
        }
        e.task = None;
        if !report.trim().is_empty() {
            e.session.report = Some(short(report, REPORT_MAX));
        }
        if !e.pending.is_empty() {
            let prompt = e.pending.drain(..).collect::<Vec<_>>().join("\n\n---\n\n");
            self.begin(e, prompt);
        } else {
            Self::end(e, state, message);
            if let (Some(chat), Some(remote)) = (e.session.report_chat, self.remote.clone()) {
                let s = e.session.clone();
                tokio::spawn(async move {
                    let body = format!(
                        "**{}** — run `{}` {}{}\n\n{}",
                        s.ticket,
                        s.name,
                        s.state.as_str(),
                        s.message.as_deref().map(|m| format!(": {m}")).unwrap_or_default(),
                        s.report.as_deref().unwrap_or("(it said nothing)")
                    );
                    if let Err(e) = remote.post(chat, body).await {
                        tracing::warn!("run {}: its report did not reach chat {chat}: {e}", s.name);
                    }
                });
            }
        }
        self.save(&all);
    }
}
