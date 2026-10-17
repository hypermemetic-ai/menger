//! TICKETS-7: a ticket's **revisions** in the app — every version the store
//! kept of it (`tickets.revisions.list`), listed in its History beside the
//! transitions; any one opened read-only as a ticket page with a banner
//! (`#/ticket/ID?rev=N`, Back to now); two picked and diffed — unified or
//! side by side, of the source or of the text a reader sees
//! (`revisions.diff`); and Restore… with a preview of what changes
//! (`revisions.restore`, itself a new revision).
//!
//! The open version follows the route's query (`?rev=N`), rewritten in place
//! — no history entry, no hashchange — so a deep link opens it and the page
//! underneath never reloads. `watch { revisions: true }` carries `revised`,
//! which the History asks again on: a change made in another window shows.

use leptos::prelude::*;
use leptos::task::spawn_local;
use menger_tickets_api::diff::{DiffLine, DiffOp, Hunk, TextDiff};
use menger_tickets_api::{FieldChange, Revision, RevisionDiff, RevisionSource, Ticket, TicketsEvent};
use wasm_bindgen::JsCast;

use crate::{dom, fmt_relative, fmt_unix, now_unix, scopes, session, st_class};

// -- the route ------------------------------------------------------------------

/// The revision the route's query opens (`rev=N`).
pub(crate) fn rev_in(hash: &str) -> Option<u32> {
    let (_, q) = hash.split_once('?')?;
    q.split('&').find_map(|p| p.strip_prefix("rev=")).and_then(|v| v.parse().ok())
}

/// The hash with `rev=N` in its query (`Some`) or out of it, the rest kept.
pub(crate) fn with_rev(hash: &str, rev: Option<u32>) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<String> = query.split('&').filter(|p| !p.is_empty() && !p.starts_with("rev=")).map(str::to_string).collect();
    if let Some(r) = rev {
        params.push(format!("rev={r}"));
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

thread_local! {
    static VIEWING: std::cell::RefCell<Option<ArcRwSignal<Option<u32>>>> = const { std::cell::RefCell::new(None) };
}

/// The revision open on the ticket page — the route's, followed. Owned by no
/// view (an `Arc` signal): the page that reads it re-renders as it moves.
pub(crate) fn viewing() -> ArcRwSignal<Option<u32>> {
    if let Some(s) = VIEWING.with(|v| v.borrow().clone()) {
        return s;
    }
    let read = || rev_in(&dom::window().location().hash().unwrap_or_default());
    let s = ArcRwSignal::new(read());
    VIEWING.with(|v| *v.borrow_mut() = Some(s.clone()));
    let follow = s.clone();
    let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || follow.set(read()));
    let _ = dom::window().add_event_listener_with_callback("hashchange", cb.as_ref().unchecked_ref());
    cb.forget();
    s
}

/// Opens revision `rev` (`None`: back to now) — the query rewritten in place.
pub(crate) fn open(rev: Option<u32>) {
    let w = dom::window();
    let hash = w.location().hash().unwrap_or_default();
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&with_rev(&hash, rev)));
    }
    viewing().set(rev);
    let _ = w.scroll_to_with_x_and_y(0.0, 0.0);
}

// -- what the server answers ------------------------------------------------------

/// `revisions list`'s answer: newest first, and the count and latest.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Listed {
    pub(crate) revisions: Vec<Revision>,
    pub(crate) count: u32,
    pub(crate) latest: u32,
}

/// `revisions list`'s events read — `None` for a refusal.
pub(crate) fn listed_of(events: Vec<TicketsEvent>) -> Option<Listed> {
    let mut l = Listed::default();
    for ev in events {
        match ev {
            TicketsEvent::Revision { revision } => l.revisions.push(revision),
            TicketsEvent::RevisionsEnd { count, latest, .. } => {
                l.count = count;
                l.latest = latest;
            }
            TicketsEvent::NotFound { .. } | TicketsEvent::Error { .. } => return None,
            _ => {}
        }
    }
    Some(l)
}

/// Whether a `watch` event moves `id`'s revisions.
pub(crate) fn touches(id: &str, ev: &TicketsEvent) -> bool {
    matches!(ev, TicketsEvent::Revised { ticket, .. } if ticket.eq_ignore_ascii_case(id))
}

