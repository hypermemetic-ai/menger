//! FILTER-55: the builder's dropdowns — every press that opened the big
//! picker opens one, under the control pressed (a phone: a sheet): a rule's
//! **field** (searched; Fields, Dates, Flags, Saved filters), its
//! **operator** (is / is not), its **values** (ticked, each counted, the
//! rule following at once; a label typed). A part being added is a ghost row
//! until its first choice.

use super::*;

/// What a press inside keeps the dropdown open for: itself, its scrim, and
/// every control that opens one (the press moves it there).
pub(super) const INSIDE: &str = ".fc-dd, .fc-dds, [data-fc=field], [data-fc=value], [data-fc=neg], [data-fc=add-rule], [data-fc=add-group], [data-fc=add-condition], [data-fc=add-subgroup], [data-fc=ghost-field]";

fn title(w: What) -> &'static str {
    match w {
        What::Field => "Choose a field",
        What::Op => "Operator",
        What::Values(c) => field_of(c).0,
    }
}

/// The part the dropdown changes, as it reads now (`None` for a ghost).
fn current(card: Card, d: &Dd) -> Option<Node> {
    (d.target == Target::At).then(|| card.owner_node(&d.owner).and_then(|t| parts::get(&t, &d.path).cloned())).flatten()
}

fn count_view(card: Card, wire: Option<String>) -> impl IntoView {
    move || {
        let n = wire.clone().and_then(|w| card.count(w));
        let z = n == Some(0);
        view! { <span class="fc-pkn" class:z=z>"\u{2192} "{n.map_or_else(|| "\u{2026}".to_string(), |n| n.to_string())}{z.then_some(" \u{00b7} nothing")}</span> }
    }
}

/// The dropdown open, if one is.
pub(super) fn view(card: Card, s: Subject) -> impl IntoView {
    let key = Memo::new(move |_| card.dd.with(|d| d.as_ref().map(|d| (d.what, d.target.clone(), d.owner.clone(), d.path.clone()))));
    move || {
        key.get()?;
        let d = card.dd.get_untracked()?;
        let phone = card.lens.phone_untracked();
        let style = move || {
            card.dd.with(|o| {
                o.as_ref().map(|o| {
                    let (l, t, b, w) = o.at;
                    format!("{};min-width:{:.0}px", tm_place((l, t, b), phone), w.max(220.0))
                })
            })
        };
        let kind = match d.what {
            What::Field => "field",
            What::Op => "op",
            What::Values(_) => "values",
        };
        let body = match d.what {
            What::Field => fields(card, s.clone(), d.clone()).into_any(),
            What::Op => ops(card, s.clone(), d.clone()).into_any(),
            What::Values(c) => values_view(card, s.clone(), c).into_any(),
        };
        let head = phone.then(|| {
            view! {
                <div class="fc-ddh">
                    <b>{title(d.what)}</b>
                    <button type="button" class="fc-x" data-ddclose="" aria-label="close" on:click=move |_| card.dd.set(None)>"\u{d7}"</button>
                </div>
            }
        });
        Some(view! {
            {phone.then(|| view! { <div class="fc-dds" on:click=move |_| card.dd.set(None)></div> })}
            <div class="fc-dd" class:sheet=phone role="menu" aria-label=title(d.what) data-dd=kind style=style>
                {head}
                {body}
            </div>
        })
    }
}

fn search(card: Card, placeholder: &'static str) -> impl IntoView {
    let field = NodeRef::<leptos::html::Input>::new();
    let phone = card.lens.phone_untracked();
    Effect::new(move |_| {
        if let Some(el) = field.get() {
            if !phone {
                let _ = el.focus();
            }
        }
    });
    view! {
        <div class="fc-ddq">
            <input node_ref=field type="text" autocomplete="off" spellcheck="false" placeholder=placeholder aria-label=placeholder prop:value=move || card.dq.get() on:input=move |ev| card.dq.set(event_target_value(&ev)) />
        </div>
    }
}

fn hits(q: &str, w: &str) -> bool {
    q.is_empty() || w.to_lowercase().contains(q)
}

