//! ORG-23: `#/epics` — the organization's epics, and the way an admin
//! keeps them.
//!
//! - **The table**: every live epic — its tickets, shortcodes, when it was
//!   made, and **people** (`members_on`: how many principals read in it) —
//!   *· default* on the default, *yours* on the reader's. An admin has
//!   **rename** (the old name stays an alias), **make default** and
//!   **archive** on each, and **+ new epic** above (creating one moves
//!   nobody). Archive is disabled on the last live epic, the reason on it.
//! - **The archived**, folded under `archived · N`, each with who archived
//!   it and when, and **restore** (it comes back; nobody moves back).
//! - **Archive asks first** ([`Confirm`]): `archive --dry-run` says who is
//!   on it, where they go (the next newer epic, else the next older) and how
//!   many tickets leave the views; the dialog says so and **archive NAME**
//!   writes it. Each person moved sees a notice once (src/epics.rs).
//!
//! Every change asks the server, which checks it; the page reads again.

use leptos::prelude::*;
use menger_tickets_api::{Epic, EpicsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::epics::{self, refused};
use crate::session;

/// What the archive dialog says, from the dry run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Confirm {
    pub(crate) name: String,
    pub(crate) people: usize,
    pub(crate) to: String,
    pub(crate) tickets: u32,
    /// Whether `to` is newer than `name` (else the archived was the newest).
    pub(crate) newer: bool,
}

impl Confirm {
    /// The dialog's first line.
    pub(crate) fn people_words(&self) -> String {
        match self.people {
            0 => "Nobody".to_string(),
            1 => "1 person".to_string(),
            n => format!("{n} people"),
        }
    }

    pub(crate) fn verb(&self) -> &'static str {
        if self.people == 1 {
            "is"
        } else {
            "are"
        }
    }

    pub(crate) fn which(&self) -> &'static str {
        if self.newer {
            "the next newer epic"
        } else {
            "the next older epic: it was the newest"
        }
    }
}

/// The dry run's answer as the dialog's words; `None` when it refused.
pub(crate) fn confirm_of(evs: &[EpicsEvent], live: &[Epic]) -> Option<Confirm> {
    evs.iter().find_map(|e| match e {
        EpicsEvent::Archived { epic, moved, to, tickets, .. } => {
            let at = |n: &str| live.iter().position(|x| x.name.as_str() == n);
            let newer = match (at(epic.name.as_str()), at(to.as_str())) {
                (Some(a), Some(b)) => b > a,
                _ => true,
            };
            Some(Confirm { name: epic.name.to_string(), people: moved.len(), to: to.to_string(), tickets: *tickets, newer })
        }
        _ => None,
    })
}

/// Why an epic cannot be archived, if it cannot.
pub(crate) fn archive_refusal(live: usize) -> Option<&'static str> {
    (live < 2).then_some("the last live epic: an organization reads in one")
}

/// A date as the table shows it.
fn day(unix: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix as f64 * 1000.0));
    d.to_iso_string().as_string().map(|s| s.chars().take(10).collect()).unwrap_or_default()
}

async fn read() -> Result<Vec<Epic>, String> {
    let client = session::epics_client().ok_or("not connected")?;
    let evs = client.list_all(None, Some(true)).await.map_err(|e| session::describe(&e))?;
    if let Some(m) = refused(&evs) {
        return Err(m);
    }
    Ok(evs
        .into_iter()
        .filter_map(|e| match e {
            EpicsEvent::Epic { epic } => Some(epic),
            _ => None,
        })
        .collect())
}

/// One admin write, then the page (and the picker) read again.
fn write(
    note: RwSignal<Option<String>>,
    reload: impl Fn() + Copy + 'static,
    call: impl std::future::Future<Output = Result<Vec<EpicsEvent>, plexus_client::ClientError>> + 'static,
    said: String,
) {
    spawn_local(async move {
        match call.await {
            Ok(evs) => match refused(&evs) {
                Some(m) => note.set(Some(m)),
                None => {
                    crate::menu::toast(said);
                    epics::refresh_list();
                    reload();
                }
            },
            Err(e) => note.set(Some(session::describe(&e))),
        }
    });
}

