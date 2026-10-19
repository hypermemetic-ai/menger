//! `runs` (TETHER-9): a detached agent run on a ticket — what was a
//! hand-launched `systemd-run … claude -p` with a prompt file. A run is a
//! session ([`crate::runner`]) started from a brief, on its archetype's
//! model (CHAT-45's file, [`crate::archetypes`]), listed, watched and
//! stopped as one, and — given `chat` — posting its report there when it
//! ends.

use std::path::PathBuf;

use futures::Stream;
use plexus_core::plexus::AuthContext;
use plexus_substrate::activations::claudecode::Model;

use crate::runner::{Runner, Start};
use crate::sessions::{principal, stopped, Origin, SessionsEvent};
use crate::vocab::{Archetype, Prompt, SessionName, TicketId, WorkDir};

/// The `runs` activation.
#[derive(Clone)]
pub struct RunsActivation {
    runner: Runner,
    archetypes: PathBuf,
}

impl RunsActivation {
    /// Runs on `runner`; archetypes' models from `archetypes`.
    pub fn new(runner: Runner, archetypes: PathBuf) -> Self {
        Self { runner, archetypes }
    }
}

#[plexus_macros::activation(
    version = "0.1.0",
    description = "Detached agent runs on tickets: a brief, an archetype's model, a report",
    roles(viewer = ["list", "watch"], editor = [viewer, "start", "stop"]),
    typed_params,
)]
impl RunsActivation {
    /// Start a run: a session working the ticket from a brief, on its
    /// archetype's model unless one is named.
    #[plexus_macros::method]
    pub async fn start(
        &self,
        auth: &AuthContext,
        /// The ticket the run works (e.g. MENGER-12).
        ticket: TicketId,
        /// What it is to do (a file: --brief @brief.md).
        brief: Prompt,
        /// architect, builder or analyst (default builder): its model from ~/.config/menger/archetypes.env.
        archetype: Option<Archetype>,
        /// opus, sonnet or haiku (default: the archetype's).
        model: Option<Model>,
        /// The directory it works in, absolute (default: tether's --work-dir).
        working_dir: Option<WorkDir>,
        /// A menger chat to post its report to when it ends (tether's key a participant).
        chat: Option<u64>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let (runner, started_by, archetype) = (self.runner.clone(), principal(auth), archetype.unwrap_or(Archetype::Builder));
        let model = model.map(Ok).unwrap_or_else(|| crate::archetypes::model_for(&self.archetypes, archetype));
        futures::stream::once(async move {
            let start = |model| Start { ticket, prompt: brief.into_string(), working_dir: working_dir.map(WorkDir::into_string), model, origin: Origin::Run, started_by, archetype: Some(archetype), report_chat: chat };
            match model.map(|m| runner.start(start(m))) {
                Ok(started) => started.await.map_or_else(|message| SessionsEvent::Error { message }, |session| SessionsEvent::Started { session }),
                Err(message) => SessionsEvent::Error { message },
            }
        })
    }

    /// Every run on this machine, oldest first.
    #[plexus_macros::method]
    pub async fn list(
        &self,
        /// Only this ticket's.
        ticket: Option<TicketId>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let all = self.runner.list(ticket.as_ref().map(TicketId::as_str), Some(Origin::Run));
        futures::stream::iter(all.into_iter().map(|session| SessionsEvent::Session { session }))
    }

    /// A run's tail: its lines from `since`, then each as it comes, then
    /// `ended`.
    #[plexus_macros::method(streaming)]
    pub async fn watch(
        &self,
        /// The run's name.
        name: SessionName,
        /// The first line wanted (default 0).
        since: Option<u64>,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        self.runner.watch(name.into_string(), since)
    }

    /// Stop a running run: its turn and its Claude Code end.
    #[plexus_macros::method]
    pub async fn stop(
        &self,
        /// The run's name.
        name: SessionName,
    ) -> impl Stream<Item = SessionsEvent> + Send + 'static {
        let name = name.into_string();
        futures::stream::iter([stopped(name.clone(), self.runner.stop(&name))])
    }
}