/// A rule's field: the listed fields open their values; a date, a flag or a
/// saved filter is the whole part, put in at once.
fn fields(card: Card, s: Subject, d: Dd) -> impl IntoView {
    let now = current(card, &d);
    let field_now = now.as_ref().map(|n| match n {
        Node::Leaf(Predicate { field: Field::Saved(_), .. }) => "Saved filter".to_string(),
        n => parts::rule_at(n).map(|r| r.field).unwrap_or_default(),
    });
    let saved_now = match &now {
        Some(Node::Leaf(Predicate { field: Field::Saved(n), .. })) => Some(n.clone()),
        _ => None,
    };
    let list = move || {
        let q = card.dq.get().trim().to_lowercase();
        let would = |text: &str| untrack(|| card.placed(&d, text).map(|(t, _)| card.would_tree(&s, &d.owner, &t)));
        let mut any = false;
        let listed: Vec<_> = LISTED
            .into_iter()
            .filter(|c| hits(&q, field_of(*c).0))
            .map(|c| {
                let (f, _) = field_of(c);
                let on = field_now.as_deref() == Some(f) || (c == PCat::Shortcode && field_now.as_deref() == Some("Under")) || (c == PCat::Status && field_now.as_deref() == Some("Category"));
                view! {
                    <button type="button" role="menuitemradio" aria-checked=on.to_string() data-ddf=f on:click=move |_| {
                        card.dq.set(String::new());
                        card.dd.update(|o| {
                            if let Some(o) = o {
                                o.what = What::Values(c);
                            }
                        });
                    }>{f}<span class="fc-pkn">"\u{203a}"</span></button>
                }
            })
            .collect();
        let dates: Vec<_> = values(PCat::Dates, &[], &[])
            .into_iter()
            .filter(|(w, _)| hits(&q, w))
            .map(|(w, t)| {
                let on = field_now.as_deref() == Some(w.as_str());
                let (d2, t2) = (d.clone(), t.clone());
                let n = count_view(card, would(&t));
                view! { <button type="button" role="menuitemradio" aria-checked=on.to_string() data-ddf=w.clone() on:click=move |_| card.choose(&d2, &t2)>"\u{25f7} "{w.clone()}{n}</button> }
            })
            .collect();
        let flags: Vec<_> = FLAGS
            .iter()
            .filter(|(w, _)| hits(&q, w))
            .map(|(w, t)| {
                let on = now.as_ref().is_some_and(|n| matches!(n, Node::Leaf(p) if p.format().trim_start_matches("not ") == *t));
                let d2 = d.clone();
                let t2 = t.to_string();
                let n = count_view(card, would(t));
                view! { <button type="button" role="menuitemradio" aria-checked=on.to_string() data-ddf=*w on:click=move |_| card.choose(&d2, &t2)>"\u{2691} "{*w}{n}</button> }
            })
            .collect();
        let names: Vec<String> = saved::hook().map(|h| h.listed().into_iter().map(|e| e.name).collect()).unwrap_or_default();
        let named: Vec<_> = names
            .into_iter()
            .filter(|n| hits(&q, n))
            .map(|name| {
                if slug(&name) == slug(&d.owner) {
                    return view! { <div class="fc-ddno">"\u{2317} "{name}" \u{2014} would name itself"</div> }.into_any();
                }
                let text = crate::naming::saved_text_of(&name);
                let on = saved_now.as_deref().is_some_and(|s| slug(s) == slug(&name));
                let n = count_view(card, would(&text));
                let d2 = d.clone();
                view! { <button type="button" role="menuitemradio" aria-checked=on.to_string() data-dds=name.clone() on:click=move |_| card.choose(&d2, &text)>"\u{2317} "{name.clone()}{n}</button> }.into_any()
            })
            .collect();
        any |= !listed.is_empty() || !dates.is_empty() || !flags.is_empty() || !named.is_empty();
        let sec = |h: &'static str, rows: Vec<AnyView>| (!rows.is_empty()).then(|| view! { <div class="fc-ddm">{h}</div>{rows} });
        view! {
            {sec("Fields", listed.into_iter().map(IntoAny::into_any).collect())}
            {sec("Dates", dates.into_iter().map(IntoAny::into_any).collect())}
            {sec("Flags", flags.into_iter().map(IntoAny::into_any).collect())}
            {sec("Saved filters", named)}
            {(!any).then(|| view! { <div class="fc-ddno">{format!("No field is called \u{201c}{q}\u{201d}")}</div> })}
        }
    };
    view! {
        {search(card, "Find a field or a saved filter")}
        <div class="fc-ddl">{list}</div>
    }
}

