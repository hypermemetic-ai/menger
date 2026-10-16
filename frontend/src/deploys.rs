//! DEPLOY-13: the app's copy of where work runs — every environment
//! (`deployments.environments`) and its kept cells (`deployments.running`,
//! DEPLOY-11's store) — what a board row's and a wave card's marks, the
//! filter menu's environments and the palette's `deployed …` are drawn from.
//!
//! **Asked for by what shows it** — the inputs' and the comments' rule
//! (src/inputs.rs, src/comments.rs). Nothing at startup asks: the first view
//! that draws a mark — the board, a wave board, a ticket page, `#/deployments`,
//! the filter menu — calls [`want`]; from then on, on every new connection,
//! `deployments.watch` is subscribed and then the environments and every
//! cell fetched, in one read each ([`fetch`]) — never a request per row.
//!
//! **Kept by the watch.** A `deployed` moves its environment's cells at once
//! by its `entered` and `left` (each since the deploy), then asks that one
//! environment's cells again (`running { project, environment }`), which
//! answer exactly — a ticket's since is its earliest still-counting deploy;
//! `environment_changed` carries the environment whole. A ticket's commits
//! linked or unlinked (the tickets `watch`, src/session.rs) ask its own
//! cells again (`running { ids }`): the join is kept by the server, so a
//! link made after a deploy moves where it runs. `lagged` fetches all again.
//! [`epoch`] ticks on every `deployed`, so a view that asks the server
//! itself — a ticket page's Running, `#/deployments`' timelines — asks again.
//!
//! **No answer undoes a newer one.** Every ask is numbered: an environment's
//! answer lands only when nothing about it was asked since (its own ask or a
//! whole fetch), and a whole fetch's only when it is the newest.
//!
//! What decides — the marks of a ticket, an environment's place, the
//! choices the filter menu offers, a since in words — is pure and pinned by
//! native tests; the rest is the session's socket.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::deployments::rank;
use menger_tickets_api::{DeploymentsEvent, Environment, RunningCell};
use plexus_client::ClientEvent;
use wasm_bindgen_futures::spawn_local;

use crate::calendar::Day;
use crate::session;

/// One environment a ticket runs in — a mark on its row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) project: String,
    pub(crate) env: String,
    pub(crate) since: i64,
    pub(crate) deployment: u64,
    /// DEPLOY-20: its commits not carried there, when it runs an earlier
    /// version (empty: current).
    pub(crate) behind: Vec<String>,
    /// DEPLOY-20: the version that runs there, when behind.
    pub(crate) version: Option<String>,
}

/// One kept cell: since when, brought by which deploy, and (DEPLOY-20)
/// what of the ticket is not there and the version that is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct KeptCell {
    since: i64,
    deployment: u64,
    behind: Vec<String>,
    version: Option<String>,
}

/// DEPLOY-20: a ticket behind, in words — `prod runs an earlier version
/// (d607756); 68e1271, 4145520 not deployed`.
pub(crate) fn behind_words(env: &str, version: Option<&str>, behind: &[String]) -> String {
    let runs = match version {
        Some(v) => format!("{env} runs an earlier version ({})", short(v)),
        None => format!("{env} runs an earlier version"),
    };
    let not: Vec<String> = behind.iter().map(|c| short(c)).collect();
    format!("{runs}; {} not deployed", not.join(", "))
}

/// Where an environment sits among its project's: test → staging → prod
/// (DEPLOY-11's [`rank`]), ties by name.
pub(crate) fn place(project: &str, name: &str) -> (String, u8, String) {
    (project.to_string(), rank(name), name.to_string())
}

/// The environments and their cells, as the app holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Held {
    /// Every environment, by project then in display order.
    envs: Vec<Environment>,
    /// Each environment's running tickets: since when, brought by which
    /// deploy — by `(project, name)`.
    cells: BTreeMap<(String, String), BTreeMap<String, KeptCell>>,
    /// Each ticket's marks, in display order — derived from `cells`.
    marks: HashMap<String, Vec<Mark>>,
}

impl Held {
    /// The answers of `environments {}` and `running {}`.
    pub(crate) fn from_events(envs: impl IntoIterator<Item = DeploymentsEvent>, cells: impl IntoIterator<Item = DeploymentsEvent>) -> Self {
        let mut held = Held::default();
        for ev in envs {
            if let DeploymentsEvent::Environment { environment } = ev {
                held.upsert_env(environment);
            }
        }
        for ev in cells {
            held.take_cells(ev, None);
        }
        held
    }

