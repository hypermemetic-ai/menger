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
//! **No** dismisses it, granting nothing. A reader who administers
//! nothing is told who can. The machine's name was kept by the server when
//! it signed in (CHAT-14's `principal_names`), so the page writes only the
//! grant.
//!
//! TETHER-12: the link is also a QR code a phone's camera opens, so the
//! page is a confirm-a-match screen first — the code large, *Does your
//! terminal show it?*, **Yes, pair it** / **No**. And `#/pair` with no
//! query (the account's **Pair a machine**) takes the code typed, or the
//! key pasted, for a phone without a camera: a code is found among the
//! machines waiting (`members.pairing`), and typing it is the match, so
//! the confirm screen does not ask it again.

use leptos::prelude::*;
use menger_tickets_api::members::Role;
use menger_tickets_api::pairing;
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

/// TETHER-12: what a person typed into the field.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Typed {
    /// A pairing code, in its one spelling (`MMG4-2KLG`).
    Code(String),
    /// A key, as its principal (`key:<hex>`).
    Key(String),
    /// Neither.
    Neither,
}

/// The field's text read as a code (any case, dashes and spaces
/// optional) or a key (`key:<hex>` or the bare hex).
pub(crate) fn typed(text: &str) -> Typed {
    if let Some(p) = pairing::principal_of(text) {
        Typed::Key(p)
    } else if let Some(c) = pairing::normalize(text) {
        Typed::Code(c)
    } else {
        Typed::Neither
    }
}

/// The machine on the confirm screen.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Machine {
    pub(crate) principal: String,
    pub(crate) name: Option<String>,
    /// The reader typed its code: the match is made, not asked.
    pub(crate) typed_code: bool,
}

