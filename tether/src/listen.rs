//! `listen` (TETHER-9): what tether listens to on the remote menger, and
//! how that stands — the logic is [`crate::listener`]'s.

use std::sync::Arc;

use futures::Stream;
use plexus_client::ClientError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::listener::{ListenStatus, Listener};

/// What `listen` answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListenEvent {
    /// The listener, as it stands.
    Status { status: ListenStatus },
    /// tether was started without `--remote`: it listens to nothing.
    Off,
    Error { message: String },
}

impl plexus_client::FromClientError for ListenEvent {
    fn from_client_error(error: ClientError) -> Self {
        ListenEvent::Error { message: error.to_string() }
    }
}

/// The `listen` activation.
#[derive(Clone)]
pub struct ListenActivation {
    listener: Option<Arc<Listener>>,
}

impl ListenActivation {
    pub fn new(listener: Option<Arc<Listener>>) -> Self {
        Self { listener }
    }
}

#[plexus_macros::activation(
    version = "0.1.0",
    description = "What tether listens to on the remote menger, and how it stands",
    roles(viewer = ["status"]),
    typed_params,
)]
impl ListenActivation {
    /// The listener: the remote, who tether is there, whether its watches
    /// are held, what it heard and acted on.
    #[plexus_macros::method]
    pub async fn status(&self) -> impl Stream<Item = ListenEvent> + Send + 'static {
        let ev = match &self.listener {
            Some(l) => ListenEvent::Status { status: l.status() },
            None => ListenEvent::Off,
        };
        futures::stream::iter([ev])
    }
}
