//! COMP-4: framed bodies — a body that holds HTML renders in a sandboxed
//! frame, lavish-axi's way (COMP-1 decisions 1 and 4).
//!
//! A body md.rs calls framed ([`crate::md::is_framed`]) is not painted into
//! the page: [`Frame`] renders it as one `<iframe sandbox=SANDBOX>` — no
//! `allow-same-origin`, so the frame's origin is opaque: its scripts run,
//! but cannot read the app's DOM, storage or cookies, nor reach its
//! WebSocket — whose `srcdoc` ([`srcdoc`]) is the body's markup
//! ([`crate::md::render_body_framed`]: raw HTML passed through, `{wave: ID}`
//! already placed by the host) under a head that carries:
//!
//! - first, IDENTITY-28's CSP ([`CSP`]): `connect-src 'none'; form-action
//!   'none'` — the frame opens no connection of its own and submits no form
//!   (nor does the sandbox allow forms); its scripts, styles, images and
//!   fonts load as before;
//! - `<base target="_blank">` — a link the SDK does not take (MENGER-14,
//!   below: it takes every click) opens a new top-level tab (the sandbox's
//!   popups escape it), never the frame itself;
//! - `<style id="menger-theme">` — the app's theme tokens ([`theme_tokens`],
//!   read where the frame sits as painted now — THEME-8: its region's
//!   theme, src/scopes.rs) and `color-scheme`, so the
//!   first paint is in the app's theme;
//! - `#menger-init` — the frame's channel, the SDK version, the theme, and
//!   where the vendored mermaid bundle is and the strict config it runs
//!   under (src/mermaid.rs);
//! - the SDK shell, inline: `frontend/sdk/menger-sdk.js` (also served at
//!   [`SDK_URL`]), whose header is the protocol's reference — base
//!   typography, theme tokens that follow a flip, mermaid drawn inside, the
//!   document's height reported, `window.menger` (COMP-5: `{ version, call,
//!   on }`; COMP-10: and `tickets`, the client hub-codegen generates from
//!   the activation — the script's GENERATED region, written by
//!   `scripts/sdk-codegen.sh`) and every registered component defined.
//!
//! ACCESS-28: **the document is handed over, not a `srcdoc`.** A srcdoc
//! inherits its parent's Content-Security-Policy, and the app's pages go
//! out under a strict one (scripts its own, hashed inline blocks, no remote
//! image) — a body's inline scripts and esm.sh imports would be refused.
//! So a frame is `<iframe src="/app/frame#<channel>">` ([`FRAME_URL`]): the
//! shell serves that document under IDENTITY-28's policy alone
//! (`web/frame.html`, `src/shell.rs`'s `FRAME_CSP`); it posts `menger:boot`
//! on its channel, and the host answers `menger:doc { html }` — the
//! [`srcdoc`] it built, kept on the `<iframe>` element itself, where the
//! frame cannot reach it — which the frame writes over itself. Every check
//! below is the same: the frame's own window, its own channel.
//!
//! The host half of the handshake is here: one `message` listener for every
//! frame ([`install_listener`]) accepts a `menger:*` message only from a
//! frame's own `contentWindow` carrying that frame's channel; it answers
//! `menger:ready` with the current theme and sizes the frame to each
//! `menger:size` (no inner scrollbar). [`retheme_all`] — called by the theme
//! toggle when the painted mode changes — posts the new tokens to every
//! frame. A body that changes (an `amended`, a keystroke in the editor's
//! preview, a wave reference placed) loads — once it has held still for
//! [`RERENDER_AFTER_MS`] — into a second, invisible frame beside the one on
//! screen, which is replaced only once the new one reports `settled` (its
//! diagrams drawn) — so the page never flashes.
//!
//! COMP-5: **components in frames.** Every frame is handed the component
//! registry (src/registry.rs): in its srcdoc's init block when the app
//! holds it, again in answer to `menger:ready` (`menger:components` — what
//! moved while the frame loaded), and to every open frame when a fetch
//! lands ([`broadcast_registry`]). Every `watch` event the session hears
//! reaches every frame whose reach admits it (IDENTITY-36, below) as
//! `menger:change { event }` ([`broadcast_change`]):
//! the SDK applies `component_changed` / `component_removed` to its
//! definitions, then hands the event to `menger.on('change')`. And the
//! frame's `menger:call { id, method, params }` is answered here —
//! `menger:result { id, items }` or `{ id, error }` — by the read-only
//! bridge (src/bridge.rs) over the app's own connection. A frame that asks
//! for the registry ([`Frame`]) makes the app keep it.
//!
//! COMP-9: **definitions, badges, tooltips, navigation.** A frame's SDK
//! asks the host three more things, answered here: `menger:navigate {
//! route, reveal? }` — `menger.navigate`, moving the app to an app route
//! ([`crate::inspect::app_route`]'s allow-list; anything else ignored), or,
//! for a `{wave: ID}` clicked in a framed board note (`reveal: { id, wave
//! }`), revealing the card on the board as an unframed note does;
//! `menger:inspect { id, name, attrs }` — an instance's badge hovered or
//! focused — answered `menger:inspected { id, inspection }`
//! ([`crate::inspect::inspect`], against the registry and the extents the
//! board holds); and `menger:revealed { component, top }` — the answer to
//! the host's `menger:reveal { component }`, sent to every frame while the
//! route names a card (`#/ticket/<OWNER>?component=m-x`): the frame holding
//! that card flashes it and says where it is, and the host scrolls there.
//!
//! INPUT-4: **the first write.** A frame that renders a ticket's stored body
//! knows which (`ticket` in its init block — `menger.input.ticket`), and
//! the host stamps it on the `<iframe data-ticket>`, where nothing in the
//! frame can change it. `menger.input.submit` sends `menger:submit { id,
//! params }` — only inside a viewer's gesture, by the SDK's own check — and
//! the host reads at once whether a gesture on THAT frame is under way
//! (IDENTITY-36, src/gesture.rs: the app's activation, focus in this very
//! frame, the pointer or a Tab brought there since the app's own last
//! gesture — never just any click in the app; APP-35: read again once
//! the app's own activation has run out, when only that was in doubt), then answers
//! `menger:submitted { id, answer }` or `{ id, error }` after the bridge's
//! checks and `tickets.inputs.answer` (src/bridge.rs). Every
//! `watch` event, `input_changed` among them, reaches the frames as
//! `menger:change` as ever.
//!
//! INPUT-9: **the second write.** `menger.input.clear` sends `menger:clear
//! { id, params: { input } }` under the same rule — the SDK's gesture
//! check, the host's own reading of a gesture on that frame, the frame's stamped
//! ticket — and the host answers `menger:cleared { id, input }` (the input
//! as the clear left it, the wire's spelling) or `{ id, error }` after the
//! bridge's checks and `tickets.inputs.clear`.
//!
//! IDENTITY-36: **a frame reads its own subtree.** The host stamps every
//! frame with its scope (`<iframe data-scope>`): the shortcode of the ticket
//! whose stored body it renders — or, for an editor's draft, of the ticket
//! being edited; for a component's help card, of its owner — where nothing
//! in the frame can change it. Every `menger.call` is answered held to that
//! shortcode and the shortcodes hosted under it (FILTER-13's `under`,
//! src/bridge.rs: [`bridge::run_scoped`]), and every `watch` event reaches a
//! frame only if its reach admits it ([`broadcast_change`]). A frame with no
//! scope reads and hears components alone.
//!
//! COMMENT-3: **annotation mode reaches into the body.** The host tells every
//! frame when the mode turns (`menger:annotate { on }`, and a frame that
//! loads meanwhile on its `menger:ready`): on, the SDK outlines every element
//! of the body, takes every click for the mode (the body's handlers never see
//! it), shows a component's info card on hover (and only then), and posts
//! `menger:hover { chain, rect, text }` (`chain: null` when the pointer
//! leaves), `menger:pick { id, chain, rect, text }` for a click, and
//! `menger:key { key }` for `a`/`A` (and Esc in the mode) — the app's keys.
//! The host decides what an element is from its chain (src/anchor.rs) and
//! asks back: `menger:probe { id, selectors }` → `menger:probed { id, ok }`
//! (which select the picked element alone), and `menger:locate { id, paths
//! }` → `menger:located { id, found: [{ rect, text } | null] }` (where
//! anchored elements are, for pins). Everything the mode draws over a frame
//! the host draws (src/annotate.rs).
//!
//! MENGER-14: **links.** The SDK takes every click on a link in the body
//! (and Enter on a focused one, which the browser makes a click; and a
//! middle click) that is not an app route (`#/…`, `menger.navigate` as
//! ever), an anchor in the body, or a click the body's own handler already
//! prevented — and posts `menger:link { href }`, the `href` as written. The
//! host classifies it ([`links::follow`]): an app route navigates, a link
//! that leaves menger asks first (the leave dialog), `javascript:` and every
//! other scheme but http(s) and mailto is refused with a notice. The sandbox
//! and the CSP are unchanged — the click is handed over, nothing loosened.
//!
//! What decides — the srcdoc, the theme block, the init block, which
//! messages count — is pure and pinned by native tests; the rest is DOM.

use std::cell::Cell;

use js_sys::{Object, Reflect};
use leptos::prelude::*;
use menger_tickets_api::{Component, TicketsEvent};
use serde::Serialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;

use crate::{anchor, annotate, board, bridge, cite, dom, gesture, highlight, inspect, links, md, mermaid, mockup, notes, present, registry, session};

/// The SDK's version — `window.menger.version` inside every frame (pinned
/// equal to the script's own `VERSION` by a native test). COMP-5's 1.0:
/// `version`, `call`, `on`, and the components defined; COMP-10's 1.1 adds
/// `tickets`, the generated typed client over the same bridge; COMP-9's 1.2
/// adds `navigate`, and the badges, tooltips and docs cards; INPUT-4's 1.3
/// adds `input` — `submit`, the one write, and `ticket` — and `inputs`
/// among the reads; INPUT-9's 1.4 adds `input.clear`, the second write;
/// COMMENT-3's 1.5 adds annotation mode (the info card only in it);
/// COMP-12's 1.6 puts the base typography in a cascade layer, and a body
/// holding `<meta name="menger-base" content="markdown">` confines it to
/// what markdown produced (`data-md`, md::MD_MARK); COMP-14's 1.7 adds the
/// built-in `<m-cite>` (`menger:cite` / `menger:uncite` / `menger:cited`),
/// a cited window's frame (`cite` in the init block; `menger:size { top,
/// found }`; `input.displayOnly`), and section anchors in `menger:reveal`;
/// MENGER-14's 1.8 hands every link clicked to the host (`menger:link`);
/// MENGER-15's 1.9 hands it `z` (full screen) and every Esc, and a mockup's
/// frames take `menger:emulate` (the page shim, `menger-page.js`); MENGER-13's
/// 1.10 regenerates the typed client over the transition log — `history`,
/// the `transition` and `history_end` events, `tz` on the filtered reads,
/// `set_status`'s `restore_children`, `source` and `message`; MENGER-30's
/// 1.11 adds `update` and its events (`updated`, `blockers_changed`,
/// `unlocks_removed`, `severity_changed`); MENGER-22's 1.12 highlights every
/// code block in the frame (`highlight` in the init block: the vendored
/// highlighter's URL and its rules), coloured by the `--code-*` tokens;
/// THEME-7's 1.13 adds `on('theme')` and `menger.theme` (`theme` in the
/// init block and in `menger:theme`: the name the frame's region wears);
/// 1.14 is both lines meeting — MENGER-34 (shipped alone as 1.12 on main)
/// hands a ticket chip's right-click (or Menu / Shift+F10) to the host
/// (`menger:ticket-menu`), which opens the app's ticket menu there; ACT-2's
/// 1.15 adds `update_many` (`updated_many`) and `ids` on `label` and
/// `link_commits`; ACT-5's 1.16 adds `delete` (`deleted`, its tombstones;
/// `get` on a deleted id answers it), `undelete` (`undeleted`) and
/// `extents`' `holes`; FILTER-25's 1.17 hands a ticket chip's Alt+click to
/// the host (`menger:filter-everywhere { id }`), which sets the global filter
/// to its shortcode's clause, as an Alt+click on an app chip does;
/// DESIGN-40's 1.18 carries the app's Motion setting into the frame
/// (`motion` and `entered` in the init block, `menger:motion`, `motion` on
/// `menger:theme`) as `html[data-motion]` / `html[data-entered]`, adds
/// `menger.motion`, and declares the house faces (`<style id="menger-fonts">`);
/// DESIGN-43's 1.19 carries the app's Typed text setting the same way
/// (`typing` in the init block, `menger:typing`, `typing` on `menger:theme`)
/// as `html[data-typing]`, and adds `menger.typing`. MENGER-39's 1.20 is
/// the generated client's: `menger.tickets.queue`, `unqueue`, `snooze` and
/// `queued`, and the `--urgent` tokens a frame receives. APP-29's 1.21 adds
/// the built-in `<m-mockup>` (`menger:mockup` / `menger:unmock` /
/// `menger:mocked`): a placeholder the host draws a website mockup over.
/// ACCESS-20's 2.0 moves the reads onto the `tickets` children's paths —
/// `menger.tickets.work.waves`, `.nav.groups`, `.inputs.list`,
/// `.components.list`; `menger.call('work.waves')` — and refuses the 1.x
/// names, naming the new one. APP-42's 2.2 has `<m-mockup>` post its
/// `title` and `description` (`menger:mockup`), the caption the host draws
/// above the devices; the page shim, in a mockup's frames, reports its
/// page's height (`menger:extent`) for the bar's one-view note. APP-45's
/// 2.3 reaches annotation mode into mockups: the SDK and the page shim walk
/// their document by key (`menger:walk { step }`), `menger:locate { reveal
/// }` scrolls a device's page to the element first, a device's frame says
/// when its page scrolls in the mode (`menger:scrolled`), and the page shim
/// takes the whole mode in a whole document (outline, hover, pick, probe,
/// locate), as the SDK does in a body. APP-46's 2.4 has `<m-mockup>` post
/// the decision's option it sits in (the bar's *pick this*), and a DECIDED
/// decision whose options hold mockups draws its chosen option first.
pub(crate) const SDK_VERSION: &str = "2.4.0";

/// Where the app serves the SDK shell (index.html copy-files
/// `sdk/menger-sdk.js` into `dist/sdk/`). The frames do not fetch it — it is
/// inlined — but it is the app's published copy of what they run.
pub(crate) const SDK_URL: &str = "/app/sdk/menger-sdk.js";

/// ACCESS-28: the document a frame starts as (menger's shell serves it, its
/// own policy and all); the fragment is the frame's channel.
pub(crate) const FRAME_URL: &str = "/app/frame";

/// ACCESS-28: where a frame's document waits for its `menger:boot` — a
/// property of the `<iframe>` element, never an attribute.
const DOC_PROP: &str = "mengerDoc";

/// ACCESS-28: a frame's `src` — the frame document, told its channel.
pub(crate) fn frame_src(channel: &str) -> String {
    format!("{FRAME_URL}#{channel}")
}

/// The shell, inlined into every srcdoc.
const SDK_JS: &str = include_str!("../sdk/menger-sdk.js");

/// MENGER-15: the page shim (its header is its reference) — inlined first
/// into a whole document's head, and after the SDK in a mockup's frames:
/// `prefers-color-scheme` / `prefers-reduced-motion` as the host emulates
/// them; in a page, its keys, its `#fragment` links, and `menger:ready`.
const PAGE_JS: &str = include_str!("../sdk/menger-page.js");

/// MENGER-15: what a frame's page is told the viewer prefers — each `None`
/// for the browser's own. A whole document follows the app's painted mode
/// (`scheme`) unless a mockup's toggles set both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Emulate {
    pub(crate) scheme: Option<Scheme>,
    pub(crate) reduce: Option<bool>,
}

impl Emulate {
    fn json(self) -> serde_json::Value {
        serde_json::json!({ "scheme": self.scheme.map(Scheme::as_str), "reduce": self.reduce })
    }
}

