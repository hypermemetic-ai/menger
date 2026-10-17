//! ACT-4: the one menu's view — mounted once ([`TicketMenu`]): the open
//! menu ([`model::entries`] drawn in order, a submenu inline under its item,
//! a form in the items' place), the delete confirmation and the toast. What
//! it holds is src/menu/model.rs's; what each item writes, src/menu/write.rs's.
//!
//! ACT-10: a value chip's chooser ([`Entry::Choice`]) is drawn by the same
//! [`options_view`] a submenu's items are, in a `.flat` group with no head
//! and no indent; `s` opens the menu with Set status already expanded
//! (`Target::open`) and focus on the current status.

use super::*;

/// ACT-4: what the reader may do (src/signin.rs) — the server decides every
/// call.
pub(super) fn grants() -> Grants {
    signin::account().with(|a| Grants { write: a.may_write(), delete: a.may_delete() })
}

/// The app's toast and its generation, for the delete dialog's Undo.
#[derive(Clone, Copy)]
pub(super) struct Toaster {
    pub(super) toast: RwSignal<Option<Toast>>,
    pub(super) gen: StoredValue<u32>,
}

impl Toaster {
    pub(super) fn show(self, text: String, undo: Option<Vec<String>>) {
        show_toast(self.toast, self.gen, Toast { text, manual: None, undo });
    }
}

/// Mounted once by the App, outside every ticket element. Renders the
/// open menu (if any), the delete confirmation (if any) and the toast (if
/// any); owns the document-level listeners.
#[component]
pub(crate) fn TicketMenu(last_error: RwSignal<Option<String>>) -> impl IntoView {
    let target = RwSignal::new(None::<Target>);
    let toast = RwSignal::new(None::<Toast>);
    let toast_gen = StoredValue::new(0u32);
    let toaster = Toaster { toast, gen: toast_gen };
    // ACT-4: Delete…'s confirmation, outside the menu (which closes).
    let confirm = RwSignal::new(None::<model::DeletePlan>);
    install_listeners(target, last_error);
    OPEN.with(|o| o.set(Some(target)));
    TOASTER.with(|t| t.set(Some(toaster)));

    view! {
        {move || {
            target.get().map(|t| {
                if t.ids.is_empty() {
                    nav_menu_view(t, target).into_any()
                } else {
                    menu_view(t, last_error, target, toaster, confirm).into_any()
                }
            })
        }}
        {move || confirm.get().map(|plan| delete::dialog(plan, confirm, toaster, last_error))}
        {move || toast.get().map(|t| toast_view(t, toaster, last_error))}
    }
}

/// How many rows a menu asks for that no view holds.
const LOAD_MAX: usize = 200;

/// The context every line of an open menu shares.
#[derive(Clone)]
struct Ctx {
    ids: Arc<Vec<String>>,
    rows: Memo<Vec<TicketSummary>>,
    nav: Option<nav::At>,
    level: Level,
    open_sub: RwSignal<Option<Sub>>,
    form: RwSignal<Option<Form>>,
    target: RwSignal<Option<Target>>,
    last_error: RwSignal<Option<String>>,
    toaster: Toaster,
    confirm: RwSignal<Option<model::DeletePlan>>,
}

