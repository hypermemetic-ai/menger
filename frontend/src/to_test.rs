//! DEPLOY-37: `#/deploy`'s **→ test** — the owner moves main's tip into the
//! test environment from the page, and watches it go.
//!
//! menger only files the request (`deployments.request --branch main`,
//! DEPLOY-25) and draws what the executor reports against it: each step
//! (`deployments.step` — `build`, `deploy`, … as the executor names them),
//! its state and times, a failed step's last lines, its log. Who carries it
//! out is not menger's: today deploy/runner.mjs on this machine, later
//! hyperforge's deploy.* behind the same webhook (HYPERFORGE-10). Cancel is
//! `cancel_request`: a pending request is canceled outright, one under way
//! asks its executor to stop (`cancel_asked_at`).
//!
//! Admins of every project only (`members.me`'s `admin_everywhere`), as
//! APP-90's `/deploy`; drawn when the project has a `test` environment.
//! Live: the newest request to test is asked again on every
//! `deployments.watch` request event (`deploys::epoch`).

use leptos::prelude::*;
use menger_tickets_api::deployments::{DeployState, DeployStep, Deployment, DeploymentsEvent, RequestState, StepState};
use wasm_bindgen_futures::spawn_local;

use crate::{deploys, session};

/// The environment the button moves work into, and the branch it moves.
const ENV: &str = "test";
const BRANCH: &str = "main";
/// How long an ended request stays drawn under the button.
const KEEP_SECS: i64 = 6 * 3600;

/// A request's state as a class: `pending`, `started`, `succeeded`, …
fn request_class(s: RequestState) -> &'static str {
    match s {
        RequestState::Pending => "pending",
        RequestState::Canceled => "canceled",
        RequestState::Started => "started",
        RequestState::Succeeded => "succeeded",
        RequestState::Failed => "failed",
        RequestState::RolledBack => "rolled_back",
    }
}

/// A step's time in words: `1m 20s`, `under way 40s`.
pub(crate) fn took(s: &DeployStep, now: i64) -> String {
    let secs = (s.ended_at.unwrap_or(now) - s.started_at).max(0);
    let t = if secs >= 60 { format!("{}m {:02}s", secs / 60, secs % 60) } else { format!("{secs}s") };
    match s.state {
        StepState::Started => format!("under way {t}"),
        _ => t,
    }
}

/// Why the button is off, or `None`: it may be pressed.
pub(crate) fn blocked(last: Option<&Deployment>) -> Option<String> {
    let q = last?.requested.as_ref()?;
    q.state.open().then(|| format!("request #{} to {} is {}", last.map_or(0, |d| d.id), last.map_or(ENV, |d| d.environment.as_str()), deploys::request_words(q)))
}

