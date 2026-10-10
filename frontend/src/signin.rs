//! IDENTITY-22: the signed-in app — who this browser is, and what it may do.
//!
//! The browser holds no credential. menger's shell signed the person in
//! (`/login`, IDENTITY-19) and keeps the session; the browser carries an
//! `HttpOnly` cookie that no script here can read. So the app only *asks*:
//!
//! 1. Before every connect, `GET /login/me` ([`gate`]): 401 sends the
//!    browser to `/login?next=…` and the app goes no further; 404 means this
//!    menger has no login, and the app runs as it always did. Anything else
//!    (unreachable, a 5xx) is asked again: never taken for "no login".
//! 2. After every connect, `members.mine` ([`after_connect`]): the person's
//!    grants, and the socket's own principal, which must be the one `/login/me`
//!    named. With no grant, the app shows [`NoAccess`] — their id, to send an
//!    admin — instead of the views.
//!
//! IDENTITY-31: until both have answered, the app shows neither the views
//! nor [`NoAccess`], only [`Checking`]: what it does not yet know, it does
//! not show. A failed answer ends the session, which retries.
//!
//! The header names the person and offers sign-out ([`Who`]); `#/settings`
//! shows the account ([`AccountGroup`]); admins get `#/members`
//! (`members_page`). Signing out is a form POST to `/logout`.
//!
//! What decides — the login redirect, the header's words, who is an admin,
//! whether the views show — is pure and pinned by native tests.

use std::rc::Rc;

use leptos::prelude::*;
use menger_tickets_api::{Grant, MembersClient, MembersEvent, Role};
use plexus_client::WsTransport;
use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::dom;

/// Who `/login/me` says this browser is.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct Me {
    pub(crate) principal: String,
    pub(crate) name: String,
    pub(crate) provider: Option<String>,
}

/// What the app knows of its person.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Account {
    /// Whether this menger has a login at all; `None` until `/login/me`
    /// has answered.
    pub(crate) login: Option<bool>,
    pub(crate) me: Option<Me>,
    /// The person's grants, once `members.mine` has answered.
    pub(crate) grants: Option<Vec<Grant>>,
    pub(crate) admin_everywhere: bool,
}

impl Account {
    /// Whether the server has said enough to decide what to show: no login,
    /// or the person's grants.
    pub(crate) fn known(&self) -> bool {
        match self.login {
            None => false,
            Some(false) => true,
            Some(true) => self.grants.is_some(),
        }
    }

    /// Whether the views may show: no login, or a person holding a grant —
    /// known, never assumed (IDENTITY-31).
    pub(crate) fn may_view(&self) -> bool {
        match self.login {
            None => false,
            Some(false) => true,
            Some(true) => self.admin_everywhere || self.grants.as_ref().is_some_and(|g| !g.is_empty()),
        }
    }

    /// Whether to show [`NoAccess`]: signed in, and known to hold nothing.
    pub(crate) fn no_access(&self) -> bool {
        self.known() && !self.may_view()
    }

    /// Whether the person administers anything (`#/members`).
    pub(crate) fn is_admin(&self) -> bool {
        self.admin_everywhere || self.grants.as_ref().is_some_and(|g| g.iter().any(|g| g.role == Role::Admin))
    }

    /// The header's words: the name, when signed in.
    pub(crate) fn who(&self) -> Option<String> {
        self.me.as_ref().map(|m| short(&m.name))
    }
}

/// Where a signed-out browser goes: the login page, coming back here.
pub(crate) fn login_url(path: &str, hash: &str) -> String {
    let back = format!("{}{}", if path.is_empty() { "/" } else { path }, hash);
    format!("/login?next={}", js_encode(&back))
}

/// `encodeURIComponent`, natively (the tests run without a browser).
fn js_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A long name (an npub) shortened for the header.
fn short(name: &str) -> String {
    if name.chars().count() > 20 {
        let head: String = name.chars().take(12).collect();
        let tail: String = name.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{head}…{tail}")
    } else {
        name.to_string()
    }
}

// ── The browser side ───────────────────────────────────────────────────

thread_local! {
    static ACCOUNT: ArcRwSignal<Account> = ArcRwSignal::new(Account::default());
    static PROBLEM: ArcRwSignal<Option<String>> = ArcRwSignal::new(None);
}

pub(crate) fn account() -> ArcRwSignal<Account> {
    ACCOUNT.with(Clone::clone)
}