/// DESIGN-40: the app's Motion setting (DESIGN-39: `html[data-motion]` on
/// the app's document — absent is `full`): `full` plays every move, `calm`
/// drops the decorative ones (a figure drawing itself, a code block typing,
/// a step's glow, an option settling in), `none` stops them all. A frame
/// wears it as its own `html[data-motion]` (the SDK sets it from the init
/// block, and from `menger:motion` / `menger:theme` when it changes), so the
/// house components honour it; `prefers-reduced-motion` stops them too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Motion {
    #[default]
    Full,
    Calm,
    None,
}

impl Motion {
    /// The attribute's value as written; anything else (or none) is `full`.
    pub(crate) fn parse(v: Option<&str>) -> Motion {
        match v.map(str::trim) {
            Some("calm") => Motion::Calm,
            Some("none") => Motion::None,
            _ => Motion::Full,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Motion::Full => "full",
            Motion::Calm => "calm",
            Motion::None => "none",
        }
    }
}

/// DESIGN-43: the app's Typed text setting (src/motion.rs: `html[data-typing]`
/// on the app's document — absent is `normal`): how fast a framed code
/// block types itself in, or `off`. A frame wears it as its own
/// `html[data-typing]` (from the init block, and from `menger:typing` /
/// `menger:theme` when it changes), which ART-9's `m-code` reads. One
/// vocabulary with the setting's.
pub(crate) use crate::motion::Typing;

/// DESIGN-40: the house faces in a framed body — the contract's families
/// (`--mono`, `--sans`: "Geist Mono", "Geist"), declared as index.html
/// declares them, from the app's `/app/fonts/` named absolutely (`base`,
/// ending in `/`). A base that could end the rule early declares nothing:
/// the system stacks the tokens fall back to paint instead.
pub(crate) fn fonts_css(base: &str) -> String {
    let unsafe_char = |c: char| c.is_whitespace() || matches!(c, '<' | '>' | '{' | '}' | '(' | ')' | ';' | '"' | '\'' | '\\');
    if base.is_empty() || base.chars().any(unsafe_char) {
        return String::new();
    }
    FONT_FACES.trim().replace("{base}", base)
}

/// The rules [`fonts_css`] fills in — one `@font-face` a line, as the token
/// lint reads a declaration.
const FONT_FACES: &str = "
@font-face{font-family:\"Geist\";font-style:normal;font-weight:100 900;font-display:swap;src:url({base}geist-latin.woff2) format(\"woff2\")}
@font-face{font-family:\"Geist Mono\";font-style:normal;font-weight:100 900;font-display:swap;src:url({base}geist-mono-latin.woff2) format(\"woff2\")}
";

/// The frame's sandbox (COMP-1 decision 1, after lavish-axi's): scripts,
/// and popups that escape the sandbox — never `allow-same-origin`, which
/// would hand the body the app's origin. IDENTITY-28: and no longer
/// `allow-forms` — a body's form submits nowhere. Popups stay, and stay
/// escaping (owner, 2026-10-10): a link in a body opens the real page in a
/// new tab (`<base target="_blank">`), unsandboxed — since MENGER-14 once
/// the app has asked (src/links.rs): the SDK hands the click over.
pub(crate) const SANDBOX: &str = "allow-scripts allow-popups allow-popups-to-escape-sandbox";

/// IDENTITY-28: the frame's Content-Security-Policy, the first element of
/// its head — in force before any of the body's markup is parsed. No
/// connection of the frame's own (`fetch`, a WebSocket, an EventSource, a
/// beacon): its only way out is the host's bridge. No form submission
/// anywhere. Scripts, styles, images and fonts are left alone — bodies
/// import modules from esm.sh and show images from anywhere.
pub(crate) const CSP: &str = "connect-src 'none'; form-action 'none'";

/// DESIGN-40: where the app serves the house faces (Trunk copies
/// `frontend/fonts/` there; index.html's `@font-face` rules name them).
const FONTS_URL: &str = "/app/fonts/";

/// The class every frame's `<iframe>` wears — what the listener and
/// [`retheme_all`] look frames up by.
pub(crate) const FRAME_CLASS: &str = "mframe-doc";

/// A re-rendered frame that never reports `settled` replaces the one on
/// screen after this long anyway.
const PROMOTE_AFTER_MS: i32 = 3000;

/// A body that changes while a frame is on screen re-renders once it has
/// held still this long — a burst of keystrokes in the editor's preview is
/// one new frame, not one per key. A first frame loads at once.
const RERENDER_AFTER_MS: i32 = 150;

/// The height a frame may claim (the SDK caps it too).
const MAX_HEIGHT: f64 = 50_000.0;

/// COMP-1 pin 10: the theme tokens a frame receives — every custom property
/// of index.html's light `:root` block (pinned equal to it by a native
/// test in main.rs), sent with the values the app paints now. THEME-8: the
/// contract's list ([`menger_tickets_api::themes::TOKENS`]), the one a
/// theme may set — so a frame receives exactly what a theme can move.
pub(crate) fn theme_tokens() -> impl Iterator<Item = &'static str> {
    menger_tickets_api::themes::TOKENS.iter().map(|t| t.name)
}

/// The painted mode, as a frame's `color-scheme` and `data-theme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }

    /// From the root's computed `color-scheme` (index.html says `light`, or
    /// `dark` in both dark blocks).
    pub(crate) fn from_color_scheme(css: &str) -> Self {
        if css.split_whitespace().any(|w| w == "dark") {
            Scheme::Dark
        } else {
            Scheme::Light
        }
    }
}

/// What one frame's head is made of.
pub(crate) struct Shell<'a> {
    pub(crate) channel: &'a str,
    pub(crate) scheme: Scheme,
    /// THEME-7: the theme its region wears (`default`: none) — the SDK's
    /// `menger.theme`, so its first `menger:theme` moves nothing.
    pub(crate) theme: &'a str,
    /// `(token, value)` — [`theme_tokens`] as painted.
    pub(crate) tokens: &'a [(String, String)],
    /// The vendored mermaid bundle, absolute (the frame's origin is opaque;
    /// a classic script loads across origins).
    pub(crate) mermaid_url: &'a str,
    /// MENGER-22: the vendored highlighter, absolute, the same way.
    pub(crate) highlight_url: &'a str,
    /// DESIGN-40: where the house faces are (`…/app/fonts/`), absolute — the
    /// frame's origin is opaque, so its document names them by the app's
    /// origin (the shell answers them with CORS). Empty: none declared.
    pub(crate) fonts_url: &'a str,
    /// DESIGN-40: the app's Motion setting as painted now
    /// (`html[data-motion]`: [`Motion`]).
    pub(crate) motion: Motion,
    /// DESIGN-43: the app's Typed text setting as stamped now
    /// (`html[data-typing]`: [`Typing`]).
    pub(crate) typing: Typing,
    /// DESIGN-40: this body was already shown here (a re-render after an
    /// amend, a keystroke in a preview): its entrance moves do not play again.
    pub(crate) entered: bool,
    /// COMP-5: the component registry as the app holds it now — `None`
    /// before the app has fetched it (the frame is handed it once it has).
    pub(crate) components: Option<&'a [Component]>,
    /// INPUT-4: the ticket whose stored body the frame renders — `None` for
    /// one that renders none (an editor's preview, a help card).
    pub(crate) ticket: Option<&'a str>,
    /// COMP-14: a cited window's frame — the section of the body it shows
    /// (the SDK hides the rest); `None` for every other frame.
    pub(crate) cite: Option<&'a cite::Section>,
    /// MENGER-15: the preferences emulated — a mockup's (the page shim is
    /// inlined), or a whole document's (always inlined); `None` in any
    /// other frame, which gets no shim.
    pub(crate) emulate: Option<Emulate>,
}

/// A token name the frame will take: `--` and lowercase ASCII words.
fn token_name_ok(name: &str) -> bool {
    name.strip_prefix("--")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
}

/// A token value that cannot end its declaration, rule or `<style>` early.
/// The values are the app's own stylesheet's, so this only ever drops a
/// value nobody wrote.
fn token_value_ok(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains(['<', '>', '{', '}', ';', '\\'])
}

/// The frame's first theme: `:root { color-scheme; --token: value; … }`.
pub(crate) fn theme_css(scheme: Scheme, tokens: &[(String, String)]) -> String {
    let mut css = format!(":root{{color-scheme:{}", scheme.as_str());
    for (name, value) in tokens {
        if token_name_ok(name) && token_value_ok(value) {
            css.push(';');
            css.push_str(name);
            css.push(':');
            css.push_str(value.trim());
        }
    }
    css.push('}');
    css
}

/// The init block the SDK reads (`#menger-init`): JSON with every `<`
/// written as `<`, so nothing in it can close its `<script>`.
pub(crate) fn init_json(shell: &Shell) -> String {
    let init = serde_json::json!({
        "channel": shell.channel,
        "version": SDK_VERSION,
        "scheme": shell.scheme.as_str(),
        "theme": shell.theme,
        "mermaid": {
            "url": shell.mermaid_url,
            // The SDK sets `theme` per draw, from the scheme.
            "config": mermaid::config_json("default"),
            // THEME-2: and the theme's diagram colours, read from the
            // frame's tokens per draw — each token and the variables it sets.
            "diagram": menger_tickets_api::tokens::DIAGRAM.iter().map(|(t, k)| (t.to_string(), serde_json::json!(k))).collect::<serde_json::Map<_, _>>(),
        },
        // MENGER-22: the highlighter — its URL and the rules a block is
        // highlighted by (src/highlight.rs, the app's own).
        "highlight": highlight::init_json(shell.highlight_url),
        // COMP-5: the SDK defines each before the body is parsed, so a
        // component's first paint is its own.
        "components": shell.components,
        // INPUT-4: `menger.input.ticket` — what `submit` answers against.
        "ticket": shell.ticket,
        // COMP-14: a cited window shows one section, display only.
        "cite": shell.cite.map(|c| match c {
            cite::Section::Field(n) => serde_json::json!({ "field": n }),
            cite::Section::Id(n) => serde_json::json!({ "anchor": n }),
        }),
        // MENGER-15: what the page shim emulates (a mockup's frames).
        "emulate": shell.emulate.map(Emulate::json),
        // DESIGN-40: the Motion setting, and a re-render's "already shown".
        "motion": shell.motion.as_str(),
        "entered": shell.entered,
        // DESIGN-43: the Typed text speed.
        "typing": shell.typing.as_str(),
    });
    init.to_string().replace('<', "\\u003c")
}

/// MENGER-15: a whole document's init block — its channel and what the
/// shim emulates; nothing of the SDK's.
pub(crate) fn page_init_json(shell: &Shell) -> String {
    let init = serde_json::json!({
        "channel": shell.channel,
        "page": true,
        "ticket": shell.ticket,
        "emulate": shell.emulate.unwrap_or_default().json(),
    });
    init.to_string().replace('<', "\\u003c")
}

/// MENGER-15: where a whole document's head begins — past a byte-order
/// mark, the doctype and comments, inside its `<html …>` tag and its
/// `<head …>` tag where it has them: the point the CSP must come first at
/// (IDENTITY-28). With no `<head>` written, the parser opens one for the
/// first element there.
pub(crate) fn head_at(doc: &str) -> usize {
    let mut at = md::page_start(doc).unwrap_or(0);
    // `<!doctype …>`, comments, and whitespace between them.
    let skip = |mut at: usize| loop {
        at += doc[at..].len() - doc[at..].trim_start().len();
        let rest = &doc[at..];
        if let Some(after) = rest.strip_prefix("<!--") {
            match after.find("-->") {
                Some(e) => at += 4 + e + 3,
                None => return doc.len(),
            }
        } else if rest.get(..9).is_some_and(|d| d.eq_ignore_ascii_case("<!doctype")) {
            match rest.find('>') {
                Some(e) => at += e + 1,
                None => return doc.len(),
            }
        } else {
            return at;
        }
    };
    // Past an open tag named `name` at `at`, if one is there.
    let past = |at: usize, name: &str| -> Option<usize> {
        let rest = &doc[at..];
        let n = name.len() + 1;
        let opens = rest.get(..n).is_some_and(|t| t.eq_ignore_ascii_case(&format!("<{name}")))
            && rest.as_bytes().get(n).is_some_and(|c| c.is_ascii_whitespace() || *c == b'>' || *c == b'/');
        if !opens {
            return None;
        }
        // The tag's end: the first `>` outside a quoted attribute value.
        let mut quote = None;
        for (i, c) in rest.char_indices().skip(n) {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => return Some(at + i + 1),
                _ => {}
            }
        }
        None
    };
    at = skip(at);
    if let Some(after) = past(at, "html") {
        at = skip(after);
    }
    if let Some(after) = past(at, "head") {
        at = after;
    }
    at
}

/// MENGER-15: a whole document's srcdoc — the document itself, its own
/// head, styles and scripts, with IDENTITY-28's CSP injected as the first
/// element of its head, then the init block and the page shim, before
/// anything the page holds. No SDK, no theme tokens, no base: the page
/// renders as itself, under the same sandbox and CSP as every frame.
pub(crate) fn page_srcdoc(doc: &str, shell: &Shell) -> String {
    let at = head_at(doc);
    let mut out = String::with_capacity(doc.len() + PAGE_JS.len() + 512);
    out.push_str(&doc[..at]);
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    out.push_str(CSP);
    out.push_str("\"><script type=\"application/json\" id=\"menger-init\">");
    out.push_str(&page_init_json(shell));
    out.push_str("</script><script id=\"menger-page\">");
    out.push_str(PAGE_JS);
    out.push_str("</script>");
    out.push_str(&doc[at..]);
    out
}

/// A framed body's whole document: the head that makes it menger's (see
/// the module doc), then the body's markup as `render_body_framed` gave it.
pub(crate) fn srcdoc(body_html: &str, shell: &Shell) -> String {
    // MENGER-15: a whole document is its own page.
    if md::is_page(body_html) {
        return page_srcdoc(body_html, shell);
    }
    let mut doc = String::with_capacity(body_html.len() + SDK_JS.len() + 2048);
    doc.push_str("<!doctype html><html lang=\"en\" data-theme=\"");
    doc.push_str(shell.scheme.as_str());
    doc.push_str("\"><head><meta http-equiv=\"Content-Security-Policy\" content=\"");
    doc.push_str(CSP);
    doc.push_str("\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    doc.push_str("<base target=\"_blank\">");
    // DESIGN-40: the house faces, before the tokens that name them.
    let fonts = fonts_css(shell.fonts_url);
    if !fonts.is_empty() {
        doc.push_str("<style id=\"menger-fonts\">");
        doc.push_str(&fonts);
        doc.push_str("</style>");
    }
    doc.push_str("<style id=\"menger-theme\">");
    doc.push_str(&theme_css(shell.scheme, shell.tokens));
    doc.push_str("</style><script type=\"application/json\" id=\"menger-init\">");
    doc.push_str(&init_json(shell));
    // The inlined shell says which one it is and where the app publishes it.
    doc.push_str("</script><script id=\"menger-sdk\" data-version=\"");
    doc.push_str(SDK_VERSION);
    doc.push_str("\" data-src=\"");
    doc.push_str(SDK_URL);
    doc.push_str("\">");
    doc.push_str(SDK_JS);
    doc.push_str("</script>");
    // MENGER-15: a mockup's frame emulates the viewer's preferences.
    if shell.emulate.is_some() {
        doc.push_str("<script id=\"menger-page\">");
        doc.push_str(PAGE_JS);
        doc.push_str("</script>");
    }
    doc.push_str("</head><body>");
    doc.push_str(body_html);
    doc.push_str("</body></html>");
    doc
}

