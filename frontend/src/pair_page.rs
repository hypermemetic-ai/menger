//! TETHER-11: `#/pair?principal=key:<hex>&name=<name>` — the page `tether
//! pair` opens, where a person enrols a machine's key.
//!
//! It shows the machine's name, its principal and the **pairing code** the
//! page computes from the principal itself
//! ([`menger_tickets_api::pairing`], the function tether prints it with):
//! the person checks it against the terminal, and that check is what stops
//! a crafted link from enrolling someone else's key. Then **Pair** grants
//! it a role (`members.grant`) on a scope the reader administers — editor
//! on their own scope by default, never more than they can grant — and
//! **Not mine** dismisses it, granting nothing. A reader who administers
//! nothing is told who can. The machine's name was kept by the server when
//! it signed in (CHAT-14's `principal_names`), so the page writes only the
//! grant.

use leptos::prelude::*;
use menger_tickets_api::members::Role;
use menger_tickets_api::{Grant, MembersEvent};
use wasm_bindgen_futures::spawn_local;

use crate::session;
use crate::signin;

/// What the link names: the machine's principal and its name.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Link {
    pub(crate) principal: Option<String>,
    pub(crate) name: Option<String>,
}

/// The link's query (`#/pair?principal=…&name=…`); the values decoded.
pub(crate) fn link(hash: &str) -> Link {
    let q = hash.split_once('?').map_or("", |(_, q)| q);
    let mut l = Link::default();
    for (k, v) in q.split('&').filter_map(|p| p.split_once('=')) {
        let v = decode(v);
        let v = (!v.trim().is_empty()).then(|| v.trim().to_string());
        match k {
            "principal" => l.principal = v,
            "name" => l.name = v,
            _ => {}
        }
    }
    l
}

