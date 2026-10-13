//! DEPLOY-13: `#/deployments` — per project, its environments as columns
//! (test → staging → prod), each with what it runs and a timeline of its
//! deploys: the state, when, the source, and the tickets each carried; above
//! them, the project's **waiting** work — Complete, not yet on its last
//! environment (`status Complete and under P and not deployed p/prod`, the
//! filter cell DEPLOY-11 keeps), each row with where it does run.
//!
//! `#/deployments` shows every project that has environments;
//! `#/deployments/<SHORTCODE>` one; `?d=<id>` picks one deployment out
//! (a Running link on a ticket page, a board row's mark) — scrolled to and
//! highlighted.
//!
//! **Live.** The environments are the app's copy (src/deploys.rs), kept by
//! `deployments.watch`; every `deployed` ticks [`crate::deploys::epoch`],
//! and each timeline and the waiting list ask again — one `list` per
//! environment, one filtered `list` per project, never a request per row.
//!
//! A source's link leaves menger through MENGER-14's dialog
//! (`data-links="deployment"`).

use leptos::prelude::*;
use menger_tickets_api::{DeployState, Deployment, DeploymentsEvent, Environment, TicketSummary, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::deploys::{self, Held};
use crate::{fmt_relative, fmt_unix, now_unix, session, st_class};

/// How many deploys a timeline shows at first, and asks per "older".
const PAGE: u32 = 10;

/// How many waiting tickets are listed (the rest are counted).
const WAITING: u32 = 25;

/// How many of a deploy's tickets show before "+n more".
const CARRIED: usize = 8;

/// `?d=<id>` of a hash — the deployment picked out.
pub(crate) fn picked_of(hash: &str) -> Option<u64> {
    let (_, q) = hash.split_once('?')?;
    q.split('&').find_map(|p| p.strip_prefix("d=")).and_then(|v| v.parse().ok())
}

/// A state as the timeline says it: `succeeded`, `failed`, `started`,
/// `rolled back`; a succeeded deploy a rollback (or an older revision) undid
/// says so.
pub(crate) fn state_words(state: DeployState, live: bool) -> String {
    match (state, live) {
        (DeployState::Succeeded, false) => "succeeded · undone".into(),
        (DeployState::RolledBack, _) => "rolled back".into(),
        (s, _) => s.as_str().into(),
    }
}

/// Where a deploy came from, in words: `menger-pipeline · promote`.
pub(crate) fn source_words(d: &Deployment) -> Option<String> {
    let parts: Vec<&str> = [d.source.system.as_deref(), d.source.run.as_deref()].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[component]
pub(crate) fn DeploymentsPage(project: Option<String>) -> impl IntoView {
    deploys::want();
    let held = deploys::held();
    let picked = picked_of(&crate::dom::window().location().hash().unwrap_or_default());
    let only = project.clone();
    let projects = {
        let held = held.clone();
        Memo::new(move |_| match &only {
            Some(p) => vec![p.clone()],
            None => held.with(|h| h.as_ref().map(Held::projects).unwrap_or_default()),
        })
    };
    let loaded = {
        let held = held.clone();
        move || held.with(Option::is_some)
    };
    let title = match &project {
        Some(p) => format!("Deployments — {p}"),
        None => "Deployments".to_string(),
    };
    // The picked deployment, scrolled to once it is drawn.
    if let Some(d) = picked {
        Effect::new(move |done: Option<bool>| {
            if done == Some(true) {
                return true;
            }
            let Some(doc) = crate::dom::window().document() else { return false };
            match doc.query_selector(&format!("[data-deploy=\"{d}\"]")).ok().flatten() {
                Some(el) => {
                    el.scroll_into_view();
                    true
                }
                None => {
                    // Drawn later: look again on the copy's next move.
                    deploys::epoch().track();
                    false
                }
            }
        });
    }
    view! {
        <div id="deployments-page">
            <h2>{title}</h2>
            {project.as_ref().map(|_| view! { <p class="dback"><a href="#/deployments">"← every project"</a></p> })}
            {move || {
                if !loaded() {
                    return view! { <p class="loading">"loading the environments…"</p> }.into_any();
                }
                let ps = projects.get();
                if ps.is_empty() || held.with(|h| h.as_ref().is_some_and(|h| ps.iter().all(|p| h.of_project(p).is_empty()))) {
                    return view! {
                        <div class="dnone">
                            <p>"No deployment recorded" {project.as_ref().map(|p| format!(" for {p}"))} " yet."</p>
                            <p class="hint">"A CI records one with "<code>"deployments.record"</code>" — "<code>"deploy/record --send"</code>", or "
                                <code>"synapse menger deployments record --project menger --environment prod --state succeeded --revision <sha>"</code>"."</p>
                        </div>
                    }
                    .into_any();
                }
                ps.into_iter().map(|p| view! { <ProjectView project=p picked /> }).collect_view().into_any()
            }}
        </div>
    }
}

/// One project: its waiting work, then its environments as columns.
#[component]
fn ProjectView(project: String, picked: Option<u64>) -> impl IntoView {
    let held = deploys::held();
    let envs = {
        let project = project.clone();
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().map(|h| h.of_project(&project)).unwrap_or_default()))
    };
    let names = Memo::new(move |_| envs.with(|es| es.iter().map(|e| e.name.clone()).collect::<Vec<_>>()));
    let last = Memo::new(move |_| envs.with(|es| es.last().map(|e| e.name.clone())));
    let col_project = project.clone();
    view! {
        <section class="dproject" data-project=project.clone()>
            <h3>
                <a href=deploys::href(&project, None)>{project.clone()}</a>
                <span class="n">{move || {
                    let n = envs.with(Vec::len);
                    format!(" · {n} environment{}", if n == 1 { "" } else { "s" })
                }}</span>
            </h3>
            <Waiting project=project.clone() last />
            <div class="denvs" role="list">
                <For
                    each=move || names.get()
                    key=|n| n.clone()
                    children=move |name: String| {
                        let env = {
                            let name = name.clone();
                            Memo::new(move |_| envs.with(|es| es.iter().find(|e| e.name == name).cloned()))
                        };
                        view! { <EnvColumn project=col_project.clone() name env picked /> }
                    }
                />
            </div>
        </section>
    }
}