/// A message the host acts on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Msg {
    /// ACCESS-28: the frame document is up and asks for its document.
    Boot,
    Ready,
    /// The document's height in CSS px (clamped), and whether its first
    /// paint is complete.
    Size { height: u32, settled: bool },
    /// COMP-5: `menger.call` — its id, method and params are read by the
    /// bridge's caller.
    Call,
    /// COMP-9: `menger.navigate` (its route, and a wave reference's
    /// `reveal`, are read by the caller).
    Navigate,
    /// COMP-9: an instance's badge asks for its tooltip.
    Inspect,
    /// COMP-9: the frame holding the card the route names says where it is.
    Revealed,
    /// INPUT-4: `menger.input.submit` — its id and params are read by the
    /// caller, with the app's activation read at once.
    Submit,
    /// INPUT-9: `menger.input.clear` — read as a submit is.
    Clear,
    /// COMMENT-3: the element under the pointer in annotation mode.
    Hover,
    /// COMMENT-3: a click in annotation mode.
    Pick,
    /// COMMENT-3: which selectors pick the clicked element alone.
    Probed,
    /// COMMENT-3: where anchored elements are.
    Located,
    /// COMMENT-3: `a`/`A`, or Esc in the mode — the app's keys.
    Key,
    /// COMP-14: an `<m-cite>` placeholder says what it cites and where it is.
    Cite,
    /// COMP-14: an `<m-cite>` left the body.
    Uncite,
    /// APP-29: an `<m-mockup>` placeholder says what it draws and where.
    Mockup,
    /// APP-29: an `<m-mockup>` left the body.
    Unmock,
    /// APP-42: the page shim says how tall its page is, in px.
    Extent { height: u32 },
    /// APP-45: in annotation mode, a device's page scrolled — what it said
    /// about where elements are is stale.
    Scrolled,
    /// MENGER-14: a link in the body was clicked — its `href` as written,
    /// classified here (src/links.rs), never by the frame.
    Link,
    /// MENGER-34: a ticket chip in the body asked for the ticket menu.
    TicketMenu,
    /// FILTER-25: a ticket chip in the body was Alt+clicked — the global
    /// filter becomes its shortcode's clause.
    FilterEverywhere,
    /// LOOK-3: a whole body's sections and inputs, where each runs — what
    /// the ticket page's section dock is drawn from (src/sections.rs).
    Sections,
}

/// Read a `menger:*` message's kind and fields; anything else is ignored.
pub(crate) fn parse_msg(kind: &str, height: Option<f64>, settled: Option<bool>) -> Option<Msg> {
    match kind {
        "menger:boot" => Some(Msg::Boot),
        "menger:ready" => Some(Msg::Ready),
        "menger:call" => Some(Msg::Call),
        "menger:navigate" => Some(Msg::Navigate),
        "menger:inspect" => Some(Msg::Inspect),
        "menger:revealed" => Some(Msg::Revealed),
        "menger:submit" => Some(Msg::Submit),
        "menger:clear" => Some(Msg::Clear),
        "menger:hover" => Some(Msg::Hover),
        "menger:pick" => Some(Msg::Pick),
        "menger:probed" => Some(Msg::Probed),
        "menger:located" => Some(Msg::Located),
        "menger:key" => Some(Msg::Key),
        "menger:cite" => Some(Msg::Cite),
        "menger:uncite" => Some(Msg::Uncite),
        "menger:mockup" => Some(Msg::Mockup),
        "menger:unmock" => Some(Msg::Unmock),
        "menger:link" => Some(Msg::Link),
        "menger:ticket-menu" => Some(Msg::TicketMenu),
        "menger:filter-everywhere" => Some(Msg::FilterEverywhere),
        "menger:sections" => Some(Msg::Sections),
        "menger:scrolled" => Some(Msg::Scrolled),
        "menger:extent" => {
            let h = height.filter(|h| h.is_finite())?;
            Some(Msg::Extent { height: h.clamp(0.0, MAX_HEIGHT).ceil() as u32 })
        }
        "menger:size" => {
            let h = height.filter(|h| h.is_finite())?;
            Some(Msg::Size { height: h.clamp(0.0, MAX_HEIGHT).ceil() as u32, settled: settled.unwrap_or(false) })
        }
        _ => None,
    }
}

// -- the DOM half ----------------------------------------------------------

thread_local! {
    static NEXT_CHANNEL: Cell<u32> = const { Cell::new(0) };
    static LISTENING: Cell<bool> = const { Cell::new(false) };
    /// COMP-9: the board's model of the store — whose extents say which
    /// shortcodes and tickets exist ([`know`]).
    static BOARD: Cell<Option<board::Board>> = const { Cell::new(None) };
    /// COMP-9: the card the route names (`?component=m-x`), until a frame
    /// says where it is.
    /// COMP-14: or the section (`?field=`, `?anchor=`).
    static ANCHOR: std::cell::RefCell<Option<inspect::Anchor>> = const { std::cell::RefCell::new(None) };
}

/// COMP-9: the app's board — what an instance's `shortcode` and `ticket`
/// attributes are checked against. Called once by the App.
pub(crate) fn know(board: board::Board) {
    BOARD.with(|b| b.set(Some(board)));
}

/// What the app knows exists: the board's extents (nothing, before they
/// are in — the checks then go by form alone).
struct AppKnows;

impl menger_tickets_api::Known for AppKnows {
    fn shortcode(&self, epic: &str) -> Option<bool> {
        BOARD.with(Cell::get).and_then(|b| b.has_shortcode_untracked(epic))
    }
    fn ticket(&self, id: &str) -> Option<bool> {
        BOARD.with(Cell::get).and_then(|b| b.has_ticket_untracked(id))
    }
}

fn next_channel() -> String {
    NEXT_CHANNEL.with(|n| {
        let v = n.get().wrapping_add(1);
        n.set(v);
        format!("f{v}")
    })
}

/// The theme painted now: the root's `color-scheme`, and each token's value.
fn read_theme() -> (Scheme, Vec<(String, String)>) {
    read_theme_at(None)
}

/// THEME-8: the theme painted now where `at` is — the root's
/// `color-scheme`, and each token's value as `at` inherits it (its region's
/// theme: src/scopes.rs). `None`: the root's, the site's.
fn read_theme_at(at: Option<&web_sys::Element>) -> (Scheme, Vec<(String, String)>) {
    let w = dom::window();
    let root = w.document().and_then(|d| d.document_element());
    let style = root.as_ref().and_then(|root| w.get_computed_style(root).ok().flatten());
    let Some(style) = style else {
        return (Scheme::Light, Vec::new());
    };
    let scheme = Scheme::from_color_scheme(&style.get_property_value("color-scheme").unwrap_or_default());
    let style = match at.and_then(|el| w.get_computed_style(el).ok().flatten()) {
        Some(here) => here,
        None => style,
    };
    let tokens = theme_tokens()
        .filter_map(|name| {
            let v = style.get_property_value(name).ok()?;
            let v = v.trim();
            (!v.is_empty()).then(|| (name.to_string(), v.to_string()))
        })
        .collect();
    (scheme, tokens)
}

/// MENGER-15: the app's painted mode — where a mockup's toggle starts.
pub(crate) fn painted_scheme() -> Scheme {
    read_theme().0
}

fn set(obj: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), value);
}

/// `menger:theme` for `frame` on `channel`, as painted now where it is.
fn theme_message(frame: &web_sys::HtmlIFrameElement, channel: &str) -> JsValue {
    let (scheme, tokens) = read_theme_at(Some(frame));
    let msg = Object::new();
    set(&msg, "type", &"menger:theme".into());
    set(&msg, "channel", &channel.into());
    set(&msg, "scheme", &scheme.as_str().into());
    let t = Object::new();
    for (name, value) in tokens {
        set(&t, &name, &value.into());
    }
    set(&msg, "tokens", &t);
    // THEME-3: the rules of the theme the frame's region wears — its fonts
    // and `::part`s — named where it sits (scopes::THEME_MARK); THEME-7: and
    // its name, the SDK's `menger.theme`.
    let worn = worn_at(frame);
    set(&msg, "css", &crate::scopes::rules_for(&worn).into());
    set(&msg, "theme", &worn.into());
    // DESIGN-40: and the Motion setting, so a frame that loaded across a
    // change wears the one painted now.
    set(&msg, "motion", &host_motion().as_str().into());
    // DESIGN-43: and the Typed text speed, the same way.
    set(&msg, "typing", &host_typing().as_str().into());
    msg.into()
}

/// DESIGN-43: the app's Typed text setting as stamped now —
/// `html[data-typing]` on the app's document; absent is `normal`.
fn host_typing() -> Typing {
    let v = dom::window().document().and_then(|d| d.document_element()).and_then(|e| e.get_attribute("data-typing"));
    Typing::parse(v.as_deref())
}

/// DESIGN-43: `menger:typing { typing }` to every open frame — the app's
/// Typed text setting moved (src/motion.rs sets `html[data-typing]`).
fn retype_all() {
    let typing = host_typing();
    for f in frames() {
        if let Some(channel) = f.get_attribute("data-channel") {
            let msg = message("menger:typing", &channel);
            set(&msg, "typing", &typing.as_str().into());
            post(&f, &msg.into());
        }
    }
}

/// DESIGN-40: the app's Motion setting as painted now — DESIGN-39's
/// `html[data-motion]` on the app's document; absent is `full`.
fn host_motion() -> Motion {
    let v = dom::window().document().and_then(|d| d.document_element()).and_then(|e| e.get_attribute("data-motion"));
    Motion::parse(v.as_deref())
}

/// DESIGN-40: `menger:motion { motion }` to every open frame — the app's
/// Motion setting moved (DESIGN-39 sets `html[data-motion]`).
fn remotion_all() {
    let motion = host_motion();
    for f in frames() {
        if let Some(channel) = f.get_attribute("data-channel") {
            let msg = message("menger:motion", &channel);
            set(&msg, "motion", &motion.as_str().into());
            post(&f, &msg.into());
        }
    }
}

/// DESIGN-40: the Motion setting reaches every frame the moment it moves —
/// one observer on the app's `<html>`, for the app's lifetime. DESIGN-43:
/// the same observer carries the Typed text setting (`data-typing`), each
/// attribute that moved posting its own message.
fn watch_motion() {
    let Some(root) = dom::window().document().and_then(|d| d.document_element()) else { return };
    let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(|records: js_sys::Array, _: web_sys::MutationObserver| {
        let (mut motion, mut typing) = (false, false);
        for r in records.iter() {
            match r.dyn_ref::<web_sys::MutationRecord>().and_then(|r| r.attribute_name()).as_deref() {
                Some("data-typing") => typing = true,
                _ => motion = true,
            }
        }
        if motion {
            remotion_all();
        }
        if typing {
            retype_all();
        }
    });
    let Ok(observer) = web_sys::MutationObserver::new(cb.as_ref().unchecked_ref()) else { return };
    cb.forget();
    let opts = web_sys::MutationObserverInit::new();
    opts.set_attributes(true);
    let filter = js_sys::Array::of2(&JsValue::from_str("data-motion"), &JsValue::from_str("data-typing"));
    opts.set_attribute_filter(&filter);
    let _ = observer.observe_with_options(&root, &opts);
}

/// THEME-3/THEME-7: the theme the region around `el` wears, by the name it
/// says where `el` sits (scopes::THEME_MARK); `default` where none is said.
fn worn_at(el: &web_sys::Element) -> String {
    let worn = dom::window()
        .get_computed_style(el)
        .ok()
        .flatten()
        .and_then(|s| s.get_property_value(crate::scopes::THEME_MARK).ok())
        .unwrap_or_default();
    let worn = worn.trim();
    if worn.is_empty() { menger_tickets_api::themes::DEFAULT_THEME.to_string() } else { worn.to_string() }
}

fn post(frame: &web_sys::HtmlIFrameElement, msg: &JsValue) {
    if let Some(w) = frame.content_window() {
        let _ = w.post_message(msg, "*");
    }
}

/// A typed value as the plain JS data a frame takes (wire spelling): what
/// serde writes, parsed by the browser. Nothing here is untyped.
fn to_js<T: Serialize + ?Sized>(value: &T) -> JsValue {
    serde_json::to_string(value)
        .ok()
        .and_then(|json| js_sys::JSON::parse(&json).ok())
        .unwrap_or(JsValue::NULL)
}

/// `{ type, channel }`, the rest of the message to be set.
fn message(kind: &str, channel: &str) -> Object {
    let msg = Object::new();
    set(&msg, "type", &kind.into());
    set(&msg, "channel", &channel.into());
    msg
}

/// COMP-5: the registry as the app holds it, to every open frame — after a
/// fetch lands (the first, a reconnect's, a `lagged`'s). A frame diffs it
/// against what it has: a changed source re-renders, a name gone is gone.
pub(crate) fn broadcast_registry() {
    let Some(list) = registry::snapshot() else { return };
    let all = frames();
    if all.is_empty() {
        return;
    }
    let components = to_js(&list);
    for f in all {
        if let Some(channel) = f.get_attribute("data-channel") {
            let msg = message("menger:components", &channel);
            set(&msg, "components", &components);
            post(&f, &msg.into());
        }
    }
}

/// COMP-5: a `watch` event, to every open frame as a change notice — the
/// SDK applies a component event to its definitions, then hands every
/// event to `menger.on('change')`. Called by the session for each event.
/// IDENTITY-36: each frame hears only what its reach admits
/// ([`bridge::admit_event`] — its scope's shortcodes, as last asked; a
/// frame with no scope hears components alone), and a `group_changed`
/// that may move a shortcode into or out of a reach asks that reach again.
pub(crate) fn broadcast_change(ev: &TicketsEvent) {
    // COMP-14: a cited ticket moved — its windows follow.
    cite::heard(ev);
    // APP-29: and a referenced mockup's page.
    mockup::heard(ev);
    if let TicketsEvent::GroupChanged { group } = ev {
        for scope in bridge::note_group(group) {
            refresh_reach(scope);
        }
    }
    let all = frames();
    if all.is_empty() {
        return;
    }
    for f in all {
        let Some(channel) = f.get_attribute("data-channel") else { continue };
        let reach = match f.get_attribute("data-scope") {
            Some(scope) => bridge::known_reach(&scope),
            None => bridge::Reach::none(),
        };
        if let Some(heard) = bridge::admit_event(ev.clone(), &reach) {
            let msg = message("menger:change", &channel);
            set(&msg, "event", &to_js(&heard));
            post(&f, &msg.into());
        }
    }
}

/// IDENTITY-36: ask the server again how far `scope` reaches (remembered by
/// the bridge for [`broadcast_change`]).
fn refresh_reach(scope: String) {
    spawn_local(async move {
        if let Some(client) = session::client() {
            let _ = bridge::reach_of(&client, &scope).await;
        }
    });
}

/// COMP-5: answer a frame's `menger.call` — the bridge parses it (a read,
/// or refused), makes it over the app's connection, and the frame gets
/// `menger:result { id, items }` or `{ id, error }`. A frame replaced
/// meanwhile has no window to answer, and is not answered. IDENTITY-36:
/// held to the frame's reach — `scope`, the shortcode the host stamped on
/// it (`data-scope`), which nothing in the frame can move.
fn answer_call(
    frame: web_sys::HtmlIFrameElement,
    channel: String,
    id: JsValue,
    method: String,
    params: Result<String, String>,
    scope: Option<String>,
) {
    spawn_local(async move {
        let reply = match params.and_then(|p| bridge::parse_call(&method, &p)) {
            Err(e) => Err(e),
            Ok(call) => match session::client() {
                Some(client) => bridge::run_scoped(&client, call, scope.as_deref()).await,
                None => Err(format!("{method}: not connected")),
            },
        };
        let msg = message("menger:result", &channel);
        set(&msg, "id", &id);
        match reply {
            Ok(items) => set(&msg, "items", &to_js(&items)),
            Err(error) => set(&msg, "error", &error.into()),
        }
        post(&frame, &msg.into());
    });
}