    /// An environment, whole, in its place.
    pub(crate) fn upsert_env(&mut self, e: Environment) -> bool {
        match self.envs.iter_mut().find(|x| x.project == e.project && x.name == e.name) {
            Some(x) if *x == e => return false,
            Some(x) => *x = e,
            None => {
                self.envs.push(e);
                self.envs.sort_by_key(|e| place(&e.project, &e.name));
            }
        }
        true
    }

    /// A `cells` answer: the environment's cells — all of them, or (`only`)
    /// those of the tickets asked. Whether anything moved.
    pub(crate) fn take_cells(&mut self, ev: DeploymentsEvent, only: Option<&[String]>) -> bool {
        let DeploymentsEvent::Cells { project, environment, running } = ev else { return false };
        let key = (project, environment);
        let had = self.cells.get(&key).cloned().unwrap_or_default();
        let mut next = match only {
            None => BTreeMap::new(),
            Some(ids) => {
                let mut kept = had.clone();
                kept.retain(|t, _| !ids.iter().any(|i| i.eq_ignore_ascii_case(t)));
                kept
            }
        };
        for RunningCell { ticket, since, deployment, behind, version } in running {
            next.insert(ticket, KeptCell { since, deployment, behind, version });
        }
        if next == had {
            return false;
        }
        let touched: Vec<String> = had.keys().chain(next.keys()).cloned().collect();
        self.cells.insert(key, next);
        for t in touched {
            self.remark(&t);
        }
        true
    }

    /// A `deployed`: its `entered` run there from the deploy, its `left` no
    /// longer — what is drawn until the environment's own answer lands.
    pub(crate) fn apply_deployed(&mut self, ev: &DeploymentsEvent) -> bool {
        let DeploymentsEvent::Deployed { deployment, entered, left, duplicate: false, .. } = ev else { return false };
        let cells = self.cells.entry((deployment.project.clone(), deployment.environment.clone())).or_default();
        let mut moved = Vec::new();
        for t in entered {
            let cell = KeptCell { since: deployment.at, deployment: deployment.id, behind: Vec::new(), version: None };
            if cells.insert(t.clone(), cell).is_none() {
                moved.push(t.clone());
            }
        }
        for t in left {
            if cells.remove(t).is_some() {
                moved.push(t.clone());
            }
        }
        for t in &moved {
            self.remark(t);
        }
        !moved.is_empty()
    }

    fn remark(&mut self, ticket: &str) {
        let mut ms: Vec<Mark> = self
            .cells
            .iter()
            .filter_map(|((p, e), c)| {
                c.get(ticket).map(|c| Mark {
                    project: p.clone(),
                    env: e.clone(),
                    since: c.since,
                    deployment: c.deployment,
                    behind: c.behind.clone(),
                    version: c.version.clone(),
                })
            })
            .collect();
        ms.sort_by_key(|m| place(&m.project, &m.env));
        if ms.is_empty() {
            self.marks.remove(ticket);
        } else {
            self.marks.insert(ticket.to_string(), ms);
        }
    }

    /// Where `ticket` runs, in display order — its row's marks.
    pub(crate) fn marks(&self, ticket: &str) -> Vec<Mark> {
        self.marks.get(ticket).cloned().unwrap_or_default()
    }

    /// Every environment, by project then in display order.
    pub(crate) fn envs(&self) -> &[Environment] {
        &self.envs
    }

    /// The projects that have environments, in order.
    pub(crate) fn projects(&self) -> Vec<String> {
        let mut out: Vec<String> = self.envs.iter().map(|e| e.project.clone()).collect();
        out.dedup();
        out
    }

    /// `project`'s environments, in display order.
    pub(crate) fn of_project(&self, project: &str) -> Vec<Environment> {
        self.envs.iter().filter(|e| e.project == project).cloned().collect()
    }

    /// How many tickets run in `(project, name)`, as the app holds them.
    pub(crate) fn count(&self, project: &str, name: &str) -> usize {
        self.cells.get(&(project.to_string(), name.to_string())).map_or(0, BTreeMap::len)
    }

    /// The environments the filter menu and the palette offer, as the
    /// language writes them: each name once, in display order (`prod` is any
    /// project's), then — with more than one project — each project's own
    /// (`menger/prod`).
    pub(crate) fn choices(&self) -> Vec<String> {
        let mut names: Vec<&str> = self.envs.iter().map(|e| e.name.as_str()).collect();
        names.sort_by_key(|n| (rank(n), n.to_string()));
        names.dedup();
        let mut out: Vec<String> = names.into_iter().map(str::to_string).collect();
        if self.projects().len() > 1 {
            out.extend(self.envs.iter().map(|e| format!("{}/{}", e.project.to_ascii_lowercase(), e.name)));
        }
        out
    }
}

