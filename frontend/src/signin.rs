//! IDENTITY-15: signing in — the app's side of IDENTITY-14's boundary.
//!
//! menger admits callers through the providers it was configured with
//! (`session.providers`), and past the WebSocket upgrade it sees only a
//! Plexus identity (`session.whoami`). A browser cannot set headers on a
//! WebSocket, so the credential rides a cookie on this page's host, which
//! the browser sends with the upgrade:
//!
//! | provider kind | how the app gets the credential | cookie |
//! |---|---|---|
//! | `oidc` | the provider's token endpoint, password grant (from its discovery) | `access_token`, for the token's lifetime |
//! | `nostr` | a NIP-07 extension signs a NIP-98 event for the WebSocket URL, before every connect | `nostr`, for a minute |
//!
//! The app talks to the provider directly; menger never sees a password.
//! Signing in or out reloads the app, so a fresh session connects with the
//! new credential, as Save in `#/settings` does for a new backend. The
//! identity is the server's word: the header shows `whoami`'s answer.
//!
//! What decides — the cookies, the form, the event, reading the token
//! response, the header's words — is pure and pinned by native tests.

use std::rc::Rc;

use leptos::prelude::*;
use menger_tickets_api::{ProviderKind, SessionClient, SessionEvent};
use plexus_client::WsTransport;
use serde_json::{json, Value};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::dom;

/// localStorage: `nostr` when this browser signs in with its nostr
/// extension. A token sign-in needs no flag: its cookie is the state.
const MODE_KEY: &str = "menger.signin";

/// How long a NIP-98 event's cookie lives: the server's own window.
const NOSTR_COOKIE_SECS: u64 = 60;

/// A token's cookie lifetime when the provider does not say.
const DEFAULT_TOKEN_SECS: u64 = 3600;

// ── What decides (pure) ────────────────────────────────────────────────

/// A cookie for this page's host, sent with the WebSocket upgrade.
pub(crate) fn cookie(name: &str, value: &str, max_age_secs: u64) -> String {
    format!("{name}={value}; Path=/; Max-Age={max_age_secs}; SameSite=Strict")
}

/// The same cookie, removed.
pub(crate) fn cleared(name: &str) -> String {
    format!("{name}=; Path=/; Max-Age=0; SameSite=Strict")
}

/// `application/x-www-form-urlencoded`, as a form would send it.
pub(crate) fn form(pairs: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    };
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

/// The password grant's body, as plexus-idp and Auth0 take it.
pub(crate) fn password_grant(username: &str, password: &str, audience: Option<&str>) -> String {
    let mut pairs = vec![("grant_type", "password"), ("username", username), ("password", password)];
    if let Some(a) = audience {
        pairs.push(("audience", a));
    }
    form(&pairs)
}

/// The discovery document's URL under an issuer.
pub(crate) fn discovery_url(issuer: &str) -> String {
    format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'))
}

/// A token endpoint's answer: the access token and its lifetime, or the
/// provider's own words for why not.
pub(crate) fn token_from(body: &str) -> Result<(String, u64), String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "the provider's answer was not JSON".to_string())?;
    if let Some(t) = v.get("access_token").and_then(Value::as_str) {
        let secs = v.get("expires_in").and_then(Value::as_u64).unwrap_or(DEFAULT_TOKEN_SECS);
        return Ok((t.to_string(), secs));
    }
    let said = ["error_description", "error"].iter().find_map(|k| v.get(*k).and_then(Value::as_str));
    Err(said.unwrap_or("the provider gave no token").to_string())
}

/// The unsigned NIP-98 event a connect to `url` carries (kind 27235).
pub(crate) fn nip98_template(url: &str, now_secs: u64) -> Value {
    json!({ "kind": 27235, "created_at": now_secs, "content": "", "tags": [["u", url], ["method", "GET"]] })
}

/// The header's words for who this connection is: the name, or nothing
/// when anonymous (`sign in` when there is somewhere to sign in).
pub(crate) fn who_words(who: Option<&SessionEvent>, offered: usize) -> Option<String> {
    match who {
        Some(SessionEvent::Identity { name, .. }) => Some(short(name)),
        _ if offered > 0 => Some("sign in".to_string()),
        _ => None,
    }
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
    /// What `whoami` said on the current connection.
    static WHO: ArcRwSignal<Option<SessionEvent>> = ArcRwSignal::new(None);
    /// What `providers` said: the providers this menger offers.
    static OFFERED: ArcRwSignal<Vec<SessionEvent>> = ArcRwSignal::new(Vec::new());
}

fn who() -> ArcRwSignal<Option<SessionEvent>> {
    WHO.with(Clone::clone)
}