/// IDENTITY-31: why the last try to learn who this is failed, while the
/// app is still checking — what [`Checking`] says after a few seconds.
pub(crate) fn problem() -> ArcRwSignal<Option<String>> {
    PROBLEM.with(Clone::clone)
}

/// The words for a backend that is not this page's own server: a saved
/// setting the person may not remember making.
fn elsewhere(url: &str, page: &str) -> Option<String> {
    (url != page).then(|| format!("This browser is set to connect to {url}, not this page's server ({page}): a saved setting."))
}

/// What `/login/me` answered.
enum Gate {
    In(Me),
    Out,
    NoLogin,
    /// No answer to go by: asked again, never taken for "no login".
    Unsure(String),
}

/// What a `/login/me` status means, the body aside.
fn gate_of(status: u16) -> Option<bool> {
    match status {
        200 => Some(true),
        401 => Some(false),
        _ => None,
    }
}

async fn login_me() -> Gate {
    let promise = dom::window().fetch_with_str("/login/me");
    let Ok(resp) = JsFuture::from(promise).await else { return Gate::Unsure("/login/me unreachable".into()) };
    let Ok(resp) = resp.dyn_into::<web_sys::Response>() else { return Gate::Unsure("/login/me answered oddly".into()) };
    match (resp.status(), gate_of(resp.status())) {
        // This menger has no login: the shell serves no /login/me.
        (404, _) => Gate::NoLogin,
        (_, Some(false)) => Gate::Out,
        (_, Some(true)) => {
            let Ok(text) = resp.text() else { return Gate::Unsure("/login/me unreadable".into()) };
            match JsFuture::from(text).await.ok().and_then(|t| t.as_string()).and_then(|t| serde_json::from_str::<Me>(&t).ok()) {
                Some(me) => Gate::In(me),
                None => Gate::Unsure("/login/me unreadable".into()),
            }
        }
        (status, None) => Gate::Unsure(format!("/login/me answered {status}")),
    }
}

/// Before every connect: who this browser is. `Err`: signed out (the
/// browser is on its way to the login page) or no answer yet; the session
/// goes no further, and is tried again.
pub(crate) async fn gate() -> Result<(), String> {
    match login_me().await {
        Gate::In(me) => {
            account().update(|a| {
                a.login = Some(true);
                a.me = Some(me);
            });
            Ok(())
        }
        Gate::Out => {
            let loc = dom::window().location();
            let to = login_url(&loc.pathname().unwrap_or_default(), &loc.hash().unwrap_or_default());
            let _ = loc.replace(&to);
            Err("signed out".into())
        }
        Gate::NoLogin => {
            account().update(|a| a.login = Some(false));
            Ok(())
        }
        Gate::Unsure(why) => Err(why),
    }
}

/// After every connect: the person's grants, from the socket as the
/// socket knows them. `Ok(false)`: they hold none, and the session asks
/// nothing more of the server (the app shows [`NoAccess`]). `Err`: no
/// answer, or the socket is not the person `/login/me` named; the session
/// ends and is tried again.
pub(crate) async fn after_connect(transport: Rc<WsTransport>) -> Result<bool, String> {
    if account().with_untracked(|a| a.login) != Some(true) {
        return Ok(true);
    }
    let events = MembersClient::new(transport).mine_all().await.map_err(|e| format!("members.mine: {e}"))?;
    let expected = account().with_untracked(|a| a.me.as_ref().map(|m| m.principal.clone()));
    let mut grants = Vec::new();
    let mut everywhere = false;
    let mut socket_is = None;
    for ev in events {
        match ev {
            MembersEvent::Me { principal, admin_everywhere, .. } => {
                socket_is = principal;
                everywhere = admin_everywhere;
            }
            MembersEvent::Grant { grant } => grants.push(grant),
            MembersEvent::Error { message } => return Err(format!("members.mine: {message}")),
            _ => {}
        }
    }
    same_person(expected.as_deref(), socket_is.as_deref())?;
    account().update(|a| {
        a.grants = Some(grants);
        a.admin_everywhere = everywhere;
    });
    Ok(account().with_untracked(|a| a.may_view()))
}

/// The socket must be the person the page is signed in as: a socket that
/// proved someone else (another credential the browser holds) is refused.
fn same_person(page: Option<&str>, socket: Option<&str>) -> Result<(), String> {
    match (page, socket) {
        (Some(p), Some(s)) if p == s => Ok(()),
        (p, s) => Err(format!("the socket is {} but this page is signed in as {}", s.unwrap_or("nobody"), p.unwrap_or("nobody"))),
    }
}

