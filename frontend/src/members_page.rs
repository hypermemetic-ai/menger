//! IDENTITY-22: `#/members` — who holds which role, and the way an admin
//! changes it.
//!
//! Three parts, each from the `members` activation over the app's own
//! socket: the people **waiting** (signed in, holding nothing) with a grant
//! form beside each; the **grants** on the scopes this admin administers,
//! each with revoke; and a form to grant a role to any principal. Every
//! change asks the server, which checks it (IDENTITY-21), then the page
//! reads again.

use leptos::prelude::*;
use menger_tickets_api::{Grant, MembersClient, MembersEvent};
use wasm_bindgen_futures::spawn_local;

use crate::session;
use crate::signin;

#[derive(Clone, Debug, Default, PartialEq)]
struct Board {
    waiting: Vec<(String, Option<String>, Option<String>)>,
    grants: Vec<Grant>,
    problem: Option<String>,
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
                    MembersEvent::Waiting { principal, name, provider, .. } => b.waiting.push((principal, name, provider)),
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
    outcome(client.grant_all(principal, scope, role).await)
}

#[component]
pub(crate) fn MembersPage() -> impl IntoView {
    let board = RwSignal::new(None::<Board>);
    let note = RwSignal::new(None::<Result<String, String>>);
    let reload = move || {
        spawn_local(async move {
            board.set(Some(read().await));
        })
    };
    reload();

    let act = move |what: String, fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>| {
        spawn_local(async move {
            note.set(Some(fut.await.map(|()| what)));
            board.set(Some(read().await));
        });
    };

    let p_text = RwSignal::new(String::new());
    let s_text = RwSignal::new(String::new());
    let r_text = RwSignal::new("viewer".to_string());

    let admin = signin::account();
    view! {
        <section id="members">
            <h2>"Members"</h2>
            {move || (!admin.get().is_admin()).then(|| view! {
                <p class="hint" id="members-not-admin">"Only an admin of a shortcode can see and change its members."</p>
            })}
            {move || note.get().map(|n| match n {
                Ok(said) => view! { <p class="probe ok" id="members-note">{said}</p> }.into_any(),
                Err(why) => view! { <p class="invalid" id="members-note" role="alert">{why}</p> }.into_any(),
            })}
            <h3>"Waiting for a role"</h3>
            {move || match board.get() {
                None => view! { <p class="hint">"…"</p> }.into_any(),
                Some(b) if b.waiting.is_empty() => view! { <p class="hint" id="waiting-none">"Nobody is waiting."</p> }.into_any(),
                Some(b) => b.waiting.into_iter().map(|(principal, name, provider)| {
                    let scope = RwSignal::new(String::new());
                    let role = RwSignal::new("viewer".to_string());
                    let p = principal.clone();
                    let go = move |_| {
                        let (p, s, r) = (p.clone(), scope.get_untracked(), role.get_untracked());
                        let Some(client) = session::members_client() else { return };
                        act(format!("granted {r} on {s} to {p}"), Box::pin(grant(client, p, s, r)));
                    };
                    view! {
                        <div class="waiting" data-principal=principal.clone()>
                            <strong>{name.unwrap_or_else(|| principal.clone())}</strong>
                            <span class="lbl">{provider.map(|p| format!(" via {p}")).unwrap_or_default()}</span>
                            <code>{principal.clone()}</code>
                            <input class="scope" placeholder="SHORTCODE or *" prop:value=move || scope.get()
                                on:input=move |ev| scope.set(event_target_value(&ev)) />
                            <select class="role" on:change=move |ev| role.set(event_target_value(&ev))>
                                <option value="viewer">"viewer"</option>
                                <option value="editor">"editor"</option>
                                <option value="admin">"admin"</option>
                            </select>
                            <button class="primary grant" on:click=go>"Grant"</button>
                        </div>
                    }
                }).collect_view().into_any(),
            }}
            <h3>"Grants"</h3>
            <table id="grants">
                <thead><tr><th>"principal"</th><th>"scope"</th><th>"role"</th><th>"granted by"</th><th></th></tr></thead>
                <tbody>
                    {move || board.get().map(|b| b.grants.into_iter().map(|g| {
                        let (p, s) = (g.principal.clone(), g.scope.clone());
                        let operator = g.granted_by == "--admin";
                        let revoke = move |_| {
                            let (p, s) = (p.clone(), s.clone());
                            let Some(client) = session::members_client() else { return };
                            act(format!("revoked {p} on {s}"), Box::pin(async move { outcome(client.revoke_all(p, s).await) }));
                        };
                        view! {
                            <tr>
                                <td><code>{g.principal.clone()}</code></td>
                                <td><code>{g.scope.clone()}</code></td>
                                <td>{g.role.to_string()}</td>
                                <td class="lbl">{g.granted_by.clone()}</td>
                                <td>{(!operator).then(|| view! { <button class="link revoke" on:click=revoke>"revoke"</button> })}</td>
                            </tr>
                        }
                    }).collect_view())}
                </tbody>
            </table>
            <h3>"Grant a role"</h3>
            <div class="line" id="grant-form">
                <input id="grant-principal" placeholder="idp:… · oidc:auth0/… · nostr:…" prop:value=move || p_text.get()
                    on:input=move |ev| p_text.set(event_target_value(&ev)) />
                <input id="grant-scope" placeholder="SHORTCODE or *" prop:value=move || s_text.get()
                    on:input=move |ev| s_text.set(event_target_value(&ev)) />
                <select id="grant-role" on:change=move |ev| r_text.set(event_target_value(&ev))>
                    <option value="viewer">"viewer"</option>
                    <option value="editor">"editor"</option>
                    <option value="admin">"admin"</option>
                </select>
                <button class="primary" id="grant-submit" on:click=move |_| {
                    let (p, s, r) = (p_text.get_untracked(), s_text.get_untracked(), r_text.get_untracked());
                    let Some(client) = session::members_client() else { return };
                    act(format!("granted {r} on {s} to {p}"), Box::pin(grant(client, p, s, r)));
                }>"Grant"</button>
            </div>
        </section>
    }
}
