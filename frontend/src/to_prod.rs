//! DEPLOY-40: `#/deploy`'s **→ prod** — the owner promotes the release test
//! passed to the page's destination (app-prod since RUN-13), beside
//! DEPLOY-37's → test, through the same seam: menger files the request
//! (`deployments.request --environment <dest> --release <test's release>`)
//! and draws the executor's steps against it; the executor
//! (deploy/runner.mjs's `app-prod` plan: image → app-test → app-prod, the
//! owner's requests only) runs it. menger knows no command.
//!
//! Shown to an admin, or one holding `deployer` on the project; its label
//! names the release, and it is off, saying why, when test runs nothing
//! newer than the destination, when test's last deploy did not succeed, or
//! while a request to the destination is open. Pressing it opens a confirm
//! sheet: the release and its commit, then what goes in — the tickets
//! `ready for` each environment after test on the track, grouped by area as
//! the page groups them.

use leptos::prelude::*;
use menger_tickets_api::deployments::{DeployState, Deployment};
use menger_tickets_api::members::{Role, Scope};
use menger_tickets_api::{Environment, TicketSummary, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::flight::{self, Cluster};
use crate::signin::Account;
use crate::to_test::{blocked, newest, recent, request_view};
use crate::{deploys, session};

/// Whether `account` may ask for a deploy of `project`: an admin of every
/// project, or `admin` / `deployer` held where it covers the project. The
/// server decides the call; this keeps the button from one who'd be refused.
pub(crate) fn may_deploy(account: &Account, project: &str) -> bool {
    account.admin_everywhere
        || account.grants.as_ref().is_some_and(|gs| {
            gs.iter().any(|g| {
                matches!(g.role, Role::Admin | Role::Deployer)
                    && match &g.scope {
                        Scope::Everywhere | Scope::Org(_) => true,
                        Scope::Shortcode { id, .. } => id.to_string().eq_ignore_ascii_case(project),
                        _ => false,
                    }
            })
        })
}

/// A release's commit: the hash it ends in (`…T192902Z-8cb79df` → `8cb79df`).
pub(crate) fn commit_of(release: &str) -> Option<&str> {
    let (_, sha) = release.rsplit_once('-')?;
    (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then_some(sha)
}

/// The release the button promotes — what `from` (the track's first
/// environment) runs — or why it is off.
pub(crate) fn offer(from: Option<&Environment>, dest: Option<&Environment>, from_name: &str, dest_name: &str) -> Result<String, String> {
    let Some(from) = from else { return Err(format!("{from_name} is not an environment of this project")) };
    let Some(release) = from.release.clone() else { return Err(format!("{from_name} runs no release yet: nothing to promote")) };
    match from.last_state {
        Some(DeployState::Started) => return Err(format!("a deploy to {from_name} is under way: promote once it has passed")),
        Some(DeployState::Failed) => return Err(format!("{from_name}'s last deploy failed: promote once {from_name} passes again")),
        Some(DeployState::RolledBack) => return Err(format!("{from_name}'s last deploy was rolled back: promote once {from_name} passes again")),
        _ => {}
    }
    if let Some(there) = dest.and_then(|d| d.release.as_deref()) {
        // A release's name begins with when it was built: later is newer.
        if there >= release.as_str() {
            return Err(format!("{dest_name} runs {there} already: {from_name} has nothing newer"));
        }
    }
    Ok(release)
}

/// What goes in: in FILTER-3's language, the work `ready for` each
/// environment after the first on the track (`ready for menger/app-test or
/// ready for menger/app-prod`).
pub(crate) fn going_in(project: &str, track: &[String]) -> Option<String> {
    let p = project.to_ascii_lowercase();
    let cells: Vec<String> = track.iter().skip(1).map(|e| format!("ready for {p}/{e}")).collect();
    (!cells.is_empty()).then(|| format!("under {project} and not kind doc and ({})", cells.join(" or ")))
}

/// The tickets a filter admits, at most 1,000.
async fn tickets(filter: String) -> Result<Vec<TicketSummary>, String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let filter = crate::typed::typed(&filter)?;
    let events = client
        .list_all(None, None, None, None, Some(1000), Some(true), Some(filter), None, None, session::zone())
        .await
        .map_err(|e| session::describe(&e))?;
    let mut out = Vec::new();
    for ev in events {
        match ev {
            TicketsEvent::Summary { summary } => out.push(summary),
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(out)
}

/// The button, its confirm sheet, and the newest request to the destination.
#[component]
pub(crate) fn ToProd(project: Memo<Option<String>>, track: Memo<Vec<String>>) -> impl IntoView {
    let account = crate::signin::account();
    let held = deploys::held();
    let allowed = move || project.get().is_some_and(|p| account.with(|a| may_deploy(a, &p)));
    // The track's first environment (test) and its end, when the track has both.
    let ends = Memo::new(move |_| {
        let t = track.get();
        (t.len() >= 2).then(|| (t[0].clone(), t[t.len() - 1].clone()))
    });
    let envs = {
        let held = held.clone();
        Memo::new(move |_| {
            let (Some(p), Some((f, d))) = (project.get(), ends.get()) else { return (None, None) };
            held.with(|h| {
                let mine = h.as_ref().map(|h| h.of_project(&p)).unwrap_or_default();
                let of = |n: &str| mine.iter().find(|e| e.name == n).cloned();
                (of(&f), of(&d))
            })
        })
    };
    let offered = Memo::new(move |_| {
        let (Some((f, d)), (fe, de)) = (ends.get(), envs.get()) else { return Err(String::new()) };
        offer(fe.as_ref(), de.as_ref(), &f, &d)
    });
    let last = RwSignal::new(None::<Deployment>);
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let sheet = RwSignal::new(None::<Result<Vec<Cluster>, String>>);
    let link = session::link();
    let epoch = deploys::epoch();
    Effect::new(move |_| {
        epoch.track();
        let (Some(p), Some((_, d)), true) = (project.get(), ends.get(), link.get()) else { return };
        spawn_local(async move {
            match newest(p, d).await {
                Ok(d) => {
                    last.try_set(d);
                }
                Err(e) => {
                    problem.try_set(Some(e));
                }
            }
        });
    });
    let open_sheet = {
        let held = held.clone();
        move |_| {
            let (Some(p), t) = (project.get_untracked(), track.get_untracked()) else { return };
            let Some(filter) = going_in(&p, &t) else { return };
            sheet.set(Some(Ok(Vec::new())));
            busy.set(true);
            let held = held.clone();
            spawn_local(async move {
                let answer = tickets(filter).await.map(|ts| {
                    let nodes = held.with_untracked(|h| ts.iter().filter_map(|row| flight::node_of(row, &h.as_ref().map(|h| h.marks(&row.id.to_string())).unwrap_or_default(), &p, &t)).collect());
                    flight::clusters(nodes)
                });
                busy.try_set(false);
                sheet.try_set(Some(answer));
            });
        }
    };
    let confirm = move |_| {
        let (Some(p), Some((f, d)), Ok(rel)) = (project.get_untracked(), ends.get_untracked(), offered.get_untracked()) else { return };
        busy.set(true);
        problem.set(None);
        spawn_local(async move {
            let asked = crate::deploys_page::ask(p, d.clone(), None, Some(rel.clone()), None, format!("{f}'s {rel} → {d}, from #/deploy")).await;
            busy.try_set(false);
            sheet.try_set(None);
            match asked {
                Ok(d) => {
                    last.try_set(Some(d));
                }
                Err(e) => {
                    problem.try_set(Some(e));
                }
            }
        });
    };
    let cancel = move |id: u64| {
        busy.set(true);
        spawn_local(async move {
            let r = crate::deploys_page::cancel_request(id).await;
            busy.try_set(false);
            if let Err(e) = r {
                problem.try_set(Some(e));
            }
        });
    };
    let on_key = window_event_listener(leptos::ev::keydown, move |e| {
        if sheet.get_untracked().is_some() && crate::bindings::is(&e, crate::bindings::Context::Global, "app.escape") {
            sheet.set(None);
        }
    });
    on_cleanup(move || on_key.remove());
    let shown = move || last.get().filter(recent);
    move || {
        let (true, Some((from, to))) = (allowed(), ends.get()) else { return None };
        let off = move || blocked(last.get().as_ref()).map(Err).unwrap_or_else(|| offered.get().map(|_| ()));
        let label = move || match offered.get() {
            Ok(rel) => format!("→ prod · {rel}"),
            Err(_) => "→ prod".to_string(),
        };
        let (from2, to2) = (from.clone(), to.clone());
        Some(view! {
            <div class="totest toprod">
                <button
                    type="button"
                    class="tobtn"
                    id="to-prod"
                    data-dest=to.clone()
                    disabled=move || busy.get() || off().is_err()
                    title=move || off().err().unwrap_or_else(|| format!("promote what {from} passed → {to}: review what goes in, then confirm"))
                    on:click=open_sheet.clone()
                >
                    {label}
                </button>
                {move || off().err().filter(|w| !w.is_empty()).map(|w| view! { <span class="toff">{w}</span> })}
                {move || shown().map(|d| request_view(d, cancel))}
                {move || problem.get().map(|p| view! { <p class="derr">{p}</p> })}
                {move || sheet.get().map(|s| confirm_sheet(s, offered.get().ok(), from2.clone(), to2.clone(), busy, confirm, move || sheet.set(None)))}
            </div>
        })
    }
}

/// The confirm sheet: the release and its commit, what goes in, confirm.
fn confirm_sheet(
    s: Result<Vec<Cluster>, String>,
    release: Option<String>,
    from: String,
    to: String,
    busy: RwSignal<bool>,
    confirm: impl Fn(leptos::ev::MouseEvent) + Clone + 'static,
    close: impl Fn() + Copy + 'static,
) -> impl IntoView {
    let rel = release.clone().unwrap_or_default();
    let commit = commit_of(&rel).map(str::to_string);
    let body = match s {
        Err(e) => view! { <p class="derr">{e}</p> }.into_any(),
        Ok(cs) if cs.is_empty() && busy.get_untracked() => view! { <p class="pwait">"asking what goes in…"</p> }.into_any(),
        Ok(cs) if cs.is_empty() => view! { <p class="pnone">{format!("No ticket is ready for {to}: the release carries no tracked work.")}</p> }.into_any(),
        Ok(cs) => {
            let n: usize = cs.iter().map(|c| c.nodes.len()).sum();
            view! {
                <p class="pcount">{format!("{n} ticket{} ready for {to}, in {} area{}", if n == 1 { "" } else { "s" }, cs.len(), if cs.len() == 1 { "" } else { "s" })}</p>
                <div class="pgroups">
                    {cs.into_iter().map(|c| view! {
                        <section class="pgroup" data-code=c.code.clone()>
                            <h3><span class="pcode">{c.code.clone()}</span><span class="n">{c.nodes.len()}</span></h3>
                            <ul>
                                {c.nodes.into_iter().map(|n| view! {
                                    <li data-ticket-id=n.id.clone()>
                                        <a href=format!("#/ticket/{}", n.id) class="pid">{n.id.clone()}</a>
                                        <span class="ptitle2">{n.title.clone()}</span>
                                        <span class=format!("ns {}", n.state.class())>{n.state_words()}</span>
                                    </li>
                                }).collect_view()}
                            </ul>
                        </section>
                    }).collect_view()}
                </div>
            }
            .into_any()
        }
    };
    view! {
        <div class="overlay psheetwrap" on:click=move |_| close()>
            <div id="prod-sheet" class="dialog" role="dialog" aria-modal="true" aria-labelledby="prod-sheet-h" on:click=|ev| ev.stop_propagation()>
                <h2 id="prod-sheet-h">{format!("Promote to {to}")}</h2>
                <p class="prel">
                    <span class="plabel">"release"</span>
                    <code class="prelname">{rel.clone()}</code>
                    {commit.map(|c| view! { <span class="plabel">"commit"</span><code class="pcommit">{c}</code> })}
                </p>
                <p class="ppath">{format!("the release {from} passed → {to}; the executor runs it and reports each step here")}</p>
                {body}
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| close()>"Cancel"</button>
                    <button class="primary" data-act="confirm" id="prod-confirm" disabled=move || busy.get() || release.is_none() on:click=confirm>
                        {move || if busy.get() { "…" } else { "Promote" }}
                    </button>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str, release: Option<&str>, last: Option<DeployState>) -> Environment {
        Environment {
            project: "MENGER".into(),
            name: name.into(),
            order: 0,
            revision: None,
            release: release.map(str::to_string),
            since: None,
            deployment: None,
            last: None,
            last_state: last,
            last_at: None,
            tickets: 0,
            from: None,
        }
    }

    const NEW: &str = "20261019T192902Z-8cb79df";
    const OLD: &str = "20261017T094508Z-ace860b";

    #[test]
    fn the_button_offers_what_test_passed_or_says_why_not() {
        let ok = Some(DeployState::Succeeded);
        assert_eq!(offer(Some(&env("test", Some(NEW), ok)), Some(&env("app-prod", Some(OLD), ok)), "test", "app-prod"), Ok(NEW.into()));
        assert_eq!(offer(Some(&env("test", Some(NEW), ok)), None, "test", "app-prod"), Ok(NEW.into()), "a destination that runs nothing yet");
        let same = offer(Some(&env("test", Some(NEW), ok)), Some(&env("app-prod", Some(NEW), ok)), "test", "app-prod");
        assert_eq!(same, Err(format!("app-prod runs {NEW} already: test has nothing newer")));
        assert!(offer(Some(&env("test", Some(OLD), ok)), Some(&env("app-prod", Some(NEW), ok)), "test", "app-prod").is_err());
        assert_eq!(offer(Some(&env("test", Some(NEW), Some(DeployState::Failed))), None, "test", "app-prod"), Err("test's last deploy failed: promote once test passes again".into()));
        assert!(offer(Some(&env("test", Some(NEW), Some(DeployState::Started))), None, "test", "app-prod").unwrap_err().contains("under way"));
        assert!(offer(Some(&env("test", None, None)), None, "test", "app-prod").unwrap_err().contains("runs no release"));
        assert!(offer(None, None, "test", "app-prod").is_err());
    }

    #[test]
    fn what_goes_in_is_ready_for_each_step_of_the_track() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let f = going_in("MENGER", &t(&["test", "app-test", "app-prod"])).expect("a filter");
        assert_eq!(f, "under MENGER and not kind doc and (ready for menger/app-test or ready for menger/app-prod)");
        assert!(menger_tickets_api::filter::Filter::parse(&f).is_ok(), "{f}");
        assert_eq!(going_in("MENGER", &t(&["edge"])), None);
        assert_eq!(commit_of(NEW), Some("8cb79df"));
        assert_eq!(commit_of("landing"), None);
    }
}