fn offered() -> ArcRwSignal<Vec<SessionEvent>> {
    OFFERED.with(Clone::clone)
}

fn document() -> Option<web_sys::HtmlDocument> {
    dom::window().document()?.dyn_into::<web_sys::HtmlDocument>().ok()
}

fn set_cookie(value: &str) {
    if let Some(d) = document() {
        let _ = d.set_cookie(value);
    }
}

fn mode() -> Option<String> {
    dom::window().local_storage().ok().flatten()?.get_item(MODE_KEY).ok().flatten()
}

fn set_mode(value: Option<&str>) {
    if let Some(s) = dom::window().local_storage().ok().flatten() {
        let _ = match value {
            Some(v) => s.set_item(MODE_KEY, v),
            None => s.remove_item(MODE_KEY),
        };
    }
}

/// The NIP-07 extension (`window.nostr`), when one is installed.
fn extension() -> Option<JsValue> {
    let n = js_sys::Reflect::get(&dom::window(), &JsValue::from_str("nostr")).ok()?;
    (!n.is_undefined() && !n.is_null()).then_some(n)
}

/// Asks the extension to sign `event`; the signed event as JSON.
async fn sign(event: &Value) -> Result<String, String> {
    let ext = extension().ok_or("no nostr extension (NIP-07) in this browser")?;
    let f: js_sys::Function = js_sys::Reflect::get(&ext, &JsValue::from_str("signEvent"))
        .ok()
        .and_then(|f| f.dyn_into().ok())
        .ok_or("the nostr extension has no signEvent")?;
    let template = js_sys::JSON::parse(&event.to_string()).map_err(|_| "event template".to_string())?;
    let promise: js_sys::Promise = f
        .call1(&ext, &template)
        .map_err(|_| "the extension refused".to_string())?
        .dyn_into()
        .map_err(|_| "signEvent returned no promise".to_string())?;
    let signed = JsFuture::from(promise).await.map_err(|_| "the extension did not sign".to_string())?;
    js_sys::JSON::stringify(&signed).map(String::from).map_err(|_| "the signed event".to_string())
}

/// Before every connect: a browser signed in with nostr signs a fresh
/// NIP-98 event for `url` and puts it in the `nostr` cookie (each event
/// works once). Anything that fails leaves the connection anonymous.
pub(crate) async fn before_connect(url: &str) {
    if mode().as_deref() != Some("nostr") {
        return;
    }
    let now = (js_sys::Date::now() / 1000.0) as u64;
    match sign(&nip98_template(url, now)).await {
        Ok(json) => match dom::window().btoa(&json) {
            Ok(b64) => set_cookie(&cookie("nostr", &b64, NOSTR_COOKIE_SECS)),
            Err(_) => leptos::logging::log!("sign-in: could not encode the nostr event"),
        },
        Err(e) => leptos::logging::log!("sign-in: {e}; connecting anonymous"),
    }
}

/// After every connect: who the connection is, and what menger offers.
pub(crate) fn after_connect(transport: Rc<WsTransport>) {
    spawn_local(async move {
        let client = SessionClient::new(transport);
        let me = client.whoami_all().await.ok().and_then(|evs| evs.into_iter().next());
        let offers = client.providers_all().await.unwrap_or_default();
        who().set(me);
        offered().set(offers);
    });
}

/// The header's `who` — the identity's name, linking to the sign-in.
#[component]
pub(crate) fn Who() -> impl IntoView {
    let (who, offered) = (who(), offered());
    move || {
        let words = who.with(|w| offered.with(|o| who_words(w.as_ref(), o.len())))?;
        let title = who.with(|w| match w {
            Some(SessionEvent::Identity { principal, provider, .. }) => {
                format!("{principal} · via {} (#/settings)", provider.as_deref().unwrap_or("?"))
            }
            _ => "not signed in — sign in from #/settings".to_string(),
        });
        let signed_in = who.with(|w| matches!(w, Some(SessionEvent::Identity { .. })));
        Some(view! {
            <a class=if signed_in { "hlink signed-in" } else { "hlink" } id="who" href="#/settings" title=title>{words}</a>
        })
    }
}

