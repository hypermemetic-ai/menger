//! APP-110: **the live test app in a ticket** — a body's `live` block draws
//! the real test app at a route, in the device frames of a mockup, beside it.
//!
//! ````markdown
//! ```live route="#/components" widths=390,1440 title="#/components on test"
//! ```
//!
//! <m-live route="#/components" widths="390,1440"></m-live>
//! ````
//!
//! It is a mockup whose page is not a page the body wrote but an address:
//! md.rs turns the fence (and the element) into the placeholder a mockup
//! fence becomes, its `source` the [`MARK`]-led text [`source`] writes, so the
//! host's mockup box (src/mockup.rs: widths, Light / Dark, Full screen,
//! Compare, comments) is the live view's too, and **Compare** sets a mockup
//! and a live view side by side at one device and theme with no code of its own.
//! Only the screen differs: each device holds an `<iframe>` of
//! `<live origin>/<route>` ([`Screen`]) where a mockup's holds the sandboxed
//! frame.
//!
//! **Which app.** `--live-origin` (stamped into `<meta name="menger-live-origin">`,
//! [`origin`]) — the test environment's. `env=` may name `test` and nothing
//! else: prod is never drawn ([`env_ok`]). The tracker's policy frames that
//! origin and nothing more (`frame-src`), and test's frames the tracker
//! (`--frame-ancestor`); without the first the box says no test app is
//! configured (src/shell.rs, [`Framing`](crate::shell) on the server).
//!
//! **Signed in.** The iframe is the app: as the reader on test where they hold
//! a session there (the cookie is test's own — a `__Host-` cookie is host-only,
//! so the tracker's does not ride along), and with none, test's sign-in — which
//! is unframeable (plexus-login), so the box offers *Open in a tab*; the
//! reader signs in there and *Reload*s.
//!
//! **Light / Dark** is the box's toggle: the frame is asked for `?theme=` (the
//! app reads it as it starts, src/theme.rs). *Reduced motion* is a page's own
//! to read and cannot reach another origin: it changes nothing in a live view.
//!
//! What decides is pure and pinned by native tests; the rest is DOM.

use leptos::prelude::*;

use crate::frame::{Emulate, Frame, Scheme};

/// What leads a mockup's `source` (and a screen's page) when it is a live
/// view — U+FDD1, a noncharacter no page begins with.
pub(crate) const MARK: char = '\u{FDD1}';

/// The longest route a block may name.
const ROUTE_MAX: usize = 512;

/// The mockup `source` of a live view of `route` on `env` (`None`: test).
pub(crate) fn source(route: &str, env: Option<&str>) -> String {
    format!("{MARK}live\n{route}\n{}", env.unwrap_or(""))
}

/// A mockup `source` as a live view's `(route, env)` — `None` for a page.
pub(crate) fn read_source(source: &str) -> Option<(String, Option<String>)> {
    let rest = source.strip_prefix(MARK)?.strip_prefix("live\n")?;
    let (route, env) = rest.split_once('\n')?;
    Some((route.to_string(), Some(env.to_string()).filter(|e| !e.is_empty())))
}

/// `route` as the app's hash route it must be: `#/…`, printable ASCII, no
/// quote, angle bracket, backslash or space, at most [`ROUTE_MAX`] long.
pub(crate) fn route_ok(route: &str) -> Result<String, String> {
    let r = route.trim();
    if r.is_empty() {
        return Err("m-live: name a route, route=\"#/components\"".into());
    }
    if !r.starts_with("#/") {
        return Err(format!("m-live: route=\"{}\" is no app route — write it as #/components", clip(r)));
    }
    if r.len() > ROUTE_MAX || !r.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`')) {
        return Err(format!("m-live: route=\"{}\" holds a character a route does not (or is over {ROUTE_MAX})", clip(r)));
    }
    Ok(r.to_string())
}

/// `env=` — none (test), or `test`. Another is refused, prod by name.
pub(crate) fn env_ok(env: Option<&str>) -> Result<(), String> {
    match env.map(str::trim).filter(|e| !e.is_empty()) {
        None => Ok(()),
        Some(e) if e.eq_ignore_ascii_case("test") => Ok(()),
        Some(e) if e.eq_ignore_ascii_case("prod") => Err("m-live: env=\"prod\" is never drawn — a live view is the test app".into()),
        Some(e) => Err(format!("m-live: env=\"{}\" is unknown — a live view is the test app (env=\"test\")", clip(e))),
    }
}

fn clip(s: &str) -> String {
    s.chars().take(64).collect()
}

/// A live view of `route` on `origin` as the page a screen is handed: its
/// address and the reload count (a change of either loads the frame again).
pub(crate) fn page(origin: &str, route: &str, reloads: u32) -> String {
    format!("{MARK}url\n{}/{route}\n{reloads}", origin.trim_end_matches('/'))
}

/// A screen's page as a live view's `(address, reloads)` — `None` for a page.
pub(crate) fn read_page(page: &str) -> Option<(&str, u32)> {
    let rest = page.strip_prefix(MARK)?.strip_prefix("url\n")?;
    let (url, n) = rest.split_once('\n')?;
    Some((url, n.parse().unwrap_or(0)))
}

/// A page that is a live view's.
pub(crate) fn is_live(page: &str) -> bool {
    read_page(page).is_some()
}

/// The same live view, loaded again.
pub(crate) fn reloaded(page: &str) -> Option<String> {
    let (url, n) = read_page(page)?;
    Some(format!("{MARK}url\n{url}\n{}", n.wrapping_add(1)))
}

/// What a screen's `src` is: the address with `theme` (the box's Light /
/// Dark) and `reload` in its query, before the route's `#`.
pub(crate) fn frame_src(url: &str, scheme: Option<Scheme>, reloads: u32) -> String {
    let (base, hash) = url.split_once('#').map_or((url, ""), |(b, h)| (b, h));
    let mut query = Vec::new();
    match scheme {
        Some(Scheme::Light) => query.push("theme=light".to_string()),
        Some(Scheme::Dark) => query.push("theme=dark".to_string()),
        None => {}
    }
    if reloads > 0 {
        query.push(format!("reload={reloads}"));
    }
    let q = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };
    let base = base.trim_end_matches('/');
    if hash.is_empty() {
        format!("{base}/{q}")
    } else {
        format!("{base}/{q}#{hash}")
    }
}

