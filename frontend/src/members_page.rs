//! IDENTITY-22: `#/members` — who holds which role, and the way an admin
//! changes it.
//!
//! Every part is from the `members` activation over the app's own socket,
//! and every change asks the server, which checks it (IDENTITY-21), then the
//! page reads again.
//!
//! CHAT-14: each person is named as the server knows them — an agent's key
//! by the name it signs in with (`builder`), marked *agent* — and has
//! **Chat**: the direct message with them, opened in `#/chats`.
//!
//! APP-113: people first. Someone **waiting** is a card with what their
//! sign-in told the server (IDENTITY-46's `members.profiles`: email, how they
//! signed in, when) and one step to let them in — a role and where, the
//! organization by default. Everyone holding a grant is **one row** —
//! people, then agents, then teams — their grants as chips (× asks once,
//! then revokes; the operator's `--admin` has none). A grant to everyone
//! signed in (ACCESS-57) is said in a sentence. Adding someone by an id is
//! folded away. Below 600 px the rows stack and nothing scrolls sideways.

use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::members::{Profile, Scope};
use menger_tickets_api::vocab::{GrantedBy, Grantee};
use menger_tickets_api::{Grant, MembersClient, MembersEvent, Role};
use wasm_bindgen_futures::spawn_local;

use crate::session;
use crate::signin;