/// One open menu: the header (one ticket's chips, or "N tickets") and
/// [`model::entries`] in order — a submenu opening inline under its item, a
/// form (a picker, Rename…, Link commits…) taking the items' place. Built
/// fresh per open, so a second right-click starts with every list closed
/// and the keyboard cursor on the menu itself.
fn menu_view(
    t: Target,
    last_error: RwSignal<Option<String>>,
    target: RwSignal<Option<Target>>,
    toaster: Toaster,
    confirm: RwSignal<Option<model::DeletePlan>>,
) -> impl IntoView {
    let Target { ids, shortcode, x, y, nav, chip, level, framed, open } = t;
    // SCALE-26: the menu holds the tickets it is open on, and asks for those
    // no view holds (no walk puts every ticket in the cache any more) — the
    // first few hundred of a selection: the rest mark nothing until shown.
    let holder = cache::Holder::new();
    let missing: Vec<String> = ids.iter().filter(|id| !holder.hold_id(id)).take(LOAD_MAX).cloned().collect();
    if !missing.is_empty() {
        session::load_rows(missing, holder);
    }
    let ids = Arc::new(ids);
    // Live through each row's signal: a change arriving over watch while the
    // menu is open moves the marks.
    let rows = {
        let ids = ids.clone();
        Memo::new(move |_| ids.iter().filter_map(|id| cache::find(id).map(|s| s.get())).collect::<Vec<TicketSummary>>())
    };
    let tree = lens::use_lens().sctree();
    let granted = grants();
    let one = (ids.len() == 1).then(|| ids[0].clone());
    // CHAT-5: the reader's chats on the one ticket, live.
    let chat_facts = {
        let one = one.clone();
        Memo::new(move |_| one.as_deref().map(chat::facts))
    };
    let entries = {
        let (ids, shortcode, chip, nav_here) = (ids.clone(), shortcode.clone(), chip.clone(), nav.is_some());
        Memo::new(move |_| {
            chat_facts.with(|cf| rows.with(|rows| {
                tree.with(|tree| {
                    model::entries(&Facts {
                        ids: &ids,
                        rows,
                        shortcode: shortcode.as_deref(),
                        chip: chip.as_ref(),
                        grants: granted,
                        nav: nav_here,
                        tree: tree.as_ref(),
                        global: level == Level::Global,
                        framed,
                        chat: cf.as_ref(),
                    })
                })
            }))
        })
    };
    let head_row = {
        let one = one.clone();
        Memo::new(move |_| one.as_ref().and_then(|id| rows.with(|r| r.iter().find(|r| &r.id == id).map(|r| (r.status, r.title.clone())))))
    };
    // ACT-10: `s` opens it at Set status.
    let open_sub = RwSignal::new(open);
    let form = RwSignal::new(None::<Form>);
    let root = NodeRef::<leptos::html::Div>::new();
    let close = move || close_menu(target);

    // Place once mounted, focus the menu so the arrow keys work at once, and
    // re-place when a list or a form changes the height.
    let focused = StoredValue::new(false);
    // ACT-10: opened at a submenu (`s`), focus goes to its current value —
    // the checked one, else its first — and follows the rows in: a row the
    // menu had to ask for marks its value a moment later. Settled once the
    // checked one has focus, or once the reader has moved it elsewhere.
    let settled = StoredValue::new(open.is_none());
    let auto = StoredValue::new_local(None::<web_sys::HtmlElement>);
    Effect::new(move |_| {
        let Some(el) = root.get() else { return };
        open_sub.track();
        form.track();
        entries.track();
        rows.track();
        place(&el, x, y);
        if !focused.get_value() {
            focused.set_value(true);
            focus(&el);
        }
        if settled.get_value() {
            return;
        }
        let active = document().active_element();
        let ours = active.as_ref().is_some_and(|a| a.is_same_node(Some(&el)))
            || auto.with_value(|a| a.as_ref().is_some_and(|a| active.as_ref().is_some_and(|x| x.is_same_node(Some(a)))));
        if !ours {
            settled.set_value(true);
            return;
        }
        let Some(w) = open else { return };
        let Some(group) = el.query_selector(&format!(".sub[data-sub='{}']", w.name())).ok().flatten() else { return };
        let list = items(&group);
        if let Some(checked) = list.iter().find(|i| i.get_attribute("aria-checked").as_deref() == Some("true")) {
            focus(checked);
            settled.set_value(true);
        } else if let Some(first) = list.first() {
            focus(first);
            auto.set_value(Some(first.clone()));
        }
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let Some(el) = root.get() else { return };
        let root_el: &web_sys::Element = &el;
        let id = bindings::hit(&ev, Context::Menu);
        let active = document().active_element();
        let list = items(root_el);
        let current = list.iter().position(|i| active.as_ref() == Some(i.as_ref()));
        let on_parent = list
            .get(current.unwrap_or(usize::MAX))
            .is_some_and(|i| i.has_attribute("aria-haspopup"));
        // ACT-7 × ACT-4: every list opens from its parent, named by its
        // `data-sub`.
        let which = |el: Option<&web_sys::Element>| el.and_then(|e| e.get_attribute("data-sub")).and_then(|n| Sub::of_name(&n));
        match id {
            Some("menu.into") if on_parent => {
                let Some(w) = which(current.map(|i| list[i].as_ref())) else { return };
                open_at_first(root, open_sub, w);
                ev.prevent_default();
            }
            // CHAT-5: `c` chooses Chat — never while a form's field is typed in.
            Some("menu.chat") if !typing(&ev) => {
                chat::key(root, open_sub);
                ev.prevent_default();
            }
            Some("menu.out") if open_sub.get_untracked().is_some() => {
                let Some(w) = open_sub.get_untracked() else { return };
                open_sub.set(None);
                if let Some(parent) = items(root_el).into_iter().find(|i| i.has_attribute("aria-haspopup") && which(Some(i)) == Some(w)) {
                    focus(&parent);
                }
                ev.prevent_default();
            }
            Some("menu.leave" | "menu.leave-back") => close(),
            id => {
                if let Some(n) = step(current, list.len(), id) {
                    focus(&list[n]);
                    ev.prevent_default();
                }
            }
        }
    };

    let ids_attr = ids.join(" ");
    let label = model::heading(&ids);
    let aria = format!("ticket menu: {label}");
    let count = ids.len();
    let ctx = Ctx { ids, rows, nav, level, open_sub, form, target, last_error, toaster, confirm };
    view! {
        <div
            id=MENU_ID
            role="menu"
            tabindex="-1"
            node_ref=root
            data-ticket=one.clone()
            data-tickets=ids_attr
            data-count=count
            data-level=level.name()
            aria-label=aria
            style=format!("left: {x}px; top: {y}px")
            on:contextmenu=|ev| ev.prevent_default()
            on:keydown=on_key
        >
            <div class="h">
                <span class="code">{label}</span>
                {move || head_row.get().map(|(st, title)| view! {
                    <span class=format!("tag {}", st_class(st.as_str()))>{st.to_string()}</span>
                    <span class="title">{title}</span>
                })}
            </div>
            {move || match form.get() {
                Some(f) => forms::view(f, ctx.ids.clone(), ctx.rows, ctx.last_error, ctx.target, ctx.form).into_any(),
                None => entries.get().into_iter().map(|e| entry_view(e, &ctx)).collect_view().into_any(),
            }}
        </div>
    }
}

/// Open submenu `w` and focus its first row — it mounts on the next render
/// tick, so once it exists.
pub(super) fn open_at_first(root: NodeRef<leptos::html::Div>, open_sub: RwSignal<Option<Sub>>, w: Sub) {
    open_sub.set(Some(w));
    spawn_local(async move {
        dom::sleep_ms(0).await;
        let first = root.get_untracked().and_then(|el| {
            let group = el.query_selector(&format!(".sub[data-sub='{}']", w.name())).ok().flatten()?;
            items(&group).into_iter().next()
        });
        if let Some(first) = first {
            focus(&first);
        }
    });
}

/// Whether a key went to a text field (a form in the menu's place).
fn typing(ev: &web_sys::KeyboardEvent) -> bool {
    ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA"))
}

/// One line of the menu.
fn entry_view(e: Entry, ctx: &Ctx) -> AnyView {
    let target = ctx.target;
    let close = move || close_menu(target);
    match e {
        Entry::Rule => view! { <div class="rule" role="separator"></div> }.into_any(),
        Entry::Chat(c) => chat::item(c, ctx.open_sub, target),
        Entry::Open(id) => {
            view! { <a class="item" role="menuitem" data-act="open" href=format!("#/ticket/{id}") on:click=move |_| close()>"Open"</a> }.into_any()
        }
        Entry::OpenNew(id) => view! {
            <a class="item" role="menuitem" data-act="open-new" href=format!("#/ticket/{id}") target="_blank" rel="noopener" on:click=move |_| close()>
                "Open in new tab"
            </a>
        }
        .into_any(),
        // MENGER-31: the parent's wave board (none for a root) and the board
        // whose final state this ticket is (none for a doc).
        Entry::Wave(w) => view! {
            <a class="item" role="menuitem" href=w.href data-wave=w.kind on:click=move |_| close()>{w.words}</a>
        }
        .into_any(),
        // WAVE-5: a shortcode's row offers the shortcode's wave board.
        Entry::WaveBoard(sc) => view! {
            <a class="item" role="menuitem" href=format!("#/waves/{sc}") data-wave-board=sc.clone() on:click=move |_| close()>
                "Wave board"
            </a>
        }
        .into_any(),
        Entry::Copy { text, words } => {
            let toaster = ctx.toaster;
            let copy = move |_| {
                let text = text.clone();
                close();
                spawn_local(async move {
                    let shown = if text.contains('\n') { format!("{} codes", text.lines().count()) } else { text.clone() };
                    let t = match copy_text(&text).await {
                        Copied::Api | Copied::Legacy => Toast { text: format!("copied {shown}"), manual: None, undo: None },
                        Copied::Manual => Toast { text: "clipboard unavailable — copy it from here:".into(), manual: Some(text), undo: None },
                    };
                    show_toast(toaster.toast, toaster.gen, t);
                });
            };
            view! { <button class="item" role="menuitem" data-act="copy" on:click=copy>{words}</button> }.into_any()
        }
        Entry::Sub(sub) => {
            let open_sub = ctx.open_sub;
            let ctx = ctx.clone();
            view! {
                <button
                    class="item"
                    role="menuitem"
                    aria-haspopup="menu"
                    data-sub=sub.name()
                    aria-expanded=move || (open_sub.get() == Some(sub)).to_string()
                    on:click=move |_| open_sub.update(|o| *o = if *o == Some(sub) { None } else { Some(sub) })
                >
                    {sub.words()}" "
                    <span class="arrow" aria-hidden="true">{move || if open_sub.get() == Some(sub) { "▾" } else { "▸" }}</span>
                </button>
                {move || {
                    (open_sub.get() == Some(sub)).then(|| {
                        let inner = match (sub, ctx.nav.clone()) {
                            (Sub::Nav, Some(at)) => nav_items(at, ctx.target),
                            (Sub::Nav, None) => ().into_any(),
                            _ => {
                                let ctx = ctx.clone();
                                (move || options_view(sub, &ctx)).into_any()
                            }
                        };
                        view! { <div class="sub" role="group" data-sub=sub.name()>{inner}</div> }
                    })
                }}
            }
            .into_any()
        }
        // ACT-10: a value chip's chooser, flat — the submenu's items without
        // its head or indent.
        Entry::Choice(sub) => {
            let ctx = ctx.clone();
            view! {
                <div class="flat" role="group" data-choice=sub.name() aria-label=sub.words()>
                    {move || options_view(sub, &ctx)}
                </div>
            }
            .into_any()
        }
        Entry::Form(f) => {
            let form = ctx.form;
            view! { <button class="item" role="menuitem" data-form=f.name() on:click=move |_| form.set(Some(f))>{f.words()}</button> }.into_any()
        }
        Entry::Filter(it) => {
            let lens = lens::use_lens();
            let level = ctx.level;
            let text = it.clause.group().map(|g| g.format());
            let tags::Item { act, clause, words } = it;
            view! {
                <button
                    class="item"
                    role="menuitem"
                    data-act=act.name()
                    data-filter=text.clone()
                    title=text
                    on:click=move |_| {
                        close();
                        tags::run(lens, level, act, &clause);
                    }
                >
                    {words}
                </button>
            }
            .into_any()
        }
        Entry::Delete(n) => {
            let (ids, rows, confirm) = (ctx.ids.clone(), ctx.rows, ctx.confirm);
            view! {
                <button
                    class="item danger"
                    role="menuitem"
                    data-act="delete"
                    on:click=move |_| {
                        let plan = rows.with_untracked(|rows| model::DeletePlan::of(&ids, rows));
                        close();
                        confirm.set(Some(plan));
                    }
                >
                    {model::delete_words(n)}
                </button>
            }
            .into_any()
        }
    }
}

/// A submenu's items ([`model::options`]), live: each marked with the value
/// one ticket holds or how many of a selection hold it; choosing one is one
/// write. Labels ▸ ends with Add a label….
fn options_view(sub: Sub, ctx: &Ctx) -> AnyView {
    let opts = ctx.rows.with(|rows| model::options(sub, &ctx.ids, rows));
    let radio = matches!(sub, Sub::Status | Sub::Kind | Sub::Severity);
    let one = ctx.ids.len() == 1;
    let (target, last_error, form) = (ctx.target, ctx.last_error, ctx.form);
    let items = opts
        .into_iter()
        .map(|o| {
            let ids = ctx.ids.clone();
            let model::Opt { value, words, mark, change } = o;
            let face = match sub {
                Sub::Status => view! { <span class=st_class(&value)>{words}</span> }.into_any(),
                Sub::Labels => view! { <span class=crate::labels::chip_class(&value)>{words}</span> }.into_any(),
                Sub::Unblock => view! { <span class="code">{words}</span> }.into_any(),
                _ => view! { <span>{words}</span> }.into_any(),
            };
            let checked = (mark != Mark::None).to_string();
            let role = if radio { "menuitemradio" } else { "menuitem" };
            // One ticket's own value is no change.
            let noop = radio && one && mark == Mark::Current;
            view! {
                <button
                    class="item"
                    role=role
                    aria-checked=radio.then(|| checked.clone())
                    data-value=value.clone()
                    data-mark=mark.text()
                    on:click=move |_| {
                        close_menu(target);
                        if !noop {
                            if let Some(c) = model::call(&ids, &change) {
                                write::send(c, change.clone(), last_error);
                            }
                        }
                    }
                >
                    <span class="mark" aria-hidden="true">{mark.text()}</span>
                    {face}
                </button>
            }
            .into_any()
        })
        .collect_view();
    let add = (sub == Sub::Labels).then(|| {
        view! {
            <button class="item" role="menuitem" data-form=Form::AddLabel.name() on:click=move |_| form.set(Some(Form::AddLabel))>
                <span class="mark" aria-hidden="true">"+"</span>
                {Form::AddLabel.words()}
            </button>
        }
    });
    view! { {items}{add} }.into_any()
}

