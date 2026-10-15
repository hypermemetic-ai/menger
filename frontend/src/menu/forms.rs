//! ACT-4: the items that ask for more, inside the menu — the menu's items
//! give way to one field:
//!
//! - **Move under…** and **Blocked by…** pick a ticket with the palette's
//!   `find` (SCALE-25: by id or title, best first) — never one of the tickets
//!   acted on; Enter takes the first, the arrows walk the list.
//! - **Rename…** edits the title in place (one ticket).
//! - **Link commits…** takes hashes (`abc1234 def5678`, or `repo@hash`).
//! - **Add a label…** (from Labels ▸) takes one label, normalized as the
//!   page's editor takes it (src/labels.rs).
//!
//! Each, done, is one write (src/menu/write.rs) and closes the menu; Escape
//! closes it untouched.

use std::sync::Arc;

use leptos::prelude::*;
use menger_tickets_api::TicketSummary;
use wasm_bindgen_futures::spawn_local;

use super::model::{self, Change, Form};
use super::{close_menu, write, Target};
use crate::{labels, session, st_class};

/// How many tickets a picker shows.
const PICKS: u32 = 8;

/// The form for `f`, in the menu's place.
pub(super) fn view(
    f: Form,
    ids: Arc<Vec<String>>,
    rows: Memo<Vec<TicketSummary>>,
    last_error: RwSignal<Option<String>>,
    target: RwSignal<Option<Target>>,
    form: RwSignal<Option<Form>>,
) -> AnyView {
    let input = NodeRef::<leptos::html::Input>::new();
    input.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
        el.select();
    });
    let problem = RwSignal::new(None::<String>);
    let found = RwSignal::new(Vec::<TicketSummary>::new());
    let asked = StoredValue::new(0u32);
    let initial = match f {
        Form::Rename => rows.with_untracked(|r| r.first().map(|r| r.title.clone()).unwrap_or_default()),
        _ => String::new(),
    };
    let draft = RwSignal::new(initial);

    // Done: one write, the menu closed.
    let done = {
        let ids = ids.clone();
        move |change: Change| {
            if let Some(c) = model::call(&ids, &change) {
                write::send(c, change, last_error);
            }
            close_menu(target);
        }
    };

    // A picker asks `find` as the reader types; an answer to an older query
    // is dropped.
    let ask = {
        let ids = ids.clone();
        move |q: String| {
            let n = asked.get_value().wrapping_add(1);
            asked.set_value(n);
            let q = q.trim().to_string();
            if q.is_empty() {
                found.set(Vec::new());
                return;
            }
            let ids = ids.clone();
            spawn_local(async move {
                let answer = session::find_under(q, PICKS + ids.len() as u32, None).await;
                if asked.try_get_value() != Some(n) {
                    return;
                }
                match answer {
                    Ok(Ok((rows, _))) => {
                        let rows: Vec<TicketSummary> = rows.into_iter().filter(|r| model::pickable(&ids, &r.id)).take(PICKS as usize).collect();
                        found.try_set(rows);
                        problem.try_set(None);
                    }
                    Ok(Err(why)) | Err(why) => {
                        problem.try_set(Some(why));
                    }
                }
            });
        }
    };

    let pick_change = move |id: String| if f == Form::MoveUnder { Change::Parent(id) } else { Change::Block(id) };

    let submit = {
        let done = done.clone();
        let ids = ids.clone();
        move || {
            let text = draft.get_untracked();
            let change = match f {
                Form::MoveUnder | Form::BlockedBy => {
                    // The first found, else what was typed as an id.
                    let first = found.with_untracked(|r| r.first().map(|r| r.id.to_string()));
                    let typed = text.trim().to_ascii_uppercase();
                    match first.or_else(|| (!typed.is_empty()).then_some(typed)) {
                        Some(id) if model::pickable(&ids, &id) => pick_change(id),
                        Some(id) => {
                            problem.set(Some(format!("{id} is one of the tickets being moved")));
                            return;
                        }
                        None => {
                            problem.set(Some("type an id or words of a title".into()));
                            return;
                        }
                    }
                }
                Form::Rename => {
                    let t = text.trim().to_string();
                    if t.is_empty() {
                        problem.set(Some("a title is not blank".into()));
                        return;
                    }
                    Change::Title(t)
                }
                Form::Commits => {
                    let c = model::parse_commits(&text);
                    if c.is_empty() {
                        problem.set(Some("type a commit hash".into()));
                        return;
                    }
                    Change::Commits(c)
                }
                Form::AddLabel => match labels::parse_add(&text) {
                    Ok(l) if l.len() == 1 => Change::Label(l[0].clone()),
                    Ok(_) => {
                        problem.set(Some("one label at a time".into()));
                        return;
                    }
                    Err(why) => {
                        problem.set(Some(why));
                        return;
                    }
                },
            };
            done(change);
        }
    };

    let on_key = {
        let submit = submit.clone();
        move |ev: web_sys::KeyboardEvent| {
            use crate::bindings::{is, Context};
            if is(&ev, Context::Field, "form.submit") {
                ev.prevent_default();
                ev.stop_propagation();
                submit();
                return;
            }
            // The menu walks its items; Escape and Tab close it.
            let menus = ["list.next", "list.prev", "dialog.close", "menu.leave", "menu.leave-back"];
            if !menus.iter().any(|id| is(&ev, Context::Menu, id)) {
                ev.stop_propagation();
            }
        }
    };
    let ask_on_input = ask.clone();
    let placeholder = match f {
        Form::MoveUnder => "the new parent: an id or a title's words",
        Form::BlockedBy => "the blocker: an id or a title's words",
        Form::Rename => "the title",
        Form::Commits => "commit hashes, or repo@hash",
        Form::AddLabel => "a label, as needs-you",
    };
    let action = match f {
        Form::MoveUnder => "Move",
        Form::BlockedBy => "Block",
        Form::Rename => "Rename",
        Form::Commits => "Link",
        Form::AddLabel => "Add",
    };
    let picks = f.picks();
    view! {
        <div class="mform" data-form=f.name()>
            <div class="mform-h">
                <button class="item back" role="menuitem" data-act="back" on:click=move |_| form.set(None)>"‹ "{f.words()}</button>
            </div>
            <div class="mform-row">
                <input
                    node_ref=input
                    class="mfield"
                    type="text"
                    aria-label=f.words()
                    placeholder=placeholder
                    prop:value=move || draft.get()
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        draft.set(v.clone());
                        problem.set(None);
                        if picks {
                            ask_on_input(v);
                        }
                    }
                    on:keydown=on_key
                />
                <button class="mgo" data-act="submit" on:click=move |_| submit()>{action}</button>
            </div>
            {move || problem.get().map(|p| view! { <p class="merr" role="alert">{p}</p> })}
            {picks.then(|| view! {
                <div class="picks" role="group">
                    {move || {
                        found
                            .get()
                            .into_iter()
                            .map(|r| {
                                let id = r.id.to_string();
                                let done = done.clone();
                                view! {
                                    <button class="item pick" role="menuitem" data-pick-id=id.clone() on:click=move |_| done(pick_change(id.to_string()))>
                                        <span class="code">{r.id.to_string()}</span>
                                        " "
                                        <span class=format!("tag {}", st_class(r.status.as_str()))>{r.status.to_string()}</span>
                                        " "
                                        <span class="ptitle">{r.title.clone()}</span>
                                    </button>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            })}
        </div>
    }
    .into_any()
}
