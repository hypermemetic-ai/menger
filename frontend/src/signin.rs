//! IDENTITY-22: the signed-in app — who this browser is, and what it may do.
//!
//! The browser holds no credential. menger's shell signed the person in
//! (`/login`, IDENTITY-19) and keeps the session; the browser carries an
//! `HttpOnly` cookie that no script here can read. So the app only *asks*:
//!
//! 1. Before every connect, `GET /login/me` ([`gate`]): 401 sends the
//!    browser to `/login?next=…` and the app goes no further; 404 means this
//!    menger has no login, and the app runs as it always did.
//! 2. After every connect, `members.mine` ([`after_connect`]): the person's
//!    grants. With none, the app shows [`NoAccess`] — their id, to send an
//!    admin — instead of the views.
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
    /// Whether this menger has a login at all.
    pub(crate) login: bool,
    pub(crate) me: Option<Me>,
    /// The person's grants, once `members.mine` has answered.
    pub(crate) grants: Option<Vec<Grant>>,
    pub(crate) admin_everywhere: bool,
}

impl Account {
    /// Whether the views may show: no login, or a person holding a grant
    /// (or not yet known — the views wait for the server's own refusals).
    pub(crate) fn may_view(&self) -> bool {
        !self.login || self.admin_everywhere || self.grants.as_ref().is_none_or(|g| !g.is_empty())
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
}

pub(crate) fn account() -> ArcRwSignal<Account> {
    ACCOUNT.with(Clone::clone)
}

/// What `/login/me` answered.
enum Gate {
    In(Me),
    Out,
    NoLogin,
}

async fn login_me() -> Gate {
    let promise = dom::window().fetch_with_str("/login/me");
    let Ok(resp) = JsFuture::from(promise).await else { return Gate::NoLogin };
    let Ok(resp) = resp.dyn_into::<web_sys::Response>() else { return Gate::NoLogin };
    match resp.status() {
        401 => Gate::Out,
        200 => {
            let Ok(text) = resp.text() else { return Gate::NoLogin };
            match JsFuture::from(text).await.ok().and_then(|t| t.as_string()).and_then(|t| serde_json::from_str::<Me>(&t).ok()) {
                Some(me) => Gate::In(me),
                None => Gate::NoLogin,
            }
        }
        _ => Gate::NoLogin,
    }
}

/// Before every connect: who this browser is. `false`: signed out — the
/// browser is on its way to the login page, and the app goes no further.
pub(crate) async fn gate() -> bool {
    match login_me().await {
        Gate::In(me) => {
            account().update(|a| {
                a.login = true;
                a.me = Some(me);
            });
            true
        }
        Gate::Out => {
            let loc = dom::window().location();
            let to = login_url(&loc.pathname().unwrap_or_default(), &loc.hash().unwrap_or_default());
            let _ = loc.replace(&to);
            false
        }
        Gate::NoLogin => {
            account().update(|a| a.login = false);
            true
        }
    }
}

/// After every connect: the person's grants. `false`: they hold none, and
/// the session asks nothing more of the server (the app shows
/// [`NoAccess`]).
pub(crate) async fn after_connect(transport: Rc<WsTransport>) -> bool {
    if !account().with_untracked(|a| a.login) {
        return true;
    }
    let Ok(events) = MembersClient::new(transport).mine_all().await else { return true };
    let mut grants = Vec::new();
    let mut everywhere = false;
    for ev in events {
        match ev {
            MembersEvent::Me { admin_everywhere, .. } => everywhere = admin_everywhere,
            MembersEvent::Grant { grant } => grants.push(grant),
            _ => {}
        }
    }
    account().update(|a| {
        a.grants = Some(grants);
        a.admin_everywhere = everywhere;
    });
    account().with_untracked(|a| a.may_view())
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
        assert!(Account::default().may_view(), "no login: as always");
        let waiting = Account { login: true, me: Some(me()), grants: Some(vec![]), admin_everywhere: false };
        assert!(!waiting.may_view(), "signed in, no grant: no views");
        let unknown = Account { grants: None, ..waiting.clone() };
        assert!(unknown.may_view(), "until the grants are known, the server's refusals speak");
        let member = Account { grants: Some(vec![grant("OUGHT", Role::Viewer)]), ..waiting.clone() };
        assert!(member.may_view());
        let operator = Account { admin_everywhere: true, ..waiting };
        assert!(operator.may_view() && operator.is_admin());
    }

    #[test]
    fn an_admin_is_admin_somewhere() {
        let a = Account { login: true, me: Some(me()), grants: Some(vec![grant("OUGHT", Role::Editor)]), admin_everywhere: false };
        assert!(!a.is_admin());
        let b = Account { grants: Some(vec![grant("IDENTITY", Role::Admin)]), ..a };
        assert!(b.is_admin());
    }

    #[test]
    fn the_header_names_the_person() {
        let a = Account { login: true, me: Some(me()), ..Default::default() };
        assert_eq!(a.who().as_deref(), Some("alice"));
        let npub = Account {
            me: Some(Me { name: "npub1luxqwxf06dxwt6y74d44pxfyxe2g87mr3lwcqucaudu2vder3evs326y9y".into(), ..me() }),
            ..a
        };
        assert_eq!(npub.who().as_deref(), Some("npub1luxqwxf…6y9y"));
        assert_eq!(Account::default().who(), None);
    }
}
