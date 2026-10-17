//! THEME-8: the `themes` activation — **the one declaration**, as `tickets`,
//! `comments` and `notes` are (MFE-11): the server's [`Themes`] and the
//! browser's `ThemesClient<T>`.
//!
//! A theme is a name and its token overrides ([`crate::themes`]); the site
//! and each shortcode say which they adopt, how far it reaches and whether
//! they take the themes above (`set`); a ticket says it in its frontmatter;
//! `theme_for` answers who wears what. Kept by the store, behind the cache
//! ([`crate::store`]'s `themes`).
//!
//! **Under login** (AUTH-7): reads are a viewer's — `theme_for` where the
//! ticket is; `define` and `set` an editor's: `define` on `*` (a theme is
//! the whole site's), `set` on the shortcode it names — and the site's
//! theme takes admin on `*`.

use crate::themes::{Takeover, ThemeReach, ThemesEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{ThemeName, ThemeScope, TicketRef, TokenName, TokenValue};
#[cfg(feature = "server")]
use crate::access::{Access, ADMINISTER};
#[cfg(feature = "server")]
use crate::store::{Store, StoreError, ThemeChange};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Grant, Reach};
use std::collections::BTreeMap;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct ThemesActivation {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl ThemesActivation {
    /// No login: every call admitted, as always.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_access(store, Access::open())
    }

    /// Behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// The store's refusal, in band.
#[cfg(feature = "server")]
fn refused(e: StoreError) -> ThemesEvent {
    ThemesEvent::Error { message: e.to_string() }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    description = "Themes: token overrides the site, a shortcode, a ticket or a frame adopts",
    typed_params,
    // AUTH-7: reading is a viewer's; defining and setting an editor's (the
    // site's theme admin's alone, asked in the method).
    roles(
        viewer = ["list", "get", "theme_for", "watch"],
        editor = [viewer, "define", "set"],
    )
)]
impl ThemesActivation {
    /// THEME-3: the authoring path is a ticket's ` ```theme name
    /// [extends=parent] ` block (`tickets.create` / `amend` / `update`
    /// register it, owned by that ticket); `define` stays for admin and
    /// tests — an ownerless theme, refused for a name a block owns.
    ///
    /// Defines the theme `name` — its token overrides for `light` and for
    /// `dark`, each a map from a token (`--accent`, or `accent`) to its
    /// value, replacing whatever it overrode before (THEME-8: a theme is a
    /// name and a map over today's tokens; `default` overrides nothing and
    /// cannot be defined). A token not in the contract, or a value that
    /// could escape its declaration, is refused, naming each; nothing is
    /// kept. Yields exactly one event: `theme`, as kept — or an in-band
    /// `error`. `watch` carries it too. Under login: an editor on `*`.
    #[plexus_macros::method(
        group = "write",
        description = "Define (or redefine) a theme: its token overrides for light and dark; yields the theme",
        params(
            name = "The theme's name: lowercase letters, digits and -, e.g. ocean",
            light = "Token overrides in light, e.g. {\"--accent\": \"#0a7\"} (optional)",
            dark = "Token overrides in dark (optional; a face set for light holds in dark too)"
        ),
        example(note = "an ocean theme", params = r##"{"name": "ocean", "light": {"--accent": "#00796b", "--bg": "#f3f8f8"}, "dark": {"--accent": "#4db6ac"}}"##)
    )]
    pub async fn define(
        &self, auth: Option<&AuthContext>, reach: Reach,
        name: ThemeName,
        light: Option<BTreeMap<TokenName, TokenValue>>,
        dark: Option<BTreeMap<TokenName, TokenValue>>,
    ) -> impl Stream<Item = ThemesEvent> + Send + 'static {
        let text = |m: Option<BTreeMap<TokenName, TokenValue>>| -> BTreeMap<String, String> {
            m.unwrap_or_default().into_iter().map(|(k, v)| (k.into_string(), v.into_string())).collect()
        };
        let (light, dark) = (text(light), text(dark));
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield ThemesEvent::Error { message };
                    return;
                }
            };
            if let Some(g) = caller.grant().cloned() {
                if let Err(message) = caller.check(&g, "*") {
                    yield ThemesEvent::Error { message: format!("a theme is the whole site's: defining one takes themes.define on *: {message}") };
                    return;
                }
            }
            // ACCESS-11: editor on `*`, the proof the store takes.
            let on = match caller.on::<crate::access::Editor>(&crate::members::Scope::Everywhere, &store) {
                Ok(on) => on,
                Err(r) => {
                    yield ThemesEvent::Error { message: format!("a theme is the whole site's: defining one takes themes.define on *: {r}") };
                    return;
                }
            };
            match store.define_theme(on, &name, light, dark).await {
                Ok(theme) => yield ThemesEvent::Theme { theme },
                Err(e) => yield refused(e),
            }
        }
    }

    /// Every theme, `default` first then by name — one `theme` event each —
    /// then what the site and every shortcode say: one `setting` each, the
    /// site first. From the cache.
    #[plexus_macros::method(group = "read", description = "Every theme, then what the site and each shortcode say")]
    pub async fn list(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = ThemesEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            if let Err(message) = admitted {
                yield ThemesEvent::Error { message };
                return;
            }
            for theme in store.themes() {
                yield ThemesEvent::Theme { theme };
            }
            for setting in store.theme_settings() {
                yield ThemesEvent::Setting { setting };
            }
        }
    }

    /// One theme, as it is now: `theme`, or an in-band `error` for a name
    /// that is none (naming the ones there are).
    #[plexus_macros::method(
        group = "read",
        description = "Get one theme by name",
        params(name = "The theme's name, e.g. ocean"),
        example(note = "the ocean theme", params = r#"{"name": "ocean"}"#)
    )]
    pub async fn get(&self, auth: Option<&AuthContext>, reach: Reach, name: ThemeName) -> impl Stream<Item = ThemesEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            if let Err(message) = admitted {
                yield ThemesEvent::Error { message };
                return;
            }
            match store.theme(&name) {
                Ok(theme) => yield ThemesEvent::Theme { theme },
                Err(e) => yield refused(e),
            }
        }
    }

    /// What `scope` says — `site` (everyone's site theme; admin on `*`) or
    /// a shortcode (an editor there) — replacing what it said: the `theme`
    /// it adopts (none or `default`: none), its `reach` (`frame` — only the
    /// frames of its tickets; `ticket`, the default — its tickets' views
    /// and frames; `site` — and the site while one of its tickets is open),
    /// and `takeover` (`off`: its tickets ignore the site's theme). The
    /// site takes neither a reach nor `takeover: off`. Saying nothing
    /// clears it. Yields exactly one event: `setting`, as kept — or an
    /// in-band `error` (an undefined theme, a shortcode that is none).
    /// `watch` carries it too.
    #[plexus_macros::method(
        group = "write",
        description = "Set what the site or a shortcode says: the theme it adopts, its reach and takeover; yields the setting",
        params(
            scope = "site, or a shortcode, e.g. PANTRY",
            theme = "The theme it adopts (optional; none or default: none)",
            reach = "frame | ticket | site — how far it takes over (optional, default ticket; a shortcode's only)",
            takeover = "on | off — off: ignore every theme from above (optional, default on; a shortcode's only)"
        ),
        values(reach = ["frame", "ticket", "site"], takeover = ["on", "off"]),
        example(note = "everyone's site theme", params = r#"{"scope": "site", "theme": "ocean"}"#),
        example(note = "a project's, taking over the site while one of its tickets is open", params = r#"{"scope": "PANTRY", "theme": "pantry", "reach": "site"}"#),
        example(note = "keep MENGER default", params = r#"{"scope": "MENGER", "takeover": "off"}"#)
    )]
    pub async fn set(
        &self, auth: Option<&AuthContext>,
        scope: ThemeScope,
        theme: Option<ThemeName>,
        reach: Option<ThemeReach>,
        takeover: Option<Takeover>,
    ) -> impl Stream<Item = ThemesEvent> + Send + 'static {
        let store = self.store.clone();
        // Its parameter `reach` is the setting's, so the gate's answer is
        // not taken: the gate admitted the call, and the grant is asked
        // here, where the scope is (`*` for the site).
        let admitted = self.access.verify_call(auth, &Reach::everywhere());
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield ThemesEvent::Error { message };
                    return;
                }
            };
            let site = scope.is_site();
            {
                let g = Grant::new(if site { ADMINISTER } else { "themes.set" });
                let target = if site { "*".to_string() } else { scope.to_string() };
                if let Err(message) = caller.check(&g, &target) {
                    let message = if site { format!("the site's theme is everyone's: setting it takes admin on *: {message}") } else { message };
                    yield ThemesEvent::Error { message };
                    return;
                }
            }
            // ACCESS-11: admin on `*` for the site, editor on the shortcode.
            let by: Result<crate::store::Authority, String> = match site {
                true => caller.admin_of(&crate::members::Scope::Everywhere, &store).map(Into::into).map_err(|r| r.to_string()),
                false => crate::activation::scope_named(&store, &scope.to_string()).and_then(|s| caller.on::<crate::access::Editor>(&s, &store).map(Into::into).map_err(|r| r.to_string())),
            };
            let by = match by {
                Ok(by) => by,
                Err(message) => {
                    yield ThemesEvent::Error { message };
                    return;
                }
            };
            match store.set_theme(by, &scope, theme.as_deref(), reach, takeover).await {
                Ok(setting) => yield ThemesEvent::Setting { setting },
                Err(e) => yield refused(e),
            }
        }
    }

    /// Who wears what for the ticket `id`: one `regions` event — the site,
    /// the ticket view and the frame, each its theme and the level it came
    /// from (`none`, `site`, `shortcode` with `from`, `ticket`, `frame`) —
    /// with what every level said (`levels`: the site's theme, the nearest
    /// shortcode's setting, the ticket's frontmatter and its frame's). The
    /// reader's own choices are the app's, outside this. An in-band `error`
    /// for a ticket that is none.
    #[plexus_macros::method(
        group = "read",
        target = "id",
        description = "Who wears what for a ticket: each region's theme (site, ticket view, frame) and the level it came from",
        params(id = "Ticket id, e.g. PANTRY-4"),
        example(note = "PANTRY-4's regions", params = r#"{"id": "PANTRY-4"}"#)
    )]
    pub async fn theme_for(&self, auth: Option<&AuthContext>, reach: Reach, id: TicketRef) -> impl Stream<Item = ThemesEvent> + Send + 'static {
        let store = self.store.clone();
        let admitted = self.access.verify_call(auth, &reach);
        stream! {
            let caller = match admitted {
                Ok(caller) => caller,
                Err(message) => {
                    yield ThemesEvent::Error { message };
                    return;
                }
            };
            match store.theme_for(&caller, &id).await {
                Ok((id, regions, levels)) => yield ThemesEvent::Regions { id, regions, levels },
                Err(StoreError::NotFound(_)) => yield ThemesEvent::Error { message: format!("no ticket {id}") },
                Err(e) => yield refused(e),
            }
        }
    }

    /// Subscribes to the themes: `theme` when one is defined and `setting`
    /// when the site or a shortcode says something new, in the order the
    /// writes were acknowledged, until the client goes; `lagged { missed }`
    /// where a slow subscriber lost changes (`list` again). A ticket's
    /// frontmatter moving is `tickets.watch`'s `amended`.
    #[plexus_macros::method(
        group = "read",
        streaming,
        description = "Subscribe to themes: theme_changed (defined, a ```theme block written, orphaned or adopted), theme_removed (an unused orphan gone) and setting (site or shortcode), until disconnect; lagged { missed } where a slow subscriber lost changes"
    )]
    pub async fn watch(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = ThemesEvent> + Send + 'static {
        let mut rx = self.store.subscribe_themes();
        // ACCESS-12: the caller followed — a revoke or sign-out ends it.
        let opened = crate::access::Following::open(&self.access, &self.store, auth, &reach);
        stream! {
            let mut following = match opened {
                Ok(f) => f,
                Err(message) => {
                    yield ThemesEvent::Error { message };
                    return;
                }
            };
            loop {
                let next = tokio::select! {
                    biased;
                    () = following.changed() => None,
                    r = rx.recv() => Some(r),
                };
                if let Err(closed) = following.again() {
                    yield ThemesEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
                let Some(next) = next else { continue };
                match next {
                    Ok(ThemeChange::Changed(theme)) => yield ThemesEvent::ThemeChanged { theme },
                    Ok(ThemeChange::Removed(name)) => yield ThemesEvent::ThemeRemoved { name },
                    Ok(ThemeChange::Set(setting)) => yield ThemesEvent::Setting { setting },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => yield ThemesEvent::Lagged { missed },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}