#[derive(Clone, Debug, Default, PartialEq)]
struct Board {
    waiting: Vec<Waiting>,
    grants: Vec<Grant>,
    /// IDENTITY-46: by principal — empty for an admin of less than `*`.
    profiles: HashMap<String, Profile>,
    problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Waiting {
    principal: String,
    name: Option<String>,
    provider: Option<String>,
    since: i64,
}

async fn read() -> Board {
    let Some(client) = session::members_client() else {
        return Board { problem: Some("not connected".into()), ..Default::default() };
    };
    let mut b = Board::default();
    match client.requests_all().await {
        Ok(evs) => {
            for ev in evs {
                match ev {
                    MembersEvent::Waiting { principal, name, provider, since } => b.waiting.push(Waiting { principal, name, provider, since }),
                    MembersEvent::Error { message } => b.problem = Some(message),
                    _ => {}
                }
            }
        }
        Err(e) => b.problem = Some(session::describe(&e)),
    }
    if let Ok(evs) = client.list_all(None).await {
        for ev in evs {
            if let MembersEvent::Grant { grant } = ev {
                b.grants.push(grant);
            }
        }
    }
    // Admin of `*` only; anyone else is refused in band and goes without.
    if let Ok(evs) = client.profiles().list_all().await {
        for ev in evs {
            if let MembersEvent::Profile { profile } = ev {
                b.profiles.insert(profile.principal.clone(), profile);
            }
        }
    }
    b
}

/// The answer to a change: `Ok(())`, or the server's words.
fn outcome(evs: Result<Vec<MembersEvent>, plexus_client::ClientError>) -> Result<(), String> {
    match evs {
        Ok(evs) => match evs.into_iter().find_map(|e| match e {
            MembersEvent::Error { message } => Some(message),
            _ => None,
        }) {
            Some(m) => Err(m),
            None => Ok(()),
        },
        Err(e) => Err(session::describe(&e)),
    }
}

async fn grant(client: MembersClient<std::rc::Rc<plexus_client::WsTransport>>, principal: String, scope: String, role: String) -> Result<(), String> {
    let typed = || -> Result<_, String> { Ok((crate::typed::typed(&principal)?, crate::typed::typed(&scope)?, crate::typed::typed(&role)?)) };
    let (principal, scope, role) = typed()?;
    outcome(client.grant_all(Some(principal), None, None, scope, role).await)
}

async fn revoke(client: MembersClient<std::rc::Rc<plexus_client::WsTransport>>, who: Grantee, scope: Scope) -> Result<(), String> {
    // ACCESS-22: a grant is to a principal or a team; ACCESS-57: or to everyone signed in.
    let (principal, team, everyone) = match who {
        Grantee::Principal(p) => (Some(p), None, None),
        Grantee::Team(t) => (None, Some(t), None),
        Grantee::Everyone => (None, None, Some(true)),
    };
    let scope = crate::typed::typed(&scope.to_string())?;
    outcome(client.revoke_all(principal, team, everyone, scope).await)
}

/// One grantee and every grant it holds, in the order the server listed them.
#[derive(Clone, Debug, PartialEq)]
struct Holder {
    who: Grantee,
    grants: Vec<Grant>,
}

/// The rows: the grants gathered per grantee — people, agents and teams;
/// a grant to everyone signed in is apart.
#[derive(Clone, Debug, Default, PartialEq)]
struct Rows {
    people: Vec<Holder>,
    agents: Vec<Holder>,
    teams: Vec<Holder>,
    everyone: Vec<Grant>,
}

fn rows(grants: &[Grant], is_agent: impl Fn(&str) -> bool) -> Rows {
    let mut out = Rows::default();
    for g in grants {
        let list = match &g.principal {
            Grantee::Everyone => {
                out.everyone.push(g.clone());
                continue;
            }
            Grantee::Team(_) => &mut out.teams,
            Grantee::Principal(p) if is_agent(p.as_str()) => &mut out.agents,
            Grantee::Principal(_) => &mut out.people,
        };
        match list.iter_mut().find(|h| h.who == g.principal) {
            Some(h) => h.grants.push(g.clone()),
            None => list.push(Holder { who: g.principal.clone(), grants: vec![g.clone()] }),
        }
    }
    out
}

/// A role as a chip says it: `Editor`.
fn role_word(role: &Role) -> String {
    let r = role.to_string();
    let mut c = r.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Where a grant reaches, in words: `everything`, `all of hypermemetic`,
/// `MENGER`, `under MENGER-12`, `MENGER-12`.
fn scope_words(scope: &Scope) -> String {
    match scope {
        Scope::Everywhere => "everything".into(),
        Scope::Org(org) => format!("all of {org}"),
        Scope::Shortcode { id, .. } => id.to_string(),
        Scope::Under(key) => format!("under {key}"),
        Scope::Ticket(key) => key.to_string(),
    }
}

/// How someone signed in, as a person says it: `GitHub`, `an email code`,
/// `a key` — the provider's method first, else the provider.
fn signed_in_with(method: Option<&str>, provider: Option<&str>) -> Option<String> {
    let said = |m: &str| -> Option<&'static str> {
        Some(match m {
            "githuboauth" => "GitHub",
            "googleoauth" => "Google",
            "microsoftoauth" => "Microsoft",
            "appleoauth" => "Apple",
            "gitlaboauth" => "GitLab",
            "email_code" | "magic_auth" | "magic-auth" => "an email code",
            "password" => "a password",
            "passkey" => "a passkey",
            "sso" => "single sign-on",
            "key" => "a key",
            "nip98" | "nostr" => "Nostr",
            "workos" => "WorkOS",
            "plexus-idp" | "idp" => "plexus-idp",
            _ => return None,
        })
    };
    let m = method.filter(|m| !m.is_empty());
    let p = provider.filter(|p| !p.is_empty());
    m.and_then(said).or_else(|| p.and_then(said)).map(str::to_string).or_else(|| m.or(p).map(str::to_string))
}

/// A waiting card's or a row's second line: `ada@example.com · signed in
/// with GitHub · 12m ago`, from what is known (`✓` is drawn by the view).
fn about(profile: Option<&Profile>, provider: Option<&str>, at: Option<i64>, now: i64) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(email) = profile.and_then(|p| p.email.clone()).filter(|e| !e.is_empty()) {
        parts.push(email);
    }
    let how = signed_in_with(profile.and_then(|p| p.method.as_deref()), profile.and_then(|p| p.provider.as_deref()).or(provider));
    let when = profile.and_then(|p| p.signed_in_at).or(at).map(|t| crate::fmt_relative(now, t));
    match (how, when) {
        (Some(h), Some(w)) => parts.push(format!("signed in with {h} · {w}")),
        (Some(h), None) => parts.push(format!("signed in with {h}")),
        (None, Some(w)) => parts.push(format!("signed in {w}")),
        (None, None) => {}
    }
    parts
}