/// What a live block resolves to: the page a screen is handed, or the line
/// the box shows instead.
pub(crate) fn resolve(route: &str, env: Option<&str>, origin: Option<&str>) -> Result<String, String> {
    env_ok(env)?;
    let route = route_ok(route)?;
    let origin = origin.map(str::trim).filter(|o| !o.is_empty()).ok_or_else(|| {
        "m-live: no test app is configured on this server (--live-origin) — nothing to draw".to_string()
    })?;
    Ok(page(origin, &route, 0))
}

/// The line naming the release test runs.
pub(crate) fn release_line(release: Option<&str>, loaded: bool) -> String {
    match (release, loaded) {
        (Some(r), _) => format!("test runs {r}"),
        (None, true) => "test runs no release yet".to_string(),
        (None, false) => "test's release…".to_string(),
    }
}

/// The app's own origin as the server stamped it (`--live-origin`) — `None`
/// when none is configured (or the page was not stamped).
pub(crate) fn origin() -> Option<String> {
    use wasm_bindgen::JsCast;
    let el = crate::dom::window().document()?.query_selector("meta[name=\"menger-live-origin\"]").ok().flatten()?;
    let content = el.dyn_into::<web_sys::HtmlMetaElement>().ok()?.content();
    let content = content.trim();
    (!content.is_empty() && !content.contains("{{")).then(|| content.to_string())
}

/// What a live view adds to its box's bar: *Reload* at the head, and in the
/// part a phone folds into ⋯ the line naming the release test runs and *Open
/// in a tab* (test's sign-in is unframeable — the reader signs in there).
pub(crate) fn extras(key: String, html: ArcRwSignal<String>, head: ViewFn, more: ViewFn) -> (ViewFn, ViewFn) {
    let release = ArcRwSignal::new(None::<Option<String>>);
    ask_release(release.clone());
    let actions = ViewFn::from(move || {
        let key = key.clone();
        view! {
            {head.run()}
            <button class="framed-btn live-reload" title="Load the test app again" on:click=move |_| crate::mockup::reload(&key)>"Reload"</button>
        }
    });
    let more = ViewFn::from(move || {
        let (release, html) = (release.clone(), html.clone());
        let said = move || match release.get() {
            Some(r) => release_line(r.as_deref(), true),
            None => release_line(None, false),
        };
        let href = move || html.with(|h| read_page(h).map(|(u, _)| u.to_string())).unwrap_or_default();
        view! {
            {more.run()}
            <span class="live-release" role="note" title="from deployments: what the test environment runs now">{said}</span>
            <a class="framed-btn live-open" target="_blank" rel="noopener" href=href title="Open the test app in a tab — sign in there if the frame shows no session">"Open in a tab ↗"</a>
        }
    });
    (actions, more)
}

/// The release test runs, read once over the viewer's socket
/// (`deployments.environments`): `Some(None)` when it runs none yet.
fn ask_release(into: ArcRwSignal<Option<Option<String>>>) {
    use wasm_bindgen_futures::spawn_local;
    spawn_local(async move {
        let Some(client) = crate::session::deployments_client() else { return };
        let Ok(events) = client.environments_all(None).await else { return };
        let release = events.into_iter().find_map(|ev| match ev {
            menger_tickets_api::DeploymentsEvent::Environment { environment } if environment.project.eq_ignore_ascii_case("menger") && environment.name == "test" => {
                Some(environment.release)
            }
            _ => None,
        });
        into.set(Some(release.flatten()));
    });
}