/// The waiting clause of `project` (DEPLOY-13): its Complete work not yet
/// on its last environment — DEPLOY-20: or running an earlier version there
/// — `None` while it has none.
pub(crate) fn waiting_filter(project: &str, envs: &[Environment]) -> Option<String> {
    let last = envs.iter().filter(|e| e.project == project).max_by_key(|e| (rank(&e.name), e.name.clone()))?;
    let env = format!("{}/{}", project.to_ascii_lowercase(), last.name);
    Some(format!("status Complete and under {project} and (not deployed {env} or behind {env})"))
}

/// The waiting work that is on its way: running on one of `project`'s
/// earlier environments — what `#/deployments` lists first (the rest of the
/// waiting, Complete before anything was recorded, is counted). `None` with
/// a single environment.
pub(crate) fn on_its_way_filter(project: &str, envs: &[Environment]) -> Option<String> {
    let waiting = waiting_filter(project, envs)?;
    let mut mine: Vec<&Environment> = envs.iter().filter(|e| e.project == project).collect();
    mine.sort_by_key(|e| (rank(&e.name), e.name.clone()));
    mine.pop();
    let p = project.to_ascii_lowercase();
    // DEPLOY-20: on its way is its current version on an earlier one.
    let earlier: Vec<String> = mine.iter().map(|e| format!("(deployed {p}/{n} and not behind {p}/{n})", n = e.name)).collect();
    match earlier.as_slice() {
        [] => None,
        [one] => Some(format!("{waiting} and {one}")),
        many => Some(format!("{waiting} and ({})", many.join(" or "))),
    }
}

/// A since in words, the reader's way: the time today (`22:04`), the day
/// this year (`Oct 12`), else the date (`Oct 12, 2025`).
pub(crate) fn since_words(now: Day, then: (Day, (u8, u8)), h12: bool) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (d, (h, m)) = then;
    let month = MONTHS[(d.m as usize).saturating_sub(1) % 12];
    if d == now {
        crate::when::clock_words(h, m, h12)
    } else if d.y == now.y {
        format!("{month} {}", d.d)
    } else {
        format!("{month} {}, {}", d.d, d.y)
    }
}

/// `unix` in the reader's zone: the day, and the time to the minute.
#[cfg(target_arch = "wasm32")]
pub(crate) fn local(unix: i64) -> (Day, (u8, u8)) {
    let t = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix as f64 * 1000.0));
    (Day::new(t.get_full_year() as i32, t.get_month() + 1, t.get_date()), (t.get_hours() as u8, t.get_minutes() as u8))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn local(unix: i64) -> (Day, (u8, u8)) {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    (Day::new(1970, 1, 1).plus_days(days), ((secs / 3600) as u8, ((secs % 3600) / 60) as u8))
}

/// A since, now, in words.
pub(crate) fn since(unix: i64) -> String {
    let now = local(crate::now_unix()).0;
    since_words(now, local(unix), crate::calendar::reader_h12())
}

/// Where a deployment is shown: its project's `#/deployments`, the
/// deployment picked out.
pub(crate) fn href(project: &str, deployment: Option<u64>) -> String {
    match deployment {
        Some(d) => format!("#/deployments/{project}?d={d}"),
        None => format!("#/deployments/{project}"),
    }
}

/// A revision as a mark shows it: git's short form, `repo@` kept.
pub(crate) fn short(rev: &str) -> String {
    match rev.rsplit_once('@') {
        Some((repo, hash)) => format!("{repo}@{}", hash.chars().take(7).collect::<String>()),
        None => rev.chars().take(7).collect(),
    }
}

thread_local! {
    /// The copy — `None` until the first fetch lands.
    static HELD: ArcRwSignal<Option<Held>> = ArcRwSignal::new(None);
    /// Ticks on every `deployed` heard.
    static EPOCH: ArcRwSignal<u32> = ArcRwSignal::new(0);
    static WANTED: Cell<bool> = const { Cell::new(false) };
    /// Every ask's number; the newest whole fetch's; each environment's
    /// newest ask.
    static ASKS: Cell<u64> = const { Cell::new(0) };
    static WHOLE: Cell<u64> = const { Cell::new(0) };
    static ENV_ASK: RefCell<HashMap<(String, String), u64>> = RefCell::new(HashMap::new());
    static PUMP: RefCell<Option<futures::future::AbortHandle>> = const { RefCell::new(None) };
}

/// The copy, as a signal.
pub(crate) fn held() -> ArcRwSignal<Option<Held>> {
    HELD.with(Clone::clone)
}

/// Ticks on every `deployed` the watch hears — what a view that asks the
/// server itself follows.
pub(crate) fn epoch() -> ArcRwSignal<u32> {
    EPOCH.with(Clone::clone)
}