#[component]
pub(crate) fn EpicsPage() -> impl IntoView {
    let all = RwSignal::new(None::<Result<Vec<Epic>, String>>);
    let note = RwSignal::new(None::<String>);
    let confirm = RwSignal::new(None::<Confirm>);
    let renaming = RwSignal::new(None::<String>);
    let naming = RwSignal::new(false);
    let admin = move || crate::signin::account().with(|a| a.is_admin());
    let reload = move || {
        spawn_local(async move {
            all.try_set(Some(read().await));
        });
    };
    reload();
    let live = move || all.with(|a| a.as_ref().and_then(|r| r.as_ref().ok()).map(|v| v.iter().filter(|e| e.archived.is_none()).cloned().collect::<Vec<_>>()).unwrap_or_default());
    let gone = move || all.with(|a| a.as_ref().and_then(|r| r.as_ref().ok()).map(|v| v.iter().filter(|e| e.archived.is_some()).cloned().collect::<Vec<_>>()).unwrap_or_default());

    let ask_archive = move |name: String| {
        let Some(client) = session::epics_client() else { return };
        let Ok(typed) = name.parse() else { return };
        spawn_local(async move {
            match client.archive_all(typed, Some(true), None).await {
                Ok(evs) => match refused(&evs) {
                    Some(m) => note.set(Some(m)),
                    None => confirm.set(confirm_of(&evs, &live())),
                },
                Err(e) => note.set(Some(session::describe(&e))),
            }
        });
    };
    let restore = move |name: String| {
        let Some(client) = session::epics_client() else { return };
        let Ok(typed) = name.parse() else { return };
        write(note, reload, async move { client.restore_all(typed, None).await }, format!("{name} restored. Nobody moved back."));
    };
    let make_default = move |name: String| {
        let Some(client) = session::epics_client() else { return };
        let Ok(typed) = name.parse() else { return };
        write(note, reload, async move { client.set_default_all(typed, None).await }, format!("{name} is the default now: who never chose reads there."));
    };

    let row = move |e: Epic, n_live: usize| {
        let name = e.name.to_string();
        let (n1, n2, n3) = (name.clone(), name.clone(), name.clone());
        let refusal = archive_refusal(n_live);
        let acts = admin().then(|| {
            view! {
                <button class="lnk" data-act="rename" on:click=move |_| renaming.set(Some(n1.clone()))>"rename"</button>
                {(!e.default).then(|| view! { <span class="muted hide-s">" · "</span><button class="lnk" data-act="default" on:click=move |_| make_default(n3.clone())>"make default"</button> })}
                <span class="muted hide-s">" · "</span>
                <button class="lnk" data-act="archive" disabled=refusal.is_some() title=refusal.unwrap_or("archive: its people move to the next epic")
                    on:click=move |_| ask_archive(n2.clone())>"archive"</button>
            }
        });
        view! {
            <tr data-epic=name.clone()>
                <td>
                    <b class="en">{name.clone()}</b>
                    {e.default.then(|| view! { " " <span class="muted">"· default"</span> })}
                    {e.current.then(|| view! { " " <span class="you">"yours"</span> })}
                </td>
                <td>{e.tickets}</td>
                <td class="hide-s">{e.shortcodes}</td>
                <td class="hide-s">{day(e.created_unix)}</td>
                <td class="eon">{e.members_on}</td>
                <td class="eacts">{acts}</td>
            </tr>
        }
    };
    let archived_row = move |e: Epic| {
        let name = e.name.to_string();
        let n = name.clone();
        let (at, by) = e.archived.as_ref().map(|a| (day(a.at_unix), crate::chat_pane::names()(&a.by))).unwrap_or_default();
        view! {
            <tr data-epic=name.clone()>
                <td><b class="en">{name.clone()}</b>" "<span class="muted">{format!("· archived {at} by {by}")}</span></td>
                <td>{e.tickets}</td>
                <td class="hide-s">{e.shortcodes}</td>
                <td class="hide-s"><a class="lnk" href=epics::old_link(&name)>"open"</a></td>
                <td class="eon">{e.members_on}</td>
                <td class="eacts">{admin().then(|| view! { <button class="lnk" data-act="restore" on:click=move |_| restore(n.clone())>"restore"</button> })}</td>
            </tr>
        }
    };

    view! {
        <div id="epics-page">
            <div class="ehead">
                <span class="muted">{move || {
                    let n = live().len();
                    format!("{n} epic{}{}", if n == 1 { "" } else { "s" }, if admin() { " · you are an admin" } else { "" })
                }}</span>
                {move || admin().then(|| view! { <button class="ebtn" data-act="new" on:click=move |_| naming.set(true)>"+ new epic"</button> })}
            </div>
            {move || naming.get().then(|| view! { <NewEpic naming reload /> })}
            {move || note.get().map(|m| view! { <p class="merr" role="alert">{m}<button class="lnk" on:click=move |_| note.set(None)>" dismiss"</button></p> })}
            {move || match all.get() {
                None => view! { <p class="muted">"reading the epics…"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="merr" role="alert">{e}</p> }.into_any(),
                Some(Ok(_)) => {
                    let l = live();
                    let n = l.len();
                    let g = gone();
                    view! {
                        <table class="etab">
                            <thead><tr><th>"epic"</th><th>"tickets"</th><th class="hide-s">"shortcodes"</th><th class="hide-s">"created"</th><th title="the people and agents reading in it">"people"</th><th></th></tr></thead>
                            <tbody>{l.into_iter().map(|e| row(e, n)).collect_view()}</tbody>
                        </table>
                        {(!g.is_empty()).then(|| view! {
                            <details class="earch">
                                <summary class="muted">{format!("archived · {}", g.len())}</summary>
                                <table class="etab"><tbody>{g.into_iter().map(archived_row).collect_view()}</tbody></table>
                            </details>
                        })}
                    }
                    .into_any()
                }
            }}
            {move || confirm.get().map(|c| view! { <ArchiveDialog c confirm note reload /> })}
            {move || renaming.get().map(|n| view! { <RenameDialog name=n renaming note reload /> })}
        </div>
    }
}

