//! DEPLOY-47: **Ask the builder about this** — a right-click (a long press on
//! a phone) on a deployment, a run's step or one of its log lines opens a
//! menu of one item; choosing it opens a chat with the builder that already
//! holds the deploy's context (owner, 2026-10-20: "we need to be able to
//! right click a deployment log and start an agent chat about it").
//!
//! **Where.** Anything drawn inside a `[data-deploy="<id>"]` — `#/deploy`'s
//! → test / → prod run (src/to_test.rs's `request_view`: the run, each
//! `[data-step]`, each `[data-line]` of a step's log), each deploy's row on
//! `#/deployments`, a ticket page's Running link. A ticket element nearer
//! than the deployment (a carried ticket's chip in a row) keeps the one
//! ticket menu; the one menu's listeners (src/menu/mod.rs) hand everything
//! else here — the right-click, Android's long press and APP-57's held
//! touch alike ([`at`], [`open_at`]).
//!
//! **The chat.** On the project's root ticket (the deploy's shortcode's root,
//! MENGER-1 for menger), with the builder (the candidate named `builder`; a
//! standing chat member joins every chat anyway), titled `deploy <env>
//! <release>` ([`title`]) — `chats.open` with a title answers the open chat
//! of that title among the same people (DEPLOY-47's change to CHAT-3), so a
//! second right-click on the same deploy continues it. A project with no
//! root ticket falls back to the direct message with the builder.
//!
//! **The first message** is the deploy's context ([`context`]): project,
//! environment, deployment and request ids, release, revision, state, the
//! failing (or picked) step, the line picked, and that step's last lines in
//! a fenced block. It is put in the chat's composer as its draft
//! (src/composer.rs's `Drafts`) with the cursor after it, not posted: the
//! chat bridge answers every message it hears, so a posted log would get its
//! answer before the owner's question — kept in the composer, the first turn
//! the bridge answers holds the question and the log together.
//!
//! **The log.** A step's lines are its `detail`: since DEPLOY-47 the runner
//! (deploy/runner.mjs) sends every ended step's command and last 40 lines
//! with its `deployments step`, so the app has them to quote — a step's
//! `url` is on the runner's machine alone.
//!
//! What decides — the title, the context, the window of lines, the fence,
//! the draft, which chat is reused, who the builder is — is pure and pinned
//! by native tests; the rest is the browser.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::deployments::{DeployStep, Deployment, DeploymentsEvent, StepState};
use menger_tickets_api::{Chat, ChatState};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::chat_pane::{self, View};
use crate::composer::{Drafts, Local};
use crate::{chats, deploys, dom, menu, nav, session};

/// The menu's DOM id.
const MENU_ID: &str = "dmenu";
/// How many of a step's lines the first message quotes.
pub(crate) const LINES: usize = 40;
/// How long after it opens a scroll is one already under way (a fling
/// settling, the line scrolled into view in its log box), not a reason to
/// close the menu.
pub(crate) const SETTLE_MS: f64 = 250.0;
/// The name the builder's key signs in with (`synapse … --as builder`).
pub(crate) const BUILDER: &str = "builder";

/// What was right-clicked: a deployment, and inside it a step and a line of
/// its log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Picked {
    pub(crate) deployment: u64,
    pub(crate) step: Option<String>,
    pub(crate) line: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct Open {
    picked: Picked,
    x: f64,
    y: f64,
}

thread_local! {
    /// Every deployment a view drew — what the menu builds its context from
    /// without asking again ([`keep`]).
    static SEEN: RefCell<HashMap<u64, Deployment>> = RefCell::new(HashMap::new());
    /// The mounted menu's open state, for the one menu's listeners.
    static OPEN: Cell<Option<RwSignal<Option<Open>>>> = const { Cell::new(None) };
    /// When it last opened (ms since the epoch).
    static OPENED_AT: Cell<f64> = const { Cell::new(0.0) };
}

/// A view drew `d`: kept for the menu's context.
pub(crate) fn keep(d: &Deployment) {
    SEEN.with(|s| s.borrow_mut().insert(d.id, d.clone()));
}

// -- what decides --------------------------------------------------------------

