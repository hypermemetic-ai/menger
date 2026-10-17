//! DEPLOY-13: `#/deployments` — per project, its environments as columns
//! (test → staging → prod), each with what it runs and a timeline of its
//! deploys: the state, when, the source, and the tickets each carried; above
//! them, the project's **waiting** work — Complete, not yet on its last
//! environment (`status Complete and under P and not deployed p/prod`, the
//! filter cell DEPLOY-11 keeps), each row with where it does run.
//!
//! **DEPLOY-21: a lane per promotion edge** — *test → prod: N ready* —
//! between the waiting work and the columns: the work whose current
//! version runs where the environment promotes from and not on it
//! (`ready for <p>/<env>`), the first few listed, what is deploying there
//! and what failed there counted beside it; each count a link that filters
//! every view by its cell. The counts are live (a membership watch each);
//! the list asks again on every deploy and every count that moves.
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
//!
//! **DEPLOY-25: Queue a deploy.** Each column has a *Queue a deploy*: what
//! it would deploy (by default what the environment it promotes from runs,
//! with the work that would move in — `ready for <p>/<env>`), a note, then
//! Queue (`deployments.request`). The column's open requests sit at its top
//! — *requested by … · webhook delivered / retrying / failed / no CI* — and
//! follow the CI's records live (`request` on `deployments.watch` ticks the
//! epoch); a pending one can be canceled. Each arrives, and each change of
//! its state is drawn, with the row's entrance (Motion's rules: `none` stops
//! it).

use leptos::prelude::*;
use menger_tickets_api::deployments::{DeliveryStage, RequestState};
use menger_tickets_api::{DeployState, Deployment, DeploymentsEvent, Environment, TicketSummary, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::deploys::{self, Held};
use crate::{fmt_relative, fmt_unix, now_unix, session, st_class};

/// How many deploys a timeline shows at first, and asks per "older".
const PAGE: u32 = 10;

/// How many waiting tickets are listed (the rest are counted).
const WAITING: u32 = 25;

/// How many ready tickets a lane lists (the rest are counted).
const LANE: u32 = 8;

/// How many of a deploy's tickets show before "+n more".
const CARRIED: usize = 8;

/// DEPLOY-25: how many requests a column asks for, to show the open ones.
const REQUESTS: u32 = 10;

/// DEPLOY-25: how many tickets the Queue form's preview lists.
const MOVES: u32 = 8;

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
        (DeployState::Requested, _) => "requested".into(),
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
            <Lanes project=project.clone() />
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
    let filter = crate::typed::typed(&filter)?;
    let events = client
        .list_all(None, None, None, None, Some(limit), Some(true), Some(filter), None, None, session::zone())
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
            lens.apply(crate::lens::Level::Global, Some(crate::saved::chosen(f)));
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
                                    <li data-ticket-id=t.id.to_string()>
                                        <a class="code" href=href.clone()>{t.id.to_string()}</a>
                                        <a class="dtitle" href=href>{t.title.clone()}</a>
                                        <span class=format!("tag {}", st_class(t.status.as_str()))>{t.status.as_str()}</span>
                                        {deploys::row_marks(t.id.to_string())}
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

/// DEPLOY-21: a stage's cell in one project's environment, as the language
/// writes it — `ready for menger/prod`.
pub(crate) fn lane_filter(stage: menger_tickets_api::deployments::DeliveryStage, project: &str, env: &str) -> String {
    format!("{stage} {}/{env}", project.to_ascii_lowercase())
}

/// DEPLOY-21: every promotion edge of `project`, a lane each.
#[component]
fn Lanes(project: String) -> impl IntoView {
    let held = deploys::held();
    let edges = {
        let project = project.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().map(|h| h.edges(&project)).unwrap_or_default()))
    };
    view! {
        <div class="dlanes">
            <For
                each=move || edges.get()
                key=|e| e.clone()
                children=move |(from, to): (String, String)| view! { <Lane project=project.clone() from to /> }
            />
        </div>
    }
}

