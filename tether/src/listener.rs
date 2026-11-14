//! TETHER-9: tether listens to the remote menger and acts on what should
//! start or resume work — signed in with its agent key ([`Remote`]).
//!
//! It holds `tickets.watch` and `comments.watch` on one connection, and:
//!
//! - **an input answered** (`input_changed`, state `answered`): the session
//!   of tether's that asked it ([`Runner::holder_of`]) is resumed with the
//!   answer at once. With none, tether waits `grace` — an agent outside
//!   tether (the chat bridge, a hand-launched run, `await_reply`) acts and
//!   closes its own input in that time — reads the input again, and if it
//!   is still answered so, starts a session on the ticket that acts on the
//!   answer and closes the input.
//! - **a queue reply** (APP-23/24's kinds): a change request (a comment of
//!   intent `changes`), or a comment on a ticket in the queue or one a
//!   session of tether's asked on — handled the same way: its holder
//!   resumed, else after `grace` a session started.
//! - **an input appearing open** on a ticket whose session's turn runs:
//!   that session asked it ([`Runner::asked`]).
//!
//! Who counts: an answer given in the app (`source: app` — a person), and a
//! comment by a person; an answer through the API, or a comment by a key,
//! only with `accept_keys` (a test's, or an owner who answers from a
//! shell) — tether's own key never. Each answer and comment is acted on
//! once (its key kept in [`crate::state`]), and the position survives a
//! restart: on every (re)connect it reads the answered inputs and the
//! comments since its mark, so what came while it was away is acted on, and
//! a backlog from before it first listened never is.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use menger_tickets_api::{AnswerSource, Comment, CommentIntent, CommentsClient, CommentsEvent, Input, InputState, TicketsClient, TicketsEvent};
use plexus_client::{ClientEvent, WsTransport};
use plexus_substrate::activations::claudecode::Model;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::remote::{now, Remote};
use crate::runner::{Runner, Start};
use crate::sessions::Origin;
use crate::state::{Position, Store};
use crate::vocab::Archetype;

/// How tether listens.
#[derive(Debug, Clone)]
pub struct ListenConfig {
    /// Act on answers through the API and comments by keys too.
    pub accept_keys: bool,
    /// How long an answer or reply no session of tether's holds waits for
    /// another agent to act on it before tether starts one.
    pub grace: Duration,
    /// CHAT-45's archetypes file: a session the listener starts is a
    /// builder, on the builder's model.
    pub archetypes: std::path::PathBuf,
}

/// What `listen.status` says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListenStatus {
    /// The remote's socket.
    pub remote: String,
    /// Who tether is there.
    pub principal: String,
    /// The watches are held now.
    pub connected: bool,
    /// Unix seconds: the current connection's start, or the last one's.
    pub connected_at: Option<i64>,
    /// Why the last connection ended or failed.
    pub last_error: Option<String>,
    /// Answers given before this are not acted on (its first listen).
    pub since: Option<i64>,
    /// Events heard on the watches since tether started.
    pub heard: u64,
    /// Answers and replies acted on since tether started.
    pub acted: u64,
    /// What it last did, in words.
    pub last_acted: Option<String>,
    /// Waiting out the grace now.
    pub waiting: u64,
    /// Tickets in the owner's queue, as last heard.
    pub queued: u64,
    pub accept_keys: bool,
    pub grace_secs: u64,
}

/// What a reply is.
enum Heard {
    Answer { input: Input },
    Comment { ticket: String, comment: Comment },
}

pub struct Listener {
    remote: Remote,
    runner: Runner,
    store: Arc<Store>,
    cfg: ListenConfig,
    status: Mutex<ListenStatus>,
    queued: Mutex<HashSet<String>>,
    waiting: Mutex<HashSet<String>>,
}

fn data<E>(ev: Option<Result<ClientEvent<E>, plexus_client::ClientError>>, what: &str) -> Result<Option<E>, String> {
    match ev {
        None => Err(format!("the {what} watch ended")),
        Some(Err(e)) => Err(format!("the {what} watch: {e}")),
        Some(Ok(ClientEvent::Data(e))) => Ok(Some(e)),
        Some(Ok(_)) => Ok(None),
    }
}