/// The chat's title: `deploy <env> <release>` — the release the deploy (or
/// its request) names, else its short revision, else `#<request or id>`.
pub(crate) fn title(d: &Deployment, req: Option<&Deployment>) -> String {
    let what = d
        .release
        .clone()
        .or_else(|| req.and_then(|r| r.release.clone()))
        .filter(|r| !r.trim().is_empty())
        .or_else(|| (!d.revision.trim().is_empty()).then(|| deploys::short(&d.revision)))
        .unwrap_or_else(|| format!("#{}", d.request.unwrap_or(d.id)));
    format!("deploy {} {what}", d.environment)
}

/// The steps a deployment's run reports: its own (a request), else its request's.
fn steps<'a>(d: &'a Deployment, req: Option<&'a Deployment>) -> &'a [DeployStep] {
    d.requested.as_ref().or_else(|| req.and_then(|r| r.requested.as_ref())).map_or(&[], |q| q.steps.as_slice())
}

/// The step the message is about: the one picked, else the first that
/// failed, else the last with lines.
pub(crate) fn step_of<'a>(steps: &'a [DeployStep], picked: Option<&str>) -> Option<&'a DeployStep> {
    picked
        .and_then(|p| steps.iter().find(|s| s.name == p))
        .or_else(|| steps.iter().find(|s| s.state == StepState::Failed))
        .or_else(|| steps.iter().rev().find(|s| s.detail.as_deref().is_some_and(|t| !t.trim().is_empty())))
}

/// Which of `n` lines are quoted: all of them up to [`LINES`]; around the
/// line picked (it in the middle, as far as the ends allow); else the last.
pub(crate) fn window(n: usize, picked: Option<usize>) -> std::ops::Range<usize> {
    if n <= LINES {
        return 0..n;
    }
    match picked.filter(|&p| p < n) {
        Some(p) => {
            let start = p.saturating_sub(LINES / 2).min(n - LINES);
            start..start + LINES
        }
        None => n - LINES..n,
    }
}

/// A fence longer than any run of backticks in `text` (at least three).
pub(crate) fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

/// The first message: the deploy's context, then the step's lines in a
/// fenced block — `d` the deployment right-clicked, `req` its request (when
/// it is a record that answers one), `step` and `line` what was picked.
pub(crate) fn context(d: &Deployment, req: Option<&Deployment>, step: Option<&str>, line: Option<usize>) -> String {
    let state = match d.requested.as_ref() {
        Some(q) => format!("requested — {}", deploys::request_words(q)),
        None => d.state.as_str().to_string(),
    };
    let mut out = format!("**Deploy #{}** · {} → {} · {state}\n\n", d.id, d.project, d.environment);
    let mut facts = Vec::new();
    if let Some(r) = d.release.as_deref().or_else(|| req.and_then(|r| r.release.as_deref())).filter(|r| !r.trim().is_empty()) {
        facts.push(format!("- release `{r}`"));
    }
    if !d.revision.trim().is_empty() {
        facts.push(format!("- revision `{}`", d.revision));
    } else if let Some(b) = d.requested.as_ref().and_then(|q| q.branch.as_deref()) {
        facts.push(format!("- revision: {b}'s tip"));
    }
    match (d.requested.is_some(), d.request) {
        (true, _) => facts.push(format!("- request #{}", d.id)),
        (false, Some(r)) => facts.push(format!("- request #{r}")),
        _ => {}
    }
    let all = steps(d, req);
    let failed = all.iter().find(|s| s.state == StepState::Failed);
    if let Some(f) = failed {
        facts.push(format!("- failing step `{}`", f.name));
    }
    let chosen = step_of(all, step);
    if let Some(s) = chosen.filter(|s| failed.map(|f| &f.name) != Some(&s.name)) {
        facts.push(format!("- step `{}` — {}", s.name, s.state));
    }
    let lines: Vec<&str> = chosen.and_then(|s| s.detail.as_deref()).map(|t| t.trim_end().lines().collect()).unwrap_or_default();
    if let Some(text) = line.and_then(|l| lines.get(l)) {
        facts.push(format!("- the line picked: `{}`", text.trim().replace('`', "'")));
    }
    out.push_str(&facts.join("\n"));
    if let (Some(s), false) = (chosen, lines.is_empty()) {
        let w = window(lines.len(), line);
        let body = lines[w.clone()].join("\n");
        let f = fence(&body);
        let which = if w.len() == lines.len() { format!("The last lines of `{}`", s.name) } else { format!("Lines {}–{} of `{}`", w.start + 1, w.end, s.name) };
        out.push_str(&format!("\n\n{which}:\n\n{f}text\n{body}\n{f}"));
    } else if chosen.is_none() && all.is_empty() {
        out.push_str("\n\n(no steps reported for it)");
    }
    out
}