/// INPUT-4: answer a frame's `menger.input.submit` — `menger:submitted {
/// id, answer }` (the recorded answer, the wire's spelling) or `{ id, error
/// }`: refused unless a gesture on this frame is under way (`gesture`,
/// IDENTITY-36: [`gesture::on_frame`], read at the arrival; APP-35: one
/// put off is read again by [`gesture::settle`]) and the frame
/// renders a ticket (`ticket`, from its `data-ticket`), then the bridge's
/// checks against that ticket's inputs, then `tickets.inputs.answer`.
/// INPUT-9: and (`write` [`Msg::Clear`]) its `menger.input.clear` —
/// `menger:cleared { id, input }` or `{ id, error }`, the same way, then
/// `tickets.inputs.clear`. A frame replaced meanwhile is
/// not answered.
fn answer_write(
    write: Msg,
    frame: web_sys::HtmlIFrameElement,
    channel: String,
    id: JsValue,
    ticket: Option<String>,
    gesture: gesture::Verdict,
    params: Result<String, String>,
    cited: bool,
) {
    let (sdk, reply_kind, field) = if write == Msg::Clear {
        (bridge::CLEAR, "menger:cleared", "input")
    } else {
        (bridge::SUBMIT, "menger:submitted", "answer")
    };
    spawn_local(async move {
        let reply = async {
            // COMP-14: a cited window's inputs are display only.
            if cited {
                return Err(bridge::display_only(sdk));
            }
            let gesture = gesture::settle(&frame, gesture).await;
            let ticket = bridge::admit(sdk, ticket.as_deref(), gesture)?;
            let params = params?;
            let client = || session::client().ok_or_else(|| format!("{sdk}: not connected (nothing sent)"));
            if write == Msg::Clear {
                let clear = bridge::parse_clear(&params)?;
                bridge::clear(&client()?, ticket, clear).await.map(|input| to_js(&input))
            } else {
                let submit = bridge::parse_submit(&params)?;
                bridge::submit(&client()?, ticket, submit).await.map(|answer| to_js(&answer))
            }
        }
        .await;
        let msg = message(reply_kind, &channel);
        set(&msg, "id", &id);
        match reply {
            Ok(done) => set(&msg, field, &done),
            Err(error) => set(&msg, "error", &error.into()),
        }
        post(&frame, &msg.into());
    });
}

/// Every frame in the document.
fn frames() -> Vec<web_sys::HtmlIFrameElement> {
    let Some(list) = dom::window()
        .document()
        .and_then(|d| d.query_selector_all(&format!("iframe.{FRAME_CLASS}")).ok())
    else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.get(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlIFrameElement>().ok())
        .collect()
}

/// COMP-1 pin 10: the painted mode changed — every frame takes the new
/// tokens (and redraws its diagrams). THEME-8: each its own region's, read
/// where the frame is. Called by the one repaint path (src/scopes.rs's
/// `repaint`) beside `mermaid::rerender_all`. Nothing here can throw.
pub(crate) fn retheme_all() {
    let scheme = read_theme().0;
    for f in frames() {
        if let Some(channel) = f.get_attribute("data-channel") {
            // MENGER-15: a whole document following the app is told its
            // `prefers-color-scheme` moved; a mockup's keeps its toggles.
            if f.has_attribute("data-follow") {
                emulate(&f, Emulate { scheme: Some(scheme), reduce: None });
            } else if !f.has_attribute("data-page") {
                post(&f, &theme_message(&f, &channel));
            }
        }
    }
}

/// MENGER-15: tell `frame` what it emulates now — `menger:emulate` to its
/// page shim, and the `<iframe>`'s own `color-scheme` (which is what
/// Firefox's `prefers-color-scheme` inside it follows; Chromium's follows
/// the embedder's whatever it says, hence the shim).
pub(crate) fn emulate(frame: &web_sys::HtmlIFrameElement, e: Emulate) {
    let style = web_sys::HtmlElement::style(frame);
    match e.scheme {
        Some(s) => {
            let _ = style.set_property("color-scheme", s.as_str());
        }
        None => {
            let _ = style.remove_property("color-scheme");
        }
    }
    let Some(channel) = frame.get_attribute("data-channel") else { return };
    let msg = message("menger:emulate", &channel);
    set(&msg, "scheme", &e.scheme.map_or(JsValue::NULL, |s| s.as_str().into()));
    set(&msg, "reduce", &e.reduce.map_or(JsValue::NULL, JsValue::from_bool));
    post(frame, &msg.into());
}

/// MENGER-15: every frame in `container` (the one on screen and one still
/// loading) emulates `e`.
pub(crate) fn emulate_in(container: &web_sys::Element, e: Emulate) {
    let Ok(list) = container.query_selector_all(&format!("iframe.{FRAME_CLASS}")) else { return };
    for i in 0..list.length() {
        if let Some(f) = list.get(i).and_then(|n| n.dyn_into::<web_sys::HtmlIFrameElement>().ok()) {
            emulate(&f, e);
        }
    }
}

/// The frame on screen in `frame`'s container becomes `frame`: the old one
/// (and any other) goes, the new one stops being invisible.
fn promote(frame: &web_sys::HtmlIFrameElement) {
    let _ = frame.set_attribute("data-state", "live");
    let Some(parent) = frame.parent_element() else { return };
    let Ok(list) = parent.query_selector_all(":scope > iframe") else { return };
    for i in 0..list.length() {
        if let Some(el) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
            if !Object::is(&el, frame) {
                el.remove();
            }
        }
    }
    // COMP-14: the old frame's cite windows go with it; the new one's show.
    cite::promoted(frame);
    // APP-29: so do its mockups.
    mockup::promoted(frame);
}

fn has_live_sibling(frame: &web_sys::HtmlIFrameElement) -> bool {
    frame
        .parent_element()
        .and_then(|p| p.query_selector(":scope > iframe[data-state=\"live\"]").ok().flatten())
        .is_some()
}

fn on_message(ev: web_sys::MessageEvent) {
    let data = ev.data();
    if !data.is_object() {
        return;
    }
    let field = |k: &str| Reflect::get(&data, &JsValue::from_str(k)).ok();
    let Some(kind) = field("type").and_then(|v| v.as_string()).filter(|k| k.starts_with("menger:")) else {
        return;
    };
    let Some(source) = ev.source() else { return };
    let Some(channel) = field("channel").and_then(|v| v.as_string()) else { return };
    // The sender must be a frame's own window, speaking on that frame's
    // channel — never another window, and never a stale document.
    let Some(frame) = frames().into_iter().find(|f| f.content_window().is_some_and(|w| Object::is(&w, &source))) else {
        return;
    };
    if frame.get_attribute("data-channel").as_deref() != Some(channel.as_str()) {
        return;
    }
    match parse_msg(&kind, field("height").and_then(|v| v.as_f64()), field("settled").and_then(|v| v.as_bool())) {
        // ACCESS-28: the document this frame was made for — again if the
        // frame reloads; nothing it did not already hold.
        Some(Msg::Boot) => {
            if let Some(html) = Reflect::get(&frame, &JsValue::from_str(DOC_PROP)).ok().filter(|v| v.is_string()) {
                let msg = message("menger:doc", &channel);
                set(&msg, "html", &html);
                post(&frame, &msg.into());
            }
        }
        Some(Msg::Ready) => {
            // MENGER-15: a whole document reports no size — ready is shown.
            if frame.has_attribute("data-page") {
                if frame.get_attribute("data-state").as_deref() == Some("pending") {
                    promote(&frame);
                }
                // APP-45: and it takes annotation mode, its shim's.
                if annotate::is_on() {
                    post(&frame, &annotate_message(&channel, true));
                }
                annotate::frame_changed(&channel);
                return;
            }
            post(&frame, &theme_message(&frame, &channel));
            // COMMENT-3: a frame born in annotation mode is in it too.
            if annotate::is_on() {
                post(&frame, &annotate_message(&channel, true));
            }
            annotate::frame_changed(&channel);
            // COMP-5: what moved since the srcdoc was built.
            if let Some(list) = registry::snapshot() {
                let msg = message("menger:components", &channel);
                set(&msg, "components", &to_js(&list));
                post(&frame, &msg.into());
            }
        }
        Some(Msg::Call) => {
            let id = field("id").filter(|v| v.as_f64().is_some()).unwrap_or(JsValue::NULL);
            let method = field("method").and_then(|v| v.as_string()).unwrap_or_default();
            let params = match field("params").filter(|v| !v.is_undefined() && !v.is_null()) {
                None => Ok(String::new()),
                Some(v) => js_sys::JSON::stringify(&v)
                    .ok()
                    .and_then(|s| s.as_string())
                    .ok_or_else(|| format!("{method}: params are not plain data")),
            };
            // IDENTITY-36: the host's own stamp — the frame cannot name one.
            let scope = frame.get_attribute("data-scope");
            answer_call(frame, channel, id, method, params, scope);
        }
        Some(write @ (Msg::Submit | Msg::Clear)) => {
            // Now, while the gesture that sent it is under way — and on
            // this very frame (IDENTITY-36), not anywhere in the app; or,
            // APP-35, once the app's own activation has run out.
            let gesture = gesture::on_frame(&frame);
            let sdk = if write == Msg::Clear { bridge::CLEAR } else { bridge::SUBMIT };
            let id = field("id").filter(|v| v.as_f64().is_some()).unwrap_or(JsValue::NULL);
            let params = field("params")
                .filter(|v| !v.is_undefined() && !v.is_null())
                .and_then(|v| js_sys::JSON::stringify(&v).ok())
                .and_then(|s| s.as_string())
                .ok_or_else(|| format!("{sdk}: its params are missing or not plain data (nothing sent)"));
            // The host's own stamp — nothing in the frame can move it.
            let ticket = frame.get_attribute("data-ticket");
            let cited = frame.has_attribute("data-cite-key");
            answer_write(write, frame, channel, id, ticket, gesture, params, cited);
        }
        Some(Msg::Size { height, settled }) => {
            // MENGER-15: a device's frame keeps its device's height.
            if !frame.has_attribute("data-fixed") {
                let _ = web_sys::HtmlElement::style(&frame).set_property("height", &format!("{height}px"));
            }
            // COMMENT-3: its elements may have moved under their pins.
            annotate::frame_changed(&channel);
            // A first frame shows at once; a re-render waits until drawn.
            if frame.get_attribute("data-state").as_deref() == Some("pending") && (settled || !has_live_sibling(&frame)) {
                promote(&frame);
            }
            match frame.get_attribute("data-cite-key") {
                // COMP-14: a window's frame — where its section runs.
                Some(key) => {
                    let top = field("top").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let found = field("found").and_then(|v| v.as_bool()).unwrap_or(false);
                    cite::sized(&frame, &key, top, f64::from(height), found, settled);
                }
                // A citing frame resized: its windows are laid again
                // (APP-29: and its mockups).
                None => {
                    cite::relayout(&channel);
                    mockup::relayout(&channel);
                }
            }
            // COMP-9: a card the route names may be in this frame.
            if settled && !frame.has_attribute("data-cite-key") {
                if let Some(anchor) = ANCHOR.with(|a| a.borrow().clone()) {
                    post(&frame, &reveal_message(&channel, &anchor));
                }
            }
        }
        // LOOK-3: the ticket page's own body says where its sections are —
        // never a cited window, a whole document or another page's frame.
        Some(Msg::Sections) => {
            let body = frame.closest("#ticket-page").ok().flatten().is_some()
                && !frame.has_attribute("data-cite-key")
                && !frame.has_attribute("data-page");
            if body {
                let items = field("items")
                    .and_then(|v| js_sys::JSON::stringify(&v).ok())
                    .and_then(|s| s.as_string())
                    .and_then(|s| serde_json::from_str::<Vec<crate::sections::Item>>(&s).ok())
                    .unwrap_or_default();
                crate::sections::heard(&channel, items);
            }
        }
        Some(Msg::Navigate) => {
            // MENGER-15: an app route leaves full screen.
            present::escape();
            let route = field("route").and_then(|v| v.as_string()).unwrap_or_default();
            let reveal = field("reveal").filter(|v| v.is_object());
            let on_board = frame.closest("#waves").ok().flatten().is_some();
            if let (Some(r), true) = (reveal, on_board) {
                let get = |k: &str| Reflect::get(&r, &JsValue::from_str(k)).ok().and_then(|v| v.as_string()).unwrap_or_default();
                if notes::reveal_on_board(&get("id"), &get("wave")) {
                    return;
                }
            }
            navigate(&route);
        }
        Some(Msg::Inspect) => {
            let id = field("id").filter(|v| v.as_f64().is_some()).unwrap_or(JsValue::NULL);
            let name = field("name").and_then(|v| v.as_string()).unwrap_or_default().to_ascii_lowercase();
            let attrs = field("attrs").map(read_pairs).unwrap_or_default();
            let component = registry::snapshot().and_then(|list| list.into_iter().find(|c| c.name == name));
            let inspection = inspect::inspect(&name, component.as_ref(), &attrs, &AppKnows);
            let msg = message("menger:inspected", &channel);
            set(&msg, "id", &id);
            set(&msg, "inspection", &to_js(&inspection));
            post(&frame, &msg.into());
        }
        Some(Msg::Hover) => {
            let chain = field("chain").and_then(read_chain);
            let rect = field("rect").map(read_rect).unwrap_or_default();
            let text = field("text").and_then(|v| v.as_string()).unwrap_or_default();
            // APP-45: the keys walk in the device last hovered.
            if chain.is_some() {
                crate::mock_anchor::hovered(&frame);
            }
            annotate::frame_hover(frame, chain, rect, text);
        }
        Some(Msg::Pick) => {
            let Some(id) = field("id").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0) else { return };
            let Some(chain) = field("chain").and_then(read_chain) else { return };
            let rect = field("rect").map(read_rect).unwrap_or_default();
            let text = field("text").and_then(|v| v.as_string()).unwrap_or_default();
            annotate::frame_pick(frame, &channel, id as u32, chain, rect, text);
        }
        Some(Msg::Probed) => {
            let Some(id) = field("id").and_then(|v| v.as_f64()) else { return };
            let ok = field("ok")
                .and_then(|v| v.dyn_into::<js_sys::Array>().ok())
                .map(|l| l.iter().map(|b| b.as_bool() == Some(true)).collect())
                .unwrap_or_default();
            annotate::frame_probed(&channel, id as u32, ok);
        }
        Some(Msg::Located) => {
            let Some(id) = field("id").and_then(|v| v.as_f64()) else { return };
            let found = field("found")
                .and_then(|v| v.dyn_into::<js_sys::Array>().ok())
                .map(|l| {
                    l.iter()
                        .map(|f| {
                            f.is_object().then(|| {
                                let rect = Reflect::get(&f, &JsValue::from_str("rect")).map(read_rect).unwrap_or_default();
                                let text = Reflect::get(&f, &JsValue::from_str("text")).ok().and_then(|t| t.as_string()).unwrap_or_default();
                                (rect, text)
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            annotate::frame_located(&channel, id as u32, found);
        }
        Some(Msg::Key) => {
            if let Some(key) = field("key").and_then(|v| v.as_string()) {
                // APP-45: in annotation mode, a device's walk keys.
                if crate::mock_anchor::frame_key(&frame, &key) {
                    return;
                }
                // AGS-28: what the posted key is, inside a framed body.
                match crate::bindings::key_hit(&key, crate::bindings::Context::Frame) {
                    // MENGER-6: the palette's chord, pressed inside the body.
                    Some("palette.toggle") => crate::keys::palette_from_frame(),
                    // MENGER-15: full screen, from inside the body.
                    Some("view.full-screen") => {
                        if crate::keys::no_overlay() {
                            present::toggle_from(&frame);
                        }
                    }
                    // What annotation mode has open first, then full screen.
                    Some("app.escape") => {
                        if !annotate::escape() {
                            present::escape();
                        }
                    }
                    Some("annotate.toggle") => annotate::frame_toggle(),
                    _ => {}
                }
            }
        }
        Some(Msg::Revealed) => {
            let top = field("top").and_then(|v| v.as_f64()).filter(|t| t.is_finite()).unwrap_or(0.0);
            // COMP-14: the anchor it answers for, by the key it came under.
            let wanted = ANCHOR.with(|a| {
                a.borrow().as_ref().is_some_and(|anchor| {
                    let (key, value) = anchor.pair();
                    field(key).and_then(|v| v.as_string()).as_deref() == Some(value)
                })
            });
            if wanted {
                ANCHOR.with(|a| *a.borrow_mut() = None);
                scroll_into_frame(&frame, top);
            }
        }
        Some(Msg::Cite) => {
            let Some(n) = field("cite").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0 && *v < 1e9) else { return };
            let reference = field("ref").and_then(|v| v.as_string()).unwrap_or_default();
            let cite_field = field("field").and_then(|v| v.as_string());
            let r = field("rect").map(read_rect).unwrap_or_default();
            let rect = cite::Rect { x: r.x, y: r.y, w: r.w, h: r.h };
            cite::asked(frame, channel, n as u32, reference, cite_field, rect);
        }
        Some(Msg::Link) => {
            // MENGER-14: the frame hands over the string it was given; the
            // host decides what it is. A trusted domain opens at once only
            // inside a viewer's gesture on this very frame (IDENTITY-36's
            // reading) — else, like any other, it asks.
            let Some(href) = field("href").and_then(|v| v.as_string()) else { return };
            // Asked at once: a link that waits would open seconds late (it asks instead).
            let gesture = gesture::on_frame(&frame) == gesture::Verdict::Yes;
            // MENGER-15: the leave dialog (or the route) is the app's, drawn
            // outside the full-screen box — which steps aside for it.
            present::escape();
            let ticket = frame.get_attribute("data-ticket").filter(|t| !t.is_empty());
            let place = if frame.closest("#waves").ok().flatten().is_some() {
                "board note"
            } else if ticket.is_none() && frame.closest(".preview").ok().flatten().is_some() {
                "draft"
            } else {
                "body"
            };
            let restore = frame.clone().dyn_into::<web_sys::HtmlElement>().ok();
            links::follow(&href, links::From { ticket, place }, gesture, None, restore);
        }
        Some(Msg::TicketMenu) => {
            // MENGER-34: the app's one ticket menu, on a chip in the body —
            // only while a gesture is live with focus in this very frame (a
            // body's script cannot pop it up; it writes nothing, so not
            // IDENTITY-36's whole reading, whose engagement a pointer resting
            // on the frame across an Esc in the app would fail), on what has
            // a ticket's form, at a point inside the frame's box.
            let Some(id) = field("id").and_then(|v| v.as_string()).and_then(|v| crate::menu::frame_ticket(&v)) else { return };
            if annotate::is_on() || !gesture::live_in(&frame) {
                return;
            }
            let (cw, ch) = (frame.client_width() as f64, frame.client_height() as f64);
            let coord = |k: &str, max: f64| field(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, max.max(0.0)));
            let (Some(x), Some(y)) = (coord("x", cw), coord("y", ch)) else { return };
            let keyboard = field("keyboard").and_then(|v| v.as_bool()).unwrap_or(false);
            // MENGER-15: the menu is the app's, drawn above the full-screen
            // box (z-index) but never above the browser's own full screen,
            // which steps aside instead — the chip is then in the page again.
            if present::native() {
                present::escape();
                return;
            }
            let r = frame.get_bounding_client_rect();
            let (vx, vy) = crate::menu::frame_point(r.left(), r.top(), r.width(), cw, x, y);
            crate::menu::open_from_frame(id, vx, vy, keyboard);
        }
        Some(Msg::FilterEverywhere) => {
            // FILTER-25: as the ticket menu's message is checked — a live
            // gesture in this very frame (a body's script cannot set the
            // filter), never while annotating, on what has a ticket's form.
            let Some(id) = field("id").and_then(|v| v.as_string()).and_then(|v| crate::menu::frame_ticket(&v)) else { return };
            if annotate::is_on() || !gesture::live_in(&frame) {
                return;
            }
            crate::tags::filter_everywhere(&id);
        }
        Some(Msg::Mockup) => {
            let Some(n) = field("mock").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0 && *v < 1e9) else { return };
            let text = |k: &str| field(k).and_then(|v| v.as_string());
            let r = field("rect").map(read_rect).unwrap_or_default();
            let rect = cite::Rect { x: r.x, y: r.y, w: r.w, h: r.h };
            let posted = mockup::Posted {
                reference: text("ref"),
                widths: text("widths"),
                source: text("source"),
                title: text("title"),
                description: text("description"),
                // APP-46: the decision's option it sits in.
                option: field("option").filter(JsValue::is_object).and_then(|o| {
                    let get = |k: &str| Reflect::get(&o, &JsValue::from_str(k)).ok().and_then(|v| v.as_string());
                    crate::mockpick::choice(get("input").as_deref(), get("value").as_deref(), get("name").as_deref())
                }),
            };
            mockup::asked(frame, channel, n as u32, posted, rect);
        }
        // APP-42: a device's frame says how tall its page is — only a frame
        // held to its device's screen counts (the bar's one-view note).
        Some(Msg::Extent { height }) => {
            if frame.has_attribute("data-fixed") {
                present::extent(&frame, height);
            }
        }
        Some(Msg::Scrolled) => annotate::frame_changed(&channel),
        Some(Msg::Unmock) => {
            if let Some(n) = field("mock").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0) {
                mockup::dropped(&channel, n as u32);
            }
        }
        Some(Msg::Uncite) => {
            if let Some(n) = field("cite").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0) {
                cite::dropped(&channel, n as u32);
            }
        }
        None => {}
    }
}

