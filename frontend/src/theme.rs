//! AGS-17: light and dark mode — three states, one attribute.
//!
//! The palette is two value sets under one set of token names
//! (index.html: the `:root` light block, and the dark block stated twice —
//! under `@media (prefers-color-scheme: dark)` guarded as
//! `:root:not([data-theme="light"])`, and under `:root[data-theme="dark"]`).
//! This module owns the one bit of state that selects between them: the
//! user's choice, [`Theme`] — `System` (follow `prefers-color-scheme`; no
//! attribute on the root), `Light`, or `Dark` (stamped as `data-theme` on
//! `<html>`, which out-ranks the media query in either direction). The
//! header [`Toggle`] cycles system → light → dark; the choice persists in
//! localStorage under [`KEY`] (absent means system) and applies
//! instantly: every component rule paints through the tokens, so stamping
//! the attribute re-colors nav, board, page, menu, toast, banner and
//! chips with no re-render and no reload. index.html carries a one-line
//! pre-paint script that stamps the stored choice before the wasm loads
//! (a reload never flashes the system theme); a native test pins that it
//! reads the same key.
//!
//! The one thing tokens cannot re-color is a mermaid SVG already drawn:
//! mermaid picks its theme from the body's painted background per render
//! (AGS-14), so a change of the *effective* mode — a toggle, or the
//! system preference flipping while the choice is System — redraws every
//! mounted diagram ([`crate::mermaid::rerender_all`]). A toggle that
//! leaves the painted mode as it was (system → light on a light system)
//! redraws nothing. COMP-4: the same change re-themes every framed body
//! ([`crate::frame::retheme_all`]) — a frame's tokens are sent, not shared.

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::{dom, scopes};

/// The localStorage key; the value is `"light"` or `"dark"`, and the key
/// is absent for the system default. index.html's pre-paint script reads
/// the same key (pinned by `tests::pre_paint_script_reads_the_same_key`).
pub(crate) const KEY: &str = "menger.theme";
/// The attribute on `<html>` the two dark CSS blocks key on.
const ATTR: &str = "data-theme";
const MEDIA: &str = "(prefers-color-scheme: dark)";

/// The user's choice: the three states of the house pattern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Theme {
    System,
    Light,
    Dark,
}

/// What is actually painted: the choice resolved against the system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Light,
    Dark,
}

impl Theme {
    /// The toggle's cycle: system → light → dark → system.
    pub(crate) fn next(self) -> Self {
        match self {
            Theme::System => Theme::Light,
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::System,
        }
    }

    /// The stored / stamped value; `None` is the system default (no key
    /// in storage, no attribute on the root).
    pub(crate) fn as_value(self) -> Option<&'static str> {
        match self {
            Theme::System => None,
            Theme::Light => Some("light"),
            Theme::Dark => Some("dark"),
        }
    }

    /// The inverse of [`as_value`](Self::as_value): anything unknown (a
    /// missing key, a stale value) is the system default.
    pub(crate) fn parse(stored: Option<&str>) -> Self {
        match stored {
            Some("light") => Theme::Light,
            Some("dark") => Theme::Dark,
            _ => Theme::System,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        self.as_value().unwrap_or("system")
    }

    /// The toggle's glyph: half for "follows the system", empty / full
    /// circle for light / dark.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Theme::System => "◐",
            Theme::Light => "○",
            Theme::Dark => "●",
        }
    }

    /// What this choice paints given the system preference.
    pub(crate) fn mode(self, system_prefers_dark: bool) -> Mode {
        match self {
            Theme::Light => Mode::Light,
            Theme::Dark => Mode::Dark,
            Theme::System if system_prefers_dark => Mode::Dark,
            Theme::System => Mode::Light,
        }
    }
}

/// localStorage, or None where it is unavailable (private mode, blocked
/// site data): the choice is then "not remembered" — the try/catch.
fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

pub(crate) fn load() -> Theme {
    Theme::parse(storage().and_then(|s| s.get_item(KEY).ok().flatten()).as_deref())
}

pub(crate) fn save(t: Theme) {
    if let Some(s) = storage() {
        let _ = match t.as_value() {
            Some(v) => s.set_item(KEY, v),
            None => s.remove_item(KEY),
        };
    }
}

/// Stamp (or clear) `data-theme` on `<html>`. This is the whole switch:
/// the CSS does the rest.
pub(crate) fn apply(t: Theme) {
    let Some(root) = dom::window().document().and_then(|d| d.document_element()) else {
        return;
    };
    let _ = match t.as_value() {
        Some(v) => root.set_attribute(ATTR, v),
        None => root.remove_attribute(ATTR),
    };
}

fn media() -> Option<web_sys::MediaQueryList> {
    dom::window().match_media(MEDIA).ok().flatten()
}