/// What a revision changed, in a reader's words: `title, body (+212 −96)`.
pub(crate) fn changed_words(r: &Revision) -> String {
    if r.source == RevisionSource::Baseline {
        return "as it stood before revisions were kept".into();
    }
    let words: Vec<String> = r
        .fields
        .iter()
        .map(|f| match f.as_str() {
            "body" => format!("body (+{} −{})", r.added, r.removed),
            "blocked_by" => "blockers".to_string(),
            "created" if r.added > 0 => format!("created ({} lines)", r.added),
            other => other.to_string(),
        })
        .collect();
    words.join(", ")
}

/// Who made a revision, as a reader names them.
pub(crate) fn who(r: &Revision) -> String {
    match r.source {
        RevisionSource::Baseline | RevisionSource::Backup => "—".into(),
        _ => r.name.clone().unwrap_or_else(|| r.actor.clone()),
    }
}

/// A field's value for a reader: none is `—`.
fn shown(v: &Option<String>) -> String {
    v.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "—".into())
}

/// Whether a body uses a component or a theme — what renders with today's
/// definitions, not the ones it was written against.
pub(crate) fn uses_definitions(body: &str) -> bool {
    body.contains("<m-") || body.contains("```component") || body.contains("```theme") || crate::md::split_frontmatter(body).0.is_some_and(|f| f.contains("theme:"))
}

// -- one row of the History -----------------------------------------------------------

/// One revision, as a History row: open it on a click; tick two to diff.
pub(crate) fn rev_row(r: Revision, latest: u32, picked: RwSignal<Vec<u32>>) -> impl IntoView {
    let rev = r.rev;
    let current = rev == latest;
    let label = if current { "now".to_string() } else { rev.to_string() };
    let ticked = move || picked.with(|p| p.contains(&rev));
    let toggle = move |_| {
        picked.update(|p| {
            if let Some(i) = p.iter().position(|x| *x == rev) {
                p.remove(i);
            } else {
                p.push(rev);
                if p.len() > 2 {
                    p.remove(0);
                }
            }
        })
    };
    let src = r.source.as_str();
    view! {
        <li class="tr rev" class:current=current data-rev=rev.to_string()>
            <input type="checkbox" class="pick" title="pick two to diff" aria-label=format!("pick revision {rev} to diff") prop:checked=ticked on:change=toggle />
            <button class="revno" title=if current { "the ticket as it is".to_string() } else { format!("open revision {rev}, read-only") } on:click=move |_| open((!current).then_some(rev))>
                {label}
            </button>
            <time class="at" title=fmt_unix(r.at_unix)>{fmt_relative(now_unix(), r.at_unix)}</time>
            <span class="what">{changed_words(&r)}</span>
            <span class="who" title=r.actor.clone()>{who(&r)}</span>
            <span class="src">{src}</span>
            {current.then_some(view! { <span class="cur">"current"</span> })}
            {r.message.clone().map(|m| view! { <span class="msg">{m}</span> })}
            {r.cause.clone().map(|c| {
                let href = format!("#/ticket/{c}");
                view! { <span class="cause">"with "<a class="code" href=href data-ticket-id=c.clone()>{c.clone()}</a></span> }
            })}
        </li>
    }
}

// -- a diff ---------------------------------------------------------------------------

/// How a diff is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Unified,
    Split,
}

fn op_class(op: DiffOp) -> &'static str {
    match op {
        DiffOp::Same => "same",
        DiffOp::Add => "add",
        DiffOp::Del => "del",
    }
}

/// A line's text, its changed words marked.
fn line_text(l: &DiffLine) -> AnyView {
    if l.words.is_empty() {
        return view! { <span class="txt">{l.text.clone()}</span> }.into_any();
    }
    l.words
        .iter()
        .map(|w| {
            let class = format!("w {}", op_class(w.op));
            view! { <span class=class>{w.text.clone()}</span> }
        })
        .collect_view()
        .into_any()
}

fn sign(op: DiffOp) -> &'static str {
    match op {
        DiffOp::Same => " ",
        DiffOp::Add => "+",
        DiffOp::Del => "−",
    }
}

