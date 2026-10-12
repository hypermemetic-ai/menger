//! MENGER-13: a ticket's **History** — every change of its status and kind
//! since the transition log began, as `tickets.history` answers it: when,
//! from → to, who (a person, an agent, `system`), from where (app, api,
//! pipeline, cascade), why, and — for a cascade — the ticket whose change
//! caused it, as a link. Then what its own changes caused in other tickets
//! (a cancel's cascade, a restore's), and a plain line saying when the log
//! began: nothing before then has a history.
//!
//! Asked once when the page opens, and again whenever `watch` carries a
//! transition of this ticket or one its changes caused — so a cascade, a
//! restore or a change made elsewhere shows without a reload.

use leptos::prelude::*;
use leptos::task::spawn_local;
use menger_tickets_api::{TicketsEvent, Transition, TransitionField, TransitionSource, SYSTEM_ACTOR};

use crate::{fmt_relative, fmt_unix, now_unix, session, st_class};

/// What `history` answered.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Answer {
    pub(crate) own: Vec<Transition>,
    pub(crate) caused: Vec<Transition>,
    pub(crate) began: Option<i64>,
    pub(crate) restorable: Vec<String>,
}

/// `history`'s events, read into an [`Answer`] — `None` for a refusal.
pub(crate) fn answer_of(id: &str, events: Vec<TicketsEvent>) -> Option<Answer> {
    let mut a = Answer::default();
    for ev in events {
        match ev {
            TicketsEvent::Transition { transition } if transition.ticket.eq_ignore_ascii_case(id) => a.own.push(transition),
            TicketsEvent::Transition { transition } => a.caused.push(transition),
            TicketsEvent::HistoryEnd { began_unix, restorable } => {
                a.began = began_unix;
                a.restorable = restorable;
            }
            TicketsEvent::NotFound { .. } | TicketsEvent::Error { .. } => return None,
            _ => {}
        }
    }
    Some(a)
}

/// Whether a `watch` event moves what `id`'s history shows: a transition of
/// it, or one caused by a change of it.
pub(crate) fn touches(id: &str, ev: &TicketsEvent) -> bool {
    match ev {
        TicketsEvent::Transition { transition } => {
            transition.ticket.eq_ignore_ascii_case(id) || transition.cause_ticket.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(id))
        }
        _ => false,
    }
}

/// Who made a transition, as a reader names them: the display name, else
/// the principal (`system` and `anonymous` as they are).
pub(crate) fn who(t: &Transition) -> String {
    t.name.clone().unwrap_or_else(|| t.actor.clone())
}

/// What a source says on hover.
pub(crate) fn source_title(s: TransitionSource) -> &'static str {
    match s {
        TransitionSource::App => "app: a viewer in the app",
        TransitionSource::Api => "api: synapse, an agent or a script",
        TransitionSource::Pipeline => "pipeline: the merge",
        TransitionSource::Cascade => "cascade: carried down from another ticket's change",
    }
}

/// When the log began, in the viewer's words — "History begins …".
pub(crate) fn began_line(began: Option<i64>) -> String {
    match began {
        Some(at) => format!(
            "History begins {} — when this menger started keeping it; changes before then were not recorded.",
            local_date(at)
        ),
        None => "History begins when this menger started keeping it; changes before then were not recorded.".into(),
    }
}

/// A moment in the viewer's own zone and words.
fn local_date(at: i64) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(at as f64 * 1000.0));
        d.to_locale_string("default", &wasm_bindgen::JsValue::UNDEFINED).into()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        at.to_string()
    }
}

/// A status's chip, or a kind's word.
fn value_view(field: TransitionField, v: Option<String>) -> AnyView {
    match (field, v) {
        (_, None) => view! { <span class="born">"born"</span> }.into_any(),
        (TransitionField::Status, Some(s)) => {
            let class = format!("status {}", st_class(&s));
            view! { <span class=class>{s}</span> }.into_any()
        }
        (TransitionField::Kind, Some(k)) => view! { <span class="kindword">{k}</span> }.into_any(),
    }
}

/// One transition, as a row.
fn row(t: Transition, show_ticket: bool) -> impl IntoView {
    let system = t.actor == SYSTEM_ACTOR;
    let field = t.field;
    let what = if field == TransitionField::Kind { "kind" } else { "" };
    let ticket = show_ticket.then(|| {
        let href = format!("#/ticket/{}", t.ticket);
        view! { <a class="code" href=href data-ticket-id=t.ticket.clone()>{t.ticket.clone()}</a> }
    });
    let cause = t.cause_ticket.clone().filter(|_| !show_ticket).map(|c| {
        let href = format!("#/ticket/{c}");
        view! { <span class="cause">"caused by "<a class="code" href=href data-ticket-id=c.clone()>{c.clone()}</a></span> }
    });
    view! {
        <li class="tr" class:system=system data-transition=t.id.to_string()>
            <time class="at" title=fmt_unix(t.at_unix)>{fmt_relative(now_unix(), t.at_unix)}</time>
            {ticket}
            <span class="move">
                {(!what.is_empty()).then_some(view! { <span class="fieldname">{what}</span> })}
                {value_view(field, t.from.clone())}
                <span class="arrow" aria-hidden="true">"→"</span>
                {value_view(field, Some(t.to.clone()))}
            </span>
            <span class="who" title=t.actor.clone()>{who(&t)}</span>
            <span class="src" title=source_title(t.source)>{t.source.as_str()}</span>
            {t.message.clone().map(|m| view! { <span class="msg">{m}</span> })}
            {cause}
        </li>
    }
}