/// The machines waiting that a `members.pairing` answer names.
fn waiting(evs: Vec<MembersEvent>) -> Result<Vec<Machine>, String> {
    let mut out = Vec::new();
    for e in evs {
        match e {
            MembersEvent::Waiting { principal, name, .. } => out.push(Machine { principal, name, typed_code: true }),
            MembersEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(out)
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

/// The code as a screen reader says it: one character at a time.
fn spoken(code: &str) -> String {
    let chars: Vec<String> = code.chars().filter(|c| *c != '-').map(String::from).collect();
    format!("pairing code {}", chars.join(" "))
}

#[component]
pub(crate) fn PairPage() -> impl IntoView {
    let here = crate::dom::window().location().hash().unwrap_or_default();
    let l = link(&here);
    let account = signin::account();
    let scopes = {
        let account = account.clone();
        Memo::new(move |_| {
            let a = account.get();
            grantable(a.admin_everywhere, a.grants.as_deref().unwrap_or(&[]))
        })
    };
    let known = Memo::new(move |_| account.get().grants.is_some() || account.get().admin_everywhere);

    let from_link = l.principal.clone().filter(|p| pairing::code(p).is_some()).map(|principal| Machine { principal, name: l.name.clone(), typed_code: false });
    // A query that names no key is a bad link; none at all is the field.
    let bad_link = from_link.is_none() && here.contains('?');
    let machine = RwSignal::new(from_link);

    view! {
        <section id="pair-page">
            {move || match machine.get() {
                Some(m) => view! { <Confirm m scopes known /> }.into_any(),
                None => view! {
                    <h2>"Pair a machine"</h2>
                    {bad_link.then(|| view! {
                        <p class="invalid" id="pair-bad" role="alert">
                            "This link names no machine key. Run "<code>"tether pair"</code>" on the machine and scan or open what it prints — or type its code below."
                        </p>
                    })}
                    {move || match (known.get(), scopes.get().is_empty()) {
                        (false, _) => view! { <p class="hint">"…"</p> }.into_any(),
                        (true, true) => cannot().into_any(),
                        (true, false) => view! { <Entry machine /> }.into_any(),
                    }}
                }.into_any(),
            }}
        </section>
    }
}

/// Said to a reader who administers nothing.
fn cannot() -> impl IntoView {
    view! {
        <p class="hint" id="pair-cannot">
            "You can't grant this machine access: only an admin can — whoever made you a member here, or an admin of the project it is for. Send them this page's link; they will see the same code."
        </p>
    }
}

/// TETHER-12: the field for a phone without a camera — the code the
/// terminal shows, or its key.
#[component]
fn Entry(machine: RwSignal<Option<Machine>>) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let note = RwSignal::new(None::<String>);
    let choices = RwSignal::new(Vec::<Machine>::new());
    let busy = RwSignal::new(false);
    let ask = move |code: String, key: Option<String>| {
        let Some(client) = session::members_client() else {
            note.set(Some("not connected".into()));
            return;
        };
        busy.set(true);
        spawn_local(async move {
            let found = match crate::typed::typed(&code) {
                Err(e) => Err(e),
                Ok(c) => client.pairing_all(c).await.map_err(|e| session::describe(&e)).and_then(waiting),
            };
            busy.set(false);
            match (found, key) {
                // A pasted key: the name the server kept for it, when it waits.
                (Ok(w), Some(k)) => {
                    let name = w.into_iter().find(|m| m.principal == k).and_then(|m| m.name);
                    machine.set(Some(Machine { principal: k, name, typed_code: false }));
                }
                (Ok(w), None) if w.is_empty() => note.set(Some(format!(
                    "No machine waiting shows {code}. Check it against the terminal — or, if tether pair has stopped, run it again."
                ))),
                (Ok(mut w), None) if w.len() == 1 => machine.set(w.pop()),
                (Ok(w), None) => choices.set(w),
                (Err(e), Some(k)) => {
                    // The name is a nicety; the key is the machine.
                    leptos::logging::log!("TETHER-12: no name for the key: {e}");
                    machine.set(Some(Machine { principal: k, name: None, typed_code: false }));
                }
                (Err(e), None) => note.set(Some(e)),
            }
        });
    };
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        note.set(None);
        choices.set(Vec::new());
        match typed(&text.get_untracked()) {
            Typed::Code(c) => ask(c, None),
            Typed::Key(k) => ask(pairing::code(&k).unwrap_or_default(), Some(k)),
            Typed::Neither => note.set(Some("That is neither a code (8 characters, as MMG4-2KLG) nor a key (key: and 64 hex digits).".into())),
        }
    };
    view! {
        <form id="pair-entry" on:submit=submit>
            <label for="pair-text">"Enter the code your terminal shows, or paste its key"</label>
            <input id="pair-text" type="text" autocomplete="off" autocapitalize="characters" spellcheck="false" enterkeyhint="go"
                placeholder="MMG4-2KLG" prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev)) />
            <p class="hint">"tether pair prints both. Typing the code is the check that it is your machine."</p>
            <div class="pair-actions">
                <button type="submit" class="primary" id="pair-find" disabled=move || busy.get()>"Find it"</button>
            </div>
        </form>
        {move || note.get().map(|m| view! { <p class="invalid" id="pair-note" role="alert">{m}</p> })}
        {move || (!choices.get().is_empty()).then(|| view! {
            <div id="pair-choices">
                <p>"More than one machine shows that code. Which is yours?"</p>
                {choices.get().into_iter().enumerate().map(|(i, m)| {
                    let label = m.name.clone().unwrap_or_else(|| "a machine".into());
                    let key = m.principal.clone();
                    view! {
                        <button class="pair-choice" id=format!("pair-choice-{i}") on:click=move |_| machine.set(Some(m.clone()))>
                            <strong>{label}</strong><code>{key}</code>
                        </button>
                    }
                }).collect_view()}
            </div>
        })}
    }
}