/// A hunk, one column.
fn unified(h: &Hunk) -> impl IntoView {
    let head = format!("@@ −{},{} +{},{} @@", h.old_start, h.old_lines, h.new_start, h.new_lines);
    view! {
        <tbody class="hunk">
            <tr class="hh"><td colspan="2">{head}</td></tr>
            {h.lines.iter().map(|l| {
                let class = op_class(l.op);
                view! { <tr class=class><td class="sg">{sign(l.op)}</td><td class="ln">{line_text(l)}</td></tr> }
            }).collect_view()}
        </tbody>
    }
}

/// A hunk, older left and newer right: each deletion beside the addition at
/// its place.
fn split(h: &Hunk) -> impl IntoView {
    let mut rows: Vec<(Option<DiffLine>, Option<DiffLine>)> = Vec::new();
    let mut i = 0;
    let lines = &h.lines;
    while i < lines.len() {
        match lines[i].op {
            DiffOp::Same => {
                rows.push((Some(lines[i].clone()), Some(lines[i].clone())));
                i += 1;
            }
            _ => {
                let start = i;
                while i < lines.len() && lines[i].op == DiffOp::Del {
                    i += 1;
                }
                let dels: Vec<DiffLine> = lines[start..i].to_vec();
                let a = i;
                while i < lines.len() && lines[i].op == DiffOp::Add {
                    i += 1;
                }
                let adds: Vec<DiffLine> = lines[a..i].to_vec();
                for k in 0..dels.len().max(adds.len()) {
                    rows.push((dels.get(k).cloned(), adds.get(k).cloned()));
                }
            }
        }
    }
    let cell = |l: Option<DiffLine>| match l {
        Some(l) => {
            let class = format!("ln {}", op_class(l.op));
            view! { <td class=class>{line_text(&l)}</td> }.into_any()
        }
        None => view! { <td class="ln gap"></td> }.into_any(),
    };
    let head = format!("@@ −{},{} +{},{} @@", h.old_start, h.old_lines, h.new_start, h.new_lines);
    view! {
        <tbody class="hunk">
            <tr class="hh"><td colspan="2">{head}</td></tr>
            {rows.into_iter().map(|(a, b)| view! { <tr>{cell(a)}{cell(b)}</tr> }).collect_view()}
        </tbody>
    }
}

fn fields_table(caption: &'static str, fields: Vec<FieldChange>) -> Option<impl IntoView> {
    (!fields.is_empty()).then(|| {
        view! {
            <table class="fields">
                <caption>{caption}</caption>
                {fields.into_iter().map(|f| view! {
                    <tr data-field=f.field.clone()><th>{f.field.clone()}</th><td class="from">{shown(&f.from)}</td><td class="arrow">"→"</td><td class="to">{shown(&f.to)}</td></tr>
                }).collect_view()}
            </table>
        }
    })
}

fn body_view(d: TextDiff, layout: Layout) -> impl IntoView {
    let truncated = d.truncated.then_some(view! { <p class="none">"…the rest of the diff is not shown (past 4,000 lines)."</p> });
    let class = if layout == Layout::Split { "difflines split" } else { "difflines" };
    view! {
        <table class=class>
            {d.hunks.iter().map(|h| match layout {
                Layout::Unified => unified(h).into_any(),
                Layout::Split => split(h).into_any(),
            }).collect_view()}
        </table>
        {truncated}
    }
}

