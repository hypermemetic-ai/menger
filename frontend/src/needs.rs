//! MENGER-12: **Needs you** — what waits on the owner, first thing on Home.
//!
//! - **What it shows** is the live result of a filter: by default
//!   [`DEFAULT`], `asking or label needs-you` — every ticket with an open
//!   input (`asking`, INPUT-1's questions) and every ticket labelled
//!   `needs-you`. The filter is a **saved filter** (FILTER-6, src/saved.rs):
//!   the one named "Needs you" (`VIEW/_MENGER/view/needs-you`) when one is
//!   saved, else the default. The panel's `change` saves another under that
//!   name — so `Saved ▾` lists it, and any level can apply it — and the
//!   panel follows it from then on, in every browser.
//! - **The header's count** ([`HeaderLink`]): `needs you N` beside
//!   `inputs`, live from the moment the app is up (`counts { filter }` kept
//!   by a membership watch — FILTER-12's [`LiveCounts`]); a click (or `g n`)
//!   goes Home and puts focus on the panel.
//! - **The panel** ([`Panel`]), above the board: "Needs you · N", then a row
//!   per match — its id, its title, its status chip, and what it needs: each
//!   open input's question, with `answer ›`, which opens the ticket at that
//!   input (`#/ticket/<ID>?anchor=<input>`: its frame scrolls to it and
//!   flashes it, COMP-14), and the ticket's labels as chips. Live: the rows
//!   are asked again on every `entered` / `left` / `recount` of the filter's
//!   membership watch, the rows' titles, statuses and labels are the row
//!   cache's (moved by `watch`), and the questions are the app's copy of the
//!   inputs (src/inputs.rs) — an answer in another window takes the input
//!   off at once, and the ticket with it when it no longer asks. Nothing
//!   matching says "Nothing needs you."
//!
//! What decides — the default, the saved filter's id, the answer link, the
//! questions a row shows, the header's words — is pure and pinned by native
//! tests.

use leptos::prelude::*;
use menger_tickets_api::filter::Filter;
use menger_tickets_api::{Input, InputState, TicketSummary, TicketsEvent};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::counts::{wire_text, LiveCounts};
use crate::session::{self, Member};
use crate::{cache, dom, inputs, labels, saved, st_class};

/// The panel's filter while none is saved: MENGER-39: every ticket in the
/// owner's queue, every ticket asking the owner something, and every ticket
/// labelled `needs-you`.
pub(crate) const DEFAULT: &str = "queued or asking or label needs-you";

/// The saved filter's name — its id (its doc's path) is the name's slug.
pub(crate) const SAVED_NAME: &str = "Needs you";

/// At most this many rows show; the rest are counted.
pub(crate) const ROWS_MAX: u32 = 100;

/// The default, parsed.
pub(crate) fn default_filter() -> Filter {
    Filter::parse(DEFAULT).expect("the default parses")
}

/// The saved filter's id.
pub(crate) fn saved_id() -> String {
    saved::slug(SAVED_NAME)
}

/// What the panel asks with: the filter's wire text — `None` for a filter
/// that admits nothing (asked of no one). MENGER-13: a canceled ticket needs
/// no one, nor does what its cancel canceled — `not status Canceled` is
/// always beside the filter (its withdrawn inputs are no longer `asking`;
/// this takes out a `needs-you` label on a canceled subtree too).
pub(crate) fn asked(f: &Filter) -> Option<String> {
    let f = f.and(&Filter::parse(NOT_CANCELED).expect("parses"));
    f.admits_nothing().is_none().then(|| wire_text(&f))
}

/// MENGER-13: what the panel never shows.
pub(crate) const NOT_CANCELED: &str = "not status Canceled";

/// Where an input is answered: its ticket's page, its frame scrolled to the
/// input (COMP-14's `?anchor=`, the element with the input's id).
pub(crate) fn answer_href(ticket: &str, input: &str) -> String {
    format!("#/ticket/{ticket}?anchor={input}")
}

/// The open questions of `ticket` among `inputs`, in its body's order.
pub(crate) fn open_of<'a>(inputs: &'a [Input], ticket: &str) -> Vec<&'a Input> {
    inputs.iter().filter(|i| i.ticket == ticket && i.state == InputState::Open).collect()
}

/// The header's title for `n` waiting.
pub(crate) fn badge_title(n: usize) -> String {
    match n {
        0 => "nothing needs you".into(),
        1 => "1 ticket needs you".into(),
        n => format!("{n} tickets need you"),
    }
}

/// The header badge's class: marked while anything waits.
fn badge_class(n: usize) -> &'static str {
    if n > 0 {
        "hbadge needs"
    } else {
        "hbadge zero"
    }
}

/// Where the panel's filter came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// Not read yet (no connection).
    Unread,
    /// Nothing is saved under the name: the default.
    Default,
    /// The saved filter.
    Saved,
    /// The saved filter could not be read — the default stands; why.
    Unreadable(String),
}