/// Fetches `url` (`POST` with `body` when given) and reads the answer as
/// text, whatever its status: a provider's refusal is JSON naming why.
async fn fetch(url: &str, body: Option<&str>) -> Result<String, String> {
    let init = web_sys::RequestInit::new();
    if let Some(b) = body {
        init.set_method("POST");
        init.set_body(&JsValue::from_str(b));
        let headers = web_sys::Headers::new().map_err(|_| "headers".to_string())?;
        let _ = headers.set("content-type", "application/x-www-form-urlencoded");
        init.set_headers(&headers);
    }
    let resp: web_sys::Response = JsFuture::from(dom::window().fetch_with_str_and_init(url, &init))
        .await
        .map_err(|_| format!("cannot reach {url}"))?
        .dyn_into()
        .map_err(|_| "no response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|_| "unreadable answer".to_string())?)
        .await
        .map_err(|_| "unreadable answer".to_string())?;
    Ok(text.as_string().unwrap_or_default())
}

/// The password grant at an OIDC provider: discovery, then its token
/// endpoint. The access token, or why not.
async fn password_sign_in(issuer: &str, audience: Option<&str>, user: &str, pass: &str) -> Result<(String, u64), String> {
    let doc = fetch(&discovery_url(issuer), None).await?;
    let endpoint = serde_json::from_str::<Value>(&doc)
        .ok()
        .and_then(|d| d.get("token_endpoint").and_then(Value::as_str).map(str::to_string))
        .ok_or("the provider's discovery names no token endpoint")?;
    let answer = fetch(&endpoint, Some(&password_grant(user, pass, audience))).await?;
    token_from(&answer)
}

/// Starts the app again, so a fresh session connects with the credential.
fn reload() {
    let _ = dom::window().location().reload();
}

fn sign_out() {
    set_cookie(&cleared("access_token"));
    set_cookie(&cleared("nostr"));
    set_mode(None);
    reload();
}

/// `#/settings`' sign-in group: who this connection is, and a way in
/// through each provider menger offers.
#[component]
pub(crate) fn SignInGroup() -> impl IntoView {
    let (who, offered) = (who(), offered());
    let has_extension = extension().is_some();
    let user = RwSignal::new(String::new());
    let pass = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let now = {
        let who = who.clone();
        move || match who.get() {
            Some(SessionEvent::Identity { name, principal, provider, roles, .. }) => view! {
                <p class="now" id="signin-now">
                    "Signed in as " <strong id="signin-name">{name}</strong> " — " <code>{principal}</code>
                    {format!(" via {}", provider.unwrap_or_default())}
                    {(!roles.is_empty()).then(|| format!(" · roles {}", roles.join(", ")))}
                    " "
                    <button id="signin-out" class="link" on:click=move |_| sign_out()>"sign out"</button>
                </p>
            }
            .into_any(),
            _ => view! { <p class="now" id="signin-now">"Not signed in: this connection is anonymous."</p> }.into_any(),
        }
    };

    let offered_rows = offered.clone();
    let rows = move || {
        offered_rows
            .get()
            .into_iter()
            .filter_map(|o| match o {
                SessionEvent::Provider { name, kind, issuer, audience } => Some((name, kind, issuer, audience)),
                _ => None,
            })
            .map(|(name, kind, issuer, audience)| match kind {
                ProviderKind::Oidc => {
                    let id = format!("signin-{name}");
                    let go = move |_| {
                        let (Some(issuer), u, p) = (issuer.clone(), user.get_untracked(), pass.get_untracked()) else {
                            return;
                        };
                        let audience = audience.clone();
                        busy.set(true);
                        problem.set(None);
                        spawn_local(async move {
                            match password_sign_in(&issuer, audience.as_deref(), &u, &p).await {
                                Ok((token, secs)) => {
                                    set_mode(None);
                                    set_cookie(&cleared("nostr"));
                                    set_cookie(&cookie("access_token", &token, secs));
                                    reload();
                                }
                                Err(e) => {
                                    busy.set(false);
                                    problem.set(Some(e));
                                }
                            }
                        });
                    };
                    view! {
                        <li>
                            <button id=id class="primary" disabled=move || busy.get() on:click=go>
                                {format!("Sign in with {name}")}
                            </button>
                            <span class="lbl">" the username and password above"</span>
                        </li>
                    }
                    .into_any()
                }
                ProviderKind::Nostr => view! {
                    <li>
                        <button
                            id="signin-nostr"
                            class="primary"
                            disabled=!has_extension
                            title=if has_extension { "your nostr extension signs each connection" } else { "no nostr extension (NIP-07) in this browser" }
                            on:click=move |_| {
                                set_cookie(&cleared("access_token"));
                                set_mode(Some("nostr"));
                                reload();
                            }
                        >
                            "Sign in with nostr"
                        </button>
                        <span class="lbl">
                            {if has_extension { " your extension signs each connection" } else { " needs a NIP-07 extension" }}
                        </span>
                    </li>
                }
                .into_any(),
            })
            .collect_view()
    };

    let any_oidc = {
        let offered = offered.clone();
        move || offered.with(|o| o.iter().any(|e| matches!(e, SessionEvent::Provider { kind: ProviderKind::Oidc, .. })))
    };

    view! {
        <fieldset class="group" id="settings-signin">
            <legend>"Sign in"</legend>
            {now}
            {move || {
                if offered.with(|o| o.is_empty()) {
                    return view! { <p class="hint" id="signin-none">"This menger offers no sign-in: it was started without --auth-provider."</p> }.into_any();
                }
                let (any_oidc, rows) = (any_oidc.clone(), rows.clone());
                view! {
                    <div class="field">
                        <label for="signin-user">"Sign in with"</label>
                        <div class="control">
                            {move || any_oidc().then(|| view! {
                                <div class="line">
                                    <input id="signin-user" type="text" autocomplete="username" placeholder="username"
                                        prop:value=move || user.get() on:input=move |ev| user.set(event_target_value(&ev)) />
                                    <input id="signin-pass" type="password" autocomplete="current-password" placeholder="password"
                                        prop:value=move || pass.get() on:input=move |ev| pass.set(event_target_value(&ev)) />
                                </div>
                            })}
                            <ul class="signin-providers">{rows}</ul>
                            {move || problem.get().map(|p| view! { <p class="invalid" id="signin-problem" role="alert">{p}</p> })}
                            <p class="status hint">"The app asks the provider itself; menger never sees a password. The credential is a cookie on this page's host."</p>
                        </div>
                    </div>
                }
                .into_any()
            }}
        </fieldset>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_ride_this_host_and_can_be_removed() {
        assert_eq!(cookie("access_token", "eyJ.x.y", 3600), "access_token=eyJ.x.y; Path=/; Max-Age=3600; SameSite=Strict");
        assert_eq!(cleared("nostr"), "nostr=; Path=/; Max-Age=0; SameSite=Strict");
    }

    #[test]
    fn the_password_grant_is_a_form() {
        assert_eq!(
            password_grant("ada", "correct horse&=", Some("https://menger.example")),
            "grant_type=password&username=ada&password=correct+horse%26%3D&audience=https%3A%2F%2Fmenger.example"
        );
        assert_eq!(password_grant("a", "b", None), "grant_type=password&username=a&password=b");
    }

    #[test]
    fn discovery_is_under_the_issuer_with_or_without_its_slash() {
        assert_eq!(discovery_url("https://acme.auth0.com/"), "https://acme.auth0.com/.well-known/openid-configuration");
        assert_eq!(discovery_url("http://127.0.0.1:4481"), "http://127.0.0.1:4481/.well-known/openid-configuration");
    }

    #[test]
    fn a_token_answer_gives_the_token_or_the_provider_s_words() {
        assert_eq!(token_from(r#"{"access_token":"t","expires_in":900}"#), Ok(("t".into(), 900)));
        assert_eq!(token_from(r#"{"access_token":"t"}"#), Ok(("t".into(), DEFAULT_TOKEN_SECS)));
        assert_eq!(token_from(r#"{"error":"invalid_grant","error_description":"Wrong email or password."}"#), Err("Wrong email or password.".into()));
        assert_eq!(token_from(r#"{"error":"invalid_grant"}"#), Err("invalid_grant".into()));
        assert!(token_from("<html>").is_err());
    }

    #[test]
    fn the_nostr_event_names_the_websocket_and_get() {
        let e = nip98_template("ws://127.0.0.1:4455", 1_791_529_096);
        assert_eq!(e["kind"], 27235);
        assert_eq!(e["tags"], json!([["u", "ws://127.0.0.1:4455"], ["method", "GET"]]));
        assert_eq!(e["content"], "");
    }

    #[test]
    fn the_header_names_the_identity_or_offers_a_sign_in() {
        let alice = SessionEvent::Identity {
            principal: "idp:c952acd3".into(),
            name: "alice".into(),
            roles: vec![],
            tenant: None,
            provider: Some("plexus-idp".into()),
        };
        assert_eq!(who_words(Some(&alice), 2), Some("alice".into()));
        assert_eq!(who_words(Some(&SessionEvent::Anonymous), 2), Some("sign in".into()));
        assert_eq!(who_words(Some(&SessionEvent::Anonymous), 0), None);
        assert_eq!(who_words(None, 0), None);
        let npub = SessionEvent::Identity {
            principal: "nostr:ff".into(),
            name: "npub1luxqwxf06dxwt6y74d44pxfyxe2g87mr3lwcqucaudu2vder3evs326y9y".into(),
            roles: vec![],
            tenant: None,
            provider: Some("nostr".into()),
        };
        assert_eq!(who_words(Some(&npub), 1), Some("npub1luxqwxf…6y9y".into()));
    }
}