/// The composer's draft: what it held (kept, above), then the context, then
/// a blank line for the question.
pub(crate) fn draft(held: &str, context: &str) -> String {
    let held = held.trim_end();
    if held.contains(context) {
        format!("{held}\n\n")
    } else if held.is_empty() {
        format!("{context}\n\n")
    } else {
        format!("{held}\n\n{context}\n\n")
    }
}

/// Whether a scroll at `now` closes a menu opened at `opened` — not one
/// already under way as it opened ([`SETTLE_MS`]).
pub(crate) fn settled(now: f64, opened: f64) -> bool {
    now - opened > SETTLE_MS
}

/// The reader's open chat on its ticket titled `title`, to continue.
pub(crate) fn reuse(chats: &[Chat], title: &str) -> Option<u64> {
    chats.iter().find(|c| c.state == ChatState::Open && c.title.as_deref() == Some(title)).map(|c| c.id.get())
}

/// The builder among the candidates: the one named [`BUILDER`] that reads.
pub(crate) fn builder_of(cands: &[(String, bool)], name: impl Fn(&str) -> Option<String>) -> Option<String> {
    cands.iter().find(|(p, reads)| *reads && name(p).as_deref() == Some(BUILDER)).map(|(p, _)| p.clone())
}

// -- the page ------------------------------------------------------------------

/// What a right-click on `el` picks, when a deployment is nearer than any
/// ticket element — `None`: the one ticket menu (or the browser's).
pub(crate) fn at(el: &web_sys::Element) -> Option<Picked> {
    let dep = el.closest("[data-deploy]").ok().flatten()?;
    if let Some(host) = menu::host_of(el) {
        if !host.contains(Some(&dep)) || host.is_same_node(Some(&dep)) {
            return None;
        }
    }
    let deployment = dep.get_attribute("data-deploy")?.parse().ok()?;
    let inside = |sel: &str| el.closest(sel).ok().flatten().filter(|e| dep.contains(Some(e)));
    let step = inside("[data-step]").and_then(|s| s.get_attribute("data-step"));
    let line = inside("[data-line]").and_then(|l| l.get_attribute("data-line")).and_then(|n| n.parse().ok());
    Some(Picked { deployment, step, line })
}

/// Open the menu on `picked` at `(x, y)` — the one menu's listeners call it.
pub(crate) fn open_at(picked: Picked, x: f64, y: f64) {
    if let Some(open) = OPEN.with(Cell::get) {
        OPENED_AT.with(|t| t.set(js_sys::Date::now()));
        open.set(Some(Open { picked, x, y }));
    }
}