/// One device's screen: the test app in an `<iframe>` when the page is a
/// live view's, else the sandboxed frame a mockup's page is (same props as
/// [`Frame`]). Which it is is read once — a page that turns from one into
/// the other redraws the screen.
#[component]
pub(crate) fn Screen(
    #[prop(into)] html: Signal<String>,
    ticket: Option<String>,
    scope: String,
    emulate: Signal<Emulate>,
) -> impl IntoView {
    let live = Memo::new(move |_| html.with(|h| is_live(h)));
    move || {
        if live.get() {
            let src = move || {
                let (url, n) = html.with(|h| read_page(h).map(|(u, n)| (u.to_string(), n))).unwrap_or_default();
                frame_src(&url, emulate.with(|e| e.scheme), n)
            };
            view! { <iframe class="live-frame" title="the live test app" src=src></iframe> }.into_any()
        } else {
            let (ticket, scope) = (ticket.clone(), scope.clone());
            view! { <Frame html ticket scope emulate fixed=true /> }.into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_round_trips_and_a_page_is_none() {
        let s = source("#/components", None);
        assert_eq!(read_source(&s), Some(("#/components".into(), None)));
        assert_eq!(read_source(&source("#/a/b?x=1", Some("test"))), Some(("#/a/b?x=1".into(), Some("test".into()))));
        for page in ["<!doctype html><p>hi", "", "live\n#/x\n", "\u{FDD1}url\nhttp://x/\n0"] {
            assert_eq!(read_source(page), None, "{page:?}");
        }
    }

    #[test]
    fn a_route_is_an_app_hash_route() {
        for ok in ["#/components", "#/ticket/APP-110", " #/board ", "#/", "#/a?b=c&d=e"] {
            assert_eq!(route_ok(ok).as_deref(), Ok(ok.trim()), "{ok}");
        }
        for bad in ["", "  ", "components", "/components", "#components", "https://evil.example/#/x", "#/a b", "#/\"><script>", "#/a\\b", "#/é"] {
            assert!(route_ok(bad).is_err(), "{bad:?}");
        }
        assert!(route_ok(&format!("#/{}", "a".repeat(ROUTE_MAX))).is_err());
    }

    #[test]
    fn only_test_is_ever_drawn() {
        assert_eq!(env_ok(None), Ok(()));
        assert_eq!(env_ok(Some("")), Ok(()));
        assert_eq!(env_ok(Some("test")), Ok(()));
        assert_eq!(env_ok(Some("TEST")), Ok(()));
        assert!(env_ok(Some("prod")).unwrap_err().contains("never drawn"));
        assert!(env_ok(Some("PROD")).is_err());
        assert!(env_ok(Some("staging")).unwrap_err().contains("unknown"));
        // Resolved, prod is refused before an origin is even looked at.
        assert!(resolve("#/board", Some("prod"), Some("https://t.example")).unwrap_err().contains("never drawn"));
    }

    #[test]
    fn a_block_resolves_to_its_address_or_says_why_not() {
        assert_eq!(
            resolve("#/components", None, Some("https://app-test.menger.sh")).map(|p| read_page(&p).map(|(u, n)| (u.to_string(), n))),
            Ok(Some(("https://app-test.menger.sh/#/components".into(), 0)))
        );
        assert_eq!(read_page(&page("http://127.0.0.1:4451/", "#/x", 3)), Some(("http://127.0.0.1:4451/#/x", 3)));
        assert!(resolve("#/components", None, None).unwrap_err().contains("--live-origin"));
        assert!(resolve("#/components", None, Some("  ")).is_err());
        assert!(resolve("nope", None, Some("https://t.example")).is_err());
    }

    #[test]
    fn a_reload_changes_the_address_the_frame_loads() {
        let p = page("https://t.example", "#/board", 0);
        let r = reloaded(&p).unwrap();
        assert_ne!(p, r);
        assert_eq!(read_page(&r), Some(("https://t.example/#/board", 1)));
        assert_eq!(reloaded("<!doctype html>"), None);
        assert!(is_live(&r) && !is_live("<!doctype html>"));
        let (url, n) = read_page(&r).unwrap();
        assert_ne!(frame_src(url, None, 0), frame_src(url, None, n));
    }

    /// The frame's address: the query goes before the route's `#`, which keeps
    /// its own query; light and dark are asked for by name; a first load is bare.
    #[test]
    fn the_frames_address_carries_the_theme_and_the_reload() {
        let u = "https://t.example/#/components";
        assert_eq!(frame_src(u, None, 0), "https://t.example/#/components");
        assert_eq!(frame_src(u, Some(Scheme::Dark), 0), "https://t.example/?theme=dark#/components");
        assert_eq!(frame_src(u, Some(Scheme::Light), 2), "https://t.example/?theme=light&reload=2#/components");
        assert_eq!(frame_src(u, None, 1), "https://t.example/?reload=1#/components");
        assert_eq!(frame_src("http://127.0.0.1:4451/#/a?b=c", Some(Scheme::Dark), 0), "http://127.0.0.1:4451/?theme=dark#/a?b=c");
    }

    #[test]
    fn the_release_is_said_or_its_absence() {
        assert_eq!(release_line(Some("20261022T0830Z-667380b"), true), "test runs 20261022T0830Z-667380b");
        assert_eq!(release_line(None, true), "test runs no release yet");
        assert_eq!(release_line(None, false), "test's release…");
    }
}