fn system_prefers_dark() -> bool {
    media().is_some_and(|m| m.matches())
}

/// The header control. Mounted once; owns the choice, its persistence,
/// the root attribute, the system-preference listener, and the diagram
/// redraw on an effective-mode change.
#[component]
pub(crate) fn Toggle() -> impl IntoView {
    let choice = RwSignal::new(load());
    let system_dark = RwSignal::new(system_prefers_dark());

    // While the choice is System the CSS follows the OS by itself; this
    // listener exists so the diagrams follow too (see the module doc).
    if let Some(mql) = media() {
        let cb = Closure::<dyn FnMut()>::new(move || system_dark.set(system_prefers_dark()));
        let _ = mql.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
        cb.forget(); // lives as long as the app
    }

    // THEME-7: a choice made in another tab of this browser is this tab's
    // too — every open page follows it (`storage` fires in the others).
    {
        let cb = Closure::<dyn FnMut(web_sys::StorageEvent)>::new(move |ev: web_sys::StorageEvent| {
            if ev.key().as_deref() == Some(KEY) {
                let next = Theme::parse(ev.new_value().as_deref());
                if choice.get_untracked() != next {
                    choice.set(next);
                }
            }
        });
        let _ = dom::window().add_event_listener_with_callback("storage", cb.as_ref().unchecked_ref());
        cb.forget(); // lives as long as the app
    }

    // One effect, in order: persist, stamp, and — only when what is
    // painted actually changed — redraw the diagrams against the newly
    // painted background. The first run (mount) stamps and redraws nothing.
    Effect::new(move |prev: Option<Mode>| {
        let t = choice.get();
        let mode = t.mode(system_dark.get());
        save(t);
        apply(t);
        if prev.is_some_and(|p| p != mode) {
            // THEME-7: the one repaint path, light/dark its special case —
            // every region's tokens for the new mode, then (COMP-4) every
            // framed body takes its region's (the frame cannot read the
            // app's CSS) and the diagrams redraw.
            scopes::repaint();
        }
        mode
    });

    view! {
        <button
            id="theme"
            class="theme"
            type="button"
            title=move || {
                let t = choice.get();
                format!("theme: {} — click for {}", t.label(), t.next().label())
            }
            aria-label=move || format!("theme: {}", choice.get().label())
            data-choice=move || choice.get().label()
            on:click=move |_| choice.update(|t| *t = t.next())
        >
            <span class="glyph" aria-hidden="true">{move || choice.get().glyph()}</span>
            " "
            {move || choice.get().label()}
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, Theme, KEY};

    #[test]
    fn the_toggle_cycles_system_light_dark() {
        assert_eq!(Theme::System.next(), Theme::Light);
        assert_eq!(Theme::Light.next(), Theme::Dark);
        assert_eq!(Theme::Dark.next(), Theme::System);
        assert_eq!(Theme::System.next().next().next(), Theme::System);
    }

    /// Storage round-trips; the system default is "no value", and a
    /// stale or foreign value degrades to it rather than to a panic.
    #[test]
    fn stored_value_round_trips_and_unknown_means_system() {
        for t in [Theme::System, Theme::Light, Theme::Dark] {
            assert_eq!(Theme::parse(t.as_value()), t);
        }
        assert_eq!(Theme::System.as_value(), None);
        assert_eq!(Theme::parse(Some("auto")), Theme::System);
        assert_eq!(Theme::parse(Some("")), Theme::System);
        assert_eq!(Theme::Light.label(), "light");
        assert_eq!(Theme::System.label(), "system");
    }

    /// Only System depends on the preference.
    #[test]
    fn effective_mode_resolves_system_against_the_preference() {
        assert_eq!(Theme::System.mode(true), Mode::Dark);
        assert_eq!(Theme::System.mode(false), Mode::Light);
        assert_eq!(Theme::Light.mode(true), Mode::Light);
        assert_eq!(Theme::Dark.mode(false), Mode::Dark);
    }

    /// index.html's pre-paint stamp reads the key this module writes and
    /// stamps the attribute the CSS keys on; it is inline (no `src=`), so
    /// AGS-14's "no page-load script" pin is untouched.
    #[test]
    fn pre_paint_script_reads_the_same_key() {
        let html = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/index.html")).unwrap();
        let line = html
            .lines()
            .find(|l| l.contains("<script>") && l.contains("localStorage.getItem"))
            .expect("the pre-paint script");
        assert!(line.contains(&format!("localStorage.getItem(\"{KEY}\")")), "{line}");
        assert!(line.contains(&format!("setAttribute(\"{}\"", super::ATTR)), "{line}");
        assert!(line.contains("try{") && line.contains("catch(e){}"), "guarded: {line}");
        assert!(!line.contains("src="), "inline, not a fetched script: {line}");
    }
}