/// The deployment `id`: as a view drew it, else asked for (`list` from just
/// above it, one row).
async fn deployment(id: u64) -> Result<Deployment, String> {
    if let Some(d) = SEEN.with(|s| s.borrow().get(&id).cloned()) {
        return Ok(d);
    }
    let client = session::deployments_client().ok_or_else(|| "not connected".to_string())?;
    let after = Some(menger_tickets_api::vocab::DeploymentId::new(id + 1));
    let events = client.list_all(None, None, None, None, after, Some(1)).await.map_err(|e| session::describe(&e))?;
    for ev in events {
        match ev {
            DeploymentsEvent::Deployment { deployment, .. } if deployment.id == id => {
                keep(&deployment);
                return Ok(deployment);
            }
            DeploymentsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Err(format!("deployment {id} is not there"))
}

/// The project's root ticket — where its deploys' chats are opened.
fn root_of(tree: nav::Tree, project: &str) -> Option<String> {
    tree.with_groups(|g| g.and_then(|m| m.get(project)).and_then(|g| g.root.as_ref().map(|r| r.to_string())))
}

/// Ask: the chat found or opened, its draft the context, the reader taken to it.
async fn ask(p: Picked, tree: nav::Tree) -> Result<(), String> {
    let d = deployment(p.deployment).await?;
    let req = match (d.requested.is_none(), d.request) {
        (true, Some(r)) => deployment(r).await.ok(),
        _ => None,
    };
    let text = context(&d, req.as_ref(), p.step.as_deref(), p.line);
    let title = title(&d, req.as_ref());
    let root = root_of(tree, &d.project.to_ascii_uppercase());
    chats::want();
    let found = root.as_deref().and_then(|r| chats::signal().with_untracked(|h| h.as_ref().and_then(|h| reuse(&h.on(r, ChatState::Open), &title))));
    let chat = match found {
        Some(c) => c,
        None => {
            let cands = chats::candidates(root.as_deref()).await.unwrap_or_default();
            let named = chats::signal();
            let builder = builder_of(&cands, |p| named.with_untracked(|h| h.as_ref().and_then(|h| h.name_of(p)).map(str::to_string)));
            match (root.as_deref(), builder) {
                (Some(r), b) => chats::open(Some(r), &b.into_iter().collect::<Vec<_>>(), Some(&title)).await?,
                (None, Some(b)) => chats::open(None, &[b], None).await?,
                (None, None) => return Err(format!("{} has no root ticket and the builder is not among your candidates", d.project)),
            }
        }
    };
    let held = Drafts::restore(&Local, chat);
    Drafts::keep(&mut Local, chat, &draft(&held, &text));
    let hash = match root {
        Some(r) => {
            chat_pane::open_view(&r, View::Chat(chat));
            chat_pane::with_chat(&crate::links::ticket_href(&r), Some(chat))
        }
        None => format!("#/chats/{chat}"),
    };
    let _ = dom::window().location().set_hash(&hash);
    Ok(())
}

fn close(open: RwSignal<Option<Open>>) {
    if open.with_untracked(Option::is_some) {
        open.set(None);
    }
}

fn install(open: RwSignal<Option<Open>>) {
    let doc = dom::window().document().expect("document");
    let on_down = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if open.with_untracked(Option::is_none) {
            return;
        }
        let inside = dom::window().document().and_then(|d| d.get_element_by_id(MENU_ID)).is_some_and(|m| {
            ev.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()).is_some_and(|n| m.contains(Some(&n)))
        });
        if !inside {
            close(open);
        }
    });
    let _ = doc.add_event_listener_with_callback_and_bool("mousedown", on_down.as_ref().unchecked_ref(), true);
    on_down.forget();
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if bindings::is(&ev, Context::Menu, "dialog.close") {
            close(open);
        }
    });
    let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();
    let on_scroll = Closure::<dyn FnMut()>::new(move || {
        if settled(js_sys::Date::now(), OPENED_AT.with(Cell::get)) {
            close(open);
        }
    });
    let _ = doc.add_event_listener_with_callback_and_bool("scroll", on_scroll.as_ref().unchecked_ref(), true);
    on_scroll.forget();
    let on_route = Closure::<dyn FnMut()>::new(move || close(open));
    let _ = dom::window().add_event_listener_with_callback("hashchange", on_route.as_ref().unchecked_ref());
    on_route.forget();
}

/// Mounted once by the App beside the ticket menu: the open menu, if any.
#[component]
pub(crate) fn DeployMenu(tree: nav::Tree) -> impl IntoView {
    let open = RwSignal::new(None::<Open>);
    OPEN.with(|o| o.set(Some(open)));
    install(open);
    // APP-116: the menu's one item opens a chat; with chat hidden it never opens.
    move || open.get().filter(|_| crate::chats::ENABLED).map(|o| menu_view(o, open, tree))
}