/// `[[name, value], …]` (an instance's attributes, as the SDK sends them).
fn read_pairs(v: JsValue) -> Vec<(String, String)> {
    let Ok(list) = v.dyn_into::<js_sys::Array>() else { return Vec::new() };
    list.iter()
        .filter_map(|pair| {
            let pair = pair.dyn_into::<js_sys::Array>().ok()?;
            Some((pair.get(0).as_string()?, pair.get(1).as_string()?))
        })
        .take(256)
        .collect()
}

/// COMMENT-3: a chain of nodes as the SDK posts it (src/anchor.rs's
/// `Node`, the same fields) — plain data, read into the typed struct.
fn read_chain(v: JsValue) -> Option<Vec<anchor::Node>> {
    if !v.is_object() {
        return None;
    }
    let json = js_sys::JSON::stringify(&v).ok()?.as_string()?;
    serde_json::from_str::<Vec<anchor::Node>>(&json).ok()
}

/// COMMENT-3: `{ x, y, w, h }`, px in the frame's document.
fn read_rect(v: JsValue) -> annotate::Rect {
    let n = |k: &str| Reflect::get(&v, &JsValue::from_str(k)).ok().and_then(|x| x.as_f64()).filter(|x| x.is_finite()).unwrap_or(0.0);
    annotate::Rect { x: n("x"), y: n("y"), w: n("w"), h: n("h") }
}

/// COMMENT-3: `menger:annotate { on }` for the frame on `channel` —
/// APP-45: with `outline`, the app's `--annot` as painted now, which a
/// whole document (no tokens of its own) outlines its elements in.
fn annotate_message(channel: &str, on: bool) -> JsValue {
    let msg = message("menger:annotate", channel);
    set(&msg, "on", &JsValue::from_bool(on));
    let outline = dom::window()
        .document()
        .and_then(|d| d.document_element())
        .and_then(|r| dom::window().get_computed_style(&r).ok().flatten())
        .and_then(|cs| cs.get_property_value("--annot").ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(o) = outline {
        set(&msg, "outline", &JsValue::from_str(&o));
    }
    msg.into()
}

/// COMMENT-3: annotation mode turned — every frame is told.
pub(crate) fn broadcast_annotate(on: bool) {
    for f in frames() {
        if let Some(channel) = f.get_attribute("data-channel") {
            post(&f, &annotate_message(&channel, on));
        }
    }
}

fn strings(list: &[String]) -> js_sys::Array {
    list.iter().map(|s| JsValue::from_str(s)).collect()
}

/// COMMENT-3: `menger:probe { id, selectors }` — which select the element
/// the frame's pick `id` named, and nothing else.
pub(crate) fn post_probe(frame: &web_sys::HtmlIFrameElement, channel: &str, id: u32, selectors: &[String]) {
    let msg = message("menger:probe", channel);
    set(&msg, "id", &JsValue::from_f64(f64::from(id)));
    set(&msg, "selectors", &strings(selectors));
    post(frame, &msg.into());
}

/// COMMENT-3: `menger:locate { id, paths }` — where these elements are.
pub(crate) fn post_locate(frame: &web_sys::HtmlIFrameElement, channel: &str, id: u32, paths: &[String]) {
    let msg = message("menger:locate", channel);
    set(&msg, "id", &JsValue::from_f64(f64::from(id)));
    set(&msg, "paths", &strings(paths));
    post(frame, &msg.into());
}

/// APP-45: `menger:locate { id, paths, reveal }` — a link back: a frame
/// held to its device's screen scrolls its page to the first element before
/// it says where it is (a body frame, sized to its page, does not).
pub(crate) fn post_locate_revealing(frame: &web_sys::HtmlIFrameElement, channel: &str, id: u32, paths: &[String]) {
    let msg = message("menger:locate", channel);
    set(&msg, "id", &JsValue::from_f64(f64::from(id)));
    set(&msg, "paths", &strings(paths));
    set(&msg, "reveal", &JsValue::from_bool(frame.has_attribute("data-fixed")));
    post(frame, &msg.into());
}

/// APP-45: `menger:walk { step }` — the keyboard path in a device's page:
/// `start`, `next`, `prev`, `in`, `out`, or `pick`.
pub(crate) fn post_walk(frame: &web_sys::HtmlIFrameElement, step: &str) {
    let Some(channel) = frame.get_attribute("data-channel") else { return };
    let msg = message("menger:walk", &channel);
    set(&msg, "step", &JsValue::from_str(step));
    post(frame, &msg.into());
}

/// `menger:reveal { component }` (COMP-14: or `{ field }`, `{ anchor }`)
/// for the frame on `channel`.
fn reveal_message(channel: &str, anchor: &inspect::Anchor) -> JsValue {
    let msg = message("menger:reveal", channel);
    let (key, value) = anchor.pair();
    set(&msg, key, &value.into());
    msg.into()
}

/// LOOK-3: `menger:flash { n }` — the frame flashes the n-th item of the
/// sections it last reported.
pub(crate) fn post_flash(frame: &web_sys::HtmlIFrameElement, n: usize) {
    let Some(channel) = frame.get_attribute("data-channel") else { return };
    let msg = message("menger:flash", &channel);
    set(&msg, "n", &JsValue::from_f64(n as f64));
    post(frame, &msg.into());
}

/// LOOK-3: the ticket page's body frame for `ticket` — its own, shown
/// (never a cited window, a whole document, or one still loading).
pub(crate) fn body_frame(ticket: &str) -> Option<web_sys::HtmlIFrameElement> {
    let doc = dom::window().document()?;
    let sel = format!(
        "#ticket-page iframe.{FRAME_CLASS}[data-state=\"live\"]:not([data-cite-key]):not([data-page])"
    );
    let list = doc.query_selector_all(&sel).ok()?;
    (0..list.length())
        .filter_map(|i| list.get(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlIFrameElement>().ok())
        .find(|f| f.get_attribute("data-ticket").is_some_and(|t| t.eq_ignore_ascii_case(ticket)))
}

/// COMP-14: `menger:cited { cite, height }` to a citing frame — the one
/// thing it learns of its window ([`cite::cited_reply`]).
pub(crate) fn post_cited(citer: &web_sys::HtmlIFrameElement, channel: &str, reply: &cite::CitedReply) {
    let msg = message("menger:cited", channel);
    set(&msg, "cite", &JsValue::from_f64(f64::from(reply.cite)));
    set(&msg, "height", &JsValue::from_f64(f64::from(reply.height)));
    post(citer, &msg.into());
}

/// APP-29: `menger:mocked { mock, height }` to a body's frame — the one
/// thing it learns of its mockup ([`mockup::mocked_reply`]).
pub(crate) fn post_mocked(citer: &web_sys::HtmlIFrameElement, channel: &str, reply: &mockup::MockedReply) {
    let msg = message("menger:mocked", channel);
    set(&msg, "mock", &JsValue::from_f64(f64::from(reply.mock)));
    set(&msg, "height", &JsValue::from_f64(f64::from(reply.height)));
    post(citer, &msg.into());
}

/// COMP-9: `menger.navigate`'s route, if the allow-list takes it: the app
/// goes there — or, already there, the card it names is revealed again.
fn navigate(route: &str) {
    let Some(hash) = inspect::app_route(route) else { return };
    let w = dom::window();
    if w.location().hash().unwrap_or_default() == hash {
        anchor_changed(&hash);
    } else {
        let _ = w.location().set_hash(&hash);
    }
}

/// The route's card anchor moved: remember it, and ask every frame on
/// screen (a frame loading now asks once settled).
fn anchor_changed(hash: &str) {
    let anchor = inspect::anchor(hash);
    ANCHOR.with(|a| *a.borrow_mut() = anchor.clone());
    if let Some(anchor) = anchor {
        for f in frames() {
            // COMP-14: never a cited window — the ticket's own page reveals it.
            if f.has_attribute("data-cite-key") {
                continue;
            }
            if let Some(channel) = f.get_attribute("data-channel") {
                post(&f, &reveal_message(&channel, &anchor));
            }
        }
    }
}

/// Scroll the app so the point `top` px down `frame`'s document sits at the
/// top of the view — a marker in the frame's container, scrolled into view.
pub(crate) fn scroll_into_frame(frame: &web_sys::HtmlIFrameElement, top: f64) {
    let Some(parent) = frame.parent_element() else { return };
    let Some(doc) = dom::window().document() else { return };
    let Ok(marker) = doc.create_element("div") else { return };
    let offset = frame.offset_top() as f64 + top.max(0.0);
    let _ = marker.set_attribute(
        "style",
        &format!("position:absolute;left:0;top:{offset}px;width:1px;height:1px;pointer-events:none;scroll-margin-top:1rem"),
    );
    let _ = parent.append_child(&marker);
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(web_sys::ScrollLogicalPosition::Start);
    marker.scroll_into_view_with_scroll_into_view_options(&opts);
    marker.remove();
}

/// The one `message` listener every frame's SDK talks to; installed by the
/// first [`Frame`], for the app's lifetime. COMP-9: with it, a `hashchange`
/// listener that keeps the card anchor ([`anchor_changed`]).
fn install_listener() {
    if LISTENING.with(|l| l.replace(true)) {
        return;
    }
    let w = dom::window();
    let cb = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(on_message);
    let _ = w.add_event_listener_with_callback("message", cb.as_ref().unchecked_ref());
    cb.forget();
    // IDENTITY-36: what a write's gesture is read from.
    gesture::install(FRAME_CLASS);
    // DESIGN-40: the Motion setting, followed into every frame.
    watch_motion();
    let hash = Closure::<dyn FnMut()>::new(|| anchor_changed(&dom::window().location().hash().unwrap_or_default()));
    let _ = w.add_event_listener_with_callback("hashchange", hash.as_ref().unchecked_ref());
    hash.forget();
    anchor_changed(&w.location().hash().unwrap_or_default());
}

/// MENGER-15: how a frame is shown — what it emulates (a mockup's toggles;
/// `None`: a whole document follows the app's painted mode, any other body
/// emulates nothing), and whether its height is its container's (a
/// device's screen) rather than its document's.
#[derive(Clone, Copy, Debug, Default)]
struct Look {
    emulate: Option<Emulate>,
    fixed: bool,
}

/// Load `body_html` into a new frame in `container`, beside the one on
/// screen (if any), invisible until [`promote`]d — INPUT-4: stamped with
/// `ticket`, the ticket whose stored body it is (none: no input to answer);
/// IDENTITY-36: and with `scope`, the shortcode its reads are held to
/// (none: it reads nothing but components). MENGER-15: shown as `look`
/// says.
fn load(container: &web_sys::Element, body_html: &str, ticket: Option<&str>, scope: Option<&str>, look: Look) {
    load_frame(container, body_html, Stamp { data_ticket: ticket, init_ticket: ticket, scope, cite: None, look });
}

/// What a new frame is stamped with — [`load`]'s, or (COMP-14) a cited
/// window's ([`load_cited`]).
struct Stamp<'a> {
    /// `<iframe data-ticket>`: the ticket a write from it answers.
    data_ticket: Option<&'a str>,
    /// `menger.input.ticket`.
    init_ticket: Option<&'a str>,
    /// `<iframe data-scope>`.
    scope: Option<&'a str>,
    /// COMP-14: the section shown, the window's key, its depth.
    cite: Option<(&'a cite::Section, &'a str, u32)>,
    /// MENGER-15: how it is shown.
    look: Look,
}

/// COMP-14: load a cited window's frame — `body_html`, the cited ticket's
/// body, into the window's view `container`, stamped `stamps` (the cited
/// ticket's own: [`cite::stamps`]) and told to show `section`; `key` names
/// the window its sizes go to.
pub(crate) fn load_cited(container: &web_sys::Element, body_html: &str, stamps: &cite::Stamps, section: &cite::Section, key: &str) {
    install_listener();
    registry::want();
    load_frame(
        container,
        body_html,
        Stamp {
            data_ticket: stamps.data_ticket.as_deref(),
            init_ticket: stamps.init_ticket.as_deref(),
            scope: stamps.scope.as_deref(),
            cite: Some((section, key, stamps.depth)),
            look: Look::default(),
        },
    );
}

fn load_frame(container: &web_sys::Element, body_html: &str, stamp: Stamp) {
    let Stamp { data_ticket, init_ticket, scope, cite, look } = stamp;
    let w = dom::window();
    let Some(doc) = w.document() else { return };
    let channel = next_channel();
    // THEME-8: the frame's first paint is its region's theme.
    let (scheme, tokens) = read_theme_at(Some(container));
    let worn = worn_at(container);
    let mermaid_url = format!("{}{}", w.location().origin().unwrap_or_default(), mermaid::ASSET_URL);
    let highlight_url = format!("{}{}", w.location().origin().unwrap_or_default(), highlight::ASSET_URL);
    // DESIGN-40: the house faces, the same way; the Motion setting as the
    // app wears it; and whether this container already shows the body (a
    // re-render: its entrance moves are not played again).
    let fonts_url = format!("{}{}", w.location().origin().unwrap_or_default(), FONTS_URL);
    let motion = host_motion();
    let typing = host_typing();
    let entered = container.query_selector(":scope > iframe:not([data-state=\"pending\"])").ok().flatten().is_some();
    let components = registry::snapshot();
    // MENGER-15: a whole document follows the app's mode unless a mockup
    // says otherwise.
    let page = md::is_page(body_html);
    let follow = page && look.emulate.is_none();
    let emulate = if follow { Some(Emulate { scheme: Some(scheme), reduce: None }) } else { look.emulate };
    let html = srcdoc(
        body_html,
        &Shell {
            channel: &channel,
            scheme,
            theme: &worn,
            tokens: &tokens,
            mermaid_url: &mermaid_url,
            highlight_url: &highlight_url,
            fonts_url: &fonts_url,
            motion,
            typing,
            entered,
            components: components.as_deref(),
            ticket: init_ticket,
            cite: cite.map(|(section, _, _)| section),
            emulate,
        },
    );
    // A re-render still loading is superseded by this one.
    if let Ok(list) = container.query_selector_all(":scope > iframe[data-state=\"pending\"]") {
        for i in 0..list.length() {
            if let Some(el) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
                el.remove();
            }
        }
    }
    let Ok(el) = doc.create_element("iframe") else { return };
    // Sandboxed before it has a document: the first navigation is too.
    let _ = el.set_attribute("sandbox", SANDBOX);
    let _ = el.set_attribute("class", FRAME_CLASS);
    let _ = el.set_attribute("data-channel", &channel);
    let _ = el.set_attribute("data-state", "pending");
    let _ = el.set_attribute("data-sdk", SDK_VERSION);
    if let Some(ticket) = data_ticket {
        let _ = el.set_attribute("data-ticket", ticket);
    }
    if let Some((_, key, depth)) = cite {
        let _ = el.set_attribute("data-cite-key", key);
        let _ = el.set_attribute("data-cite-depth", &depth.to_string());
    }
    if let Some(scope) = scope {
        let _ = el.set_attribute("data-scope", scope);
        // What it hears before it asks anything: its reach, once known.
        if !bridge::reach_known(scope) {
            refresh_reach(scope.to_string());
        }
    }
    // MENGER-15: a whole document (no SDK: shown once its shim is ready,
    // never sized to its document), one following the app's mode, one held
    // to its device's screen.
    if page {
        let _ = el.set_attribute("data-page", "");
    }
    if follow {
        let _ = el.set_attribute("data-follow", "");
    }
    if look.fixed || page {
        let _ = el.set_attribute("data-fixed", "");
    }
    let _ = el.set_attribute("title", if cite.is_some() { "cited section" } else { "ticket body" });
    let Ok(frame) = el.dyn_into::<web_sys::HtmlIFrameElement>() else { return };
    if let Some(s) = emulate.and_then(|e| e.scheme) {
        let _ = web_sys::HtmlElement::style(&frame).set_property("color-scheme", s.as_str());
    }
    // ACCESS-28: the document waits on the element for the frame's boot.
    let _ = Reflect::set(&frame, &JsValue::from_str(DOC_PROP), &JsValue::from_str(&html));
    frame.set_src(&frame_src(&channel));
    let _ = container.append_child(&frame);
    spawn_local(async move {
        dom::sleep_ms(PROMOTE_AFTER_MS).await;
        if frame.is_connected() && frame.get_attribute("data-state").as_deref() == Some("pending") {
            promote(&frame);
        }
    });
}