/// The newest request to `env` of `project` (DEPLOY-40: → prod's too).
pub(crate) async fn newest(project: String, env: String) -> Result<Option<Deployment>, String> {
    let client = session::deployments_client().ok_or_else(|| "not connected".to_string())?;
    let (p, e) = (crate::typed::typed(&project)?, crate::typed::typed(&env)?);
    let events = client.list_all(Some(p), Some(e), Some(DeployState::Requested), None, None, Some(1)).await.map_err(|e| session::describe(&e))?;
    for ev in events {
        match ev {
            DeploymentsEvent::Deployment { deployment, .. } => return Ok(Some(deployment)),
            DeploymentsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(None)
}

/// The button, and the newest request to test under it.
#[component]
pub(crate) fn ToTest(project: Memo<Option<String>>) -> impl IntoView {
    let account = crate::signin::account();
    let held = deploys::held();
    let admin = move || account.with(|a| a.admin_everywhere);
    let has_test = move || {
        let p = project.get();
        held.with(|h| h.as_ref().zip(p).is_some_and(|(h, p)| h.of_project(&p).iter().any(|e| e.name == ENV)))
    };
    let last = RwSignal::new(None::<Deployment>);
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let link = session::link();
    let epoch = deploys::epoch();
    Effect::new(move |_| {
        epoch.track();
        let (Some(p), true) = (project.get(), link.get()) else { return };
        spawn_local(async move {
            match newest(p, ENV.into()).await {
                Ok(d) => {
                    last.try_set(d);
                }
                Err(e) => {
                    problem.try_set(Some(e));
                }
            }
        });
    });
    let press = move |_| {
        let Some(p) = project.get_untracked() else { return };
        busy.set(true);
        problem.set(None);
        spawn_local(async move {
            let asked = crate::deploys_page::ask(p, ENV.into(), None, None, Some(BRANCH.into()), format!("{BRANCH} → {ENV}, from #/deploy")).await;
            busy.try_set(false);
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
    let shown = move || last.get().filter(recent);
    move || {
        (admin() && has_test()).then(|| {
            let off = move || blocked(last.get().as_ref());
            view! {
                <div class="totest">
                    <button
                        type="button"
                        class="tobtn"
                        id="to-test"
                        disabled=move || busy.get() || off().is_some()
                        title=move || off().unwrap_or_else(|| format!("build {BRANCH}'s tip and deploy it to {ENV}"))
                        on:click=press
                    >
                        {format!("→ {ENV}")}
                    </button>
                    {move || shown().map(|d| request_view(d, cancel))}
                    {move || problem.get().map(|p| view! { <p class="derr">{p}</p> })}
                </div>
            }
        })
    }
}

/// Whether a request is still drawn under its button: open, or ended in
/// the last [`KEEP_SECS`].
pub(crate) fn recent(d: &Deployment) -> bool {
    let q = d.requested.as_ref();
    q.is_some_and(|q| q.state.open()) || crate::now_unix() - d.recorded_at.max(d.at) < KEEP_SECS || q.is_some_and(|q| q.steps.iter().any(|s| s.ended_at.is_some_and(|t| crate::now_unix() - t < KEEP_SECS)))
}

/// One request: its number, what it moves, where it stands, its steps.
pub(crate) fn request_view(d: Deployment, cancel: impl Fn(u64) + Copy + 'static) -> impl IntoView {
    let Some(q) = d.requested.clone() else { return ().into_any() };
    let now = crate::now_unix();
    let what = q.branch.clone().map(|b| format!("{b}'s tip")).or_else(|| d.release.clone()).unwrap_or_else(|| deploys::short(&d.revision));
    let cancelable = q.state.open() && q.cancel_asked_at.is_none();
    let id = d.id;
    let steps = q
        .steps
        .iter()
        .map(|s| {
            let failed = s.state == StepState::Failed;
            view! {
                <li class=format!("step {}", s.state) data-step=s.name.clone()>
                    <span class="sname">{s.name.clone()}</span>
                    <span class="sstate">{s.state.to_string()}</span>
                    <span class="stime">{took(s, now)}</span>
                    {s.url.clone().map(|u| view! { <a class="slog" href=u target="_blank" rel="noopener">"log"</a> })}
                    {(failed || s.state == StepState::Canceled).then(|| s.detail.clone()).flatten().map(|t| view! { <pre class="sdetail">{t}</pre> })}
                </li>
            }
        })
        .collect_view();
    // A run stopped on a cancel ends in a failed record: it was canceled.
    let stopped = q.cancel_asked_at.is_some() && !q.state.open();
    let (class, words) = if stopped { ("canceled", "canceled") } else { (request_class(q.state), deploys::request_words(&q)) };
    view! {
        <div class=format!("treq {class}") data-request=id>
            <div class="trow">
                <span class="tid">{format!("#{id}")}</span>
                <span class="twhat">{what}" → "{d.environment.clone()}</span>
                <span class="tstate">{words}</span>
                {(q.state == RequestState::Pending).then(|| view! { <span class="thook">{deploys::webhook_words(&q)}</span> })}
                {(q.cancel_asked_at.is_some() && q.state.open()).then(|| view! { <span class="tcancel">"stopping — cancel asked"</span> })}
                {cancelable.then(|| view! { <button type="button" class="tcancelbtn" on:click=move |_| cancel(id)>"cancel"</button> })}
            </div>
            {(!q.steps.is_empty()).then(|| view! { <ol class="steps">{steps}</ol> })}
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(state: StepState, started_at: i64, ended_at: Option<i64>) -> DeployStep {
        DeployStep { name: "build".into(), state, started_at, ended_at, detail: None, url: None, by: "key:x".into() }
    }

    #[test]
    fn a_steps_time_is_its_span_or_how_long_it_has_run() {
        assert_eq!(took(&step(StepState::Succeeded, 100, Some(185)), 999), "1m 25s");
        assert_eq!(took(&step(StepState::Started, 100, None), 140), "under way 40s");
        assert_eq!(took(&step(StepState::Failed, 100, Some(90)), 0), "0s");
    }
}
