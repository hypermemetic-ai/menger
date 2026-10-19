//! COMMENT-5: the `media` activation — **the one declaration**, as
//! `tickets` and `comments` are (MFE-11).
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the menger binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (the browser app, wasm32) | `MediaClient<T>` — one typed method per `#[method]`, over [`crate::media`]' types |
//!
//! The images a comment shows, kept by the store behind the service-wide
//! cache ([`crate::store`]'s `media`): `put` keeps one for a ticket, `get`
//! answers its bytes. No side door: the shell serves no upload route and no
//! image path; the app turns `get`'s bytes into a `blob:` URL.
//!
//! **Under login** (AUTH-7): `get` is a viewer's, answering an image only
//! to a reader who can see a ticket it was put for (ACCESS-10); `put` is an
//! editor's, on the ticket (`target = "ticket"`, an editor's permit on it,
//! ACCESS-11) — or, put with no ticket (for a comment on the app), held
//! anywhere, and the image is then its putter's and the admins' of `*`.
//! CHAT-14: put for a chat (`chat`), by one of its participants — a
//! viewer chats, so `put` is a viewer's, and a ticket's image still takes
//! an editor's permit on it — and seen by its participants alone.

use crate::media::MediaEvent;
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{ChatId, MediaData, MediaHash, MediaType, TicketRef};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

// The methods' logic, beside the activation (MENGER-29, ACCESS-13).
#[cfg(feature = "server")]
mod streams;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct MediaActivation {
    store: Arc<Store>,
    /// The rule every method's caller is verified by ([`crate::access`]).
    access: Access,
}

#[cfg(feature = "server")]
impl MediaActivation {
    /// ACCESS-24: a test's, behind [`Access::for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(store: Arc<Store>) -> Self {
        let access = Access::for_tests();
        access.attach(Arc::clone(&store));
        Self::with_access(store, access)
    }

    /// Behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
/// Images for comments — put one for a ticket (base64), get it back by its hash.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["get", "put"],
        editor = [viewer],
        // CHAT-16: a chat-only seat's images, in its chats.
        chatter = ["get", "put"],
    )
)]
impl MediaActivation {
    /// Keep an image for a ticket (PNG, JPEG, GIF or WebP, at most 7 MiB, base64); yields stored { hash, mime, bytes, width, height, duplicate }.
    ///
    /// The image is named by its SHA-256 (`hash`, 64 hex digits); a body
    /// shows it as `![alt](media:<hash>)`. Its type is read from its own
    /// magic bytes — PNG, JPEG, GIF or WebP, never SVG — and must be the
    /// `mime` given; its width and height from its header. At most 7 MiB
    /// (an image travels as base64 in one WebSocket message, whose limit is
    /// 10 MiB). Yields exactly one event: `stored` — `duplicate: true` when
    /// the same bytes were kept already (one row: nothing new is stored but,
    /// for a second ticket, that it is that one's too) — or `refused {
    /// reason, message, bytes?, max? }` (`too_large`, `not_an_image`,
    /// `type_mismatch`, `truncated`, `bad_data`), naming why, with nothing
    /// kept; `not_found { ticket }` for a ticket that is none or out of
    /// reach. The image belongs to `ticket`: `get` answers it to whoever can
    /// see that ticket (an editor's: a viewer is answered `not_found`).
    /// CHAT-14: with `chat` instead, it is that chat's — `get` answers it to
    /// its participants alone; `not_found { hash: null }` for a chat the
    /// caller is not in. Without `ticket` (a comment on the app) it is the
    /// putter's, and the admins' of `*`. Through synapse the data is a file
    /// (`base64 -w0 shot.png > shot.b64`, then `--data @shot.b64`) or stdin
    /// (`--data -`), never `"$(base64 …)"`, which Linux caps at 128 KB
    /// (README "Comments").
    #[plexus_macros::method(
        group = "write",
        target = "ticket",
        example(note = "a 1×1 PNG for a ticket", params = r#"{"ticket": "KILN-4", "mime": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="}"#)
    )]
    pub async fn put(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket it is for, e.g. KILN-4 (optional: none for a comment on the app — the image is then the putter's).
        ticket: Option<TicketRef>,
        /// CHAT-14: the chat it is for, e.g. 41 — one the caller is in; seen by its participants alone (optional; not with ticket).
        chat: Option<ChatId>,
        /// What it is: image/png | image/jpeg | image/gif | image/webp (checked against its bytes).
        mime: MediaType,
        /// The bytes, standard base64 (`base64 -w0 shot.png`); a `data:image/png;base64,` prefix is read past.
        data: MediaData,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { streams::put(store, user, ticket, chat.map(ChatId::get), mime, data).await })
    }

    /// An image's bytes by its hash; yields media { hash, mime, bytes, width, height, data } (data base64), or not_found.
    ///
    /// Yields exactly one event: `media` — the image as kept, its bytes in
    /// standard base64 — or `not_found { hash }` for a hash that names no
    /// image, and alike for one put only for tickets the caller cannot see
    /// (ACCESS-10: not found ≡ not yours). COMMENT-10: with `ticket`, held
    /// to that ticket's body — the image only when it was put for that
    /// ticket (one kept for another ticket, a chat or the app is
    /// `not_found` there). The bytes are read from the database, never the
    /// cache.
    #[plexus_macros::method(
        group = "read",
        example(note = "an image a comment shows", params = r#"{"hash": "9f2c6ae2b4d1e1a8f7c3b0d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718"}"#)
    )]
    pub async fn get(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The image's hash, as `put` answered it (a `media:` before it is read past).
        hash: MediaHash,
        /// COMMENT-10: the ticket whose body shows it, e.g. KILN-4 (optional: then only an image kept for that ticket).
        ticket: Option<TicketRef>,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { streams::get(store, user, hash, ticket).await })
    }
}