/// `%XX` and `+` decoded; a malformed escape kept as written.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => match s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(x) => {
                    out.push(x);
                    i += 3;
                    continue;
                }
                None => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The scopes the reader may grant on — where they hold admin (`*` when
/// they are admin everywhere) — and the one the page picks first: their
/// own scope, before `*` and before an organization.
pub(crate) fn grantable(admin_everywhere: bool, grants: &[Grant]) -> Vec<String> {
    let mut own: Vec<String> = grants.iter().filter(|g| g.role == Role::Admin).map(|g| g.scope.to_string()).collect();
    if admin_everywhere {
        own.push("*".into());
    }
    // A shortcode first, then an organization (`acme:`), then `*`.
    own.sort_by_key(|s| (s == "*", s.ends_with(':'), s.clone()));
    own.dedup();
    own
}

/// What the page has done.
#[derive(Clone, Debug, PartialEq)]
enum Done {
    Paired(String),
    Dismissed,
    Refused(String),
}

/// The roles a machine is offered.
const ROLES: [&str; 2] = ["editor", "viewer"];

#[component]
pub(crate) fn PairPage() -> impl IntoView {
    let here = crate::dom::window().location().hash().unwrap_or_default();
    let l = link(&here);
    let account = signin::account();
    let done = RwSignal::new(None::<Done>);
    let busy = RwSignal::new(false);
    let scope = RwSignal::new(None::<String>);
    let role = RwSignal::new("editor".to_string());

    let Some(principal) = l.principal.clone().filter(|p| menger_tickets_api::pairing::code(p).is_some()) else {
        return view! {
            <section id="pair-page">
                <h2>"Pair a machine"</h2>
                <p class="invalid" id="pair-bad" role="alert">
                    "This link names no machine key. Run "<code>"tether pair"</code>" on the machine and open the link it prints."
                </p>
            </section>
        }
        .into_any();
    };
    let code = menger_tickets_api::pairing::code(&principal).unwrap_or_default();
    let name = l.name.clone().unwrap_or_else(|| "a machine".into());

    let scopes = {
        let account = account.clone();
        Memo::new(move |_| {
            let a = account.get();
            grantable(a.admin_everywhere, a.grants.as_deref().unwrap_or(&[]))
        })
    };
    let known = {
        let account = account.clone();
        Memo::new(move |_| account.get().grants.is_some() || account.get().admin_everywhere)
    };
    let chosen = move || scope.get().or_else(|| scopes.get().into_iter().next());

    let pair = {
        let (principal, name) = (principal.clone(), name.clone());
        move |_| {
            let (Some(s), r) = (chosen(), role.get_untracked()) else { return };
            let Some(client) = session::members_client() else {
                done.set(Some(Done::Refused("not connected".into())));
                return;
            };
            let (p, n) = (principal.clone(), name.clone());
            busy.set(true);
            spawn_local(async move {
                let typed = || -> Result<_, String> { Ok((crate::typed::typed(&p)?, crate::typed::typed(&s)?, crate::typed::typed(&r)?)) };
                let said = match typed() {
                    Err(e) => Done::Refused(e),
                    Ok((p, sc, ro)) => match client.grant_all(Some(p), None, None, sc, ro).await {
                        Ok(evs) => match evs.into_iter().find_map(|e| match e {
                            MembersEvent::Error { message } => Some(Err(message)),
                            MembersEvent::Grant { grant } => Some(Ok(grant)),
                            _ => None,
                        }) {
                            Some(Ok(g)) => Done::Paired(format!("{n} is {} on {}. tether, in the terminal, says paired in a few seconds.", g.role, g.scope)),
                            Some(Err(m)) => Done::Refused(m),
                            None => Done::Refused("menger answered nothing".into()),
                        },
                        Err(e) => Done::Refused(session::describe(&e)),
                    },
                };
                busy.set(false);
                done.set(Some(said));
            });
        }
    };

    let card = view! {
        <dl class="pair-who">
            <dt>"machine"</dt><dd id="pair-name">{name.clone()}</dd>
            <dt>"key"</dt><dd><code id="pair-principal">{principal.clone()}</code></dd>
        </dl>
        <p class="pair-codelabel">"Pairing code"</p>
        <p class="pair-code" id="pair-code" aria-label=format!("pairing code {}", code.chars().filter(|c| *c != '-').map(String::from).collect::<Vec<_>>().join(" "))>{code.clone()}</p>
        <p class="hint">"Check it is the code "<code>"tether pair"</code>" printed in your terminal. If it is not, or you did not just run it, choose "<strong>"Not mine"</strong>"."</p>
    };

    view! {
        <section id="pair-page">
            <h2>"Pair a machine"</h2>
            <div class="pair-card">
                {card}
                {move || match done.get() {
                    Some(Done::Paired(said)) => view! { <p class="probe ok" id="pair-done" role="status">"Paired: "{said}</p> }.into_any(),
                    Some(Done::Dismissed) => view! {
                        <p class="hint" id="pair-dismissed" role="status">"Dismissed — nothing was granted. If someone sent you this link, they wanted a key of theirs in your name: ignore it."</p>
                    }.into_any(),
                    other => {
                        let refused = match other { Some(Done::Refused(m)) => Some(m), _ => None };
                        let pair = pair.clone();
                        view! {
                            {refused.map(|m| view! { <p class="invalid" id="pair-note" role="alert">{m}</p> })}
                            {move || match (known.get(), scopes.get().is_empty()) {
                                (false, _) => view! { <p class="hint">"…"</p> }.into_any(),
                                (true, true) => view! {
                                    <p class="hint" id="pair-cannot">
                                        "You can't grant this machine access: only an admin can — whoever made you a member here, or an admin of the project it is for. Send them this page's link; they will see the same code."
                                    </p>
                                }.into_any(),
                                (true, false) => view! {
                                    <div class="pair-grant">
                                        <label>"as "
                                            <select id="pair-role" on:change=move |ev| role.set(event_target_value(&ev))>
                                                {ROLES.iter().map(|r| view! { <option value=*r selected=move || role.get() == *r>{*r}</option> }).collect_view()}
                                            </select>
                                        </label>
                                        <label>" on "
                                            <select id="pair-scope" on:change=move |ev| scope.set(Some(event_target_value(&ev)))>
                                                {move || scopes.get().into_iter().map(|s| {
                                                    let (v, label) = (s.clone(), s.clone());
                                                    view! { <option value=s selected=move || chosen().as_deref() == Some(v.as_str())>{label}</option> }
                                                }).collect_view()}
                                            </select>
                                        </label>
                                    </div>
                                }.into_any(),
                            }}
                            <div class="pair-actions">
                                {move || (known.get() && !scopes.get().is_empty()).then(|| {
                                    let pair = pair.clone();
                                    view! { <button class="primary" id="pair-go" disabled=move || busy.get() on:click=pair>"Pair"</button> }
                                })}
                                <button id="pair-not-mine" disabled=move || busy.get() on:click=move |_| done.set(Some(Done::Dismissed))>"Not mine"</button>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </section>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::members::Scope;
    use menger_tickets_api::vocab::{GrantedBy, Grantee};

    /// The fixture tether's tests pin too (`tether/src/pair.rs`).
    const FIXTURE: &str = "key:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn the_route_is_pair_whatever_the_query() {
        assert_eq!(crate::parse_route(&format!("#/pair?principal={FIXTURE}&name=alice-laptop")), crate::Route::Pair);
        assert_eq!(crate::parse_route("#/pair"), crate::Route::Pair);
        assert_eq!(crate::route_text(&crate::Route::Pair), "#/pair");
    }

    #[test]
    fn the_page_shows_the_code_tether_prints() {
        let l = link(&format!("#/pair?principal={FIXTURE}&name=alice-laptop"));
        assert_eq!((l.principal.as_deref(), l.name.as_deref()), (Some(FIXTURE), Some("alice-laptop")));
        assert_eq!(menger_tickets_api::pairing::code(l.principal.as_deref().unwrap()).as_deref(), Some("MMG4-2KLG"));
        // Escaped as a browser might pass it on.
        let l = link("#/pair?name=alice%2Dlaptop&principal=key%3A000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        assert_eq!((l.principal.as_deref(), l.name.as_deref()), (Some(FIXTURE), Some("alice-laptop")));
        assert_eq!(link("#/pair"), Link::default());
        assert_eq!(decode("a%2"), "a%2", "a short escape kept");
        assert_eq!(decode("%zz+x"), "%zz x");
    }

    fn g(scope: &str, role: Role) -> Grant {
        Grant { principal: Grantee::Principal(FIXTURE.parse().unwrap()), scope: Scope::try_from(scope).unwrap(), role, granted_by: GrantedBy::Operator, at: 0 }
    }

    #[test]
    fn it_offers_only_where_the_reader_is_admin_their_own_scope_first() {
        let grants = [g("hypermemetic:", Role::Admin), g("hypermemetic:DOCS", Role::Editor), g("hypermemetic:PROJ", Role::Admin)];
        assert_eq!(grantable(false, &grants), vec!["hypermemetic:PROJ", "hypermemetic:"]);
        assert_eq!(grantable(true, &grants[..2]), vec!["hypermemetic:", "*"]);
        assert!(grantable(false, &grants[1..2]).is_empty(), "an editor grants nothing");
    }
}