/// `+ new epic`: a name, *create* — nobody moves.
#[component]
fn NewEpic(naming: RwSignal<bool>, reload: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let go = move || {
        let n = name.get_untracked().trim().to_string();
        if !n.is_empty() {
            epics::create(n, move || {
                naming.set(false);
                reload();
            });
        }
    };
    view! {
        <div class="enew">
            <input placeholder="name" aria-label="new epic name" prop:value=move || name.get()
                on:input=move |ev| name.set(event_target_value(&ev))
                on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { go() } />
            <button class="primary" on:click=move |_| go()>"create"</button>
            <button on:click=move |_| naming.set(false)>"cancel"</button>
        </div>
    }
}

/// The archive confirmation, from the dry run.
#[component]
fn ArchiveDialog(c: Confirm, confirm: RwSignal<Option<Confirm>>, note: RwSignal<Option<String>>, reload: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let busy = RwSignal::new(false);
    let close = move || confirm.set(None);
    let name = c.name.clone();
    let go = {
        let name = name.clone();
        move || {
            let Some(client) = session::epics_client() else { return };
            let Ok(typed) = name.parse() else { return };
            busy.set(true);
            let said = format!("{name} archived.");
            write(note, reload, async move { client.archive_all(typed, None, None).await }, said);
            confirm.set(None);
        }
    };
    let on_key = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" {
            close();
        }
    };
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div id="epic-archive" class="dialog" role="dialog" aria-modal="true" aria-labelledby="ea-h" on:click=|ev| ev.stop_propagation() on:keydown=on_key>
                <h2 id="ea-h">"Archive "<span class="mono">{name.clone()}</span>"?"</h2>
                <p>
                    <b>{c.people_words()}</b>{format!(" {} on ", c.verb())}<span class="mono">{name.clone()}</span>". "
                    {(c.people > 0).then(|| view! { "They move to "<b class="mono">{c.to.clone()}</b>{format!(", {}, and each sees a notice once.", c.which())} })}
                </p>
                <ul class="efacts">
                    <li>{format!("{} tickets leave every view, the picker and ", c.tickets)}<span class="mono">"epic any"</span>"."</li>
                    <li>"Links, chips and "<span class="mono">{format!("epic {name}")}</span>" still reach them."</li>
                    <li>"Restore brings it back. Nobody moves back."</li>
                </ul>
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| close()>"cancel"</button>
                    <button class="primary danger" data-act="archive" disabled=move || busy.get() on:click=move |_| go()>{format!("archive {name}")}</button>
                </div>
            </div>
        </div>
    }
}