/// A rule's operator: is or is not (a saved filter: matches or does not).
fn ops(card: Card, s: Subject, d: Dd) -> impl IntoView {
    let now = current(card, &d);
    let named = matches!(now, Some(Node::Leaf(Predicate { field: Field::Saved(_), .. })));
    let not_now = now.as_ref().map_or(d.not, |n| match n {
        Node::Leaf(p) => p.not,
        g => parts::list_of(g).is_some_and(|(_, n, _)| n),
    });
    let words = if named { ["matches", "does not match"] } else { ["is", "is not"] };
    let rows = words
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            let not = i == 1;
            let wire = untrack(|| {
                let mut t = card.owner_node(&d.owner)?;
                if d.target != Target::At {
                    return None;
                }
                if not != not_now && !parts::negate(&mut t, &d.path) {
                    return None;
                }
                Some(card.would_tree(&s, &d.owner, &t))
            });
            let d2 = d.clone();
            let n = (d.target == Target::At).then(|| count_view(card, wire));
            view! { <button type="button" role="menuitemradio" aria-checked=(not == not_now).to_string() data-ddop=if not { "1" } else { "0" } on:click=move |_| card.set_not(&d2, not)>{w}{n}</button> }
        })
        .collect_view();
    view! { <div class="fc-ddl">{rows}</div> }
}

/// A listed field's values, ticked: the rule follows each tick at once.
fn values_view(card: Card, s: Subject, c: PCat) -> impl IntoView {
    let codes: Vec<String> = card.groups.with_untracked(|g| g.as_ref().map(|g| g.iter().map(|g| g.epic.to_string()).collect()).unwrap_or_default());
    let all = raw_values(c, &codes);
    let long = all.len() > 6;
    let rule = move || card.dd.with(|o| o.clone()).and_then(|d| current(card, &d)).and_then(|n| parts::rule_at(&n));
    let of_field = move |r: &parts::Rule| r.field == field_of(c).0 || (c == PCat::Shortcode && r.field == "Under");
    let picked = move || rule().filter(|r| of_field(r)).map(|r| r.values).unwrap_or_default();
    let list = {
        let s = s.clone();
        move || {
            let q = card.dq.get().trim().to_lowercase();
            let Some(d) = card.dd.get() else { return ().into_any() };
            let have = picked();
            let mut shown: Vec<String> = all.clone();
            if c == PCat::Label {
                shown = have.clone();
            }
            let rows: Vec<_> = shown
                .into_iter()
                .filter(|v| hits(&q, v))
                .map(|v| {
                    let on = have.iter().any(|x| x.eq_ignore_ascii_case(&v));
                    let wire = untrack(|| card.ticked_tree(&d, c, &v).map(|(t, _)| card.would_tree(&s, &d.owner, &t)));
                    let n = count_view(card, wire);
                    let (d2, v2) = (d.clone(), v.clone());
                    view! {
                        <button type="button" role="menuitemcheckbox" aria-checked=on.to_string() data-ddv=v.clone() on:click=move |_| card.tick(&d2, c, &v2)>
                            <span class="fc-ck">{on.then_some("\u{2713}")}</span>
                            {v.clone()}
                            {n}
                        </button>
                    }
                })
                .collect();
            if rows.is_empty() && c != PCat::Label {
                return view! { <div class="fc-ddno">{format!("Nothing called \u{201c}{q}\u{201d}")}</div> }.into_any();
            }
            rows.collect_view().into_any()
        }
    };
    let label = (c == PCat::Label).then(|| {
        let add = move |raw: String| {
            let v = raw.trim().to_lowercase();
            if label_clause(&v).is_none() {
                return;
            }
            if let Some(d) = card.dd.get_untracked() {
                card.label.set_value(String::new());
                card.tick(&d, PCat::Label, &v);
            }
        };
        let field = NodeRef::<leptos::html::Input>::new();
        Effect::new(move |_| {
            if let Some(el) = field.get() {
                let _ = el.focus();
            }
        });
        view! {
            <div class="fc-ddq fc-lbl">
                <input node_ref=field type="text" autocomplete="off" spellcheck="false" placeholder="a label, e.g. needs-you" aria-label="a label" on:input=move |ev| card.label.set_value(event_target_value(&ev)) on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if bindings::hit(&ev, Context::Field) == Some("form.submit") {
                        ev.prevent_default();
                        add(event_target_value(&ev));
                        if let Some(el) = field.get_untracked() {
                            el.set_value("");
                        }
                    }
                } />
                <button type="button" class="fc-bt sm" data-fc="label-add" on:click=move |_| {
                    add(card.label.get_value());
                    if let Some(el) = field.get_untracked() {
                        el.set_value("");
                    }
                }>"Add"</button>
            </div>
        }
    });
    let n = move || picked().len();
    view! {
        {(long && c != PCat::Label).then(|| search(card, "Find a value"))}
        {label}
        <div class="fc-ddl">{list}</div>
        <div class="fc-ddf">
            <span>{move || format!("{} picked", n())}</span>
            <button type="button" class="fc-bt pri sm" data-ddclose="" on:click=move |_| card.dd.set(None)>"Done"</button>
        </div>
    }
}
