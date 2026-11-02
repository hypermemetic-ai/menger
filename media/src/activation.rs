//! The `media` activation — **the one declaration**, as `tickets` and `ui`
//! are: the `Activation` impl, dispatch, the schema and the methods'
//! bodies from this `impl`, and `MediaClient<T>` — one typed method per
//! `#[method]`, over [`crate::image`]'s types — for a client of it (the
//! shell's `GET /media/<hash>`, a test).
//!
//! The images a ticket's body shows, kept by the service's own store
//! ([`MediaStore`], `media.db`): `put` keeps one for a ticket (or for its
//! putter, with none), `get` answers its bytes. **Under login**: `get` is a
//! viewer's, answering an image only to a reader the tickets service says
//! sees a ticket it was put for (ACCESS-10); `put` is an editor's, and the
//! editor permit on the ticket is the tickets service's word, asked over
//! the seam ([`Seam`]) as the caller — never read from its store (V2-1).
//! Without a ticket the image is the putter's, and the admins' of `*`.

use crate::caller::{Caller, Callers, MediaRequest};
use crate::image::MediaEvent;
use crate::seam::Seam;
use crate::store::MediaStore;
use crate::vocab::{MediaData, MediaHash, MediaType};
use futures::Stream;
use menger_tickets_api::vocab::TicketRef;
use std::sync::Arc;

/// The activation: the store over `media.db`, the seam to the tickets
/// service, and the resolver every caller is verified by.
#[derive(Clone)]
pub struct MediaActivation {
    store: Arc<MediaStore>,
    seam: Seam,
    callers: Callers,
}

impl MediaActivation {
    /// Over `store`; every caller the one the service's gate admitted.
    pub fn new(store: Arc<MediaStore>) -> Self {
        Self { store, seam: Seam::default(), callers: Callers }
    }

    /// The seam the builder attaches its running service to, once built —
    /// how a put asks the tickets service for the ticket's permit.
    pub fn seam(&self) -> &Seam {
        &self.seam
    }

    /// The store it keeps.
    pub fn store(&self) -> &Arc<MediaStore> {
        &self.store
    }
}

/// Images for tickets — put one (base64) for a ticket you may edit, get it back by its hash.
#[plexus_macros::activation(
    auth = "required",
    request = MediaRequest,
    typed_params,
    roles(
        viewer = ["get"],
        editor = [viewer, "put"],
    )
)]
impl MediaActivation {
    /// Keep an image for a ticket (PNG, JPEG, GIF or WebP, at most 7 MiB, base64); yields stored { hash, mime, bytes, width, height, duplicate }.
    ///
    /// The image is named by its SHA-256 (`hash`, 64 hex digits); a body
    /// shows it as `![alt](media:<hash>)`, and the page draws it from
    /// `/media/<hash>`. Its type is read from its own magic bytes — PNG,
    /// JPEG, GIF or WebP, never SVG — and must be the `mime` given; its
    /// width and height from its header. At most 7 MiB (an image travels as
    /// base64 in one WebSocket message, whose limit is 10 MiB). Yields
    /// exactly one event: `stored` — `duplicate: true` when the same bytes
    /// were kept already (one row: nothing new is stored but, for a second
    /// ticket, that it is that one's too) — or `refused { reason, message,
    /// bytes?, max? }` (`too_large`, `not_an_image`, `type_mismatch`,
    /// `truncated`, `bad_data`), naming why, with nothing kept; `not_found
    /// { ticket }` for a ticket that is none, or that the tickets service
    /// says you may not edit (your editor permit on it is asked of
    /// `tickets.may`, as you). The image belongs to `ticket`: `get` answers
    /// it to whoever sees that ticket. Without `ticket` (what the editor
    /// pastes before its ticket exists) it is yours, and the admins' of
    /// `*`. Through synapse the data is a file (`base64 -w0 shot.png >
    /// shot.b64`, then `--data @shot.b64`) or stdin (`--data -`), never
    /// `"$(base64 …)"`, which Linux caps at 128 KB. Editor.
    #[plexus_macros::method(
        group = "write",
        example(note = "a 1×1 PNG for a ticket", params = r#"{"ticket": "KILN-4", "mime": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="}"#)
    )]
    pub async fn put(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The ticket it is for, e.g. KILN-4 (optional: with none, the image is yours).
        ticket: Option<TicketRef>,
        /// What it is: image/png | image/jpeg | image/gif | image/webp (checked against its bytes).
        mime: MediaType,
        /// The bytes, standard base64 (`base64 -w0 shot.png`); a `data:image/png;base64,` prefix is read past.
        data: MediaData,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        futures::stream::once(crate::streams::put(store, seam, user, ticket, mime, data))
    }

    /// An image's bytes by its hash; yields media { hash, mime, bytes, width, height, data } (data base64), or not_found.
    ///
    /// Yields exactly one event: `media` — the image as kept, its bytes in
    /// standard base64 — or `not_found { hash }` for a hash that names no
    /// image, and alike for one put only for tickets you cannot see
    /// (ACCESS-10: not found ≡ not yours; whether you see a ticket is asked
    /// of `tickets.may`, as you) or put with no ticket by someone else (its
    /// putter's, and the admins' of `*`). The same bytes are at
    /// `GET /media/<hash>` on the shell, to a signed-in session. Viewer.
    #[plexus_macros::method(
        group = "read",
        example(note = "an image a body shows", params = r#"{"hash": "9f2c6ae2b4d1e1a8f7c3b0d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718"}"#)
    )]
    pub async fn get(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The image's hash, as `put` answered it (a `media:` before it is read past).
        hash: MediaHash,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        let (store, seam) = (self.store.clone(), self.seam.clone());
        futures::stream::once(crate::streams::get(store, seam, user, hash))
    }
}