/// The panel's state for the app's lifetime — every handle `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct NeedsYou {
    pub(crate) filter: RwSignal<Filter>,
    pub(crate) source: RwSignal<Source>,
    pub(crate) count: LiveCounts,
    text: Memo<Option<String>>,
}

impl NeedsYou {
    /// Made once by the App: the default at first, the saved filter read on
    /// every new connection, the count live from then on.
    pub(crate) fn install() -> Self {
        let filter = RwSignal::new(default_filter());
        let source = RwSignal::new(Source::Unread);
        let text = Memo::new(move |_| filter.with(asked));
        let count = LiveCounts::new(text.into());
        let link = session::link();
        Effect::new(move |_| {
            if !link.get() {
                return;
            }
            spawn_local(async move {
                match saved::load_if_saved(&saved_id()).await {
                    Ok(Some(f)) => {
                        filter.try_set(f);
                        source.try_set(Source::Saved);
                    }
                    Ok(None) => {
                        source.try_set(Source::Default);
                    }
                    Err(why) => {
                        source.try_set(Source::Unreadable(why));
                    }
                }
            });
        });
        let me = NeedsYou { filter, source, count, text };
        provide_context(me);
        me
    }

    /// The filter asked, as wire text.
    pub(crate) fn text(self) -> Memo<Option<String>> {
        self.text
    }
}

/// The app's Needs you.
pub(crate) fn use_needs() -> NeedsYou {
    use_context::<NeedsYou>().expect("Needs you is provided by the App")
}

/// Go Home and put focus on the panel — its first row, or its heading.
pub(crate) fn go() {
    let w = dom::window();
    if w.location().hash().unwrap_or_default().trim_start_matches('#').trim_start_matches('/').split('?').next() != Some("") {
        let _ = w.location().set_hash("/");
    }
    spawn_local(async {
        for _ in 0..20 {
            dom::sleep_ms(50).await;
            let Some(doc) = dom::window().document() else { return };
            let target = doc
                .query_selector("#needs-you .nrow a.code")
                .ok()
                .flatten()
                .or_else(|| doc.query_selector("#needs-you h2").ok().flatten());
            if let Some(el) = target.and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = el.focus();
                el.scroll_into_view();
                return;
            }
        }
    });
}

/// The header's link: `needs you` and the live count.
#[component]
pub(crate) fn HeaderLink() -> impl IntoView {
    let needs = use_needs();
    // DESIGN-39: the count ticks when it moves.
    crate::motion::tick_on_change("#needs-link .hbadge", move || needs.count.n());
    view! {
        <a
            class="hlink"
            id="needs-link"
            href="#/"
            title="what waits on you — the Needs you panel on Home (g n)"
            on:click=move |ev| {
                ev.prevent_default();
                go();
            }
        >
            "needs you"
            {move || {
                needs.count.n().map(|n| view! {
                    <span class=badge_class(n) title=badge_title(n)>{n}</span>
                })
            }}
        </a>
    }
}