impl Listener {
    pub fn new(remote: Remote, runner: Runner, store: Arc<Store>, cfg: ListenConfig) -> Arc<Self> {
        let status = ListenStatus {
            remote: remote.url().into(),
            principal: remote.principal().into(),
            accept_keys: cfg.accept_keys,
            grace_secs: cfg.grace.as_secs(),
            ..Default::default()
        };
        Arc::new(Self { remote, runner, store, cfg, status: Mutex::new(status), queued: Mutex::default(), waiting: Mutex::default() })
    }

    pub fn status(&self) -> ListenStatus {
        let mut s = self.status.lock().unwrap_or_else(|p| p.into_inner()).clone();
        s.waiting = self.waiting.lock().unwrap_or_else(|p| p.into_inner()).len() as u64;
        s.queued = self.queued.lock().unwrap_or_else(|p| p.into_inner()).len() as u64;
        s.since = self.store.kept().listen.since;
        s
    }

    fn say(&self, f: impl FnOnce(&mut ListenStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|p| p.into_inner()));
    }

    /// Listen until the process ends: hold the watches, and when they go,
    /// connect again (1 s, doubling to 60 s; reset once a connection held).
    pub fn spawn(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                let started = std::time::Instant::now();
                let why = me.hold().await.err().unwrap_or_else(|| "the watches ended".into());
                me.say(|s| {
                    s.connected = false;
                    s.last_error = Some(why.clone());
                });
                tracing::warn!("listening on {}: {why}; again in {}s", me.remote.url(), backoff.as_secs());
                if started.elapsed() > Duration::from_secs(60) {
                    backoff = Duration::from_secs(1);
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        })
    }

    fn position(&self) -> Position {
        self.store.kept().listen
    }

    /// One connection: the watches subscribed first, then the queue and
    /// what came while away read, then every event as it comes.
    async fn hold(self: &Arc<Self>) -> Result<(), String> {
        let t = self.remote.connect().await?;
        let tickets = TicketsClient::new(t.clone());
        let comments = CommentsClient::new(t.clone());
        let mut tw = tickets.watch(None, None, None, None, None, None);
        let mut cw = comments.watch();
        self.start_position(&comments).await?;
        let rows = tickets.queued_all(Some(true)).await.map_err(|e| format!("the queue: {e}"))?;
        *self.queued.lock().unwrap_or_else(|p| p.into_inner()) =
            rows.into_iter().filter_map(|e| match e { TicketsEvent::QueueRow { entry, .. } => Some(entry.ticket), _ => None }).collect();
        self.catch_up(&tickets, &comments).await?;
        self.say(|s| {
            s.connected = true;
            s.connected_at = Some(now());
            s.last_error = None;
        });
        tracing::info!("listening on {} as {}", self.remote.url(), self.remote.principal());
        loop {
            tokio::select! {
                ev = tw.next() => if let Some(ev) = data(ev, "tickets")? { self.on_tickets(ev, &tickets, &comments).await? },
                ev = cw.next() => if let Some(CommentsEvent::Added { comment }) = data(ev, "comments")? { self.heard(); self.on_comment(comment) },
            }
        }
    }

    fn heard(&self) {
        self.say(|s| s.heard += 1);
    }

    /// The first listen on this remote: from now, after the newest comment.
    async fn start_position(&self, comments: &CommentsClient<WsTransport>) -> Result<(), String> {
        let p = self.position();
        if p.remote.as_deref() == Some(self.remote.url()) && p.since.is_some() && p.comment_mark.is_some() {
            return Ok(());
        }
        let newest = comments
            .list_all(None, None, None, None, None, None, None, None, None, None, Some(1))
            .await
            .map_err(|e| format!("the comments: {e}"))?
            .into_iter()
            .find_map(|e| match e { CommentsEvent::Comment { id, .. } => Some(id), _ => None })
            .unwrap_or(0);
        let url = self.remote.url().to_string();
        self.store.update(|k| {
            if k.listen.remote.as_deref() != Some(url.as_str()) {
                k.listen = Position { remote: Some(url), ..Default::default() };
            }
            k.listen.since.get_or_insert(now());
            k.listen.comment_mark.get_or_insert(newest);
        });
        Ok(())
    }

    /// What came while tether was away (or behind): every input answered,
    /// every comment after the mark, oldest first.
    async fn catch_up(self: &Arc<Self>, tickets: &TicketsClient<WsTransport>, comments: &CommentsClient<WsTransport>) -> Result<(), String> {
        let answered = tickets.inputs().list_all(None, None, Some(InputState::Answered), None, None).await.map_err(|e| format!("the answered inputs: {e}"))?;
        for ev in answered {
            if let TicketsEvent::Input { input } = ev {
                self.on_answer(input);
            }
        }
        let mark = self.position().comment_mark.unwrap_or(0);
        let mut newer = Vec::new();
        let mut after = None;
        'pages: loop {
            let page = comments
                .list_all(None, None, None, None, None, None, None, None, None, after, Some(500))
                .await
                .map_err(|e| format!("the comments: {e}"))?;
            let mut next = None;
            for ev in page {
                match ev {
                    CommentsEvent::Page { next: n } => next = n,
                    CommentsEvent::Comment { id, .. } if id <= mark => break 'pages,
                    ev => newer.extend(ev.into_comment()),
                }
            }
            match next {
                Some(n) => after = Some(menger_tickets_api::vocab::CommentId::new(n)),
                None => break,
            }
        }
        newer.sort_by_key(|c| c.id.get());
        for c in newer {
            self.on_comment(c);
        }
        Ok(())
    }

    async fn on_tickets(self: &Arc<Self>, ev: TicketsEvent, tickets: &TicketsClient<WsTransport>, comments: &CommentsClient<WsTransport>) -> Result<(), String> {
        self.heard();
        match ev {
            TicketsEvent::InputChanged { input } => match input.state {
                InputState::Open => {
                    if let Some(name) = self.runner.asked(input.ticket.as_str(), input.id.as_str()) {
                        tracing::info!("{} asked {} on {}", name, input.id.as_str(), input.ticket.as_str());
                    }
                }
                InputState::Answered => self.on_answer(input),
                _ => {}
            },
            TicketsEvent::Queued { entry, .. } => {
                self.queued.lock().unwrap_or_else(|p| p.into_inner()).insert(entry.ticket);
            }
            TicketsEvent::Unqueued { id, .. } => {
                self.queued.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            }
            TicketsEvent::Lagged { missed } => {
                tracing::warn!("the watch lagged ({missed} missed): reading what came");
                self.catch_up(tickets, comments).await?;
            }
            TicketsEvent::WatchClosed { message, .. } => return Err(format!("menger closed the watch: {message}")),
            _ => {}
        }
        Ok(())
    }

    fn handled(&self, key: &str) -> bool {
        self.position().has(key) || self.waiting.lock().unwrap_or_else(|p| p.into_inner()).contains(key)
    }

    fn mark(&self, key: &str) {
        self.store.update(|k| k.listen.mark(key.to_string()));
    }

    fn on_answer(self: &Arc<Self>, input: Input) {
        let Some(a) = input.answer.clone() else { return };
        let counts = match a.source {
            AnswerSource::App => true,
            AnswerSource::Api => self.cfg.accept_keys,
            AnswerSource::System => false,
        };
        let (ticket, id) = (input.ticket.as_str().to_string(), input.id.as_str().to_string());
        let key = format!("a:{ticket}:{id}:{}", a.at_unix);
        if !counts || a.at_unix < self.position().since.unwrap_or(i64::MAX) || self.handled(&key) {
            return;
        }
        let prompt = crate::prompts::answered(&input, &a, Some(&self.remote));
        match self.runner.holder_of(&ticket, &id) {
            Some(name) => self.resume(&key, &name, prompt, format!("resumed {name} with {ticket}'s answer to {id}")),
            None => self.after_grace(key, ticket, crate::prompts::unheld(prompt), Origin::Answer, Heard::Answer { input }),
        }
    }

    fn on_comment(self: &Arc<Self>, c: Comment) {
        let id = c.id.get();
        if id <= self.position().comment_mark.unwrap_or(u64::MAX) {
            return;
        }
        self.store.update(|k| k.listen.comment_mark = Some(id));
        let route = c.anchor.route.strip_prefix("#/ticket/").map(|r| r.split(['/', '?']).next().unwrap_or(r).to_string());
        let Some(ticket) = c.anchor.ticket.clone().or(route).filter(|t| !t.is_empty()) else { return };
        let by = c.principal.as_deref().unwrap_or("");
        let counts = !by.is_empty()
            && by != self.remote.principal()
            && (self.cfg.accept_keys || !(by.starts_with("key:") || by.starts_with("agent:")));
        if !counts {
            return;
        }
        let holder = self.runner.holder_on(&ticket);
        let queued = self.queued.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|q| crate::runner::same_ticket(q, &ticket));
        if c.intent != CommentIntent::Changes && holder.is_none() && !queued {
            return;
        }
        let key = format!("c:{id}");
        if self.handled(&key) {
            return;
        }
        let prompt = crate::prompts::replied(&ticket, &c);
        match holder {
            Some(name) => self.resume(&key, &name, prompt, format!("resumed {name} with comment {id} on {ticket}")),
            None => self.after_grace(key, ticket.clone(), crate::prompts::unheld(prompt), Origin::Reply, Heard::Comment { ticket, comment: c }),
        }
    }

    fn acted(&self, what: String) {
        tracing::info!("{what}");
        self.say(|s| {
            s.acted += 1;
            s.last_acted = Some(what);
        });
    }

    fn resume(&self, key: &str, name: &str, prompt: String, what: String) {
        self.mark(key);
        if self.runner.resume(name, prompt).is_some() {
            self.acted(what);
        }
    }

    /// No session of tether's holds it: wait out the grace, read it again,
    /// and start one if it still stands.
    fn after_grace(self: &Arc<Self>, key: String, ticket: String, prompt: String, origin: Origin, heard: Heard) {
        self.waiting.lock().unwrap_or_else(|p| p.into_inner()).insert(key.clone());
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(me.cfg.grace).await;
            let stands = me.still_stands(&heard).await;
            let outcome = match stands {
                Ok(true) => {
                    me.mark(&key);
                    let model = crate::archetypes::model_for(&me.cfg.archetypes, Archetype::Builder).unwrap_or(Model::Opus);
                    let start = Start {
                        ticket: ticket.parse().unwrap_or_else(|_| "TETHER-9".parse().expect("a ticket")),
                        prompt,
                        working_dir: None,
                        model,
                        origin,
                        started_by: format!("tether ({})", me.remote.principal()),
                        archetype: Some(Archetype::Builder),
                        report_chat: None,
                    };
                    match me.runner.start(start).await {
                        Ok(s) => me.acted(format!("started {} on {ticket} ({key})", s.name)),
                        Err(e) => tracing::error!("could not start a session on {ticket} for {key}: {e}"),
                    }
                    Ok(())
                }
                Ok(false) => {
                    me.mark(&key);
                    tracing::info!("{key} on {ticket}: acted on elsewhere in the grace");
                    Ok(())
                }
                Err(e) => Err(e),
            };
            me.waiting.lock().unwrap_or_else(|p| p.into_inner()).remove(&key);
            if let Err(e) = outcome {
                // Unmarked: the next connection's catch-up reads it again.
                tracing::warn!("{key} on {ticket}: could not read it again ({e}); the next catch-up will");
            }
        });
    }

    /// After the grace: an answer still stands when its input is still
    /// answered so (not closed, cleared or changed); a comment always does.
    async fn still_stands(&self, heard: &Heard) -> Result<bool, String> {
        let Heard::Answer { input } = heard else {
            let Heard::Comment { ticket, comment } = heard else { unreachable!() };
            tracing::debug!("comment {} on {ticket}", comment.id.get());
            return Ok(true);
        };
        let t = self.remote.connect().await?;
        let ticket = input.ticket.as_str().parse().map_err(|e: String| e)?;
        let now_ = TicketsClient::new(t).inputs().list_all(Some(ticket), None, None, None, None).await.map_err(|e| e.to_string())?;
        let at = input.answer.as_ref().map(|a| a.at_unix);
        Ok(now_.into_iter().any(|e| {
            matches!(e, TicketsEvent::Input { input: ref i } if i.id == input.id && i.state == InputState::Answered && i.answer.as_ref().map(|a| a.at_unix) == at)
        }))
    }
}