/// A principal's display name: the profile's, else the name the server
/// knows, else the principal shortened.
fn display_name(principal: &str, profile: Option<&Profile>, known: &str) -> String {
    profile.and_then(|p| p.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
        if known.is_empty() || known == principal {
            crate::chats::short(principal)
        } else {
            known.to_string()
        }
    })
}

/// A key shown short — its scheme, the head and the tail of its id
/// (`key:54a9…0d28`); anything short as it is.
fn short_key(principal: &str) -> String {
    match principal.split_once(':') {
        Some((scheme, rest)) if rest.chars().count() > 12 => {
            let tail: String = rest.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{scheme}:{}…{tail}", rest.chars().take(4).collect::<String>())
        }
        _ => principal.to_string(),
    }
}

/// What a role lets someone do, for a sentence: `read`, `read and change`.
fn role_verb(role: &Role) -> &'static str {
    match role {
        Role::Chatter => "chat in",
        Role::Viewer => "read",
        Role::Deployer => "deploy",
        Role::Editor => "read and change",
        Role::Admin => "manage",
    }
}

/// Who granted it, said in a chip's tooltip.
fn granted_by_words(g: &GrantedBy, name: &dyn Fn(&str) -> String) -> String {
    match g {
        GrantedBy::Principal(p) => format!("granted by {}", name(p.as_str())),
        GrantedBy::Operator => "set when the server starts (--admin)".into(),
        GrantedBy::DevLogin => "granted by dev login".into(),
        GrantedBy::AuthGrant => "granted by the server".into(),
    }
}

/// The places a grant may reach, for the picker: each organization, its
/// root shortcodes, and everything. `(value, label)`.
fn scope_choices(orgs: &[menger_tickets_api::Org]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for o in orgs {
        out.push((format!("{}:", o.name), format!("all of {}", o.name)));
    }
    for o in orgs {
        for root in &o.roots {
            let code = root.split_once(':').map_or(root.as_str(), |(_, c)| c);
            let label = if orgs.len() > 1 { format!("{code} ({})", o.name) } else { code.to_string() };
            out.push((root.clone(), label));
        }
    }
    out.push(("*".into(), "everything".into()));
    out
}

/// What the picker's value means for the role buttons' wording.
const OTHER: &str = "";

/// Role buttons and a place to grant on — the waiting cards' and the add form's.
#[component]
fn RolePlace(role: RwSignal<String>, scope: RwSignal<String>) -> impl IntoView {
    let orgs = crate::orgs::held();
    let orgs2 = orgs.clone();
    let other = RwSignal::new(String::new());
    let picked = RwSignal::new(None::<String>);
    // The organization by default, once the organizations are known.
    let chosen = Memo::new(move |_| picked.get().unwrap_or_else(|| scope_choices(&orgs.get()).first().map(|(v, _)| v.clone()).unwrap_or_else(|| "*".into())));
    Effect::new(move |_| {
        let c = chosen.get();
        scope.set(if c == OTHER { other.get() } else { c });
    });
    view! {
        <span class="seg" role="group" aria-label="role">
            {["viewer", "editor", "admin"].into_iter().map(|r| view! {
                <button type="button" data-role=r aria-pressed=move || (role.get() == r).to_string()
                    on:click=move |_| role.set(r.to_string())>{role_word(&r.parse::<Role>().unwrap_or(Role::Viewer))}</button>
            }).collect_view()}
        </span>
        <span class="lbl">"of"</span>
        <select class="where" aria-label="where" on:change=move |ev| picked.set(Some(event_target_value(&ev)))>
            {move || {
                let now = chosen.get();
                scope_choices(&orgs2.get()).into_iter().map(|(v, l)| {
                    let sel = v == now;
                    view! { <option value=v selected=sel>{l}</option> }
                }).collect_view()
            }}
            <option value=OTHER>"a shortcode or ticket…"</option>
        </select>
        {move || (chosen.get() == OTHER).then(|| view! {
            <input class="scope" placeholder="MENGER · under MENGER-12 · MENGER-12" prop:value=move || other.get()
                on:input=move |ev| other.set(event_target_value(&ev)) />
        })}
    }
}