/// One filtered `list`: its first `limit` rows and how many match in all.
async fn matching(filter: String, limit: u32) -> Result<(Vec<TicketSummary>, u64), String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let events = client
        .list_all(None, None, None, None, Some(limit), Some(true), Some(filter), None, None, session::tz())
        .await
        .map_err(|e| session::describe(&e))?;
    let mut out = Vec::new();
    let mut remaining = 0;
    for ev in events {
        match ev {
            TicketsEvent::Summary { summary } => out.push(summary),
            TicketsEvent::Page { remaining: r, .. } => remaining = r,
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    let n = out.len() as u64 + remaining;
    Ok((out, n))
}

/// What the waiting box shows: the work on its way (on an earlier
/// environment, not yet on the last) listed, how many of it, and how many
/// wait in all (the rest Complete before anything carried it).
#[derive(Clone, Debug, PartialEq)]
struct Waits {
    listed: Vec<TicketSummary>,
    on_its_way: u64,
    all: u64,
}

/// The project's Complete work not yet on its last environment — what is on
/// its way listed first (it runs on an earlier one), the rest counted.
#[component]
fn Waiting(project: String, last: Memo<Option<String>>) -> impl IntoView {
    let rows = RwSignal::new(None::<Result<Waits, String>>);
    let epoch = deploys::epoch();
    let link = session::link();
    let filters = {
        let project = project.clone();
        Memo::new(move |_| {
            deploys::held().with(|h| {
                h.as_ref().and_then(|h| Some((deploys::waiting_filter(&project, h.envs())?, deploys::on_its_way_filter(&project, h.envs()))))
            })
        })
    };
    let filter = Memo::new(move |_| filters.get().map(|(f, _)| f));
    Effect::new(move |_| {
        epoch.track();
        let up = link.get();
        let Some((all, way)) = filters.get() else { return };
        if !up {
            return;
        }
        spawn_local(async move {
            let answer = async {
                let (some, n_all) = matching(all, WAITING).await?;
                Ok(match way {
                    Some(w) => {
                        let (listed, n_way) = matching(w, WAITING).await?;
                        Waits { listed, on_its_way: n_way, all: n_all }
                    }
                    None => Waits { on_its_way: n_all, listed: some, all: n_all },
                })
            }
            .await;
            rows.try_set(Some(answer));
        });
    });
    let lens = crate::lens::use_lens();
    let on_board = move |_| {
        if let Some(f) = filter.get_untracked().and_then(|f| menger_tickets_api::filter::Filter::parse(&f).ok()) {
            lens.edit(crate::lens::Level::Global, |k| *k = crate::saved::chosen(f));
            crate::dom::window().location().set_hash("#/").ok();
        }
    };
    view! {
        <div class="dwaiting">
            <h4>
                "Waiting for "
                <span class="envmark">{move || last.get().unwrap_or_default()}</span>
                <span class="n">{move || rows.with(|r| match r {
                    Some(Ok(w)) => format!(" · {} Complete, not yet there", w.all),
                    _ => String::new(),
                })}</span>
                <button type="button" class="dlink" title="filter every view by this" on:click=on_board>"on the board ›"</button>
            </h4>
            <p class="hint">{move || filter.get().map(|f| view! { <code>{f}</code> })}</p>
            {move || match rows.get() {
                None => view! { <p class="loading">"…"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="derr">{e}</p> }.into_any(),
                Some(Ok(w)) if w.all == 0 => view! { <p class="none">"Nothing waits: everything Complete runs there."</p> }.into_any(),
                Some(Ok(w)) => {
                    let more = (w.on_its_way as usize > w.listed.len()).then(|| view! { <li class="more">{format!("and {} more on its way", w.on_its_way as usize - w.listed.len())}</li> });
                    let rest = w.all.saturating_sub(w.on_its_way);
                    let rest = (rest > 0 && w.on_its_way != w.all).then(|| {
                        view! { <p class="hint">{format!("{rest} more Complete ran on no earlier environment — finished before deployments were recorded, or not deployed yet.")}</p> }
                    });
                    let way = (w.on_its_way > 0 && w.on_its_way != w.all).then(|| view! { <p class="dsub">"On its way — on an earlier environment, not yet on this one:"</p> });
                    view! {
                        {way}
                        <ul class="drows">
                            {w.listed.into_iter().map(|t| {
                                let href = format!("#/ticket/{}", t.id);
                                view! {
                                    <li data-ticket-id=t.id.clone()>
                                        <a class="code" href=href.clone()>{t.id.clone()}</a>
                                        <a class="dtitle" href=href>{t.title.clone()}</a>
                                        <span class=format!("tag {}", st_class(t.status.as_str()))>{t.status.as_str()}</span>
                                        {deploys::row_marks(t.id.clone())}
                                    </li>
                                }
                            }).collect_view()}
                            {more}
                        </ul>
                        {rest}
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

/// One deploy as the timeline shows it.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    d: Deployment,
    carried: Vec<String>,
    unlinked: Vec<String>,
    live: bool,
}

async fn page(project: String, name: String, after: Option<u64>) -> Result<(Vec<Row>, Option<u64>), String> {
    let client = session::deployments_client().ok_or_else(|| "not connected".to_string())?;
    let events = client
        .list_all(Some(project), Some(name), None, None, after, Some(PAGE))
        .await
        .map_err(|e| session::describe(&e))?;
    let mut rows = Vec::new();
    let mut next = None;
    for ev in events {
        match ev {
            DeploymentsEvent::Deployment { deployment, carried, unlinked, live } => rows.push(Row { d: deployment, carried, unlinked, live }),
            DeploymentsEvent::Page { next: n } => next = n,
            DeploymentsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok((rows, next))
}

/// One environment's column: what it runs, then its deploys, newest first.
#[component]
fn EnvColumn(project: String, name: String, env: Memo<Option<Environment>>, picked: Option<u64>) -> impl IntoView {
    let rows = RwSignal::new(Vec::<Row>::new());
    let next = RwSignal::new(None::<u64>);
    let problem = RwSignal::new(None::<String>);
    let epoch = deploys::epoch();
    let link = session::link();
    {
        let (project, name) = (project.clone(), name.clone());
        Effect::new(move |_| {
            epoch.track();
            if !link.get() {
                return;
            }
            let (project, name) = (project.clone(), name.clone());
            // The newest page again; older pages already shown stay.
            spawn_local(async move {
                match page(project, name, None).await {
                    Ok((fresh, n)) => {
                        let shown = rows.try_with_untracked(Vec::len).unwrap_or(0);
                        rows.try_update(|r| {
                            let older: Vec<Row> = if shown > fresh.len() {
                                let oldest_fresh = fresh.last().map(|x| (x.d.at, x.d.id));
                                r.iter().filter(|x| oldest_fresh.is_some_and(|o| (x.d.at, x.d.id) < o)).cloned().collect()
                            } else {
                                Vec::new()
                            };
                            *r = fresh;
                            r.extend(older);
                        });
                        if shown <= PAGE as usize {
                            next.try_set(n);
                        }
                        problem.try_set(None);
                    }
                    Err(e) => {
                        problem.try_set(Some(e));
                    }
                }
            });
        });
    }
    let older = {
        let (project, name) = (project.clone(), name.clone());
        move |_| {
            let Some(after) = next.get_untracked() else { return };
            let (project, name) = (project.clone(), name.clone());
            spawn_local(async move {
                match page(project, name, Some(after)).await {
                    Ok((more, n)) => {
                        rows.try_update(|r| r.extend(more));
                        next.try_set(n);
                    }
                    Err(e) => {
                        problem.try_set(Some(e));
                    }
                }
            });
        }
    };
    let count = {
        let (project, name) = (project.clone(), name.clone());
        move || deploys::held().with(|h| h.as_ref().map_or(0, |h| h.count(&project, &name)))
    };
    let head = move || {
        env.get().map(|e| {
            let runs = match &e.revision {
                Some(r) => view! { "runs " <code class="rev" title=r.clone()>{deploys::short(r)}</code> }.into_any(),
                None => view! { <span class="none">"nothing deployed yet"</span> }.into_any(),
            };
            let release = e.release.clone().map(|r| view! { <div class="rel" title="release">{r}</div> });
            let since = e.since.map(|s| view! { <span class="since" title=fmt_unix(s)>{format!(" since {}", deploys::since(s))}</span> });
            view! {
                <div class="dhead">
                    <div class="dname"><span class="envmark big">{e.name.clone()}</span></div>
                    <div class="druns">{runs}{since}</div>
                    {release}
                </div>
            }
        })
    };
    view! {
        <div class="denv" role="listitem" data-env=name.clone()>
            {head}
            <div class="dcount">{move || {
                let n = count();
                format!("{n} ticket{} running here", if n == 1 { "" } else { "s" })
            }}</div>
            {move || problem.get().map(|p| view! { <p class="derr">{p}</p> })}
            <ol class="dtimeline">
                <For
                    each=move || rows.get()
                    key=|r| (r.d.id, r.live, r.carried.len(), r.unlinked.len())
                    children=move |r: Row| deploy_view(r, picked)
                />
            </ol>
            {move || next.get().map(|_| view! { <button type="button" class="dmore" on:click=older.clone()>"older deploys"</button> })}
        </div>
    }
}

fn deploy_view(r: Row, picked: Option<u64>) -> impl IntoView {
    let Row { d, carried, unlinked, live } = r;
    let state = d.state;
    let cls = match (state, live) {
        (DeployState::Succeeded, true) => "ok",
        (DeployState::Succeeded, false) => "undone",
        (DeployState::Failed, _) => "failed",
        (DeployState::Started, _) => "started",
        (DeployState::RolledBack, _) => "rolledback",
    };
    let source = source_words(&d);
    let url = d.source.url.clone().filter(|u| !u.trim().is_empty());
    let source_view = match (source, url) {
        (None, None) => None,
        (words, Some(u)) => Some(
            view! { <span class="dsrc" data-links="deployment"><a href=u.clone() title=u rel="noopener noreferrer">{words.unwrap_or_else(|| "the run".into())}" ↗"</a></span> }.into_any(),
        ),
        (Some(w), None) => Some(view! { <span class="dsrc">{w}</span> }.into_any()),
    };
    let more = carried.len().saturating_sub(CARRIED);
    let all = RwSignal::new(false);
    let shown = carried.clone();
    let chips = move || {
        let n = if all.get() { shown.len() } else { CARRIED.min(shown.len()) };
        shown[..n]
            .iter()
            .map(|t| view! { <a class="code" href=format!("#/ticket/{t}") data-ticket-id=t.clone()>{t.clone()}</a> })
            .collect_view()
    };
    let rollback = (state == DeployState::RolledBack).then(|| {
        let from = d.previous.as_deref().map(|p| format!(" from {}", deploys::short(p))).unwrap_or_default();
        format!("back to {}{from}", deploys::short(&d.revision))
    });
    let rev = (state != DeployState::RolledBack).then(|| view! { <code class="rev" title=d.revision.clone()>{deploys::short(&d.revision)}</code> });
    view! {
        <li class=format!("ddeploy {cls}") class:picked=picked == Some(d.id) data-deploy=d.id data-state=state.as_str() data-live=live.to_string()>
            <div class="dline">
                <span class=format!("dstate {cls}")>{state_words(state, live)}</span>
                {rev}
                {rollback.map(|w| view! { <span class="dback-to">{w}</span> })}
                <time title=fmt_unix(d.at)>{fmt_relative(now_unix(), d.at)}</time>
            </div>
            {d.release.clone().map(|r| view! { <div class="rel">{r}</div> })}
            {source_view}
            {(!carried.is_empty()).then(|| view! {
                <div class="dcarried" aria-label="the tickets it carried">
                    {chips}
                    {(more > 0).then(|| view! {
                        <button type="button" class="dlink" on:click=move |_| all.update(|a| *a = !*a)>
                            {move || if all.get() { "fewer".to_string() } else { format!("+{more} more") }}
                        </button>
                    })}
                </div>
            })}
            {(!unlinked.is_empty()).then(|| {
                let n = unlinked.len();
                view! { <div class="dunlinked" title=unlinked.join(" ")>{format!("{n} commit{} no ticket links", if n == 1 { "" } else { "s" })}</div> }
            })}
        </li>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picked_deploy_and_a_states_words() {
        assert_eq!(picked_of("#/deployments/MENGER?d=12"), Some(12));
        assert_eq!(picked_of("#/deployments/MENGER?g=x&d=3"), Some(3));
        assert_eq!(picked_of("#/deployments/MENGER"), None);
        assert_eq!(state_words(DeployState::Succeeded, true), "succeeded");
        assert_eq!(state_words(DeployState::Succeeded, false), "succeeded · undone");
        assert_eq!(state_words(DeployState::RolledBack, false), "rolled back");
        assert_eq!(state_words(DeployState::Failed, false), "failed");
    }
}