/// `ticket`'s marks, as the copy says now (tracked).
pub(crate) fn marks_of(ticket: &str) -> Vec<Mark> {
    held().with(|h| h.as_ref().map(|h| h.marks(ticket)).unwrap_or_default())
}

/// A view draws marks: watch and fetch now if this is the first ask (and
/// the socket is up — else the session's [`refresh`] will).
pub(crate) fn want() {
    if !WANTED.with(|w| w.replace(true)) {
        start();
    }
}

/// A new connection, or a `lagged`: the watch on it, and everything again.
pub(crate) fn refresh() {
    if WANTED.with(Cell::get) {
        start();
    }
}

/// The tickets `watch` linked or unlinked commits of `ticket`: its cells
/// asked again.
pub(crate) fn relinked(ticket: &str) {
    if !WANTED.with(Cell::get) || HELD.with(|h| h.with_untracked(Option::is_none)) {
        return;
    }
    let Some(client) = session::deployments_client() else { return };
    let ids = vec![ticket.to_string()];
    let n = next_ask();
    spawn_local(async move {
        let Ok(asked) = crate::typed::many(&ids) else { return };
        let Ok(events) = client.running_all(None, None, Some(asked)).await else { return };
        if WHOLE.with(Cell::get) > n {
            return;
        }
        HELD.with(|s| {
            s.maybe_update(|h| {
                let Some(h) = h.as_mut() else { return false };
                let mut moved = false;
                for ev in events {
                    moved |= h.take_cells(ev, Some(&ids));
                }
                moved
            })
        });
    });
}

fn next_ask() -> u64 {
    ASKS.with(|a| {
        let n = a.get() + 1;
        a.set(n);
        n
    })
}

fn start() {
    let Some(client) = session::deployments_client() else {
        return; // not connected: the session refreshes once it is
    };
    let (abort, reg) = futures::future::AbortHandle::new_pair();
    if let Some(old) = PUMP.with(|p| p.borrow_mut().replace(abort)) {
        old.abort();
    }
    // The watch's subscribe frame goes first (this task runs before the
    // fetch's): nothing recorded between the two is lost.
    let pump = async move {
        let mut stream = client.watch();
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(ClientEvent::Data(DeploymentsEvent::Lagged { missed })) => {
                    leptos::logging::log!("deployments watch lagged: {missed} missed; fetching again");
                    fetch();
                }
                Ok(ClientEvent::Data(ev)) => heard(ev),
                Ok(_) => {}
                Err(_) => break, // the socket: the next session starts it again
            }
        }
    };
    spawn_local(async move {
        let _ = futures::future::Abortable::new(pump, reg).await;
    });
    fetch();
}

fn heard(ev: DeploymentsEvent) {
    match &ev {
        DeploymentsEvent::Deployed { deployment, duplicate, .. } => {
            if !duplicate {
                HELD.with(|s| s.maybe_update(|h| h.as_mut().is_some_and(|h| h.apply_deployed(&ev))));
                ask_env(deployment.project.clone(), deployment.environment.clone());
            }
            EPOCH.with(|e| e.update(|n| *n = n.wrapping_add(1)));
        }
        DeploymentsEvent::EnvironmentChanged { environment } => {
            let e = environment.clone();
            HELD.with(|s| s.maybe_update(|h| h.as_mut().is_some_and(|h| h.upsert_env(e))));
        }
        _ => {}
    }
}

/// One environment's cells asked again.
fn ask_env(project: String, name: String) {
    let Some(client) = session::deployments_client() else { return };
    let n = next_ask();
    let key = (project.clone(), name.clone());
    ENV_ASK.with(|m| m.borrow_mut().insert(key.clone(), n));
    spawn_local(async move {
        let (Ok(project), Ok(name)) = (crate::typed::typed(&project), crate::typed::typed(&name)) else { return };
        let Ok(events) = client.running_all(Some(project), Some(name), None).await else { return };
        let newest = ENV_ASK.with(|m| m.borrow().get(&key).copied()) == Some(n);
        if !newest || WHOLE.with(Cell::get) > n {
            return;
        }
        HELD.with(|s| {
            s.maybe_update(|h| {
                let Some(h) = h.as_mut() else { return false };
                let mut moved = false;
                for ev in events {
                    moved |= h.take_cells(ev, None);
                }
                moved
            })
        });
    });
}