/// COMP-4: a framed body — `html` (a framed body's markup) in a sandboxed
/// frame with the SDK shell, re-rendered without a flash whenever it
/// changes. The frontmatter is the caller's: it stays outside. INPUT-4:
/// `ticket` is the ticket whose stored body `html` is — the one a click in
/// the frame may answer an input of; `None` where the markup is no
/// ticket's stored body (an editor's draft, a help card). IDENTITY-36: its
/// reads are held to `ticket`'s shortcode — or, with no ticket, to `scope`
/// (an editor's draft: the shortcode of the ticket it edits; a help card:
/// its component's owner's), and with neither it reads only components.
/// MENGER-15: `html` may be a whole document (md::is_page), which renders
/// as itself ([`page_srcdoc`]); a mockup's device passes `emulate` (its
/// toggles, applied live) and `fixed` (the screen's height, not the
/// document's).
#[component]
pub(crate) fn Frame(
    #[prop(into)] html: Signal<String>,
    ticket: Option<String>,
    #[prop(optional)] scope: Option<String>,
    #[prop(optional)] emulate: Option<Signal<Emulate>>,
    #[prop(optional)] fixed: bool,
) -> impl IntoView {
    let scope = bridge::scope_of(ticket.as_deref()).or_else(|| bridge::scope_of(scope.as_deref()));
    install_listener();
    // COMP-5: a frame defines the components — the app keeps the registry.
    registry::want();
    let host = NodeRef::<leptos::html::Div>::new();
    let look = move || Look { emulate: emulate.map(|e| e.get_untracked()), fixed };
    // The newest body asked for: a debounced re-render loads only if it is
    // still the newest when its wait ends (and the frame still mounted).
    let asked = StoredValue::new(0u32);
    Effect::new(move |_| {
        let body = html.get();
        let Some(el) = host.get() else { return };
        let n = asked.with_value(|n| n.wrapping_add(1));
        asked.set_value(n);
        if el.query_selector(":scope > iframe").ok().flatten().is_none() {
            load(&el, &body, ticket.as_deref(), scope.as_deref(), look());
            return;
        }
        let ticket = ticket.clone();
        let scope = scope.clone();
        spawn_local(async move {
            dom::sleep_ms(RERENDER_AFTER_MS).await;
            if asked.try_with_value(|v| *v == n) == Some(true) {
                load(&el, &body, ticket.as_deref(), scope.as_deref(), look());
            }
        });
    });
    // MENGER-15: a mockup's toggles reach its frames live, no reload.
    if let Some(e) = emulate {
        Effect::new(move |prev: Option<Emulate>| {
            let now = e.get();
            if prev.is_some_and(|p| p != now) {
                if let Some(el) = host.get_untracked() {
                    emulate_in(&el, now);
                }
            }
            now
        });
    }
    view! { <div class="mframe" class:mframe-fixed=fixed node_ref=host></div> }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn shell_tokens() -> Vec<(String, String)> {
        vec![
            ("--bg".into(), "#fbfbf9".into()),
            ("--ink".into(), " #1f2328".into()),
            ("--sans".into(), "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif".into()),
        ]
    }

    /// COMP-1 decision 1: lavish-axi's sandbox, minus downloads — and never
    /// the same origin. IDENTITY-28: minus forms too; popups (a body's
    /// links) still open the real page.
    #[test]
    fn the_sandbox_runs_scripts_but_never_shares_the_apps_origin() {
        let flags: Vec<&str> = SANDBOX.split_whitespace().collect();
        assert_eq!(flags, ["allow-scripts", "allow-popups", "allow-popups-to-escape-sandbox"]);
        assert!(!SANDBOX.contains("same-origin") && !SANDBOX.contains("top-navigation"));
        assert!(!SANDBOX.contains("allow-forms"), "IDENTITY-28: no forms");
    }

    /// IDENTITY-28: the CSP is the head's first element — before the theme,
    /// the init block, the SDK and the body — and says exactly: no
    /// connections of the frame's own, no form submission; scripts, styles,
    /// images and fonts left alone (bodies import from esm.sh).
    #[test]
    fn the_csp_comes_first_and_closes_connections_and_forms() {
        assert_eq!(CSP, "connect-src 'none'; form-action 'none'");
        let tokens = shell_tokens();
        let shell = Shell { channel: "f9", scheme: Scheme::Light, theme: "default", tokens: &tokens, mermaid_url: "u", highlight_url: "h", fonts_url: "", motion: Motion::Full, typing: Typing::Normal, entered: false, components: None, ticket: Some("A-1"), cite: None, emulate: None };
        let doc = srcdoc("<script>fetch('https://x')</script><form action=\"https://x\"></form>", &shell);
        let head = "<!doctype html><html lang=\"en\" data-theme=\"light\"><head>";
        assert!(
            doc.starts_with(&format!("{head}<meta http-equiv=\"Content-Security-Policy\" content=\"connect-src 'none'; form-action 'none'\"><meta charset=\"utf-8\">")),
            "{}",
            &doc[..200]
        );
        assert_eq!(doc.matches("Content-Security-Policy").count(), 1);
        for directive in ["script-src", "default-src", "img-src", "style-src", "font-src", "frame-src"] {
            assert!(!CSP.contains(directive), "{directive} is left alone");
        }
        let at = |needle: &str| doc.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        let csp = at("Content-Security-Policy");
        for later in ["<base target", "<style id=\"menger-theme\">", "id=\"menger-init\"", "id=\"menger-sdk\"", "<body>"] {
            assert!(csp < at(later), "the CSP precedes {later}");
        }
    }

    /// The srcdoc: the theme first (no flash of the wrong one), links to a
    /// new tab, the init block, the SDK inline before the body, then the
    /// body's markup as given.
    /// ACCESS-28: a frame starts as the shell's frame document, told its
    /// channel; the document says `menger:boot` on it and writes only a
    /// `menger:doc` from its own parent, on its channel.
    #[test]
    fn a_frame_starts_as_the_frame_document_and_takes_its_own() {
        assert_eq!(frame_src("f7"), "/app/frame#f7");
        let boot = include_str!("../../web/frame.html");
        for part in [
            "location.hash.slice(1)",
            "parent.postMessage({ type: \"menger:boot\", channel: channel }, \"*\")",
            "e.source !== parent",
            "d.type !== \"menger:doc\"",
            "d.channel !== channel",
            "document.write(d.html)",
        ] {
            assert!(boot.contains(part), "{part}");
        }
    }

    #[test]
    fn the_srcdoc_is_the_shell_then_the_body() {
        let tokens = shell_tokens();
        let shell = Shell { channel: "f7", scheme: Scheme::Dark, theme: "default", tokens: &tokens, mermaid_url: "http://h:1/app/vendor/m.js", highlight_url: "http://h:1/app/vendor/hl.js", fonts_url: "http://h:1/app/fonts/", motion: Motion::Calm, typing: Typing::Normal, entered: true, components: None, ticket: None, cite: None, emulate: None };
        let doc = srcdoc("<details><summary>s</summary><p>x</p></details>", &shell);
        assert!(doc.starts_with("<!doctype html><html lang=\"en\" data-theme=\"dark\"><head><meta http-equiv=\"Content-Security-Policy\""), "{doc}");
        let at = |needle: &str| doc.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("<base target=\"_blank\">") < at("<style id=\"menger-theme\">"));
        assert!(doc.contains("<style id=\"menger-theme\">:root{color-scheme:dark;--bg:#fbfbf9;--ink:#1f2328;--sans:-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif}</style>"), "{doc}");
        assert!(at("id=\"menger-init\"") < at(&format!("var VERSION = \"{SDK_VERSION}\"")));
        assert!(at("var VERSION") < at("<body>"));
        assert!(doc.ends_with("<body><details><summary>s</summary><p>x</p></details></body></html>"), "{doc}");
        // The SDK is the one script before the body, whole, and labelled.
        assert!(doc.contains(&format!("<script id=\"menger-sdk\" data-version=\"{SDK_VERSION}\" data-src=\"{SDK_URL}\">{SDK_JS}</script></head>")));
    }

    /// DESIGN-40: a framed body declares the house faces from the app's
    /// origin (its own is opaque), before the tokens that name them; and is
    /// told the Motion setting and whether it was shown before.
    #[test]
    fn a_frame_declares_the_house_faces_and_wears_the_motion_setting() {
        let tokens = shell_tokens();
        let shell = Shell { channel: "f7", scheme: Scheme::Dark, theme: "default", tokens: &tokens, mermaid_url: "u", highlight_url: "h", fonts_url: "http://h:1/app/fonts/", motion: Motion::Calm, typing: Typing::Normal, entered: true, components: None, ticket: None, cite: None, emulate: None };
        let doc = srcdoc("<p>x</p>", &shell);
        let at = |needle: &str| doc.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("<base target=\"_blank\">") < at("<style id=\"menger-fonts\">"));
        assert!(at("<style id=\"menger-fonts\">") < at("<style id=\"menger-theme\">"));
        assert!(doc.contains("font-family:\"Geist\";") && doc.contains("src:url(http://h:1/app/fonts/geist-latin.woff2) format(\"woff2\")"), "{doc}");
        assert!(doc.contains("font-family:\"Geist Mono\";") && doc.contains("src:url(http://h:1/app/fonts/geist-mono-latin.woff2)"), "{doc}");
        let v: serde_json::Value = serde_json::from_str(&init_json(&shell)).unwrap();
        assert_eq!(v["motion"], "calm");
        assert_eq!(v["entered"], true);
        // No base, or one that could end the rule: nothing declared.
        for bad in ["", "http://h/a)b/", "http://h/</style>/", "http://h/a b/", "x;}"] {
            assert_eq!(fonts_css(bad), "", "{bad}");
        }
        let none = Shell { fonts_url: "", ..shell };
        assert!(!srcdoc("<p>x</p>", &none).contains("menger-fonts"));
        // The setting as the app's <html> says it; absent or unknown is full.
        assert_eq!(Motion::parse(None), Motion::Full);
        assert_eq!(Motion::parse(Some("calm")), Motion::Calm);
        assert_eq!(Motion::parse(Some(" none ")), Motion::None);
        assert_eq!(Motion::parse(Some("fast")), Motion::Full);
        assert_eq!(Motion::None.as_str(), "none");
    }

    /// DESIGN-43: a frame is told the Typed text setting as the app stamps
    /// it — `normal` when nothing is stamped.
    #[test]
    fn a_frame_is_told_the_typed_text_setting() {
        let tokens = shell_tokens();
        let shell = |typing| Shell { channel: "f8", scheme: Scheme::Light, theme: "default", tokens: &tokens, mermaid_url: "u", highlight_url: "h", fonts_url: "", motion: Motion::Calm, typing, entered: false, components: None, ticket: None, cite: None, emulate: None };
        let v: serde_json::Value = serde_json::from_str(&init_json(&shell(Typing::Fast))).unwrap();
        assert_eq!((v["motion"].as_str(), v["typing"].as_str()), (Some("calm"), Some("fast")));
        for t in Typing::ALL {
            let v: serde_json::Value = serde_json::from_str(&init_json(&shell(t))).unwrap();
            assert_eq!(Typing::parse(v["typing"].as_str()), t);
        }
        assert_eq!(Typing::parse(None), Typing::Normal);
    }

    /// The init block parses back to what the SDK reads, and nothing in it
    /// can close its script element.
    #[test]
    fn the_init_block_is_json_that_cannot_close_its_script() {
        let tokens = shell_tokens();
        let shell = Shell { channel: "f1", scheme: Scheme::Light, theme: "default", tokens: &tokens, mermaid_url: "http://h/</script><script>alert(1)//", highlight_url: "http://h/hl.js", fonts_url: "", motion: Motion::Full, typing: Typing::Normal, entered: false, components: None, ticket: None, cite: None, emulate: None };
        let json = init_json(&shell);
        assert!(!json.contains('<'), "{json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["channel"], "f1");
        assert_eq!(v["version"], SDK_VERSION);
        assert_eq!(v["scheme"], "light");
        assert_eq!(v["theme"], "default", "THEME-7: the theme its region wears");
        assert_eq!(init_json(&Shell { theme: "pantry", ..shell })[..].contains(r#""theme":"pantry""#), true);
        assert_eq!(v["mermaid"]["url"], "http://h/</script><script>alert(1)//");
        // The app's strict mermaid posture rides along unchanged.
        assert_eq!(v["mermaid"]["config"]["securityLevel"], "strict");
        assert_eq!(v["mermaid"]["config"]["htmlLabels"], false);
        // MENGER-22: the highlighter's URL and its rules, the app's own.
        assert_eq!(v["highlight"]["url"], "http://h/hl.js");
        assert_eq!(v["highlight"]["unlabeled"], highlight::UNLABELED.as_str());
        assert_eq!(v["highlight"]["plain"], serde_json::json!(["text", "plain", "plaintext"]));
        // COMP-5: no registry held yet is `null` — the SDK waits for one.
        assert!(v["components"].is_null());
        // INPUT-4: no ticket's body — `menger.input.ticket` is null.
        assert!(v["ticket"].is_null());
        let shell = Shell { ticket: Some("INPUT-9"), ..shell };
        let v: serde_json::Value = serde_json::from_str(&init_json(&shell)).unwrap();
        assert_eq!(v["ticket"], "INPUT-9");
    }

    /// COMP-14: a cited window's frame is a frame like any other — the
    /// same CSP first, the same sandbox (`load_frame` stamps SANDBOX on
    /// every frame), the SDK inline — and its init block names the cited
    /// ticket and the one section it shows; every other frame's `cite` is
    /// null.
    #[test]
    fn a_cited_windows_frame_is_a_frame_told_its_section() {
        let tokens = shell_tokens();
        let field = cite::Section::Field("findings".into());
        let shell = Shell {
            channel: "f3",
            scheme: Scheme::Light,
            theme: "default",
            tokens: &tokens,
            mermaid_url: "u", highlight_url: "h",
            fonts_url: "", motion: Motion::Full, typing: Typing::Normal, entered: false,
            components: None,
            ticket: Some("AUTH-10"),
            cite: Some(&field),
            emulate: None,
        };
        let doc = srcdoc("<m-field data-field=\"findings\">x</m-field>", &shell);
        assert!(doc.starts_with(&format!(
            "<!doctype html><html lang=\"en\" data-theme=\"light\"><head><meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"
        )));
        assert_eq!(doc.matches("Content-Security-Policy").count(), 1);
        let v: serde_json::Value = serde_json::from_str(&init_json(&shell)).unwrap();
        assert_eq!(v["cite"], serde_json::json!({ "field": "findings" }));
        assert_eq!(v["ticket"], "AUTH-10");
        let id = cite::Section::Id("t".into());
        let v: serde_json::Value = serde_json::from_str(&init_json(&Shell { cite: Some(&id), ..shell })).unwrap();
        assert_eq!(v["cite"], serde_json::json!({ "anchor": "t" }));
        let v: serde_json::Value = serde_json::from_str(&init_json(&Shell { cite: None, ..shell })).unwrap();
        assert!(v["cite"].is_null());
    }

    /// COMP-14: in the SDK, `<m-cite>` is built in and defined before the
    /// registry's names (a registry entry named m-cite only documents it);
    /// the placeholder posts what it cites and where, and of the host's
    /// answer takes the height alone; a cited window's frame rejects both
    /// writes before anything is sent.
    #[test]
    fn the_sdk_cites_by_placeholder_and_takes_only_a_height() {
        let define = SDK_JS.find("CE.define(\n        \"m-cite\"").expect("m-cite defined");
        let sync = SDK_JS.find("if (Array.isArray(init.components)) sync(init.components);").unwrap();
        assert!(define < sync, "m-cite is defined before the registry's names");
        let at = SDK_JS.find("function citedHeight(m) {").unwrap();
        let body = &SDK_JS[at..at + SDK_JS[at..].find("\n  }\n").unwrap()];
        assert!(body.contains("m.height") && !body.contains("innerHTML") && !body.contains("m.html"), "{body}");
        for sdk in ["menger.input.submit", "menger.input.clear"] {
            assert!(SDK_JS.contains(&format!("if (citeTarget) return Promise.reject(new Error(DISPLAY_ONLY(\"{sdk}\")));")), "{sdk}");
        }
        // The SDK's words and the host's agree.
        assert!(bridge::display_only("X").ends_with(
            "display only — this is a cited window; its inputs are answered in their own ticket (nothing sent)"
        ));
        assert!(SDK_JS.contains(
            "\": display only — this is a cited window; its inputs are answered in their own ticket (nothing sent)\""
        ));
    }

    /// APP-29: `<m-mockup>` is built in as `<m-cite>` is — defined before
    /// the registry's names, under the tag md.rs's fence writes; it posts
    /// its three attributes as written and where it is, and of the host's
    /// answer takes the height alone (nothing of a referenced ticket).
    #[test]
    fn the_sdk_mocks_by_placeholder_and_takes_only_a_height() {
        let define = SDK_JS.find(&format!("CE.define(\n        \"{}\"", md::MOCKUP_TAG)).expect("m-mockup defined");
        let sync = SDK_JS.find("if (Array.isArray(init.components)) sync(init.components);").unwrap();
        assert!(define < sync, "m-mockup is defined before the registry's names");
        assert!(SDK_JS.contains("return [\"ref\", \"widths\", \"source\", \"title\", \"description\"];"), "the attributes it watches");
        let at = SDK_JS.find("function mockReport(el, force) {").unwrap();
        let body = &SDK_JS[at..at + SDK_JS[at..].find("\n  }\n").unwrap()];
        for attr in [
            "el.getAttribute(\"ref\")",
            "el.getAttribute(\"widths\")",
            "el.getAttribute(\"source\")",
            "el.getAttribute(\"title\")",
            "el.getAttribute(\"description\")",
            "post(\"menger:mockup\"",
        ] {
            assert!(body.contains(attr), "{attr}: {body}");
        }
        let at = SDK_JS.find("function mockedHeight(m) {").unwrap();
        let body = &SDK_JS[at..at + SDK_JS[at..].find("\n  }\n").unwrap()];
        assert!(body.contains("m.height") && !body.contains("innerHTML") && !body.contains("m.html"), "{body}");
        // The host's reply is the height alone.
        let reply = serde_json::to_value(mockup::mocked_reply(2, 700.4)).unwrap();
        assert_eq!(reply, serde_json::json!({ "mock": 2, "height": 701 }));
    }

    /// COMP-5: the registry rides in the init block whole, in the wire's
    /// spelling — and a source is HTML with scripts in it, which must not
    /// end the init block's `<script>` either.
    #[test]
    fn the_init_block_carries_the_registry_and_cannot_be_closed_by_a_source() {
        let tokens = shell_tokens();
        let source = "<template><b>x</b></template>\n<script>this.root.append('</script><script>alert(1)</script>')</script>\n<!-- c -->";
        let components = [
            Component { name: "m-a".into(), source: source.into(), owner: Some("A-2".into()), references: vec!["A-3".into()], docs: None, preview: vec![] },
            Component { name: "m-b".into(), source: "<i>b</i>".into(), owner: None, references: vec!["m-a".into()], docs: None, preview: vec![] },
        ];
        let shell = Shell { channel: "f2", scheme: Scheme::Light, theme: "default", tokens: &tokens, mermaid_url: "u", highlight_url: "h", fonts_url: "", motion: Motion::Full, typing: Typing::Normal, entered: false, components: Some(&components), ticket: None, cite: None, emulate: None };
        let json = init_json(&shell);
        assert!(!json.contains('<'), "{json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["components"][0]["name"], "m-a");
        assert_eq!(v["components"][0]["source"], source);
        assert_eq!(v["components"][0]["owner"], "A-2");
        assert_eq!(v["components"][1]["owner"], serde_json::Value::Null);
        assert_eq!(v["components"][1]["references"][0], "m-a");
        let doc = srcdoc("<m-a></m-a>", &shell);
        assert_eq!(doc.matches("</script>").count(), 2, "the init block's and the SDK's own");
    }

    /// A token the frame cannot take whole is dropped, never spliced.
    #[test]
    fn the_theme_block_takes_only_well_formed_tokens() {
        let tokens: Vec<(String, String)> = [
            ("--bg", "#101014"),
            ("--x", "red}body{display:none"),
            ("--y", "</style><script>1</script>"),
            ("--z", "a;b"),
            ("color", "red"),
            ("--Upper", "red"),
            ("--empty", "  "),
            ("--ok-2", "rgba(0, 0, 0, .55)"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(theme_css(Scheme::Light, &tokens), ":root{color-scheme:light;--bg:#101014;--ok-2:rgba(0, 0, 0, .55)}");
    }

    #[test]
    fn the_scheme_is_read_off_the_painted_color_scheme() {
        assert_eq!(Scheme::from_color_scheme("dark"), Scheme::Dark);
        assert_eq!(Scheme::from_color_scheme(" light "), Scheme::Light);
        assert_eq!(Scheme::from_color_scheme("light dark"), Scheme::Dark);
        assert_eq!(Scheme::from_color_scheme("normal"), Scheme::Light);
        assert_eq!(Scheme::from_color_scheme(""), Scheme::Light);
    }

    /// Only the protocol's three inbound kinds count; a size is a finite,
    /// clamped height.
    #[test]
    fn the_host_reads_ready_and_size_and_nothing_else() {
        assert_eq!(parse_msg("menger:boot", None, None), Some(Msg::Boot));
        assert_eq!(parse_msg("menger:ready", None, None), Some(Msg::Ready));
        assert_eq!(parse_msg("menger:size", Some(312.2), Some(true)), Some(Msg::Size { height: 313, settled: true }));
        assert_eq!(parse_msg("menger:size", Some(40.0), None), Some(Msg::Size { height: 40, settled: false }));
        assert_eq!(parse_msg("menger:size", Some(-5.0), Some(false)), Some(Msg::Size { height: 0, settled: false }));
        assert_eq!(parse_msg("menger:size", Some(1e12), Some(true)), Some(Msg::Size { height: 50_000, settled: true }));
        assert_eq!(parse_msg("menger:size", Some(f64::NAN), Some(true)), None);
        assert_eq!(parse_msg("menger:size", None, Some(true)), None);
        assert_eq!(parse_msg("menger:extent", Some(5000.4), None), Some(Msg::Extent { height: 5001 }));
        assert_eq!(parse_msg("menger:scrolled", None, None), Some(Msg::Scrolled));
        assert_eq!(parse_msg("menger:extent", Some(f64::NAN), None), None);
        assert_eq!(parse_msg("menger:extent", None, None), None);
        assert_eq!(parse_msg("menger:call", None, None), Some(Msg::Call));
        assert_eq!(parse_msg("menger:navigate", None, None), Some(Msg::Navigate));
        assert_eq!(parse_msg("menger:inspect", None, None), Some(Msg::Inspect));
        assert_eq!(parse_msg("menger:revealed", None, None), Some(Msg::Revealed));
        assert_eq!(parse_msg("menger:sections", None, None), Some(Msg::Sections));
        assert_eq!(parse_msg("menger:submit", None, None), Some(Msg::Submit));
        assert_eq!(parse_msg("menger:clear", None, None), Some(Msg::Clear));
        // COMMENT-3: annotation mode's inbound five.
        assert_eq!(parse_msg("menger:hover", None, None), Some(Msg::Hover));
        assert_eq!(parse_msg("menger:pick", None, None), Some(Msg::Pick));
        assert_eq!(parse_msg("menger:probed", None, None), Some(Msg::Probed));
        assert_eq!(parse_msg("menger:located", None, None), Some(Msg::Located));
        assert_eq!(parse_msg("menger:key", None, None), Some(Msg::Key));
        // COMP-14: a placeholder's two.
        assert_eq!(parse_msg("menger:cite", None, None), Some(Msg::Cite));
        assert_eq!(parse_msg("menger:uncite", None, None), Some(Msg::Uncite));
        // APP-29: a mockup placeholder's two.
        assert_eq!(parse_msg("menger:mockup", None, None), Some(Msg::Mockup));
        assert_eq!(parse_msg("menger:unmock", None, None), Some(Msg::Unmock));
        // MENGER-14: a link clicked.
        assert_eq!(parse_msg("menger:link", None, None), Some(Msg::Link));
        // MENGER-34: a chip's ticket menu.
        assert_eq!(parse_msg("menger:ticket-menu", None, None), Some(Msg::TicketMenu));
        // FILTER-25: a chip's Alt+click.
        assert_eq!(parse_msg("menger:filter-everywhere", None, None), Some(Msg::FilterEverywhere));
        for outbound in [
            "menger:theme", "menger:result", "menger:components", "menger:change", "menger:inspected", "menger:reveal",
            "menger:submitted", "menger:cleared", "menger:annotate", "menger:probe", "menger:locate", "menger:cited",
            "menger:mocked",
        ] {
            assert_eq!(parse_msg(outbound, None, None), None, "{outbound}: outbound only");
        }
        assert_eq!(parse_msg("lavish:queuePrompt", None, None), None);
    }

    /// The shell's three names agree — the version the host stamps, the
    /// script's own, and `window.menger.version` — and the app serves the
    /// script it inlines (index.html copy-files it to `SDK_URL`). The
    /// script is safe to inline: nothing in it ends a `<script>`.
    #[test]
    fn the_sdk_shell_is_versioned_served_and_inlinable() {
        assert!(SDK_JS.contains(&format!("var VERSION = \"{SDK_VERSION}\";")), "the script's VERSION");
        // THEME-7 (1.13): `on('theme')` and `menger.theme`, told by
        // `menger:theme`'s `theme` and the init block's.
        assert!(SDK_JS.contains(r#"kind !== "change" && kind !== "theme""#), "on('theme') is an event");
        assert!(SDK_JS.contains(r#"notify(null, "theme")"#) && SDK_JS.contains("if (key !== paintedKey)"), "told only when something moved");
        assert!(SDK_JS.contains(r#"Object.defineProperty(api, "theme""#), "menger.theme");
        assert!(SDK_JS.contains(r#"typeof init.theme === "string""#), "the first theme from the init block");
        let dts = include_str!("../sdk/menger.d.ts");
        assert!(dts.contains("on(kind: 'theme', listener: (theme: MengerTheme) => void): () => void;") && dts.contains("readonly theme: MengerTheme;"), "typed");
        assert!(
            SDK_JS.contains("var api = { version: VERSION, tickets: tickets, call: call, on: on, navigate: navigate, input: input };")
                && SDK_JS.contains("api = Object.freeze(api);"),
            "window.menger = {{ version, tickets, call, on, navigate, input, theme }}, frozen"
        );
        // INPUT-4: `menger.input` is frozen too, with its writes (INPUT-9: clear).
        assert!(
            SDK_JS.contains("var input = Object.freeze({ ticket: ticket, submit: submit, clear: clear, displayOnly: !!citeTarget });"),
            "menger.input"
        );
        let lower = SDK_JS.to_ascii_lowercase();
        assert!(!lower.contains("</script") && !lower.contains("<!--"), "inlinable");
        let name = SDK_URL.rsplit('/').next().unwrap();
        assert!(SDK_URL.starts_with("/app/sdk/"), "{SDK_URL}");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(dir.join("sdk").join(name).is_file());
        let index = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(
            index.contains(&format!("<link data-trunk rel=\"copy-file\" href=\"sdk/{name}\" data-target-path=\"sdk\" />")),
            "index.html copy-files the SDK into dist/sdk/"
        );
        // Every message the script sends or takes is one the protocol names.
        for kind in [
            "menger:ready", "menger:size", "menger:theme", "menger:call", "menger:result", "menger:components", "menger:change",
            "menger:navigate", "menger:inspect", "menger:inspected", "menger:reveal", "menger:revealed", "menger:submit",
            "menger:submitted", "menger:clear", "menger:cleared", "menger:annotate", "menger:hover", "menger:pick",
            "menger:probe", "menger:probed", "menger:locate", "menger:located", "menger:key", "menger:cite",
            "menger:uncite", "menger:cited", "menger:link", "menger:mockup", "menger:unmock", "menger:mocked",
        ] {
            assert!(SDK_JS.contains(&format!("\"{kind}\"")), "{kind}");
        }
        // Base typography paints through the app's tokens only.
        for token in ["var(--ink)", "var(--sans)", "var(--mono)", "var(--panel)", "var(--line)", "var(--accent)"] {
            assert!(SDK_JS.contains(token), "{token}");
        }
        assert!(!SDK_JS.contains("#fff") && !SDK_JS.contains("rgb("), "no color literal in the shell");
    }

    /// COMP-12: the base typography's selectors, as the script builds them —
    /// each `"…"` literal of `BASE_CSS` one rule, with `EVERY` and the
    /// card's `CARD_*` parts put in — split into single selectors (`:where`'s list opened,
    /// a comma inside `:is(…)` kept).
    fn base_selectors() -> Vec<String> {
        let at = SDK_JS.find("var BASE_CSS = [").expect("BASE_CSS");
        let end = at + SDK_JS[at..].find("].join(").expect("BASE_CSS's end");
        let mut region = SDK_JS[at..end].to_string();
        for line in SDK_JS[..at].lines().map(str::trim).filter(|l| l.starts_with("var CARD_") || l.starts_with("var EVERY ")) {
            let (name, value) = line["var ".len()..].split_once(" = ").expect("var CARD_X = \"…\";");
            region = region.replace(&format!("\" + {name} + \""), value.trim_end_matches(';').trim_matches('"'));
        }
        // A literal continued on the next line (`"…" +` / `"…"`) is one.
        let region = region.split("\" +\n").map(|l| l.trim_start().trim_start_matches('"')).collect::<Vec<_>>().join("");
        let mut out = Vec::new();
        for rule in region.split('"').skip(1).step_by(2) {
            let Some(selectors) = rule.split_once('{').map(|(s, _)| s.trim()) else { continue };
            if selectors.starts_with('@') || selectors.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                continue;
            }
            let inner = selectors.strip_prefix(":where(").and_then(|s| s.strip_suffix(')')).unwrap_or(selectors);
            let (mut depth, mut from) = (0, 0);
            for (i, c) in inner.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    ',' if depth == 0 => {
                        out.push(inner[from..i].trim().to_string());
                        from = i + 1;
                    }
                    _ => {}
                }
            }
            out.push(inner[from..].trim().to_string());
        }
        out
    }

    /// COMP-12: the base typography, in its layer, reaches every element by
    /// default, and only what markdown produced in a body that opts out
    /// (`<meta name="menger-base" content="markdown">`). Every selector is
    /// the box-sizing reset, the document (html, body), a block the SDK owns
    /// (a mermaid placeholder, a docs card, a wave reference), an element
    /// md.rs marked `data-md` — or the default reach, behind `EVERY`, which
    /// the opt-out's meta turns off. So in an opted-out body its own `<code
    /// class="x">` matches no rule and gets no border, and a markdown ``
    /// `code` `` (`<code data-md>`) does. The tags the SDK styles under
    /// `data-md` are the ones md.rs marks, and each has its default reach.
    #[test]
    fn the_base_typography_reaches_markdown_only_in_a_body_that_opts_out() {
        let selectors = base_selectors();
        assert!(selectors.len() > 30, "{selectors:?}");
        assert!(SDK_JS.contains("base.textContent = \"@layer menger-base{\\n\" + BASE_CSS + \"\\n}"), "the base is a layer");
        let every = SDK_JS.split("var EVERY = \"").nth(1).and_then(|r| r.split('"').next()).expect("EVERY");
        assert_eq!(every, ":root:not(:has(meta[name=menger-base][content=markdown i])) ");
        // MENGER-22: and the highlighter's spans (`.hljs-*`), which only the
        // SDK's own pass writes.
        let owned = [".mermaid", ".menger-card", ".wave-ref", "a.wave-ref", ".hljs"];
        let tags_of = |compound: &str| -> Vec<String> {
            let named = compound.trim_end_matches("[data-md]");
            let named = named.strip_prefix(":is(").and_then(|n| n.strip_suffix(')')).unwrap_or(named);
            named.split(',').filter(|t| !t.is_empty()).map(String::from).collect()
        };
        let (mut marked_tags, mut default_tags): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for sel in &selectors {
            if ["*", "*::before", "*::after", "html", "body"].contains(&sel.as_str()) || owned.iter().any(|o| sel.starts_with(o)) {
                continue;
            }
            if let Some(rest) = sel.strip_prefix(every) {
                default_tags.extend(rest.split([' ', '>']).flat_map(tags_of));
                continue;
            }
            // Under the opt-out, every compound that names a tag is marked.
            for compound in sel.split([' ', '>']).filter(|c| !c.is_empty()) {
                assert!(compound.contains("[data-md]"), "a base rule reaches an opted-out body's own HTML: {sel}");
                marked_tags.extend(tags_of(compound));
            }
        }
        marked_tags.sort();
        marked_tags.dedup();
        let mut md: Vec<String> = crate::md::MD_MARKED.iter().map(|t| t.to_string()).collect();
        md.sort();
        assert_eq!(marked_tags, md, "the SDK's [data-md] tags are md.rs's");
        assert!(md.iter().all(|t| default_tags.contains(t)), "each has its default reach: {default_tags:?}");
        // The two `code`s of the done-when: the markdown's carries the border,
        // in either mode; the body's own only by default.
        let at = SDK_JS.find(":where(\" + EVERY + \"code,code[data-md],").expect("the code rule");
        let rule = &SDK_JS[at..at + SDK_JS[at..].find("}\"").unwrap()];
        assert!(rule.contains("border:var(--border) solid var(--line)"), "{rule}");
        let html = crate::md::render_body_framed("<meta name=\"menger-base\" content=\"markdown\">\n\nA `code` and <code class=\"x\">own</code>.\n", &|_| None).html;
        assert!(html.contains("<code data-md>code</code>") && html.contains("<code class=\"x\">own</code>"), "{html}");
        // The opt-out is documented where the protocol is.
        let readme = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../README.md")).unwrap();
        for doc in [SDK_JS, readme.as_str()] {
            assert!(doc.contains("<meta name=\"menger-base\" content=\"markdown\">"), "the opt-out documented");
        }
    }

    /// COMP-10: the shell's GENERATED region is the bundle the script wrote
    /// — `frontend/sdk/generated/sdk-bundle.js`, byte for byte, never edited
    /// in place — and that bundle holds the generated client's every method
    /// the manifest names (the activation's, held to it by tickets-api's
    /// drift guard). The SDK's types are published beside the SDK.
    #[test]
    fn the_generated_region_is_the_bundle_of_the_generated_client() {
        const BEGIN: &str = "  // @generated-begin scripts/sdk-codegen.sh\n";
        const END: &str = "  // @generated-end\n";
        let a = SDK_JS.find(BEGIN).expect("the @generated-begin line") + BEGIN.len();
        let b = SDK_JS.find(END).expect("the @generated-end line");
        assert_eq!(SDK_JS.matches(BEGIN).count(), 1);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("sdk");
        let bundle = std::fs::read_to_string(dir.join("generated/sdk-bundle.js")).unwrap();
        assert!(
            SDK_JS[a..b] == bundle,
            "menger-sdk.js's GENERATED region is not generated/sdk-bundle.js — run scripts/sdk-codegen.sh, never edit it by hand"
        );
        assert!(bundle.contains("mengerTyped = {"), "the bundle hands the shell its client");
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("generated/manifest.json")).unwrap()).unwrap();
        // COMMENT-2: the manifest holds every activation; the bundle is the
        // bridge's, which reaches `tickets` — ACCESS-17: and its child
        // `tickets work`.
        let methods = manifest["activations"]["tickets"].as_object().expect("the tickets activation's methods");
        assert!(methods.contains_key("set_status"), "{methods:?}");
        let work = manifest["activations"]["tickets.work"].as_object().expect("tickets work's methods");
        assert!(work.contains_key("waves") && work.contains_key("blockers"), "{work:?}");
        for (ns, methods) in [("tickets", methods), ("tickets.work", work)] {
            for name in methods.keys() {
                assert!(bundle.contains(&format!("\"{ns}.{name}\"")), "the bundle calls {ns}.{name}");
            }
        }
        for published in ["menger.d.ts", "generated/tickets/client.d.ts", "generated/tickets/types.d.ts", "generated/tickets/work/client.d.ts"] {
            assert!(dir.join(published).is_file(), "{published}");
        }
        let index = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("index.html")).unwrap();
        assert!(index.contains("<link data-trunk rel=\"copy-file\" href=\"sdk/menger.d.ts\" data-target-path=\"sdk\" />"));
        assert!(index.contains("<link data-trunk rel=\"copy-dir\" href=\"sdk/generated\" data-target-path=\"sdk/generated\" />"));
    }

    fn page_shell(tokens: &[(String, String)], emulate: Option<Emulate>) -> Shell<'_> {
        Shell { channel: "f5", scheme: Scheme::Dark, theme: "default", tokens, mermaid_url: "u", highlight_url: "h", fonts_url: "", motion: Motion::Full, typing: Typing::Normal, entered: false, components: None, ticket: Some("MENGER-16"), cite: None, emulate }
    }

    /// MENGER-15: a whole document renders as itself — its own head, styles
    /// and scripts, no SDK, no theme tokens, no base — with IDENTITY-28's CSP
    /// the first element of its head, then the init block and the page
    /// shim, before anything the page holds; the rest byte for byte.
    #[test]
    fn a_whole_document_is_its_own_page_with_the_csp_first_in_its_head() {
        let tokens = shell_tokens();
        let page = "<!doctype html>\n<!-- a comment naming <head> -->\n<html lang=\"en\" data-x='a>b'>\n<head>\n<meta charset=\"utf-8\"><style>@media (prefers-color-scheme: dark){body{background:#000}}</style><script>window.x=1</script></head><body><h1>Hi</h1></body></html>";
        let doc = srcdoc(page, &page_shell(&tokens, Some(Emulate { scheme: Some(Scheme::Dark), reduce: None })));
        let csp = format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">");
        let head = "<!doctype html>\n<!-- a comment naming <head> -->\n<html lang=\"en\" data-x='a>b'>\n<head>";
        assert!(doc.starts_with(&format!("{head}{csp}<script type=\"application/json\" id=\"menger-init\">")), "{}", &doc[..300]);
        assert_eq!(doc.matches("Content-Security-Policy").count(), 1);
        let at = |needle: &str| doc.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at(&csp) < at("id=\"menger-page\"") && at("id=\"menger-page\"") < at("<meta charset"));
        assert!(doc.ends_with(&page[head.len()..]), "the page follows, as written");
        // Nothing of a framed body's shell.
        for absent in ["id=\"menger-sdk\"", "id=\"menger-theme\"", "<base target", "--bg:"] {
            assert!(!doc.contains(absent), "{absent}");
        }
        let init = &doc[at("id=\"menger-init\">") + "id=\"menger-init\">".len()..];
        let init: serde_json::Value = serde_json::from_str(&init[..init.find("</script>").unwrap()]).unwrap();
        assert_eq!(init, serde_json::json!({ "channel": "f5", "page": true, "ticket": "MENGER-16", "emulate": { "scheme": "dark", "reduce": null } }));
        // The sandbox and the CSP are every frame's.
        assert_eq!(SANDBOX, "allow-scripts allow-popups allow-popups-to-escape-sandbox");
        assert_eq!(CSP, "connect-src 'none'; form-action 'none'");
    }

    /// MENGER-15: where the head begins — inside `<head>`; with none
    /// written, inside `<html>`; with neither, past the doctype.
    #[test]
    fn the_head_is_found_past_doctype_comments_and_attributes() {
        let cases = [
            ("<!DOCTYPE html><HTML><HEAD><title>t</title>", "<!DOCTYPE html><HTML><HEAD>"),
            ("\u{feff}  <html>\n  <body>x", "\u{feff}  <html>\n  "),
            ("<!doctype html><title>t</title>", "<!doctype html>"),
            ("<!doctype html><!-- <html> --> <html class=\"a\"> <!-- c --> <head data-x=\"<>\">x", "<!doctype html><!-- <html> --> <html class=\"a\"> <!-- c --> <head data-x=\"<>\">"),
            ("<html><header>x", "<html>"),
        ];
        for (doc, before) in cases {
            assert_eq!(&doc[..head_at(doc)], before, "{doc:?}");
        }
    }

    /// MENGER-15: a mockup's frame of an ordinary body keeps the SDK and
    /// gains the page shim after it; any other frame has no shim.
    #[test]
    fn a_mockups_frame_carries_the_page_shim() {
        let tokens = shell_tokens();
        let plain = srcdoc("<b>x</b>", &page_shell(&tokens, None));
        assert!(!plain.contains("id=\"menger-page\""));
        let mock = srcdoc("<b>x</b>", &page_shell(&tokens, Some(Emulate { scheme: Some(Scheme::Light), reduce: Some(true) })));
        let at = |needle: &str| mock.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("id=\"menger-sdk\"") < at("id=\"menger-page\"") && at("id=\"menger-page\"") < at("<body>"));
        assert!(mock.contains("\"emulate\":{\"reduce\":true,\"scheme\":\"light\"}"), "{}", &mock[..600]);
    }

    /// MENGER-15: the page shim answers both features, listens only to its
    /// parent on its channel, and keeps a page's links in the page.
    #[test]
    fn the_page_shim_emulates_both_preferences() {
        for needle in [
            "prefers-color-scheme",
            "prefers-reduced-motion",
            "window.matchMedia = function",
            "\"menger:emulate\"",
            "ev.source !== host",
            "m.channel !== channel",
            "post(\"menger:ready\"",
            "post(\"menger:key\"",
            "post(\"menger:link\", { href: href })",
            "post(\"menger:navigate\"",
            "data-menger-scheme",
        ] {
            assert!(PAGE_JS.contains(needle), "{needle}");
        }
        assert!(!PAGE_JS.contains("</script"), "nothing in it closes its element");
    }
}