/// The ticket page's History section.
#[component]
pub(crate) fn HistoryView(ticket: String) -> impl IntoView {
    let answer: RwSignal<Option<Answer>> = RwSignal::new(None);
    let asked = StoredValue::new(0u32);
    let id = ticket.clone();
    let ask = move || {
        let Some(client) = session::client() else { return };
        let id = id.clone();
        let n = asked.get_value().wrapping_add(1);
        asked.set_value(n);
        spawn_local(async move {
            let Ok(events) = client.history_all(id.clone()).await else { return };
            // Only the newest question's answer.
            if asked.try_get_value() == Some(n) {
                answer.try_set(answer_of(&id, events));
            }
        });
    };
    ask();
    let me = ticket.clone();
    let again = ask.clone();
    session::listen(move |ev| {
        if touches(&me, ev) {
            again();
        }
    });
    let id = ticket.clone();
    view! {
        <section class="histsec" id="t-history">
            <h3>"History"</h3>
            {move || match answer.get() {
                None => view! { <p class="loading">"loading history…"</p> }.into_any(),
                Some(a) => {
                    let began = began_line(a.began);
                    let own = if a.own.is_empty() {
                        view! { <p class="none">"No change recorded yet."</p> }.into_any()
                    } else {
                        view! { <ol class="transitions">{a.own.into_iter().rev().map(|t| row(t, false)).collect_view()}</ol> }.into_any()
                    };
                    let caused = (!a.caused.is_empty()).then(|| {
                        let n = a.caused.len();
                        view! {
                            <h4>{format!("What its changes caused · {n}")}</h4>
                            <ol class="transitions caused">{a.caused.into_iter().rev().map(|t| row(t, true)).collect_view()}</ol>
                        }
                    });
                    let restorable = (!a.restorable.is_empty()).then(|| {
                        let n = a.restorable.len();
                        view! {
                            <p class="restorable">
                                {format!(
                                    "Moving {} out of Canceled offers to restore the {n} ticket{} its cancel canceled.",
                                    id,
                                    if n == 1 { "" } else { "s" }
                                )}
                            </p>
                        }
                    });
                    view! {
                        {own}
                        {caused}
                        {restorable}
                        <p class="began">{began}</p>
                    }
                    .into_any()
                }
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: u64, ticket: &str, from: Option<&str>, to: &str, actor: &str, cause: Option<(u64, &str)>) -> Transition {
        Transition {
            id,
            ticket: ticket.into(),
            field: TransitionField::Status,
            from: from.map(String::from),
            to: to.into(),
            at_unix: 100 + id as i64,
            actor: actor.into(),
            name: None,
            source: if cause.is_some() { TransitionSource::Cascade } else { TransitionSource::Api },
            message: cause.map(|(_, c)| format!("canceled because {c} was canceled by ada")),
            cause: cause.map(|(c, _)| c),
            cause_ticket: cause.map(|(_, c)| c.to_string()),
        }
    }

    /// `history`'s answer read: the ticket's own, what they caused, when the
    /// log began, what a restore brings back; a refusal is none.
    #[test]
    fn an_answer_is_read_from_the_events() {
        let events = vec![
            TicketsEvent::Transition { transition: t(1, "R-2", None, "Pending", "anonymous", None) },
            TicketsEvent::Transition { transition: t(4, "R-2", Some("Pending"), "Canceled", "idp:ada", None) },
            TicketsEvent::Transition { transition: t(5, "R-3", Some("Ready"), "Canceled", SYSTEM_ACTOR, Some((4, "R-2"))) },
            TicketsEvent::HistoryEnd { began_unix: Some(50), restorable: vec!["R-3".into()] },
        ];
        let a = answer_of("r-2", events).unwrap();
        assert_eq!(a.own.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 4]);
        assert_eq!(a.caused.iter().map(|t| t.ticket.as_str()).collect::<Vec<_>>(), ["R-3"]);
        assert_eq!((a.began, a.restorable), (Some(50), vec!["R-3".to_string()]));
        assert!(answer_of("R-9", vec![TicketsEvent::NotFound { id: "R-9".into() }]).is_none());
    }

    /// A transition of the ticket, or one its change caused, moves its
    /// history; another ticket's does not.
    #[test]
    fn what_moves_a_history() {
        let ev = |tr| TicketsEvent::Transition { transition: tr };
        assert!(touches("R-2", &ev(t(4, "R-2", Some("Pending"), "Canceled", "a", None))));
        assert!(touches("R-2", &ev(t(5, "R-3", Some("Ready"), "Canceled", SYSTEM_ACTOR, Some((4, "R-2"))))));
        assert!(!touches("R-2", &ev(t(6, "R-8", Some("Ready"), "Coding", "a", None))));
        assert!(!touches("R-2", &TicketsEvent::Lagged { missed: 1 }));
    }

    /// Who, as named: the name, else the principal; and the line saying when
    /// history began.
    #[test]
    fn who_and_when_history_began() {
        let mut x = t(1, "R-2", None, "Pending", "idp:ada", None);
        assert_eq!(who(&x), "idp:ada");
        x.name = Some("Ada".into());
        assert_eq!(who(&x), "Ada");
        assert!(began_line(Some(1_791_799_200)).starts_with("History begins 1791799200 — "));
        assert!(began_line(None).contains("changes before then were not recorded"));
    }
}