/// Rename: the old name stays an alias.
#[component]
fn RenameDialog(name: String, renaming: RwSignal<Option<String>>, note: RwSignal<Option<String>>, reload: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let to = RwSignal::new(name.clone());
    let close = move || renaming.set(None);
    let go = {
        let name = name.clone();
        move || {
            let Some(client) = session::epics_client() else { return };
            let (Ok(from), Ok(next)) = (name.parse(), crate::typed::typed(to.get_untracked().trim())) else {
                note.set(Some("not an epic's name".into()));
                return;
            };
            let said = format!("{name} is {} now; {name} still names it.", to.get_untracked().trim());
            write(note, reload, async move { client.rename_all(from, next, None).await }, said);
            renaming.set(None);
        }
    };
    let go2 = go.clone();
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div id="epic-rename" class="dialog" role="dialog" aria-modal="true" on:click=|ev| ev.stop_propagation()>
                <h2>"Rename "<span class="mono">{name.clone()}</span></h2>
                <input aria-label="new name" prop:value=move || to.get() on:input=move |ev| to.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { go2() } else if ev.key() == "Escape" { close() } />
                <p class="hint">"The old name stays an alias, so filters and links that name it still work."</p>
                <div class="sacts">
                    <button on:click=move |_| close()>"cancel"</button>
                    <button class="primary" on:click=move |_| go()>"rename"</button>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{EpicName, OrgId};

    fn epic(name: &str) -> Epic {
        Epic {
            org: OrgId::hypermemetic(),
            name: EpicName::new(name).unwrap(),
            first: false,
            current: false,
            shortcodes: 0,
            tickets: 0,
            created_unix: 0,
            default: false,
            members_on: 0,
            archived: None,
        }
    }

    fn dry(name: &str, to: &str, people: usize) -> Vec<EpicsEvent> {
        vec![EpicsEvent::Archived { epic: epic(name), moved: (0..people).map(|i| format!("key:{i}")).collect(), to: EpicName::new(to).unwrap(), tickets: 212, dry_run: true }]
    }

    #[test]
    fn the_dry_run_says_who_goes_where() {
        let live = [epic("genesis"), epic("spring"), epic("fresh")];
        let c = confirm_of(&dry("spring", "fresh", 4), &live).unwrap();
        assert_eq!((c.people_words(), c.verb(), c.to.as_str(), c.tickets), ("4 people".into(), "are", "fresh", 212));
        assert_eq!(c.which(), "the next newer epic");
        let c = confirm_of(&dry("fresh", "spring", 1), &live).unwrap();
        assert_eq!((c.people_words(), c.verb()), ("1 person".into(), "is"));
        assert!(c.which().starts_with("the next older"), "the newest goes to the one before");
        assert!(confirm_of(&[EpicsEvent::Error { message: "x".into() }], &live).is_none());
    }

    #[test]
    fn the_last_live_epic_is_not_archived() {
        assert!(archive_refusal(1).is_some());
        assert!(archive_refusal(2).is_none());
    }
}