/// Sign-out: the shell ends the session and shows the login page.
#[component]
pub(crate) fn SignOut() -> impl IntoView {
    view! {
        <form method="post" action="/logout" class="signout">
            <button type="submit" class="hbtn" id="signout">"Sign out"</button>
        </form>
    }
}

/// The header's person: their name and sign-out, when this menger has a
/// login.
#[component]
pub(crate) fn Who() -> impl IntoView {
    let account = account();
    move || {
        let a = account.get();
        let name = a.who()?;
        let title = a.me.as_ref().map(|m| format!("{} · via {}", m.principal, m.provider.as_deref().unwrap_or("?")));
        Some(view! {
            <a class="hlink signed-in" id="who" href="#/settings" title=title>{name}</a>
            <SignOut />
        })
    }
}

/// `#/settings`' account group.
#[component]
pub(crate) fn AccountGroup() -> impl IntoView {
    let account = account();
    move || {
        let a = account.get();
        let me = a.me.clone()?;
        let grants = a.grants.clone().unwrap_or_default();
        let rows = if a.admin_everywhere {
            view! { <li><code>"*"</code>" · admin (the operator's)"</li> }.into_any()
        } else if grants.is_empty() {
            view! { <li class="hint">"no grants yet: send an admin your id"</li> }.into_any()
        } else {
            grants
                .into_iter()
                .map(|g| view! { <li><code>{g.scope}</code>" · "{g.role.to_string()}</li> })
                .collect_view()
                .into_any()
        };
        Some(view! {
            <fieldset class="group" id="settings-account">
                <legend>"Account"</legend>
                <p class="now" id="account-now">
                    "Signed in as " <strong id="account-name">{me.name.clone()}</strong>
                    {me.provider.clone().map(|p| format!(" via {p}"))}
                </p>
                <div class="field">
                    <label>"Your id"</label>
                    <div class="control"><code id="account-principal">{me.principal.clone()}</code></div>
                </div>
                <div class="field">
                    <label>"Roles"</label>
                    <div class="control"><ul class="grants">{rows}</ul></div>
                </div>
                <div class="line actions"><SignOut /></div>
            </fieldset>
        })
    }
}

/// IDENTITY-31: instead of the views, until the server has said who this
/// is and what they hold. After a few seconds it says what it is waiting
/// on — the socket, the last failure, and a saved backend that is not this
/// page's server (with the one click that drops it) — never a silent hang.
#[component]
pub(crate) fn Checking() -> impl IntoView {
    let late = RwSignal::new(false);
    wasm_bindgen_futures::spawn_local(async move {
        dom::sleep_ms(3000).await;
        let _ = late.try_set(true);
    });
    let problem = problem();
    let b = crate::settings::backend();
    let moved = elsewhere(&b.url, &b.page);
    view! {
        <section id="checking">
            <p class="hint">"Checking your access…"</p>
            {move || late.get().then(|| {
                let moved = moved.clone();
                view! {
                    <div id="checking-detail">
                        <p class="hint">"Still waiting on " <code>{b.url.clone()}</code></p>
                        {problem.get().map(|p| view! { <p class="invalid" id="checking-problem">{p}</p> })}
                        {moved.map(|m| view! {
                            <p id="checking-elsewhere">{m}" "
                                <button class="hbtn" id="checking-reset" on:click=|_| crate::settings::use_page_server()>"Use this page's server"</button></p>
                        })}
                        <p class="hint">"Signing out and in again starts afresh: "<SignOut /></p>
                    </div>
                }
            })}
        </section>
    }
}