#[component]
pub(crate) fn MembersPage() -> impl IntoView {
    let board = RwSignal::new(None::<Board>);
    let note = RwSignal::new(None::<Result<String, String>>);
    // The chip whose × was pressed once: `(grantee, scope)`.
    let asking = RwSignal::new(None::<(String, String)>);
    let reload = move || {
        spawn_local(async move {
            board.set(Some(read().await));
        })
    };
    reload();
    crate::orgs::refresh();

    let act = move |what: String, fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>| {
        spawn_local(async move {
            note.set(Some(fut.await.map(|()| what)));
            asking.set(None);
            board.set(Some(read().await));
        });
    };

    let admin = signin::account();
    // CHAT-14: the names (and agents) the server knows.
    spawn_local(async {
        let _ = crate::chats::candidates(None).await;
    });
    let name = crate::chat_pane::names();
    let me = crate::chats::me();
    let now = crate::now_unix();

    let chip = move |g: Grant, label_name: std::rc::Rc<dyn Fn(&str) -> String>| {
        let key = (g.principal.to_string(), g.scope.to_string());
        let fixed = g.granted_by == GrantedBy::Operator;
        let title = format!("{} · {}", granted_by_words(&g.granted_by, &*label_name), crate::fmt_unix(g.at));
        let words = scope_words(&g.scope);
        let role = role_word(&g.role);
        let (who, scope) = (g.principal.clone(), g.scope.clone());
        let k2 = key.clone();
        let ask = move |_| asking.set(Some(k2.clone()));
        let yes = {
            let (who, scope, role, words) = (who.clone(), scope.clone(), role.clone(), words.clone());
            move |_| {
                let Some(client) = session::members_client() else { return };
                act(format!("Removed {role} on {words}."), Box::pin(revoke(client, who.clone(), scope.clone())));
            }
        };
        let k3 = key.clone();
        view! {
            <span class="chip" class:fixed=fixed title=title data-scope=g.scope.to_string()>
                <b>{role.clone()}</b>" · "{words.clone()}{fixed.then_some(" · server")}
                {(!fixed).then(|| view! {
                    <button type="button" class="rm revoke" aria-label=format!("remove {} on {}", role, words) on:click=ask>"×"</button>
                })}
            </span>
            {move || (asking.get().as_ref() == Some(&k3)).then(|| view! {
                <span class="confirm">"remove? "
                    <button type="button" class="yes" on:click=yes.clone()>"yes"</button>
                    <button type="button" class="no" on:click=move |_| asking.set(None)>"no"</button>
                </span>
            })}
        }
    };

    view! {
        <section id="members">
            <h2>"Members"</h2>
            <p class="lede">"Who can see and change the tickets, and how they got in."</p>
            {move || (!admin.get().is_admin()).then(|| view! {
                <p class="hint" id="members-not-admin">"Only an admin of a shortcode can see and change its members."</p>
            })}
            {move || note.get().map(|n| match n {
                Ok(said) => view! { <p class="probe ok" id="members-note">{said}</p> }.into_any(),
                Err(why) => view! { <p class="invalid" id="members-note" role="alert">{why}</p> }.into_any(),
            })}
            {move || board.get().and_then(|b| b.problem).map(|p| view! { <p class="invalid" role="alert">{p}</p> })}
            <h3>"Waiting to be let in"
                {move || board.get().map(|b| view! { <span class="n">{b.waiting.len()}</span> })}
            </h3>
            {move || match board.get() {
                None => view! { <p class="hint">"…"</p> }.into_any(),
                Some(b) if b.waiting.is_empty() => view! { <p class="hint" id="waiting-none">"Nobody is waiting."</p> }.into_any(),
                Some(b) => b.waiting.into_iter().map(|w| {
                    let profile = b.profiles.get(&w.principal).cloned();
                    let shown = display_name(&w.principal, profile.as_ref(), w.name.as_deref().unwrap_or(""));
                    let verified = profile.as_ref().and_then(|p| p.email_verified).unwrap_or(false);
                    let lines = about(profile.as_ref(), w.provider.as_deref(), Some(w.since), now);
                    let scope = RwSignal::new(String::new());
                    let role = RwSignal::new("editor".to_string());
                    let (p, n) = (w.principal.clone(), shown.clone());
                    let go = move |_| {
                        let (p, s, r) = (p.clone(), scope.get_untracked(), role.get_untracked());
                        let Some(client) = session::members_client() else { return };
                        act(format!("{n} is in — {r} of {s}."), Box::pin(grant(client, p, s, r)));
                    };
                    let has_email = profile.as_ref().is_some_and(|p| p.email.as_deref().is_some_and(|e| !e.is_empty()));
                    view! {
                        <div class="waiting" data-principal=w.principal.clone()>
                            <span class="mav" aria-hidden="true">{crate::conversation::initials(&shown)}</span>
                            <div class="who"><strong>{shown.clone()}</strong></div>
                            <div class="sub">
                                {lines.into_iter().enumerate().map(|(i, l)| view! {
                                    {(i > 0).then_some(" · ")}{l}
                                    {(i == 0 && has_email && verified).then(|| view! { <span class="ok" title="verified by the provider">" ✓"</span> })}
                                }).collect_view()}
                            </div>
                            <code class="pid" title="their principal">{w.principal.clone()}</code>
                            <div class="letin">
                                <span class="lbl">"Let in as"</span>
                                <RolePlace role scope />
                                <button type="button" class="primary grant" on:click=go>{move || format!("Let in as {}", role.get())}</button>
                            </div>
                        </div>
                    }
                }).collect_view().into_any(),
            }}
            {move || board.get().map(|b| {
                let r = rows(&b.grants, crate::chats::is_agent);
                let profiles = std::rc::Rc::new(b.profiles);
                let label_name: std::rc::Rc<dyn Fn(&str) -> String> = {
                    let (profiles, name) = (profiles.clone(), name.clone());
                    std::rc::Rc::new(move |p: &str| display_name(p, profiles.get(p), &name(p)))
                };
                let holder = {
                    let me = me.clone();
                    let profiles = profiles.clone();
                    let label_name = label_name.clone();
                    move |h: Holder| {
                        let (person, team) = match &h.who {
                            Grantee::Principal(p) => (Some(p.to_string()), None),
                            Grantee::Team(t) => (None, Some(t.to_string())),
                            Grantee::Everyone => (None, None),
                        };
                        let pid = person.clone().unwrap_or_default();
                        let profile = profiles.get(&pid).cloned();
                        let agent = person.as_deref().is_some_and(crate::chats::is_agent);
                        let shown = team.clone().unwrap_or_else(|| label_name(&pid));
                        let mine = person.as_deref() == Some(me.as_str());
                        let lines = if agent { Vec::new() } else { about(profile.as_ref(), None, None, now) };
                        // CHAT-14: a person (not a team, not the reader) has Chat.
                        let chat = person.clone().filter(|_| crate::chats::ENABLED && !mine).map(|p| {
                            let go = move |_| crate::chats_page::message(p.clone(), move |e| note.set(Some(Err(e))));
                            view! { <button type="button" class="link chatwith" title="a direct message, in #/chats" on:click=go>"Chat"</button> }
                        });
                        // TICKETS-9: a person's tickets — the board under `assigned "<principal>"`.
                        let assigned = person.clone().map(|p| {
                            view! { <a class="assigned-link" href=crate::assignees::board_link(&p) title="the board: tickets assigned to them">"Assigned"</a> }
                        });
                        let copy = person.clone().filter(|_| agent || lines.is_empty()).map(|p| {
                            let shortened = short_key(&p);
                            let full = p.clone();
                            let go = move |_| {
                                let _ = crate::dom::window().navigator().clipboard().write_text(&full);
                                note.set(Some(Ok(format!("Copied {full}"))));
                            };
                            view! { <button type="button" class="key" title=format!("copy {p}") on:click=go>{shortened}" ⧉"</button> }
                        });
                        let chips = h.grants.into_iter().map(|g| chip(g, label_name.clone())).collect_view();
                        view! {
                            <div class="person" data-principal=h.who.to_string()>
                                <span class="mav" class:agent=agent class:team=team.is_some() aria-hidden="true">{crate::conversation::initials(&shown)}</span>
                                <div class="who">
                                    <strong class="mname">{shown}</strong>
                                    {mine.then(|| view! { <span class="tag you">"you"</span> })}
                                    {agent.then(|| view! { <span class="tag ag">"agent"</span> })}
                                    {team.is_some().then(|| view! { <span class="tag">"team"</span> })}
                                </div>
                                <div class="acts">{chat}{assigned}</div>
                                <div class="sub">{lines.join(" · ")}{copy}</div>
                                <div class="roles">{chips}</div>
                            </div>
                        }
                    }
                };
                let section = |title: &'static str, id: &'static str, list: Vec<Holder>| {
                    let holder = holder.clone();
                    (!list.is_empty()).then(|| view! {
                        <h3>{title}<span class="n">{list.len()}</span></h3>
                        <div class="card list" id=id>{list.into_iter().map(holder).collect_view()}</div>
                    })
                };
                let everyone = (!r.everyone.is_empty()).then(|| {
                    let what = r.everyone.iter().map(|g| format!("{} {}", role_verb(&g.role), scope_words(&g.scope))).collect::<Vec<_>>().join(", ");
                    let all = r.everyone.clone();
                    let stop = move |_| {
                        let Some(client) = session::members_client() else { return };
                        let all = all.clone();
                        act("Nobody gets anything just by signing in now.".into(), Box::pin(async move {
                            for g in all {
                                revoke(client.clone(), g.principal, g.scope).await?;
                            }
                            Ok(())
                        }));
                    };
                    view! {
                        <h3>"Everyone who signs in"</h3>
                        <div class="card everyone" id="everyone">
                            <span class="mav" aria-hidden="true">"∗"</span>
                            <p>"Anyone with an account can "<b>{what}</b>" — even while they wait to be let in."</p>
                            <button type="button" class="stop" on:click=stop>"Stop"</button>
                        </div>
                    }
                });
                view! {
                    {section("People", "people", r.people)}
                    {section("Agents", "agents", r.agents)}
                    {section("Teams", "teams", r.teams)}
                    {everyone}
                }
            })}
            <AddSomeone act />
        </section>
    }
}