/// The confirm-a-match screen: the code large, the machine, the role and
/// scope, **Yes, pair it** / **No**.
#[component]
fn Confirm(m: Machine, scopes: Memo<Vec<String>>, known: Memo<bool>) -> impl IntoView {
    let done = RwSignal::new(None::<Done>);
    let busy = RwSignal::new(false);
    let scope = RwSignal::new(None::<String>);
    let role = RwSignal::new("editor".to_string());
    let code = pairing::code(&m.principal).unwrap_or_default();
    let name = m.name.clone().unwrap_or_else(|| "a machine".into());
    let chosen = move || scope.get().or_else(|| scopes.get().into_iter().next());

    let pair = {
        let (principal, name) = (m.principal.clone(), name.clone());
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

    let typed_code = m.typed_code;
    let head = view! {
        <p class="pair-codelabel">"Pairing code"</p>
        <p class="pair-code" id="pair-code" aria-label=spoken(&code)>{code.clone()}</p>
        {if typed_code {
            view! { <p class="pair-ask" id="pair-matched">"You typed the code "<strong>{name.clone()}</strong>" shows."</p> }.into_any()
        } else {
            view! { <p class="pair-ask" id="pair-ask">"Does your terminal show "<strong>{code.clone()}</strong>"?"</p> }.into_any()
        }}
        <dl class="pair-who">
            <dt>"machine"</dt><dd id="pair-name">{name.clone()}</dd>
            <dt>"key"</dt><dd><code id="pair-principal">{m.principal.clone()}</code></dd>
        </dl>
    };

    view! {
        <div class="pair-card">
            {head}
            {move || match done.get() {
                Some(Done::Paired(said)) => view! { <p class="probe ok" id="pair-done" role="status">"Paired: "{said}</p> }.into_any(),
                Some(Done::Dismissed) => view! {
                    <p class="hint" id="pair-dismissed" role="status">
                        "Not paired — nothing was granted. If someone sent you this link or this code, it was to get a key of theirs into your account: ignore it, and tell whoever runs this menger."
                    </p>
                }.into_any(),
                other => {
                    let refused = match other { Some(Done::Refused(m)) => Some(m), _ => None };
                    let pair = pair.clone();
                    view! {
                        {refused.map(|m| view! { <p class="invalid" id="pair-note" role="alert">{m}</p> })}
                        {move || match (known.get(), scopes.get().is_empty()) {
                            (false, _) => view! { <p class="hint">"…"</p> }.into_any(),
                            (true, true) => cannot().into_any(),
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
                                view! {
                                    <button class="primary" id="pair-go" disabled=move || busy.get() on:click=pair>
                                        {if typed_code { "Pair it" } else { "Yes, pair it" }}
                                    </button>
                                }
                            })}
                            <button id="pair-not-mine" disabled=move || busy.get() on:click=move |_| done.set(Some(Done::Dismissed))>"No"</button>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
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
        assert_eq!(spoken("MMG4-2KLG"), "pairing code M M G 4 2 K L G");
    }

    #[test]
    fn the_field_takes_the_code_in_any_spelling_or_the_key() {
        for code in ["MMG4-2KLG", "mmg42klg", " mmg4 2klg ", "Mmg4-2Klg"] {
            assert_eq!(typed(code), Typed::Code("MMG4-2KLG".into()), "{code}");
        }
        assert_eq!(typed(FIXTURE), Typed::Key(FIXTURE.into()));
        assert_eq!(typed(&FIXTURE[4..].to_uppercase()), Typed::Key(FIXTURE.into()), "the bare hex");
        for neither in ["", "MMG4", "MMG4-2KL1", "idp:4b1c", "key:00"] {
            assert_eq!(typed(neither), Typed::Neither, "{neither}");
        }
    }

    #[test]
    fn a_pairing_answer_is_the_machines_waiting() {
        let w = |p: &str, n: &str| MembersEvent::Waiting { principal: p.into(), name: Some(n.into()), provider: None, since: 0 };
        let found = waiting(vec![w(FIXTURE, "alice-laptop"), w("key:ff", "bob-box")]).unwrap();
        assert_eq!(found.iter().map(|m| (m.name.as_deref().unwrap(), m.typed_code)).collect::<Vec<_>>(), [("alice-laptop", true), ("bob-box", true)]);
        assert!(waiting(vec![]).unwrap().is_empty());
        assert_eq!(waiting(vec![MembersEvent::Error { message: "no".into() }]), Err("no".into()));
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