/// The whole of every view filtered by `filter`, and the board shown.
fn on_board(lens: crate::lens::Lens, filter: &str) {
    if let Ok(f) = menger_tickets_api::filter::Filter::parse(filter) {
        lens.apply(crate::lens::Level::Global, Some(crate::saved::chosen(f)));
        crate::dom::window().location().set_hash("#/").ok();
    }
}

/// DEPLOY-21: one promotion edge — *test → prod: N ready* — the ready work
/// listed, deploying and failed counted; each count filters every view.
#[component]
fn Lane(project: String, from: String, to: String) -> impl IntoView {
    use menger_tickets_api::deployments::DeliveryStage;
    let ready_f = lane_filter(DeliveryStage::ReadyFor, &project, &to);
    let deploying_f = lane_filter(DeliveryStage::Deploying, &project, &to);
    let failed_f = lane_filter(DeliveryStage::Failed, &project, &to);
    let ready = crate::counts::LiveCounts::new(Signal::stored(Some(ready_f.clone())));
    let deploying = crate::counts::LiveCounts::new(Signal::stored(Some(deploying_f.clone())));
    let failed = crate::counts::LiveCounts::new(Signal::stored(Some(failed_f.clone())));
    let rows = RwSignal::new(None::<Result<Vec<TicketSummary>, String>>);
    let epoch = deploys::epoch();
    let link = session::link();
    {
        let ready_f = ready_f.clone();
        Effect::new(move |_| {
            epoch.track();
            ready.n();
            if !link.get() {
                return;
            }
            let f = ready_f.clone();
            spawn_local(async move {
                let answer = matching(f, LANE).await.map(|(rows, _)| rows);
                rows.try_set(Some(answer));
            });
        });
    }
    let lens = crate::lens::use_lens();
    let n = move |c: crate::counts::LiveCounts| c.n().map_or_else(|| "…".to_string(), |n| n.to_string());
    let (rf, df, ff) = (ready_f.clone(), deploying_f.clone(), failed_f.clone());
    let none = format!("Nothing waits for {to}: what runs on {from} runs there too.");
    view! {
        <div class="dlane" data-from=from.clone() data-to=to.clone() data-ready=move || ready.n().map(|n| n.to_string())>
            <h4>
                <span class="envmark">{from.clone()}</span>
                <span class="larrow" aria-hidden="true">"→"</span>
                <span class="envmark ready">{to.clone()}</span>
                <button type="button" class="dlink lready" title=format!("filter every view by {rf}") on:click=move |_| on_board(lens, &rf)>
                    // Keyed by the count: a new count arrives, as a state change.
                    {move || {
                        let k = n(ready);
                        let shown = k.clone();
                        view! { <span class="lcount" data-n=k>{shown}</span> }
                    }}
                    " ready"
                </button>
                <span class="lstages">
                    <button type="button" class="dlink ldeploying" class:some=move || deploying.n().is_some_and(|n| n > 0) title=format!("filter every view by {df}") on:click=move |_| on_board(lens, &df)>
                        {move || n(deploying)}" deploying"
                    </button>
                    " · "
                    <button type="button" class="dlink lfailed" class:some=move || failed.n().is_some_and(|n| n > 0) title=format!("filter every view by {ff}") on:click=move |_| on_board(lens, &ff)>
                        {move || n(failed)}" failed"
                    </button>
                </span>
            </h4>
            <p class="hint"><code>{ready_f.clone()}</code></p>
            {move || match rows.get() {
                None => view! { <p class="loading">"…"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="derr">{e}</p> }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! { <p class="none">{none.clone()}</p> }.into_any(),
                Some(Ok(list)) => {
                    let total = ready.n().unwrap_or(list.len());
                    let more = (total > list.len()).then(|| view! { <li class="more">{format!("and {} more ready", total - list.len())}</li> });
                    view! {
                        <ul class="drows lrows">
                            {list.into_iter().map(|t| {
                                let href = format!("#/ticket/{}", t.id);
                                view! {
                                    <li data-ticket-id=t.id.to_string()>
                                        <a class="code" href=href.clone()>{t.id.to_string()}</a>
                                        <a class="dtitle" href=href>{t.title.clone()}</a>
                                        {deploys::row_marks(t.id.to_string())}
                                    </li>
                                }
                            }).collect_view()}
                            {more}
                        </ul>
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
    let (project, name) = (crate::typed::typed(&project)?, crate::typed::typed(&name)?);
    let after = after.map(menger_tickets_api::vocab::DeploymentId::new);
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

/// DEPLOY-25: whether a row is an open request — shown at the column's
/// top, not in its timeline.
fn open_request(r: &Row) -> bool {
    r.d.requested.as_ref().is_some_and(|q| q.state.open())
}

/// DEPLOY-25: the open requests to one environment, newest first.
async fn open_requests(project: String, name: String) -> Result<Vec<Row>, String> {
    let client = session::deployments_client().ok_or_else(|| "not connected".to_string())?;
    let (project, name) = (crate::typed::typed(&project)?, crate::typed::typed(&name)?);
    let events = client
        .list_all(Some(project), Some(name), Some(DeployState::Requested), None, None, Some(REQUESTS))
        .await
        .map_err(|e| session::describe(&e))?;
    let mut rows = Vec::new();
    for ev in events {
        match ev {
            DeploymentsEvent::Deployment { deployment, carried, unlinked, live } => rows.push(Row { d: deployment, carried, unlinked, live }),
            DeploymentsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    rows.retain(open_request);
    Ok(rows)
}

/// A row's key in a list: it is drawn again when anything it shows moves
/// (DEPLOY-25: a request's state, its webhook, its attempts).
type RowKey = (u64, bool, usize, usize, Option<(RequestState, menger_tickets_api::deployments::WebhookState, usize)>);

fn row_key(r: &Row) -> RowKey {
    (r.d.id, r.live, r.carried.len(), r.unlinked.len(), r.d.requested.as_ref().map(|q| (q.state, q.webhook, q.attempts.len())))
}

/// One environment's column: what it runs, then its deploys, newest first.
#[component]
fn EnvColumn(project: String, name: String, env: Memo<Option<Environment>>, picked: Option<u64>) -> impl IntoView {
    let requests = RwSignal::new(Vec::<Row>::new());
    {
        let (project, name) = (project.clone(), name.clone());
        let (epoch, link) = (deploys::epoch(), session::link());
        Effect::new(move |_| {
            epoch.track();
            if !link.get() {
                return;
            }
            let (project, name) = (project.clone(), name.clone());
            spawn_local(async move {
                if let Ok(open) = open_requests(project, name).await {
                    requests.try_set(open);
                }
            });
        });
    }
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
    // DEPLOY-25: the open requests are at the top, not in the timeline.
    let timeline = move || rows.get().into_iter().filter(|r| !open_request(r)).collect::<Vec<Row>>();
    view! {
        <div class="denv" role="listitem" data-env=name.clone()>
            {head}
            <Queue project=project.clone() name=name.clone() />
            {move || {
                let open = requests.get();
                (!open.is_empty()).then(|| view! {
                    <ol class="drequests" aria-label="requested deploys">
                        <For each=move || requests.get() key=row_key children=move |r: Row| request_view(r, picked) />
                    </ol>
                })
            }}
            <div class="dcount">{move || {
                let n = count();
                format!("{n} ticket{} running here", if n == 1 { "" } else { "s" })
            }}</div>
            {move || problem.get().map(|p| view! { <p class="derr">{p}</p> })}
            <ol class="dtimeline">
                <For each=timeline key=row_key children=move |r: Row| timeline_row(r, picked) />
            </ol>
            {move || next.get().map(|_| view! { <button type="button" class="dmore" on:click=older.clone()>"older deploys"</button> })}
        </div>
    }
}

/// A timeline's row: a deploy, or (DEPLOY-25) a request no longer open.
fn timeline_row(r: Row, picked: Option<u64>) -> AnyView {
    match r.d.state {
        DeployState::Requested => request_view(r, picked).into_any(),
        _ => deploy_view(r, picked).into_any(),
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
        (DeployState::Requested, _) => "requested",
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

/// DEPLOY-25: one request — *requested by … · webhook …*, its revision, its
/// note, what the CI made of it, what it would carry; a pending one may be
/// canceled. Drawn again (with its entrance) as it moves.
fn request_view(r: Row, picked: Option<u64>) -> impl IntoView {
    let Row { d, carried, .. } = r;
    let Some(q) = d.requested.clone() else {
        return view! { <li class="ddeploy dreq" data-deploy=d.id>"requested"</li> }.into_any();
    };
    let state = q.state;
    let cls = match state {
        RequestState::Pending => "pending",
        RequestState::Canceled => "canceled",
        RequestState::Started => "started",
        RequestState::Succeeded => "ok",
        RequestState::Failed | RequestState::RolledBack => "failed",
    };
    let hook = deploys::webhook_words(&q);
    let attempts = q
        .attempts
        .iter()
        .map(|a| format!("attempt {} at {}: {}", a.attempt, fmt_unix(a.at), a.status.map_or_else(|| a.error.clone().unwrap_or_default(), |s| s.to_string())))
        .collect::<Vec<_>>()
        .join("\n");
    let from = q.from.clone().map(|f| view! { <span class="dfrom">" — what "<span class="envmark">{f}</span>" ran"</span> });
    let project_of_row = d.project.clone();
    let outcome = q.outcome.map(|o| {
        view! { <a class="doutcome" href=deploys::href(&project_of_row, Some(o)) on:click=move |ev| { ev.prevent_default(); scroll_to(o); }>{format!("→ deploy {o}")}</a> }
    });
    let canceled = q.canceled_by.clone().map(|by| view! { <span class="dhint">{format!("by {by}")}</span> });
    let problem = RwSignal::new(None::<String>);
    let cancel = (state == RequestState::Pending).then(|| {
        let id = d.id;
        view! {
            <button type="button" class="dlink dcancel" title="withdraw this request (its CI has not picked it up)" on:click=move |_| {
                spawn_local(async move {
                    if let Err(e) = cancel_request(id).await {
                        problem.try_set(Some(e));
                    }
                });
            }>"cancel"</button>
        }
    });
    let more = carried.len().saturating_sub(CARRIED);
    let chips = carried
        .iter()
        .take(CARRIED)
        .map(|t| view! { <a class="code" href=format!("#/ticket/{t}") data-ticket-id=t.clone()>{t.clone()}</a> })
        .collect_view();
    view! {
        <li class=format!("ddeploy dreq {cls}") class:picked=picked == Some(d.id) data-deploy=d.id data-state="requested" data-request=state.as_str() data-webhook=q.webhook.as_str()>
            <div class="dline">
                <span class="dstate requested">"requested"</span>
                <code class="rev" title=d.revision.clone()>{deploys::short(&d.revision)}</code>
                {from}
                <time title=fmt_unix(d.at)>{fmt_relative(now_unix(), d.at)}</time>
            </div>
            {d.release.clone().map(|r| view! { <div class="rel">{r}</div> })}
            <div class="dby">"by "{d.recorded_by.clone()}" · "<span class=format!("dhook {}", q.webhook.as_str()) title=attempts>{hook}</span></div>
            {q.note.clone().map(|n| view! { <div class="dnote">{n}</div> })}
            <div class="dreqstate">
                <span class=format!("dreqword {cls}")>{deploys::request_words(&q)}</span>
                {outcome}
                {canceled}
                {cancel}
            </div>
            {(!carried.is_empty() && state == RequestState::Pending).then(|| view! {
                <div class="dcarried" aria-label="the tickets it would carry">
                    {chips}
                    {(more > 0).then(|| view! { <span class="dhint">{format!("+{more} more")}</span> })}
                </div>
            })}
            {move || problem.get().map(|p| view! { <p class="derr">{p}</p> })}
        </li>
    }
    .into_any()
}

/// The deployment `id`, scrolled to where it is drawn.
fn scroll_to(id: u64) {
    if let Some(el) = crate::dom::window().document().and_then(|d| d.query_selector(&format!("[data-deploy=\"{id}\"]")).ok().flatten()) {
        el.scroll_into_view();
    }
}

/// DEPLOY-25: `deployments.cancel_request`, its refusal in words.
async fn cancel_request(id: u64) -> Result<(), String> {
    let client = session::deployments_client().ok_or_else(|| "not connected".to_string())?;
    let events = client.cancel_request_all(menger_tickets_api::vocab::DeploymentId::new(id)).await.map_err(|e| session::describe(&e))?;
    match events.into_iter().find_map(|e| match e {
        DeploymentsEvent::Error { message } => Some(message),
        _ => None,
    }) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

/// DEPLOY-25: what a Queue would deploy when it names no revision — the
/// environment it promotes from, and what that runs (revision, release).
fn source_of(project: &str, name: &str) -> Option<(String, Option<String>, Option<String>)> {
    deploys::held().with(|h| {
        let h = h.as_ref()?;
        let from = h.from_of(project, name)?;
        let src = h.of_project(project).into_iter().find(|e| e.name == from);
        Some((from, src.as_ref().and_then(|e| e.revision.clone()), src.and_then(|e| e.release)))
    })
}

/// DEPLOY-25: `deployments.request` — the request as kept, or why not.
async fn queue(project: String, name: String, revision: String, note: String) -> Result<Deployment, String> {
    let client = session::deployments_client().ok_or_else(|| "not connected".to_string())?;
    let (p, e) = (crate::typed::typed(&project)?, crate::typed::typed(&name)?);
    let (rev, note) = (crate::typed::opt(Some(revision))?, crate::typed::opt(Some(note))?);
    let events = client.request_all(p, e, rev, None, note).await.map_err(|e| session::describe(&e))?;
    for ev in events {
        match ev {
            DeploymentsEvent::Request { deployment, .. } => return Ok(deployment),
            DeploymentsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Err("no answer".into())
}

/// DEPLOY-25: a column's **Queue a deploy** — closed, a button; open, what
/// it would deploy (the source's revision, or one named), the work that
/// would move in, a note, and Queue.
#[component]
fn Queue(project: String, name: String) -> impl IntoView {
    let open = RwSignal::new(false);
    let revision = RwSignal::new(String::new());
    let note = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let source = {
        let (project, name) = (project.clone(), name.clone());
        Memo::new(move |_| source_of(&project, &name))
    };
    let moves = RwSignal::new(None::<Result<(Vec<TicketSummary>, u64), String>>);
    {
        let filter = lane_filter(DeliveryStage::ReadyFor, &project, &name);
        let (epoch, link) = (deploys::epoch(), session::link());
        Effect::new(move |_| {
            epoch.track();
            if !open.get() || !link.get() || source.with(Option::is_none) {
                return;
            }
            let f = filter.clone();
            spawn_local(async move {
                moves.try_set(Some(matching(f, MOVES).await));
            });
        });
    }
    let submit = {
        let (project, name) = (project.clone(), name.clone());
        move |ev: leptos::ev::SubmitEvent| {
            ev.prevent_default();
            if busy.get_untracked() {
                return;
            }
            busy.set(true);
            problem.set(None);
            let (project, name, rev, why) = (project.clone(), name.clone(), revision.get_untracked(), note.get_untracked());
            spawn_local(async move {
                match queue(project, name, rev, why).await {
                    Ok(_) => {
                        open.try_set(false);
                        revision.try_set(String::new());
                        note.try_set(String::new());
                    }
                    Err(e) => {
                        problem.try_set(Some(e));
                    }
                }
                busy.try_set(false);
            });
        }
    };
    let env_name = name.clone();
    let what = move || {
        let named = revision.with(|r| !r.trim().is_empty());
        match (named, source.get()) {
            (true, _) => view! { <span>"Deploys the revision named below."</span> }.into_any(),
            (false, Some((from, Some(rev), release))) => view! {
                <span>"Deploys what "<span class="envmark">{from}</span>" runs: "<code class="rev" title=rev.clone()>{deploys::short(&rev)}</code>
                {release.map(|r| view! { <span class="rel">{format!(" · {r}")}</span> })}</span>
            }
            .into_any(),
            (false, Some((from, None, _))) => view! { <span class="derr">{format!("{from} runs nothing yet: name a revision.")}</span> }.into_any(),
            (false, None) => view! { <span>{format!("{} promotes from no environment: name a revision.", env_name.clone())}</span> }.into_any(),
        }
    };
    let preview = move || {
        if revision.with(|r| !r.trim().is_empty()) || source.with(Option::is_none) {
            return None;
        }
        Some(match moves.get() {
            None => view! { <p class="loading">"…"</p> }.into_any(),
            Some(Err(e)) => view! { <p class="derr">{e}</p> }.into_any(),
            Some(Ok((list, _))) if list.is_empty() => view! { <p class="none">"Nothing new moves in: it runs here already."</p> }.into_any(),
            Some(Ok((list, n))) => {
                let more = (n as usize > list.len()).then(|| view! { <li class="more">{format!("and {} more", n as usize - list.len())}</li> });
                view! {
                    <p class="dsub">{format!("Moves in — {n} ticket{}:", if n == 1 { "" } else { "s" })}</p>
                    <ul class="drows dqmoves">
                        {list.into_iter().map(|t| {
                            let href = format!("#/ticket/{}", t.id);
                            view! {
                                <li data-ticket-id=t.id.to_string()>
                                    <a class="code" href=href.clone()>{t.id.to_string()}</a>
                                    <a class="dtitle" href=href>{t.title.clone()}</a>
                                </li>
                            }
                        }).collect_view()}
                        {more}
                    </ul>
                }
                .into_any()
            }
        })
    };
    let label = format!("Queue a deploy to {name}");
    view! {
        <div class="dqueue" class:open=move || open.get()>
            {move || (!open.get()).then(|| view! {
                <button type="button" class="dqbtn" title=label.clone() on:click=move |_| { problem.set(None); open.set(true); }>"Queue a deploy"</button>
            })}
            {move || open.get().then(|| view! {
                <form class="dqform" on:submit=submit.clone()>
                    <div class="dqwhat">{what.clone()}</div>
                    {preview}
                    <label class="dqfield">
                        <span>"Revision"</span>
                        <input type="text" name="revision" autocomplete="off" spellcheck="false" placeholder=move || source.get().and_then(|(_, r, _)| r).map_or_else(|| "a commit, e.g. 235ff62".to_string(), |r| format!("default: {}", deploys::short(&r)))
                            prop:value=move || revision.get() on:input=move |ev| revision.set(event_target_value(&ev)) />
                    </label>
                    <label class="dqfield">
                        <span>"Note"</span>
                        <textarea name="note" rows="2" placeholder="why — the CI gets it too" prop:value=move || note.get() on:input=move |ev| note.set(event_target_value(&ev))></textarea>
                    </label>
                    {move || problem.get().map(|p| view! { <p class="derr">{p}</p> })}
                    <div class="dqact">
                        <button type="submit" class="dqgo" disabled=move || busy.get()>{move || if busy.get() { "Queueing…" } else { "Queue" }}</button>
                        <button type="button" class="dlink" on:click=move |_| open.set(false)>"cancel"</button>
                    </div>
                </form>
            })}
        </div>
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
