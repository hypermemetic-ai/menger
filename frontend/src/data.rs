//! APP-94 (APP-69's first build): the app's data asked again without the
//! page.
//!
//! Owner, 2026-10-19 (chat 6): *"then we need to move forward on the data
//! reload without ui reload"*.
//!
//! **A reconnect re-asks, in place.** APP-63 caught up after a dropped
//! socket by reloading the page, because one view held what it fetched
//! once per mount (an open chat's messages) and nothing asked it again. A
//! reload paints from nothing (APP-67, APP-75) and loses the chat's place.
//! Now a reconnect goes on with the same page: the session re-asks its own
//! reads on the new connection as it always has (`watch`, the groups, the
//! extents, the board's window, the open ticket, each module's `refresh`),
//! and [`resync`] moves one epoch that every read a view makes for itself
//! follows — each re-asks and merges its answer into what it holds, by id,
//! so the nodes on screen, the scroll, focus and drafts stay.
//!
//! **The reload is the fallback.** A page that outlived a deploy is not the
//! app the server now serves: its wire may not match. So before a
//! reconnect goes on, [`new_release`] asks the shell for the page again and
//! compares the build it names (the wasm's hashed name) with this page's;
//! when they differ, APP-63's reload (its snapshot, its budget) runs.
//!
//! APP-95 grows this into the resource registry: a read as a key, a loader
//! and its signals, asked once however many views use it.

use leptos::prelude::*;

thread_local! {
    static EPOCH: ArcRwSignal<u32> = ArcRwSignal::new(0);
}

/// The data epoch: moved by every [`resync`]. A view's own read tracks it
/// and re-asks when it moves, merging in place.
pub(crate) fn epoch() -> ArcRwSignal<u32> {
    EPOCH.with(Clone::clone)
}

/// Every view's own read asked again — after a reconnect, or a `watch`
/// that lagged. The markup is not touched: each answer merges into what
/// its view holds.
pub(crate) fn resync() {
    EPOCH.with(|e| e.update(|n| *n = n.wrapping_add(1)));
}

/// A view's own read, asked again on every [`resync`] while the view is
/// mounted (the Effect is its owner's): `ask` re-runs the read, and its
/// answer is the view's to merge — never a remount.
pub(crate) fn again(ask: impl Fn() + 'static) {
    let epoch = epoch();
    Effect::new(move |seen: Option<u32>| {
        let now = epoch.get();
        if seen.is_some_and(|s| s != now) {
            ask();
        }
        now
    });
}

/// The build a page names: the hashed name of its wasm (`menger-app-<hash>`
/// in `/app/menger-app-<hash>_bg.wasm`), or none in a page without one.
pub(crate) fn build_of(html: &str) -> Option<&str> {
    let end = html.find("_bg.wasm")?;
    let start = html[..end].rfind(['/', '\'', '"']).map_or(0, |i| i + 1);
    let name = &html[start..end];
    (!name.is_empty() && !name.contains(char::is_whitespace)).then_some(name)
}

/// Whether the shell now serves another build than the one this page runs
/// — `false` when either is unknown (the page is kept: a reload is the
/// fallback, never the guess).
pub(crate) fn differs(mine: Option<&str>, served: Option<&str>) -> bool {
    matches!((mine, served), (Some(a), Some(b)) if a != b)
}

/// The browser half: this page's build, and the one the shell serves now.
pub(crate) async fn new_release() -> bool {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let w = crate::dom::window();
    let Some(mine) = w.document().and_then(|d| d.document_element()).map(|e| e.outer_html()) else { return false };
    let path = w.location().pathname().unwrap_or_else(|_| "/".into());
    // A query of its own, so no cache answers for the server.
    let url = format!("{path}?build={}", js_sys::Date::now() as u64);
    let Ok(resp) = JsFuture::from(w.fetch_with_str(&url)).await else { return false };
    let Ok(resp) = resp.dyn_into::<web_sys::Response>() else { return false };
    if !resp.ok() {
        return false;
    }
    let Ok(text) = resp.text() else { return false };
    let Some(served) = JsFuture::from(text).await.ok().and_then(|t| t.as_string()) else { return false };
    let changed = differs(build_of(&mine), build_of(&served));
    if changed {
        leptos::logging::log!("reconnected: the server runs build {:?}, this page {:?}", build_of(&served), build_of(&mine));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_names_its_build() {
        let page = r#"<script type="module">import init from '/app/menger-app-6ba3.js'; await init({ module_or_path: '/app/menger-app-6ba3_bg.wasm' });</script>"#;
        assert_eq!(build_of(page), Some("menger-app-6ba3"));
        assert_eq!(build_of(r#"<link rel="preload" href="/app/menger-app-77_bg.wasm">"#), Some("menger-app-77"));
        assert_eq!(build_of("<p>no app here</p>"), None);
    }

    #[test]
    fn only_a_known_other_build_reloads() {
        assert!(differs(Some("menger-app-1"), Some("menger-app-2")), "a deploy: reload");
        assert!(!differs(Some("menger-app-1"), Some("menger-app-1")), "the same build: refill in place");
        assert!(!differs(None, Some("menger-app-2")), "this page's unknown: stay");
        assert!(!differs(Some("menger-app-1"), None), "the served page unreadable: stay");
    }
}