/// The diff from `from` to `to` (`None`: the latest) of ticket `id` — field
/// by field, the frontmatter key by key, the body's lines; unified or side
/// by side, the source or the visible text.
#[component]
pub(crate) fn DiffPanel(id: String, #[prop(into)] from: Signal<u32>, #[prop(into)] to: Signal<Option<u32>>) -> impl IntoView {
    let layout = RwSignal::new(Layout::Unified);
    let text = RwSignal::new(false);
    let answer: RwSignal<Option<Result<RevisionDiff, String>>> = RwSignal::new(None);
    let asked = StoredValue::new(0u32);
    Effect::new(move |_| {
        let (from, to, text) = (from.get(), to.get(), text.get());
        let Some(client) = session::client() else { return };
        let Ok(t) = crate::typed::typed(&id) else { return };
        let n = asked.get_value().wrapping_add(1);
        asked.set_value(n);
        answer.set(None);
        spawn_local(async move {
            let got = match client.revisions().diff_all(t, Some(from), to, Some(text)).await {
                Ok(evs) => evs
                    .into_iter()
                    .find_map(|e| match e {
                        TicketsEvent::RevisionDiff { diff } => Some(Ok(diff)),
                        TicketsEvent::Error { message } => Some(Err(message)),
                        _ => None,
                    })
                    .unwrap_or_else(|| Err("no diff".into())),
                Err(e) => Err(format!("{e:?}")),
            };
            if asked.try_get_value() == Some(n) {
                answer.try_set(Some(got));
            }
        });
    });
    view! {
        <div class="diffpanel" id="t-diff">
            <div class="difftools">
                <span class="title">{move || match answer.get() {
                    Some(Ok(d)) => format!("Revision {} → {}", d.from, d.to),
                    _ => "Diff".to_string(),
                }}</span>
                <span class="seg" role="group" aria-label="layout">
                    <button aria-pressed=move || (layout.get() == Layout::Unified).to_string() on:click=move |_| layout.set(Layout::Unified)>"Unified"</button>
                    <button aria-pressed=move || (layout.get() == Layout::Split).to_string() on:click=move |_| layout.set(Layout::Split)>"Side by side"</button>
                </span>
                <span class="seg" role="group" aria-label="what is compared">
                    <button aria-pressed=move || (!text.get()).to_string() on:click=move |_| text.set(false)>"Source"</button>
                    <button aria-pressed=move || text.get().to_string() on:click=move |_| text.set(true)>"Rendered text"</button>
                </span>
            </div>
            {move || match answer.get() {
                None => view! { <p class="loading">"loading the diff…"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="none">{e}</p> }.into_any(),
                Some(Ok(d)) => {
                    let nothing = d.fields.is_empty() && d.frontmatter.is_empty() && d.body.is_none();
                    let fields = fields_table("Fields", d.fields.into_iter().filter(|f| f.field != "body").collect());
                    let fm = fields_table("Frontmatter", d.frontmatter);
                    let body = d.body.map(|b| {
                        let counts = format!("Body · +{} −{}", b.added, b.removed);
                        view! { <h5>{counts}</h5>{body_view(b, layout.get())}}
                    });
                    view! {
                        {nothing.then_some(view! { <p class="none">"No difference."</p> })}
                        {fields}
                        {fm}
                        {body}
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

// -- a version, read-only ------------------------------------------------------------

/// What `revisions get` answered.
type At = Result<(Revision, Ticket), String>;

fn at_of(events: Vec<TicketsEvent>) -> At {
    events
        .into_iter()
        .find_map(|e| match e {
            TicketsEvent::RevisionAt { revision, ticket } => Some(Ok((revision, ticket))),
            TicketsEvent::Error { message } => Some(Err(message)),
            TicketsEvent::NotFound { .. } => Some(Err("not found".into())),
            _ => None,
        })
        .unwrap_or_else(|| Err("no answer".into()))
}

/// The page's read-only version: the banner, then the ticket as it was.
#[component]
pub(crate) fn Version(id: String, rev: u32) -> impl IntoView {
    let at: RwSignal<Option<At>> = RwSignal::new(None);
    let latest = RwSignal::new(0u32);
    let panel = RwSignal::new(None::<&'static str>);
    {
        let id = id.clone();
        let Some(client) = session::client() else { return view! { <p class="loading">"connecting…"</p> }.into_any() };
        spawn_local(async move {
            let Ok(t) = crate::typed::typed(&id) else { return };
            let got = client.revisions().get_all(t, rev).await.map_err(|e| format!("{e:?}")).and_then(|evs| at_of(evs).map(Ok)).unwrap_or_else(Err);
            at.try_set(Some(got));
            if let Ok(t) = crate::typed::typed(&id) {
                if let Ok(evs) = client.revisions().list_all(t, None, Some(1)).await {
                    if let Some(l) = listed_of(evs) {
                        latest.try_set(l.latest);
                    }
                }
            }
        });
    }
    // A later revision written while this one is open moves "of N".
    {
        let id = id.clone();
        session::listen(move |ev| {
            if let TicketsEvent::Revised { ticket, rev, .. } = ev {
                if ticket.eq_ignore_ascii_case(&id) {
                    latest.set(*rev);
                }
            }
        });
    }
    let diff_id = id.clone();
    let restore_id = id.clone();
    view! {
        <div class="version" id="t-version" data-rev=rev.to_string()>
            {move || match at.get() {
                None => view! { <p class="loading">{format!("loading revision {rev}…")}</p> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="revbanner"><span>{format!("Revision {rev}: {e}")}</span><button class="back" on:click=move |_| open(None)>"Back to now"</button></div>
                }.into_any(),
                Some(Ok((r, t))) => {
                    let of = move || {
                        let n = latest.get();
                        if n > 0 { format!("Viewing revision {rev} of {n}") } else { format!("Viewing revision {rev}") }
                    };
                    let defs = uses_definitions(&t.body).then_some(view! {
                        <span class="defs">"Components and themes render with today's definitions."</span>
                    });
                    let when = format!(" · {} · read-only", fmt_unix(r.at_unix));
                    let by = format!("by {} · {}", who(&r), r.source.as_str());
                    view! {
                        <div class="revbanner" role="status">
                            <span class="of">{of}</span>
                            <span class="when">{when}</span>
                            <span class="by">{by}</span>
                            {defs}
                            <span class="acts">
                                <button class="diffnow" aria-pressed=move || (panel.get() == Some("diff")).to_string() on:click=move |_| panel.update(|p| *p = if *p == Some("diff") { None } else { Some("diff") })>"Diff with now"</button>
                                <button class="restore" aria-pressed=move || (panel.get() == Some("restore")).to_string() on:click=move |_| panel.update(|p| *p = if *p == Some("restore") { None } else { Some("restore") })>"Restore…"</button>
                                <button class="back" on:click=move |_| open(None)>"Back to now"</button>
                            </span>
                        </div>
                        {
                            let (diff_id, restore_id) = (diff_id.clone(), restore_id.clone());
                            move || match panel.get() {
                                Some("diff") => {
                                    let now: Option<u32> = None;
                                    view! { <DiffPanel id=diff_id.clone() from=rev to=now /> }.into_any()
                                }
                                Some("restore") => view! { <RestorePanel id=restore_id.clone() rev=rev /> }.into_any(),
                                _ => ().into_any(),
                            }
                        }
                        {page(t)}
                    }
                    .into_any()
                }
            }}
        </div>
    }
    .into_any()
}

/// The ticket as it was — its title, its body (framed, mermaid and all;
/// read-only: no input answers from here) and the fields a revision keeps.
fn page(t: Ticket) -> impl IntoView {
    let title = t.title.clone();
    let body = t.body.clone();
    let list = |v: Vec<String>| if v.is_empty() { "—".to_string() } else { v.join(", ") };
    let status = t.status.to_string();
    let class = format!("status {}", st_class(t.status.as_str()));
    let blocked = list(t.blocked_by.iter().map(ToString::to_string).collect());
    let labels = list(t.labels.iter().map(|l| l.as_str().to_string()).collect());
    let commits = list(t.commits.iter().map(ToString::to_string).collect());
    let parent = t.parent.as_ref().map_or_else(|| "—".to_string(), ToString::to_string);
    let sev = t.severity.map_or_else(|| "—".to_string(), |s| s.to_string());
    let empty = body.trim().is_empty();
    let title_sig = Signal::derive(move || title.clone());
    let body_sig = Signal::derive(move || body.clone());
    view! {
        <article class="ticket asof">
            <h2>
                <span class="id code">{t.id.to_string()}</span>
                <span class="t">{t.title.clone()}</span>
            </h2>
            <div class=scopes::FRAME_CLASS>
                {if empty {
                    view! { <p class="none">"(no body)"</p> }.into_any()
                } else {
                    view! { <crate::notes::BodyView body=body_sig refs=None ticket=None title=title_sig /> }.into_any()
                }}
            </div>
        </article>
        <aside class="props asof">
            <dl class="meta">
                <div><dt>"Status"</dt><dd class=class>{status}</dd></div>
                <div><dt>"Kind"</dt><dd class="kind">{t.kind.to_string()}</dd></div>
                <div><dt>"Severity"</dt><dd>{sev}</dd></div>
                <div><dt>"Parent"</dt><dd>{parent}</dd></div>
                <div><dt>"Blocked by"</dt><dd>{blocked}</dd></div>
                <div><dt>"Labels"</dt><dd>{labels}</dd></div>
                <div><dt>"Commits"</dt><dd>{commits}</dd></div>
            </dl>
        </aside>
    }
}

// -- restore, with a preview ----------------------------------------------------------

/// The fields a restore puts back.
const RESTORABLE: [&str; 8] = ["title", "body", "status", "kind", "severity", "parent", "blocked_by", "labels"];

/// Restore…: what would change (the diff from now back to `rev`), each field
/// ticked to put back, a reason, and the write.
#[component]
pub(crate) fn RestorePanel(id: String, rev: u32) -> impl IntoView {
    let diff: RwSignal<Option<Result<RevisionDiff, String>>> = RwSignal::new(None);
    let ticked: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let message = RwSignal::new(String::new());
    let said: RwSignal<Option<String>> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    {
        let id = id.clone();
        if let Some(client) = session::client() {
            spawn_local(async move {
                let Ok(t) = crate::typed::typed(&id) else { return };
                // The latest against `rev`: what restoring it changes.
                let list = match crate::typed::typed(&id) {
                    Ok(t2) => client.revisions().list_all(t2, None, Some(1)).await.ok().and_then(listed_of),
                    Err(_) => None,
                };
                let latest = list.map_or(rev, |l| l.latest);
                let got = match client.revisions().diff_all(t, Some(latest), Some(rev), Some(false)).await {
                    Ok(evs) => evs
                        .into_iter()
                        .find_map(|e| match e {
                            TicketsEvent::RevisionDiff { diff } => Some(Ok(diff)),
                            TicketsEvent::Error { message } => Some(Err(message)),
                            _ => None,
                        })
                        .unwrap_or_else(|| Err("no diff".into())),
                    Err(e) => Err(format!("{e:?}")),
                };
                if let Ok(d) = &got {
                    ticked.try_set(d.fields.iter().map(|f| f.field.clone()).filter(|f| RESTORABLE.contains(&f.as_str())).collect());
                }
                diff.try_set(Some(got));
            });
        }
    }
    let go = {
        let id = id.clone();
        move |_| {
            let Some(client) = session::client() else { return };
            let fields = ticked.get_untracked();
            if fields.is_empty() {
                return;
            }
            let id = id.clone();
            let msg = message.get_untracked();
            busy.set(true);
            spawn_local(async move {
                let Ok(t) = crate::typed::typed(&id) else { return };
                let fields = crate::typed::many(fields.iter()).ok();
                let msg = crate::typed::opt(Some(msg)).ok().flatten();
                let src = crate::typed::typed("app").ok();
                let out = client.revisions().restore_all(t, rev, fields, src, msg).await;
                busy.try_set(false);
                match out {
                    Ok(evs) => match evs.into_iter().next() {
                        Some(TicketsEvent::RevisionRestored { rev: Some(n), .. }) => {
                            said.try_set(Some(format!("Restored as revision {n}.")));
                            open(None);
                        }
                        Some(TicketsEvent::RevisionRestored { rev: None, .. }) => {
                            said.try_set(Some("Nothing differed; nothing was written.".into()));
                        }
                        Some(TicketsEvent::Error { message }) => {
                            said.try_set(Some(message));
                        }
                        other => {
                            said.try_set(Some(format!("{other:?}")));
                        }
                    },
                    Err(e) => {
                        said.try_set(Some(format!("{e:?}")));
                    }
                }
            });
        }
    };
    view! {
        <div class="restorepanel" id="t-restore">
            <h4>{format!("Restore revision {rev}")}</h4>
            {move || match diff.get() {
                None => view! { <p class="loading">"working out what changes…"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="none">{e}</p> }.into_any(),
                Some(Ok(d)) if d.fields.is_empty() => view! { <p class="none">"Revision is the ticket as it is: nothing to restore."</p> }.into_any(),
                Some(Ok(d)) => {
                    let rows = d.fields.clone().into_iter().map(|f| {
                        let name = f.field.clone();
                        let can = RESTORABLE.contains(&name.as_str());
                        let n2 = name.clone();
                        let on = move || ticked.with(|t| t.contains(&n2));
                        let n3 = name.clone();
                        let flip = move |_| ticked.update(|t| {
                            if let Some(i) = t.iter().position(|x| *x == n3) { t.remove(i); } else { t.push(n3.clone()); }
                        });
                        view! {
                            <li data-field=name.clone()>
                                <label>
                                    <input type="checkbox" prop:checked=on prop:disabled=!can on:change=flip />
                                    <span class="f">{name.clone()}</span>
                                    <span class="from">{shown(&f.from)}</span>
                                    <span class="arrow">"→"</span>
                                    <span class="to">{shown(&f.to)}</span>
                                    {(!can).then_some(view! { <span class="none">" (linked, never restored)"</span> })}
                                </label>
                            </li>
                        }
                    }).collect_view();
                    let body = d.body.clone().map(|b| view! { <details><summary>{format!("the body's lines · +{} −{}", b.added, b.removed)}</summary>{body_view(b, Layout::Unified)}</details> });
                    view! {
                        <p>"What changes: the ticket now → revision "{rev}". Untick what to keep as it is."</p>
                        <ul class="restorefields">{rows}</ul>
                        {body}
                    }.into_any()
                }
            }}
            <p class="restorewhy">
                <input type="text" placeholder="why (optional) — kept in the revision" prop:value=move || message.get() on:input=move |e| message.set(event_target_value(&e)) />
                <button class="primary" prop:disabled=move || busy.get() || ticked.with(Vec::is_empty) on:click=go>"Restore"</button>
            </p>
            {move || said.get().map(|s| view! { <p class="said" role="status">{s}</p> })}
            <p class="none">"A restore is a new revision: nothing is lost."</p>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(rev: u32, fields: &[&str], added: u32, removed: u32) -> Revision {
        Revision {
            ticket: menger_tickets_api::TicketId::new("R-2"),
            rev,
            at_unix: 100,
            actor: "idp:ada".into(),
            name: Some("Ada".into()),
            source: RevisionSource::App,
            message: None,
            cause: None,
            fields: fields.iter().map(|f| f.to_string()).collect(),
            body_hash: "h".into(),
            body_bytes: 1,
            added,
            removed,
        }
    }

    #[test]
    fn the_route_query_opens_a_revision() {
        assert_eq!(rev_in("#/ticket/R-2?rev=4"), Some(4));
        assert_eq!(rev_in("#/ticket/R-2?view=full&rev=12&g=1"), Some(12));
        assert_eq!(rev_in("#/ticket/R-2"), None);
        assert_eq!(with_rev("#/ticket/R-2", Some(3)), "#/ticket/R-2?rev=3");
        assert_eq!(with_rev("#/ticket/R-2?g=1&rev=3", None), "#/ticket/R-2?g=1");
        assert_eq!(with_rev("#/ticket/R-2?rev=3", Some(5)), "#/ticket/R-2?rev=5");
    }

    #[test]
    fn what_a_revision_changed_in_words() {
        assert_eq!(changed_words(&r(2, &["title", "body"], 212, 96)), "title, body (+212 −96)");
        assert_eq!(changed_words(&r(1, &["created"], 3, 0)), "created (3 lines)");
        assert_eq!(changed_words(&r(3, &["blocked_by", "labels"], 0, 0)), "blockers, labels");
        let mut b = r(1, &[], 0, 0);
        b.source = RevisionSource::Baseline;
        assert_eq!((changed_words(&b).as_str(), who(&b).as_str()), ("as it stood before revisions were kept", "—"));
        assert_eq!(who(&r(2, &[], 0, 0)), "Ada");
    }

    #[test]
    fn a_list_is_read_and_a_revised_of_this_ticket_moves_it() {
        let l = listed_of(vec![
            TicketsEvent::Revision { revision: r(2, &["title"], 0, 0) },
            TicketsEvent::Revision { revision: r(1, &["created"], 1, 0) },
            TicketsEvent::RevisionsEnd { count: 2, latest: 2, next: None },
        ])
        .unwrap();
        assert_eq!((l.revisions.len(), l.count, l.latest), (2, 2, 2));
        assert!(listed_of(vec![TicketsEvent::Error { message: "x".into() }]).is_none());
        let ev = TicketsEvent::Revised { ticket: menger_tickets_api::TicketId::new("R-2"), rev: 3, fields: vec![] };
        assert!(touches("r-2", &ev));
        assert!(!touches("R-3", &ev));
    }

    #[test]
    fn a_body_that_uses_definitions_says_so() {
        assert!(uses_definitions("<m-field>x</m-field>"));
        assert!(uses_definitions("---\ntheme: ocean\n---\nhi"));
        assert!(!uses_definitions("plain *markdown*"));
    }
}