/// Everything: the environments, then every cell.
fn fetch() {
    let Some(client) = session::deployments_client() else { return };
    let n = next_ask();
    WHOLE.with(|w| w.set(n));
    spawn_local(async move {
        let envs = match client.environments_all(None).await {
            Ok(e) => e,
            Err(e) => {
                leptos::logging::log!("deployments: {}", session::describe(&e));
                return;
            }
        };
        let cells = match client.running_all(None, None, None).await {
            Ok(c) => c,
            Err(e) => {
                leptos::logging::log!("deployments: {}", session::describe(&e));
                return;
            }
        };
        if WHOLE.with(Cell::get) != n {
            return; // a newer fetch went out; its answer is the one kept
        }
        for ev in envs.iter().chain(&cells) {
            if let DeploymentsEvent::Error { message } = ev {
                leptos::logging::log!("deployments: {message}");
            }
        }
        // An environment asked after this fetch went keeps its own answer's
        // turn: it lands after this one.
        HELD.with(|s| s.set(Some(Held::from_events(envs, cells))));
    });
}

/// DEPLOY-13: a ticket's marks — one small mark per environment it runs in,
/// in its project's order, each linking to the deploy that brought it. A
/// board row's, a wave card's.
pub(crate) fn row_marks(ticket: String) -> impl IntoView {
    want();
    let marks = Memo::new(move |_| marks_of(&ticket));
    move || {
        let ms = marks.get();
        (!ms.is_empty()).then(|| {
            let many = ms.iter().map(|m| &m.project).collect::<std::collections::BTreeSet<_>>().len() > 1;
            view! {
                <span class="envmarks">
                    {ms.into_iter().map(|m| {
                        let pre = if many { format!("{} ", m.project) } else { String::new() };
                        let title = match m.behind.is_empty() {
                            true => format!("running on {pre}{} since {}", m.env, since(m.since)),
                            // DEPLOY-20: an earlier version, said apart.
                            false => format!("behind — {pre}{}", behind_words(&m.env, m.version.as_deref(), &m.behind)),
                        };
                        let label = if m.env.chars().count() > 8 { m.env.chars().take(7).chain(['…']).collect() } else { m.env.clone() };
                        let behind = !m.behind.is_empty();
                        view! {
                            <a class="envmark" class:behind=behind data-env=m.env.clone() data-project=m.project.clone() data-behind=behind.then_some("true") href=href(&m.project, Some(m.deployment)) title=title>
                                {label}
                            </a>
                        }
                    }).collect_view()}
                </span>
            }
        })
    }
}

/// One entry of a ticket page's Running: where it runs (since when, by
/// which deploy) or an environment of its projects it is not on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Where {
    On { project: String, env: String, since: i64, deployment: u64, revision: Option<String>, behind: Vec<String>, version: Option<String> },
    NotOn { project: String, env: String },
}