fn menu_view(o: Open, open: RwSignal<Option<Open>>, tree: nav::Tree) -> impl IntoView {
    let Open { picked, x, y } = o;
    let root = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        let Some(el) = root.get() else { return };
        menu::place(&el, x, y);
        if let Some(first) = menu::items(&el).first() {
            let _ = first.focus();
        }
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        let Some(el) = root.get_untracked() else { return };
        let id = bindings::hit(&ev, Context::Menu);
        let list = menu::items(&el);
        let active = dom::window().document().and_then(|d| d.active_element());
        let current = list.iter().position(|i| active.as_ref() == Some(i.as_ref()));
        if matches!(id, Some("menu.leave" | "menu.leave-back")) {
            close(open);
        } else if let Some(n) = menu::step(current, list.len(), id) {
            let _ = list[n].focus();
            ev.prevent_default();
        }
    };
    let head = match (&picked.step, picked.line) {
        (Some(s), Some(l)) => format!("deploy #{} · {s} · line {}", picked.deployment, l + 1),
        (Some(s), None) => format!("deploy #{} · {s}", picked.deployment),
        _ => format!("deploy #{}", picked.deployment),
    };
    let go = move |_| {
        close(open);
        let p = picked.clone();
        menu::toast("opening the builder's chat…".into());
        spawn_local(async move {
            if let Err(e) = ask(p, tree).await {
                menu::toast(format!("couldn't open the chat: {e}"));
            }
        });
    };
    view! {
        <div id=MENU_ID role="menu" tabindex="-1" node_ref=root aria-label="this deployment" style=format!("left: {x}px; top: {y}px")
            on:contextmenu=|ev| ev.prevent_default() on:keydown=on_key>
            <div class="h"><span class="title">{head}</span></div>
            <button class="item" role="menuitem" data-act="ask-builder" title="a chat with the builder, this deploy's context and log in its first message" on:click=go>
                "Ask the builder about this"
            </button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::deployments::{DeployRequest, DeploySource, DeployState, RequestState, WebhookState};

    fn step(name: &str, state: StepState, detail: Option<&str>) -> DeployStep {
        DeployStep { name: name.into(), state, started_at: 100, ended_at: Some(160), detail: detail.map(str::to_string), url: None, by: "key:r".into() }
    }

    fn dep(id: u64, state: DeployState) -> Deployment {
        Deployment {
            id,
            project: "MENGER".into(),
            environment: "test".into(),
            state,
            revision: "abc1234def5678".into(),
            previous: None,
            commits: Vec::new(),
            tickets: Vec::new(),
            release: Some("20261020T010203Z-abc1234".into()),
            at: 1_000,
            source: DeploySource::default(),
            recorded_at: 1_000,
            recorded_by: "key:p".into(),
            request: None,
            requested: None,
        }
    }

    fn request(id: u64, steps: Vec<DeployStep>) -> Deployment {
        let mut d = dep(id, DeployState::Requested);
        d.release = None;
        d.revision = String::new();
        d.requested = Some(DeployRequest {
            note: None,
            from: None,
            state: RequestState::Failed,
            outcome: None,
            canceled_at: None,
            canceled_by: None,
            delivery: None,
            webhook: WebhookState::None,
            attempts: Vec::new(),
            next_attempt_at: None,
            branch: Some("main".into()),
            steps,
            cancel_asked_at: None,
            cancel_asked_by: None,
        });
        d
    }

    fn log(n: usize) -> String {
        std::iter::once("$ deploy/pipeline build abc1234".to_string()).chain((1..n).map(|i| format!("line {i}"))).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn the_title_names_the_environment_and_the_release() {
        assert_eq!(title(&dep(7, DeployState::Failed), None), "deploy test 20261020T010203Z-abc1234");
        let mut d = dep(7, DeployState::Failed);
        d.release = None;
        assert_eq!(title(&d, None), "deploy test abc1234");
        let mut rec = dep(9, DeployState::Failed);
        rec.release = None;
        let mut req = request(8, Vec::new());
        req.release = Some("r8".into());
        assert_eq!(title(&rec, Some(&req)), "deploy test r8", "the request's release");
        assert_eq!(title(&request(8, Vec::new()), None), "deploy test #8", "main's tip, nothing built yet");
    }

    #[test]
    fn the_window_is_the_last_lines_or_around_the_one_picked() {
        assert_eq!(window(12, None), 0..12);
        assert_eq!(window(12, Some(3)), 0..12);
        assert_eq!(window(100, None), 60..100);
        assert_eq!(window(100, Some(50)), 30..70);
        assert_eq!(window(100, Some(5)), 0..40);
        assert_eq!(window(100, Some(98)), 60..100);
        assert_eq!(window(100, Some(400)), 60..100, "a line past the end: the last");
    }

    #[test]
    fn a_fence_outruns_the_backticks_inside() {
        assert_eq!(fence("plain"), "```");
        assert_eq!(fence("a ``` b"), "````");
        assert_eq!(fence("`````"), "``````");
    }

    #[test]
    fn the_context_carries_the_deploy_the_failing_step_and_its_lines() {
        let q = request(41, vec![step("build", StepState::Succeeded, Some("$ b\nok")), step("deploy", StepState::Failed, Some(&log(60)))]);
        let text = context(&q, None, None, None);
        assert!(text.starts_with("**Deploy #41** · MENGER → test · requested"), "{text}");
        assert!(text.contains("- revision: main's tip"));
        assert!(text.contains("- request #41"));
        assert!(text.contains("- failing step `deploy`"));
        assert!(text.contains("Lines 21–60 of `deploy`:\n\n```text\nline 20\n"), "{text}");
        assert!(text.ends_with("line 59\n```"));
        assert!(!text.contains("$ b"), "the succeeded step's lines are not the failing one's");
    }

    #[test]
    fn a_picked_step_and_line_are_said_and_quoted_around() {
        let q = request(41, vec![step("build", StepState::Succeeded, Some("$ build\nwarning: `x` unused\ndone")), step("deploy", StepState::Failed, Some("boom"))]);
        let text = context(&q, None, Some("build"), Some(1));
        assert!(text.contains("- failing step `deploy`"));
        assert!(text.contains("- step `build` — succeeded"));
        assert!(text.contains("- the line picked: `warning: 'x' unused`"), "{text}");
        assert!(text.contains("The last lines of `build`:\n\n```text\n$ build\nwarning: `x` unused\ndone\n```"), "{text}");
    }

    #[test]
    fn a_record_quotes_its_requests_steps() {
        let mut rec = dep(42, DeployState::Failed);
        rec.request = Some(41);
        let q = request(41, vec![step("deploy", StepState::Failed, Some("$ deploy test\nsmoke failed"))]);
        let text = context(&rec, Some(&q), None, None);
        assert!(text.starts_with("**Deploy #42** · MENGER → test · failed"));
        assert!(text.contains("- release `20261020T010203Z-abc1234`\n- revision `abc1234def5678`\n- request #41\n- failing step `deploy`"), "{text}");
        assert!(text.contains("smoke failed"));
        let alone = context(&dep(43, DeployState::Succeeded), None, None, None);
        assert!(alone.ends_with("(no steps reported for it)"), "{alone}");
    }

    #[test]
    fn the_draft_keeps_what_was_there_and_leaves_room_for_the_question() {
        assert_eq!(draft("", "CTX"), "CTX\n\n");
        assert_eq!(draft("half a thought\n", "CTX"), "half a thought\n\nCTX\n\n");
        assert_eq!(draft("CTX\n\nwhy?", "CTX"), "CTX\n\nwhy?\n\n", "asked twice: not quoted twice");
    }

    fn chat(id: u64, title: Option<&str>, state: ChatState) -> Chat {
        let mut c = crate::chats::tests::chat(id, "MENGER-1", &["me", "key:b"]);
        c.title = title.map(str::to_string);
        c.state = state;
        c
    }

    #[test]
    fn the_same_deploys_chat_is_continued() {
        let held = [chat(1, None, ChatState::Open), chat(2, Some("deploy test r1"), ChatState::Closed), chat(3, Some("deploy test r1"), ChatState::Open), chat(4, Some("deploy test r2"), ChatState::Open)];
        assert_eq!(reuse(&held, "deploy test r1"), Some(3));
        assert_eq!(reuse(&held, "deploy test r3"), None);
    }

    #[test]
    fn a_scroll_under_way_as_it_opens_keeps_the_menu() {
        assert!(!settled(1_100.0, 1_000.0));
        assert!(settled(1_000.0 + SETTLE_MS + 1.0, 1_000.0));
    }

    #[test]
    fn the_builder_is_the_candidate_named_builder() {
        let cands = vec![("key:op".to_string(), true), ("key:b".to_string(), true), ("key:x".to_string(), false)];
        let name = |p: &str| match p {
            "key:op" => Some("operator".to_string()),
            "key:b" | "key:x" => Some("builder".to_string()),
            _ => None,
        };
        assert_eq!(builder_of(&cands, name), Some("key:b".into()));
        assert_eq!(builder_of(&cands[..1], name), None);
    }
}