/// Instead of the views, for a person holding no grant: their id, and
/// what to do with it.
#[component]
pub(crate) fn NoAccess() -> impl IntoView {
    let account = account();
    let copied = RwSignal::new(false);
    move || {
        let me = account.get().me?;
        let principal = me.principal.clone();
        let copy = move |_| {
            let clip = dom::window().navigator().clipboard();
            let _ = clip.write_text(&principal);
            copied.set(true);
        };
        Some(view! {
            <section id="no-access">
                <h2>"No access yet"</h2>
                <p>"You are signed in as " <strong>{me.name.clone()}</strong>", but you hold no role in this menger."</p>
                <p>"Send an admin your id, and they can grant you one:"</p>
                <p class="id"><code id="no-access-id">{me.principal.clone()}</code>" "
                    <button class="hbtn" on:click=copy>{move || if copied.get() { "copied" } else { "copy" }}</button></p>
                <p class="hint">"Once you have a role, reload this page."</p>
                <SignOut />
            </section>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Me {
        Me { principal: "idp:c952acd3".into(), name: "alice".into(), provider: Some("plexus-idp".into()) }
    }

    fn grant(scope: &str, role: Role) -> Grant {
        Grant { principal: "idp:c952acd3".into(), scope: scope.into(), role, granted_by: "ada".into(), at: 1 }
    }

    #[test]
    fn a_signed_out_browser_goes_to_the_login_page_and_back() {
        assert_eq!(login_url("/app/", "#/ticket/X-1"), "/login?next=%2Fapp%2F%23%2Fticket%2FX-1");
        assert_eq!(login_url("", ""), "/login?next=%2F");
    }

    #[test]
    fn the_views_show_without_login_or_with_a_grant() {
        let no_login = Account { login: Some(false), ..Default::default() };
        assert!(no_login.may_view() && no_login.known(), "no login: as always");
        let waiting = Account { login: Some(true), me: Some(me()), grants: Some(vec![]), admin_everywhere: false };
        assert!(!waiting.may_view() && waiting.no_access(), "signed in, no grant: no views, the id instead");
        let member = Account { grants: Some(vec![grant("OUGHT", Role::Viewer)]), ..waiting.clone() };
        assert!(member.may_view() && !member.no_access());
        let operator = Account { admin_everywhere: true, ..waiting };
        assert!(operator.may_view() && operator.is_admin());
    }

    #[test]
    fn what_is_not_known_is_not_shown() {
        // IDENTITY-31: before /login/me answers, and before the grants do.
        let before = Account::default();
        assert!(!before.known() && !before.may_view() && !before.no_access(), "neither the views nor no-access");
        let signed_in = Account { login: Some(true), me: Some(me()), ..Default::default() };
        assert!(!signed_in.known() && !signed_in.may_view() && !signed_in.no_access(), "the grants not yet in");
    }

    #[test]
    fn login_me_s_answers_mean_what_they_say() {
        assert_eq!(gate_of(200), Some(true));
        assert_eq!(gate_of(401), Some(false));
        for odd in [0, 500, 502, 503] {
            assert_eq!(gate_of(odd), None, "{odd}: asked again, never taken for no login");
        }
    }

    #[test]
    fn a_saved_backend_elsewhere_is_said() {
        assert_eq!(elsewhere("ws://127.0.0.1:4450", "ws://127.0.0.1:4450"), None);
        let said = elsewhere("ws://localhost:4450", "ws://127.0.0.1:4450").unwrap();
        assert!(said.contains("ws://localhost:4450") && said.contains("this page's server (ws://127.0.0.1:4450)"), "{said}");
    }

    #[test]
    fn the_socket_must_be_the_page_s_person() {
        assert!(same_person(Some("oidc:workos/user_1"), Some("oidc:workos/user_1")).is_ok());
        assert!(same_person(Some("oidc:workos/user_1"), Some("idp:3fd4717e-a0cf-44fd-bf1d-a144d5b0829d")).is_err());
        assert!(same_person(Some("oidc:workos/user_1"), None).is_err(), "an anonymous socket");
    }

    #[test]
    fn an_admin_is_admin_somewhere() {
        let a = Account { login: Some(true), me: Some(me()), grants: Some(vec![grant("OUGHT", Role::Editor)]), admin_everywhere: false };
        assert!(!a.is_admin());
        let b = Account { grants: Some(vec![grant("IDENTITY", Role::Admin)]), ..a };
        assert!(b.is_admin());
    }

    #[test]
    fn the_header_names_the_person() {
        let a = Account { login: Some(true), me: Some(me()), ..Default::default() };
        assert_eq!(a.who().as_deref(), Some("alice"));
        let npub = Account {
            me: Some(Me { name: "npub1luxqwxf06dxwt6y74d44pxfyxe2g87mr3lwcqucaudu2vder3evs326y9y".into(), ..me() }),
            ..a
        };
        assert_eq!(npub.who().as_deref(), Some("npub1luxqwxf…6y9y"));
        assert_eq!(Account::default().who(), None);
    }
}