/// Granting to an id that never waited here — a key, or an id someone sent.
#[component]
fn AddSomeone(act: impl Fn(String, std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>) + Copy + 'static) -> impl IntoView {
    let p_text = RwSignal::new(String::new());
    let role = RwSignal::new("viewer".to_string());
    let scope = RwSignal::new(String::new());
    view! {
        <details class="add">
            <summary>"＋ Add someone by their id"</summary>
            <div class="card addform" id="grant-form">
                <label>"Their id"
                    <input id="grant-principal" placeholder="oidc:workos/user_… · key:… · idp:…" prop:value=move || p_text.get()
                        on:input=move |ev| p_text.set(event_target_value(&ev)) />
                </label>
                <div class="line">
                    <RolePlace role scope />
                    <button type="button" class="primary" id="grant-submit" on:click=move |_| {
                        let (p, s, r) = (p_text.get_untracked(), scope.get_untracked(), role.get_untracked());
                        let Some(client) = session::members_client() else { return };
                        act(format!("Granted {r} of {s} to {p}."), Box::pin(grant(client, p, s, r)));
                    }>"Add"</button>
                </div>
                <p class="hint">"Someone who signed in shows above under Waiting — this is for a key or an id you were sent."</p>
            </div>
        </details>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(who: &str, scope: &str, role: &str) -> Grant {
        Grant {
            principal: Grantee::try_from(who).unwrap(),
            scope: Scope::stored(scope).unwrap(),
            role: role.parse().unwrap(),
            granted_by: GrantedBy::Operator,
            at: 0,
        }
    }

    #[test]
    fn grants_gather_one_row_per_grantee_and_everyone_apart() {
        let grants = [
            g("key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "*", "viewer"),
            g("oidc:workos/user_1", "hypermemetic:", "editor"),
            g("key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "hypermemetic:MENGER", "deployer"),
            g("team:crew", "hypermemetic:", "viewer"),
            g("everyone", "hypermemetic:DOCS", "viewer"),
        ];
        let r = rows(&grants, |p| p.starts_with("key:"));
        assert_eq!(r.agents.len(), 1, "pipeline's two grants are one row");
        assert_eq!(r.agents[0].grants.len(), 2);
        assert_eq!(r.people.len(), 1);
        assert_eq!(r.teams.len(), 1);
        assert_eq!(r.everyone.len(), 1);
    }

    #[test]
    fn a_scope_and_a_role_read_as_words() {
        assert_eq!(scope_words(&Scope::stored("*").unwrap()), "everything");
        assert_eq!(scope_words(&Scope::stored("hypermemetic:").unwrap()), "all of hypermemetic");
        assert_eq!(scope_words(&Scope::stored("hypermemetic:MENGER").unwrap()), "MENGER");
        assert_eq!(scope_words(&Scope::stored("under MENGER-12").unwrap()), "under MENGER-12");
        assert_eq!(role_word(&Role::Deployer), "Deployer");
        assert_eq!(format!("{} {}", role_verb(&Role::Viewer), scope_words(&Scope::stored("hypermemetic:DOCS").unwrap())), "read DOCS");
    }

    #[test]
    fn how_someone_signed_in_is_said_the_way_a_person_says_it() {
        assert_eq!(signed_in_with(Some("githuboauth"), Some("workos")).as_deref(), Some("GitHub"));
        assert_eq!(signed_in_with(Some("email_code"), None).as_deref(), Some("an email code"));
        assert_eq!(signed_in_with(None, Some("plexus-idp")).as_deref(), Some("plexus-idp"));
        assert_eq!(signed_in_with(Some("somethingnew"), Some("workos")).as_deref(), Some("WorkOS"), "an unknown method falls to the provider");
        assert_eq!(signed_in_with(Some("somethingnew"), None).as_deref(), Some("somethingnew"));
        assert_eq!(signed_in_with(None, None), None);
    }

    #[test]
    fn the_second_line_says_email_then_how_and_when() {
        let p = Profile { email: Some("ada@example.com".into()), method: Some("githuboauth".into()), signed_in_at: Some(1000), ..Default::default() };
        assert_eq!(about(Some(&p), None, None, 1000 + 12 * 60), ["ada@example.com", "signed in with GitHub · 12m ago"]);
        assert_eq!(about(None, Some("workos"), Some(0), 3 * 86_400), ["signed in with WorkOS · 3d ago"]);
        assert!(about(None, None, None, 0).is_empty());
    }

    #[test]
    fn a_name_is_the_profile_s_then_the_known_then_the_principal_short() {
        let p = Profile { name: Some("Ada Lovelace".into()), ..Default::default() };
        assert_eq!(display_name("oidc:workos/user_01ABCDEFGH", Some(&p), "ada"), "Ada Lovelace");
        assert_eq!(display_name("key:54a9677a00", None, "builder"), "builder");
        assert_eq!(display_name("key:54a9677a00", None, "key:54a9677a00"), "key:54a9…");
        assert_eq!(short_key("key:54a9677aabcdef0123456789003b0d28"), "key:54a9…0d28");
        assert_eq!(short_key("anonymous"), "anonymous");
    }

    #[test]
    fn the_picker_offers_the_organization_first_then_its_roots_then_everything() {
        let o = menger_tickets_api::Org {
            name: "hypermemetic".parse().unwrap(),
            aliases: vec![],
            roots: vec!["hypermemetic:MENGER".into(), "hypermemetic:DOCS".into()],
            shortcodes: 2,
            tickets: 0,
        };
        let c = scope_choices(&[o]);
        assert_eq!(c[0], ("hypermemetic:".to_string(), "all of hypermemetic".to_string()));
        assert_eq!(c[1], ("hypermemetic:MENGER".to_string(), "MENGER".to_string()));
        assert_eq!(c.last().unwrap(), &("*".to_string(), "everything".to_string()));
    }
}