/// The panel, first on Home.
#[component]
pub(crate) fn Panel() -> impl IntoView {
    let needs = use_needs();
    inputs::want();
    let held = inputs::signal();
    let holder = StoredValue::new(cache::Holder::new());
    // The rows: the matches' ids, in the store's order — `None` until the
    // first answer for this filter; the server's words when it refused.
    let ids = RwSignal::new(None::<Vec<String>>);
    let refused = RwSignal::new(None::<String>);
    let asked_n = StoredValue::new(0u64);
    let text = needs.text();
    let ask = move || {
        let Some(t) = text.get_untracked() else {
            ids.set(Some(Vec::new()));
            return;
        };
        let Some(client) = session::client() else { return };
        let n = asked_n.get_value().wrapping_add(1);
        asked_n.set_value(n);
        spawn_local(async move {
            let answer = client.list_all(None, None, None, None, Some(ROWS_MAX), Some(true), Some(t), None, None, crate::session::tz()).await;
            if asked_n.try_get_value() != Some(n) {
                return;
            }
            match answer {
                Ok(events) => {
                    let mut rows: Vec<TicketSummary> = Vec::new();
                    let mut why = None;
                    for ev in events {
                        match ev {
                            TicketsEvent::Summary { summary } => rows.push(summary),
                            TicketsEvent::Error { message } => why = Some(message),
                            _ => {}
                        }
                    }
                    let order: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
                    holder.get_value().hold_page(rows);
                    refused.try_set(why);
                    ids.try_set(Some(order));
                }
                Err(e) => leptos::logging::log!("needs you: {}", session::describe(&e)),
            }
        });
    };
    // A new filter: asked at once (the watch's `opened` asks again).
    Effect::new(move |_| {
        text.track();
        ids.set(None);
        ask();
    });
    session::follow_membership(move || text.get(), move |ev: &Member| match ev {
        Member::Refused(why) => {
            refused.try_set(Some(why.clone()));
        }
        _ => ask(),
    });
    // ACT-4: a `lagged` on the session's own `watch` — a bulk write's
    // events (~6 a ticket) outrun it — may have lost the rows' changes
    // (their statuses, their labels) while the membership watch saw nothing
    // enter or leave: ask again.
    let stale = session::stale();
    Effect::new(move |prev: Option<u32>| {
        let now = stale.get();
        if prev.is_some_and(|p| p != now) {
            ask();
        }
        now
    });

    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let start = move |_| {
        draft.set(needs.filter.with_untracked(Filter::format));
        problem.set(None);
        editing.set(true);
    };
    let save = move || {
        let parsed = match Filter::parse(draft.get_untracked().trim()) {
            Ok(f) => f,
            Err(e) => {
                problem.set(Some(e.to_string()));
                return;
            }
        };
        busy.set(true);
        spawn_local(async move {
            let done = saved::save_as(parsed.clone(), SAVED_NAME).await;
            busy.try_set(false);
            match done {
                Ok(_) => {
                    needs.filter.try_set(parsed);
                    needs.source.try_set(Source::Saved);
                    editing.try_set(false);
                }
                Err(why) => {
                    problem.try_set(Some(why));
                }
            }
        });
    };

    let source_words = move || match needs.source.get() {
        Source::Saved => format!("saved filter \u{201c}{SAVED_NAME}\u{201d}"),
        Source::Unreadable(why) => format!("the default — the saved filter could not be read: {why}"),
        Source::Default | Source::Unread => "the default".to_string(),
    };
    let count_words = move || needs.count.n().or_else(|| ids.with(|i| i.as_ref().map(Vec::len))).map_or("…".to_string(), |n| n.to_string());
    let more = move || {
        let shown = ids.with(|i| i.as_ref().map_or(0, Vec::len));
        needs.count.n().filter(|n| *n > shown).map(|n| {
            let href = format!("#/?g={}", js_sys::encode_uri_component(&needs.filter.with(Filter::format)));
            view! { <p class="nmore"><a href=href>{format!("{} more — on the board", n - shown)}</a></p> }
        })
    };
    let rows = move || {
        let Some(list) = ids.get() else {
            return view! { <p class="none loading">"loading…"</p> }.into_any();
        };
        if list.is_empty() {
            return view! { <p class="none nempty">"Nothing needs you."</p> }.into_any();
        }
        view! {
            <ul class="nrows">
                {list.into_iter().map(|id| row_view(id, held.clone())).collect_view()}
            </ul>
        }
        .into_any()
    };
    view! {
        <section id="needs-you" aria-labelledby="needs-you-h">
            <div class="nhead">
                <h2 id="needs-you-h" tabindex="-1">"Needs you"<span class="sep">" · "</span><span class="n">{count_words}</span></h2>
                <span class="nsrc" title=move || needs.filter.with(Filter::format)>
                    <code>{move || needs.filter.with(Filter::format)}</code>
                    " — "{source_words}
                </span>
                <button class="hbtn" id="needs-change" on:click=start title="change what Needs you shows — saved as the filter “Needs you”">"change"</button>
            </div>
            {move || editing.get().then(|| view! {
                <form class="nedit" on:submit=move |ev| { ev.prevent_default(); save(); }>
                    <input
                        id="needs-filter"
                        type="text"
                        aria-label="the filter Needs you shows"
                        prop:value=move || draft.get()
                        on:input=move |ev| { draft.set(event_target_value(&ev)); problem.set(None); }
                        disabled=move || busy.get()
                    />
                    <button type="submit" class="hbtn" disabled=move || busy.get()>"save"</button>
                    <button type="button" class="hbtn" on:click=move |_| draft.set(DEFAULT.to_string())>"default"</button>
                    <button type="button" class="hbtn" on:click=move |_| editing.set(false)>"cancel"</button>
                </form>
            })}
            {move || problem.get().map(|p| view! { <p class="lproblem" role="alert">{p}</p> })}
            {move || refused.get().map(|p| view! { <p class="lproblem" role="alert">"The server did not take the filter: "{p}</p> })}
            {rows}
            {more}
        </section>
    }
}

