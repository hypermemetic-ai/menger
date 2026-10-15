//! ACCESS-6: one parent (`tickets`, two methods) and one `#[child]`
//! namespace (`inputs`, two methods), built the way ACCESS-13 builds
//! menger's: `auth = "required"`, `request = SpikeRequest`, `client`,
//! `server_cfg`, every method `#[from_auth(self.access.verify)] user:
//! VerifiedUser`, typed params, `///` docs on methods and parameters.

pub mod vocab;

pub use vocab::{SpikeEvent, Status, TicketId};

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
pub use server::*;

#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;

/// The parent activation. Server-only: it holds the access rule.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Tickets {
    access: Access,
    inputs: Inputs,
}

#[cfg(feature = "server")]
impl Tickets {
    pub fn new() -> Self {
        Self { access: Access, inputs: Inputs { access: Access } }
    }
}

#[cfg(feature = "server")]
impl Default for Tickets {
    fn default() -> Self {
        Self::new()
    }
}

#[plexus_macros::activation(
    namespace = "tickets",
    description = "ACCESS-6 spike parent",
    auth = "required",
    request = SpikeRequest,
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
)]
impl Tickets {
    /// Read one ticket.
    #[plexus_macros::method]
    async fn get(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket to read, `SHORT-N`.
        id: TicketId,
    ) -> impl Stream<Item = SpikeEvent> + Send + 'static {
        let by = user.name().to_string();
        stream! { yield SpikeEvent::Ticket { id, status: Status::Ready, by }; }
    }

    /// Move a ticket to a status.
    #[plexus_macros::method]
    async fn set_status(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket to move.
        id: TicketId,
        /// The status to move it to; `active` is Coding.
        status: Status,
    ) -> impl Stream<Item = SpikeEvent> + Send + 'static {
        let by = user.name().to_string();
        stream! { yield SpikeEvent::Ticket { id, status, by }; }
    }

    /// The inputs namespace — `tickets.inputs.<method>`.
    #[plexus_macros::child]
    fn inputs(&self) -> Inputs {
        self.inputs.clone()
    }
}

/// The child activation.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Inputs {
    access: Access,
}

#[plexus_macros::activation(
    namespace = "inputs",
    description = "ACCESS-6 spike child",
    auth = "required",
    request = SpikeRequest,
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
)]
impl Inputs {
    /// A ticket's inputs.
    #[plexus_macros::method]
    async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket whose inputs to list.
        ticket: TicketId,
    ) -> impl Stream<Item = SpikeEvent> + Send + 'static {
        let by = user.name().to_string();
        stream! { yield SpikeEvent::Ticket { id: ticket, status: Status::Pending, by }; }
    }

    /// Answer an input.
    #[plexus_macros::method]
    async fn answer(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket holding the input.
        ticket: TicketId,
        /// The answer, a status (the spike's stand-in for a value).
        value: Status,
    ) -> impl Stream<Item = SpikeEvent> + Send + 'static {
        let by = user.name().to_string();
        stream! { yield SpikeEvent::Answered { ticket, value, by }; }
    }
}