/// `for_ticket`'s answer as the page says it: where it runs, the last
/// environment first, then where it does not, in display order — or the
/// refusal.
pub(crate) fn where_of(events: Vec<DeploymentsEvent>) -> Result<Vec<Where>, String> {
    let mut on = Vec::new();
    let mut off = Vec::new();
    for ev in events {
        match ev {
            DeploymentsEvent::Running { project, environment, since, deployment, revision, behind, version, .. } => {
                on.push(Where::On { project, env: environment, since, deployment, revision, behind, version })
            }
            DeploymentsEvent::NotRunning { project, environment, .. } => off.push(Where::NotOn { project, env: environment }),
            DeploymentsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    let key = |w: &Where| match w {
        Where::On { project, env, .. } | Where::NotOn { project, env } => place(project, env),
    };
    on.sort_by_key(|w| std::cmp::Reverse(key(w)));
    off.sort_by_key(key);
    on.extend(off);
    Ok(on)
}

/// DEPLOY-13: the ticket page's **Running** — `prod since 22:04 · test since
/// 22:04 · not on staging`, each linked to its deploy on `#/deployments`.
/// Asked (`for_ticket`) when the page opens, on every connection, on every
/// `deployed` the watch hears, and when the ticket's commits move.
#[component]
pub(crate) fn Running(id: String, commits: Memo<Vec<String>>) -> impl IntoView {
    want();
    let answer = RwSignal::new(None::<Result<Vec<Where>, String>>);
    let link = session::link();
    let ep = epoch();
    {
        let id = id.clone();
        Effect::new(move |_| {
            ep.track();
            commits.track();
            if !link.get() {
                return;
            }
            let Some(client) = session::deployments_client() else { return };
            let id = id.clone();
            spawn_local(async move {
                let got = match crate::typed::typed(&id) {
                    Ok(id) => match client.for_ticket_all(id).await {
                        Ok(events) => where_of(events),
                        Err(e) => Err(session::describe(&e)),
                    },
                    Err(why) => Err(why),
                };
                answer.try_set(Some(got));
            });
        });
    }
    move || match answer.get() {
        None => view! { <span class="none">"…"</span> }.into_any(),
        Some(Err(e)) => view! { <span class="derr" title=e.clone()>"—"</span> }.into_any(),
        Some(Ok(ws)) if ws.is_empty() => {
            view! { <span class="none" title="nothing of its project has been deployed anywhere">"—"</span> }.into_any()
        }
        Some(Ok(ws)) => {
            let many = ws
                .iter()
                .map(|w| match w {
                    Where::On { project, .. } | Where::NotOn { project, .. } => project.clone(),
                })
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1;
            let n = ws.len();
            ws.into_iter()
                .enumerate()
                .map(|(i, w)| {
                    let sep = (i + 1 < n).then_some(" · ");
                    let item = match w {
                        Where::On { project, env, since: at, deployment, revision, behind, version } if !behind.is_empty() => {
                            // DEPLOY-20: an earlier version runs there.
                            let title = format!(
                                "{project} {env} has run this ticket since {} — the version of {}; {} linked since",
                                crate::fmt_unix(at),
                                version.as_deref().map_or_else(|| "its deploy".to_string(), short),
                                behind.iter().map(|c| short(c)).collect::<Vec<_>>().join(", "),
                            );
                            let _ = revision;
                            let pre = many.then(|| format!("{project} "));
                            let runs = match &version {
                                Some(v) => view! { " runs an earlier version (" <code class="rev">{short(v)}</code> "); " }.into_any(),
                                None => view! { " runs an earlier version; " }.into_any(),
                            };
                            let not = behind.iter().map(|c| short(c)).collect::<Vec<_>>().join(", ");
                            view! {
                                <a class="run behind" data-env=env.clone() data-behind="true" href=href(&project, Some(deployment)) title=title>
                                    {pre}<span class="envmark behind">{env.clone()}</span>{runs}<code class="rev">{not}</code>" not deployed"
                                </a>
                            }
                            .into_any()
                        }
                        Where::On { project, env, since: at, deployment, revision, .. } => {
                            let title = format!(
                                "running on {project} {env} since {}{}",
                                crate::fmt_unix(at),
                                revision.map(|r| format!(" — it runs {}", short(&r))).unwrap_or_default()
                            );
                            let pre = many.then(|| format!("{project} "));
                            view! {
                                <a class="run" data-env=env.clone() href=href(&project, Some(deployment)) title=title>
                                    {pre}<span class="envmark">{env.clone()}</span>{format!(" since {}", since(at))}
                                </a>
                            }
                            .into_any()
                        }
                        Where::NotOn { project, env } => {
                            let pre = many.then(|| format!("{project} "));
                            view! {
                                <a class="notrun" data-env=env.clone() href=href(&project, None) title=format!("not running on {project} {env}")>
                                    "not on "{pre}<span class="envmark off">{env.clone()}</span>
                                </a>
                            }
                            .into_any()
                        }
                    };
                    view! { {item}{sep} }
                })
                .collect_view()
                .into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::Deployment;

    fn env(project: &str, name: &str) -> DeploymentsEvent {
        DeploymentsEvent::Environment {
            environment: Environment {
                project: project.into(),
                name: name.into(),
                order: 0,
                revision: None,
                release: None,
                since: None,
                deployment: None,
                last: None,
                last_state: None,
                last_at: None,
                tickets: 0,
            },
        }
    }

    fn cells(project: &str, name: &str, running: &[(&str, i64, u64)]) -> DeploymentsEvent {
        DeploymentsEvent::Cells {
            project: project.into(),
            environment: name.into(),
            running: running
                .iter()
                .map(|(t, s, d)| RunningCell { ticket: t.to_string(), since: *s, deployment: *d, behind: vec![], version: None })
                .collect(),
        }
    }

    fn deployed(env: &str, id: u64, at: i64, entered: &[&str], left: &[&str]) -> DeploymentsEvent {
        DeploymentsEvent::Deployed {
            deployment: Deployment {
                id,
                project: "MENGER".into(),
                environment: env.into(),
                state: menger_tickets_api::DeployState::Succeeded,
                revision: "aaaaaaa".into(),
                previous: None,
                commits: vec![],
                tickets: vec![],
                release: None,
                at,
                source: Default::default(),
                recorded_at: at,
                recorded_by: "ci".into(),
            },
            carried: vec![],
            unlinked: vec![],
            live: true,
            entered: entered.iter().map(|s| s.to_string()).collect(),
            left: left.iter().map(|s| s.to_string()).collect(),
            duplicate: false,
        }
    }

    fn envs_of(h: &Held, id: &str) -> Vec<String> {
        h.marks(id).into_iter().map(|m| m.env).collect()
    }

    /// The copy from the two answers: a ticket's marks in its project's
    /// order (test → staging → prod), whatever order the answers came in.
    #[test]
    fn marks_come_in_the_projects_order() {
        let h = Held::from_events(
            [env("MENGER", "prod"), env("MENGER", "test"), env("MENGER", "staging")],
            [cells("MENGER", "prod", &[("MENGER-2", 10, 1)]), cells("MENGER", "test", &[("MENGER-2", 5, 2), ("MENGER-3", 5, 2)])],
        );
        assert_eq!(h.envs().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["test", "staging", "prod"]);
        assert_eq!(envs_of(&h, "MENGER-2"), ["test", "prod"]);
        assert_eq!(envs_of(&h, "MENGER-3"), ["test"]);
        assert!(h.marks("MENGER-4").is_empty());
        assert_eq!(
            h.marks("MENGER-2")[1],
            Mark { project: "MENGER".into(), env: "prod".into(), since: 10, deployment: 1, behind: vec![], version: None }
        );
        assert_eq!((h.count("MENGER", "test"), h.count("MENGER", "staging")), (2, 0));
    }

    /// A `deployed` moves the marks at once by its entered and left; the
    /// environment's own answer then replaces its cells whole; an answer for
    /// some ids replaces only theirs.
    #[test]
    fn a_deploy_then_its_answer_then_a_relink() {
        let mut h = Held::from_events([env("MENGER", "test"), env("MENGER", "prod")], [cells("MENGER", "test", &[("MENGER-2", 5, 1)])]);
        assert!(h.apply_deployed(&deployed("prod", 2, 50, &["MENGER-2", "MENGER-3"], &[])));
        assert_eq!(envs_of(&h, "MENGER-3"), ["prod"]);
        assert!(!h.apply_deployed(&deployed("prod", 2, 50, &["MENGER-2"], &[])), "nothing new");
        // The rollback leaves.
        assert!(h.apply_deployed(&deployed("prod", 3, 60, &[], &["MENGER-3"])));
        assert!(h.marks("MENGER-3").is_empty());
        // The answer is the truth: since the earliest deploy that still counts.
        assert!(h.take_cells(cells("MENGER", "prod", &[("MENGER-2", 40, 1)]), None));
        assert_eq!(h.marks("MENGER-2")[1].since, 40);
        assert!(!h.take_cells(cells("MENGER", "prod", &[("MENGER-2", 40, 1)]), None), "the same answer moves nothing");
        // A relink: MENGER-4 asked, MENGER-2 kept.
        let ids = vec!["MENGER-4".to_string()];
        assert!(h.take_cells(cells("MENGER", "prod", &[("MENGER-4", 40, 1)]), Some(&ids)));
        assert_eq!(envs_of(&h, "MENGER-2"), ["test", "prod"]);
        assert_eq!(envs_of(&h, "MENGER-4"), ["prod"]);
        assert!(h.take_cells(cells("MENGER", "prod", &[]), Some(&ids)), "unlinked: out");
        assert!(h.marks("MENGER-4").is_empty());
        let mut dup = deployed("prod", 2, 50, &["MENGER-9"], &[]);
        if let DeploymentsEvent::Deployed { duplicate, .. } = &mut dup {
            *duplicate = true;
        }
        assert!(!h.apply_deployed(&dup), "a retry moves nothing");
    }

    /// The menu's environments: each name once in order; with two projects,
    /// each project's own too. And the waiting clause names the last one.
    #[test]
    fn choices_and_the_waiting_clause() {
        let one = Held::from_events([env("MENGER", "prod"), env("MENGER", "test")], []);
        assert_eq!(one.choices(), ["test", "prod"]);
        let two = Held::from_events([env("MENGER", "prod"), env("MENGER", "test"), env("PANTRY", "prod")], []);
        assert_eq!(two.choices(), ["test", "prod", "menger/test", "menger/prod", "pantry/prod"]);
        assert_eq!(two.projects(), ["MENGER", "PANTRY"]);
        assert_eq!(
            waiting_filter("MENGER", two.envs()).as_deref(),
            Some("status Complete and under MENGER and (not deployed menger/prod or behind menger/prod)")
        );
        assert_eq!(waiting_filter("NONE", two.envs()), None);
        let f = menger_tickets_api::filter::Filter::parse(&waiting_filter("MENGER", two.envs()).unwrap()).unwrap();
        assert_eq!(f.groups.len(), 3, "the language reads it");
        assert_eq!(
            on_its_way_filter("MENGER", two.envs()).as_deref(),
            Some("status Complete and under MENGER and (not deployed menger/prod or behind menger/prod) and (deployed menger/test and not behind menger/test)")
        );
        assert_eq!(on_its_way_filter("PANTRY", two.envs()), None, "one environment: nothing earlier");
        let three = Held::from_events([env("MENGER", "prod"), env("MENGER", "test"), env("MENGER", "staging")], []);
        let way = on_its_way_filter("MENGER", three.envs()).unwrap();
        assert!(
            way.ends_with("and ((deployed menger/test and not behind menger/test) or (deployed menger/staging and not behind menger/staging))"),
            "{way}"
        );
        assert_eq!(menger_tickets_api::filter::Filter::parse(&way).unwrap().groups.len(), 4);
    }

    /// The page's Running: the last environment first, then the rest it
    /// runs in, then where it is not.
    #[test]
    fn running_reads_prod_then_test_then_not_on_staging() {
        let on = |e: &str, s| DeploymentsEvent::Running {
            ticket: "MENGER-2".into(),
            project: "MENGER".into(),
            environment: e.into(),
            since: s,
            deployment: 1,
            revision: None,
            release: None,
            behind: vec![],
            version: None,
        };
        let off = |e: &str| DeploymentsEvent::NotRunning { ticket: "MENGER-2".into(), project: "MENGER".into(), environment: e.into() };
        let ws = where_of(vec![on("test", 5), off("staging"), on("prod", 9)]).unwrap();
        let names: Vec<String> = ws
            .iter()
            .map(|w| match w {
                Where::On { env, .. } => format!("on {env}"),
                Where::NotOn { env, .. } => format!("not {env}"),
            })
            .collect();
        assert_eq!(names, ["on prod", "on test", "not staging"]);
        assert_eq!(where_of(vec![DeploymentsEvent::Error { message: "no ticket X".into() }]), Err("no ticket X".to_string()));
    }

    /// DEPLOY-20: a cell behind makes its mark behind — the versions said
    /// in words — and the next answer makes it current; a page's Running
    /// carries it.
    #[test]
    fn a_behind_cell_marks_its_row_apart() {
        let behind = DeploymentsEvent::Cells {
            project: "MENGER".into(),
            environment: "prod".into(),
            running: vec![RunningCell {
                ticket: "LOOK-7".into(),
                since: 10,
                deployment: 4,
                behind: vec!["68e1271".into(), "4145520".into()],
                version: Some("d607756".into()),
            }],
        };
        let mut h = Held::from_events([env("MENGER", "test"), env("MENGER", "prod")], [behind, cells("MENGER", "test", &[("LOOK-7", 20, 5)])]);
        let ms = h.marks("LOOK-7");
        assert_eq!(ms.iter().map(|m| (m.env.as_str(), m.behind.len())).collect::<Vec<_>>(), [("test", 0), ("prod", 2)]);
        assert_eq!(
            behind_words("prod", ms[1].version.as_deref(), &ms[1].behind),
            "prod runs an earlier version (d607756); 68e1271, 4145520 not deployed"
        );
        assert_eq!(behind_words("prod", None, &["0123456789abcdef".to_string()]), "prod runs an earlier version; 0123456 not deployed");
        // The next promote's answer: current.
        assert!(h.take_cells(cells("MENGER", "prod", &[("LOOK-7", 10, 4)]), None));
        assert!(h.marks("LOOK-7").iter().all(|m| m.behind.is_empty()));
        let ws = where_of(vec![DeploymentsEvent::Running {
            ticket: "LOOK-7".into(),
            project: "MENGER".into(),
            environment: "prod".into(),
            since: 10,
            deployment: 4,
            revision: Some("4145520".into()),
            release: None,
            behind: vec!["68e1271".into()],
            version: Some("d607756".into()),
        }])
        .unwrap();
        assert!(matches!(&ws[0], Where::On { behind, version: Some(v), .. } if behind.len() == 1 && v == "d607756"));
    }

    /// A since in words: the time today, the day this year, else the date.
    #[test]
    fn since_reads_the_readers_way() {
        let today = Day::new(2026, 10, 13);
        assert_eq!(since_words(today, (today, (22, 4)), false), "22:04");
        assert_eq!(since_words(today, (today, (22, 4)), true), "10:04 pm");
        assert_eq!(since_words(today, (Day::new(2026, 10, 12), (9, 0)), false), "Oct 12");
        assert_eq!(since_words(today, (Day::new(2025, 1, 2), (9, 0)), false), "Jan 2, 2025");
        assert_eq!(href("MENGER", Some(4)), "#/deployments/MENGER?d=4");
        assert_eq!(short("0123456789abcdef"), "0123456");
        assert_eq!(short("synapse@0123456789"), "synapse@0123456");
    }
}