/// One row: the ticket, its status, and what it needs.
fn row_view(id: String, held: ArcRwSignal<Option<inputs::Inputs>>) -> impl IntoView {
    let href = format!("#/ticket/{id}");
    let row = {
        let id = id.clone();
        Memo::new(move |_| cache::find(&id).map(|r| r.get()))
    };
    let questions = {
        let id = id.clone();
        Memo::new(move |_| {
            held.with(|h| {
                h.as_ref().map(|h| open_of(h.list(), &id).into_iter().map(|i| (i.id.clone(), i.question.clone())).collect::<Vec<_>>())
            })
        })
    };
    let tid = id.clone();
    view! {
        <li class="nrow" data-ticket-id=id.clone()>
            <div class="nline">
                <a class="code" href=href.clone()>{id.clone()}</a>
                <a class="ntitle" href=href>{move || row.with(|r| r.as_ref().map(|t| t.title.clone()).unwrap_or_default())}</a>
                {move || row.with(|r| r.as_ref().map(|t| {
                    let st = t.status;
                    view! { <span class=format!("tag {}", st_class(st.as_str()))>{st.as_str()}</span> }
                }))}
                {labels::row_chips(move || row.with(|r| r.as_ref().map(|t| t.labels.clone()).unwrap_or_default()))}
            </div>
            {move || {
                let tid = tid.clone();
                questions.get().map(|qs| {
                    (!qs.is_empty()).then(|| view! {
                        <ul class="nqs">
                            {qs.into_iter().map(|(input, q)| {
                                let href = answer_href(&tid, &input);
                                let q = q.unwrap_or_else(|| format!("#{input}"));
                                view! {
                                    <li class="nq" data-input=input.clone()>
                                        <span class="iq">{q}</span>
                                        " "
                                        <a class="nanswer" href=href title="open the ticket at this input, and answer it there">"answer ›"</a>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    })
                })
            }}
        </li>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::filter::Facts;
    use menger_tickets_api::{Kind, Status};

    fn facts(asking: bool, labels: &[&str]) -> Facts {
        Facts {
            id: "T-1".into(),
            epic: "T".into(),
            title: String::new(),
            status: Status::Ready,
            kind: Kind::Build,
            severity: None,
            blocked: false,
            has_commits: false,
            menger_doc: false,
            asking,
            labels: labels.iter().map(|l| l.to_string()).collect(),
            lineage: vec!["T-1".into(), "T".into()],
            history: Vec::new(),
        }
    }

    /// The default is the ticket's: MENGER-39: queued, asking, or labelled
    /// `needs-you` — and it is asked as written.
    #[test]
    fn the_default_is_queued_asking_or_needs_you() {
        let f = default_filter();
        // Canonical, an `or` group is parenthesized.
        assert_eq!(f.format(), "(queued or asking or label needs-you)");
        assert_eq!(
            asked(&f).as_deref(),
            Some("(queued or asking or label needs-you) and not status Canceled"),
            "MENGER-13: never a canceled ticket"
        );
        assert_eq!(Filter::parse(&f.format()).unwrap(), f, "the canonical text reads back");
        assert!(f.matches(&facts(true, &[])));
        assert!(f.matches(&facts(false, &["needs-you"])));
        assert!(f.matches(&facts(true, &["p1", "needs-you"])));
        assert!(!f.matches(&facts(false, &["p1"])));
        assert!(!f.matches(&facts(false, &[])));
        let mut queued = facts(false, &[]);
        queued.history = vec!["queued".into()];
        assert!(f.matches(&queued), "queued: the store says so, as for every history predicate");
        assert_eq!(asked(&Filter::parse("asking and not asking").unwrap()), None, "admits nothing: asked of no one");
    }

    /// The saved filter is FILTER-6's, named "Needs you".
    #[test]
    fn the_saved_filter_is_named_needs_you() {
        assert_eq!(saved_id(), "needs-you");
        assert_eq!(saved::address_of(&saved_id()), "VIEW/_MENGER/view/needs-you");
    }

    /// An input is answered in its ticket's frame, scrolled to it.
    #[test]
    fn an_input_is_answered_at_its_anchor() {
        let href = answer_href("MENGER-12", "landing");
        assert_eq!(href, "#/ticket/MENGER-12?anchor=landing");
        assert_eq!(crate::inspect::anchor(&href), Some(crate::inspect::Anchor::Id("landing".into())));
    }

    /// A row shows its ticket's open inputs only, in order.
    #[test]
    fn a_row_shows_its_open_questions() {
        let mut a = crate::inputs::tests::input("T-1", "a");
        let mut b = crate::inputs::tests::input("T-1", "b");
        b.state = InputState::Answered;
        let c = crate::inputs::tests::input("T-2", "c");
        a.question = Some("archive or delete?".into());
        let all = vec![a, b, c];
        let open: Vec<&str> = open_of(&all, "T-1").into_iter().map(|i| i.id.as_str()).collect();
        assert_eq!(open, ["a"]);
        assert!(open_of(&all, "T-3").is_empty());
    }

    #[test]
    fn the_badge_says_how_many() {
        assert_eq!(badge_title(0), "nothing needs you");
        assert_eq!(badge_title(1), "1 ticket needs you");
        assert_eq!(badge_title(7), "7 tickets need you");
    }
}
