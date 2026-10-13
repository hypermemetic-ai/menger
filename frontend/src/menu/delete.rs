//! ACT-4: **Delete…** — a confirmation inside the app (the viewer allows no
//! `confirm()`), listing every ticket the delete takes and what moves:
//! children — to the ticket's parent, or gone too (the reader's choice, when
//! one has any) — and the tickets outside that wait on one, which are
//! unblocked. Delete is one `tickets.delete` (ACT-5); what it removed comes
//! back in a toast with **Undo**, which calls `undelete` while the
//! tombstones keep them (30 days).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use super::model::{self, DeletePlan};
use super::{write, Toaster};

/// The dialog, while `confirm` holds a plan.
pub(super) fn dialog(
    plan: DeletePlan,
    confirm: RwSignal<Option<DeletePlan>>,
    toaster: Toaster,
    last_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let go_too = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let cancel_btn = NodeRef::<leptos::html::Button>::new();
    cancel_btn.on_load(|el: web_sys::HtmlButtonElement| {
        let _ = el.focus();
    });
    let close = move || confirm.set(None);
    let run = {
        let plan = plan.clone();
        move || {
            if busy.get_untracked() {
                return;
            }
            busy.set(true);
            let (children, unblock) = model::delete_args(&plan, go_too.get_untracked());
            let ids = plan.ids.clone();
            spawn_local(async move {
                match write::delete(ids, children, unblock).await {
                    Ok(removed) => {
                        confirm.set(None);
                        toaster.show(model::deleted_words(&removed), Some(removed));
                    }
                    Err(why) => {
                        busy.try_set(false);
                        problem.try_set(Some(why.clone()));
                        last_error.set(Some(format!("delete: {why}")));
                    }
                }
            });
        }
    };
    let on_key = move |ev: web_sys::KeyboardEvent| {
        if crate::bindings::is(&ev, crate::bindings::Context::Menu, "dialog.close") {
            ev.stop_propagation();
            close();
        }
    };
    let lines = plan
        .doomed
        .iter()
        .map(|d| {
            let mut notes = Vec::new();
            if d.children > 0 {
                notes.push(format!("{} child{}", d.children, if d.children == 1 { "" } else { "ren" }));
            }
            if !d.waiting.is_empty() {
                notes.push(format!("{} wait{} on it", d.waiting.join(", "), if d.waiting.len() == 1 { "s" } else { "" }));
            }
            let notes = (!notes.is_empty()).then(|| view! { <span class="dnote">{notes.join(" · ")}</span> });
            view! {
                <li data-id=d.id.clone()>
                    <span class="code">{d.id.clone()}</span>" "
                    <span class="dtitle">{d.title.clone()}</span>
                    {notes}
                </li>
            }
        })
        .collect_view();
    let children = plan.has_children().then(|| {
        view! {
            <fieldset class="dchildren">
                <legend>"Their children"</legend>
                <label>
                    <input type="radio" name="dchildren" value="reparent" prop:checked=move || !go_too.get() on:change=move |_| go_too.set(false) />
                    " move to the deleted ticket's parent"
                </label>
                <label>
                    <input type="radio" name="dchildren" value="delete" prop:checked=move || go_too.get() on:change=move |_| go_too.set(true) />
                    " are deleted too"
                </label>
            </fieldset>
        }
    });
    let waiting = plan.has_waiting().then(|| view! { <p class="dwaiting">"The tickets that wait on them are unblocked."</p> });
    let heading = plan.words();
    let n = plan.ids.len();
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div
                id="delete"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-labelledby="delete-h"
                data-count=n
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <h2 id="delete-h">{heading}</h2>
                <ul class="dlist">{lines}</ul>
                {children}
                {waiting}
                <p class="hint">"An admin can undo it for 30 days; Undo is offered at once."</p>
                {move || problem.get().map(|p| view! { <p class="merr" role="alert">{p}</p> })}
                <div class="sacts">
                    <button data-act="cancel" node_ref=cancel_btn on:click=move |_| close()>"Cancel"</button>
                    <button class="primary danger" data-act="delete" disabled=move || busy.get() on:click=move |_| run()>
                        {if n == 1 { "Delete".to_string() } else { format!("Delete {n}") }}
                    </button>
                </div>
            </div>
        </div>
    }
}
