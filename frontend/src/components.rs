//! COMP-6: `#/components` — every component in the registry (COMP-3), its
//! owner or "orphan", what references it, and its source.
//!
//! One row per component, in name order: its tag (a link to
//! `#/components/<tag>`), its owner — the ticket whose body defines it, a
//! link — or **orphan** (no ticket defines it any more; it is kept, read-only,
//! while something references it), its reference count with each referrer
//! as a link (a ticket to its page, a component to its row), and its source,
//! collapsed, read-only. `#/components/<tag>` is that one component with its
//! source open.
//!
//! The rows are the app's copy of the registry (src/registry.rs), which the
//! page asks for ([`registry::want`]) and `watch` keeps —
//! `component_changed` moves a row in place, `component_removed` takes it
//! away — so the page is live with no fetch of its own. A row whose
//! component did not change keeps its DOM (an opened source stays open).
//!
//! COMP-9: each row shows the component's summary (its `<docs>`), and
//! `#/components/<tag>` is its **help page**: its docs card (src/card.rs,
//! from the registry's docs — examples and a preview live, so in a frame),
//! then what references it, then its source, open.
//!
//! What decides — the rows, a referrer's kind, the summary line — is pure
//! and pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::Component;

use crate::frame::Frame;
use crate::{card, md, registry};

/// Who references a component: a ticket (its body holds the tag) or
/// another component (its source does) — COMP-3's `references`, tickets
/// first.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Referrer {
    Ticket(String),
    Component(String),
}

impl Referrer {
    /// A component's name is lowercase and starts with a letter; a ticket's
    /// id starts with its upper-case shortcode.
    pub(crate) fn of(r: &str) -> Self {
        if r.starts_with(|c: char| c.is_ascii_lowercase()) {
            Referrer::Component(r.to_string())
        } else {
            Referrer::Ticket(r.to_string())
        }
    }
}

/// One component as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Row {
    pub(crate) name: String,
    /// COMP-9: its docs' summary — `None` without `<docs>`.
    pub(crate) summary: Option<String>,
    /// `None`: an orphan.
    pub(crate) owner: Option<String>,
    pub(crate) referrers: Vec<Referrer>,
    pub(crate) source: String,
    /// The source shown open (`#/components/<tag>`).
    pub(crate) open: bool,
}

/// The page's rows: every component in `list`'s order (the registry's,
/// by name) — or, given `only`, that one component, its source open (none
/// when the registry has no such name).
pub(crate) fn rows(list: &[Component], only: Option<&str>) -> Vec<Row> {
    list.iter()
        .filter(|c| only.is_none_or(|n| n == c.name))
        .map(|c| Row {
            name: c.name.clone(),
            summary: c.docs.as_ref().map(|d| d.summary.clone()),
            owner: c.owner.clone(),
            referrers: c.references.iter().map(|r| Referrer::of(r)).collect(),
            source: c.source.clone(),
            open: only.is_some(),
        })
        .collect()
}

/// "3 references" / "1 reference" / "no references".
pub(crate) fn count_label(n: usize) -> String {
    match n {
        0 => "no references".into(),
        1 => "1 reference".into(),
        n => format!("{n} references"),
    }
}

/// The source's fold: "source · 18 lines".
pub(crate) fn source_label(source: &str) -> String {
    let lines = source.trim_end_matches('\n').lines().count();
    format!("source · {lines} line{}", if lines == 1 { "" } else { "s" })
}

fn referrer_view(r: Referrer) -> impl IntoView {
    match r {
        Referrer::Ticket(id) => {
            let href = format!("#/ticket/{id}");
            view! { <a class="code ref" href=href title="a ticket whose body uses it">{id}</a> }.into_any()
        }
        Referrer::Component(name) => {
            let href = format!("#/components/{name}");
            view! { <a class="code ref comp" href=href title="a component whose source uses it">{format!("<{name}>")}</a> }.into_any()
        }
    }
}

/// COMP-9: the help page's card — the registry's docs, preview and source,
/// framed; no references link and no source in it (the page shows both).
pub(crate) fn help_card(c: &Component) -> String {
    card::card_html(
        &card::Card { name: &c.name, preview: &c.preview, docs: Ok(c.docs.as_ref()), source: &c.source, in_body: false },
        &md::render_help,
    )
}

fn row_view(row: Row) -> impl IntoView {
    let Row { name, summary, owner, referrers, source, open } = row;
    let summary = match summary {
        Some(s) if !s.is_empty() => view! { <div class="c-summary">{s}</div> }.into_any(),
        Some(_) => ().into_any(),
        None => view! { <div class="c-summary none">"no docs yet"</div> }.into_any(),
    };
    let href = format!("#/components/{name}");
    let owner_view = match owner.clone() {
        Some(id) => {
            let href = format!("#/ticket/{id}");
            view! { <a class="code owner" href=href title="the ticket whose body defines it">{id}</a> }.into_any()
        }
        None => view! {
            <span class="orphan" title="no ticket defines it any more — kept, read-only, while something references it">"orphan"</span>
        }
        .into_any(),
    };
    let count = count_label(referrers.len());
    let refs = referrers.into_iter().map(referrer_view).collect_view();
    let fold = source_label(&source);
    view! {
        <tr class="comp" class:orphaned=owner.is_none() data-component=name.clone()>
            <td class="c-name"><a class="tag" href=href>{format!("<{name}>")}</a>{summary}</td>
            <td class="c-owner">{owner_view}</td>
            <td class="c-refs"><span class="count">{count}</span>" "{refs}</td>
        </tr>
        <tr class="src" data-component=name>
            <td colspan="3">
                <details open=open>
                    <summary>{fold}</summary>
                    <pre><code>{source}</code></pre>
                </details>
            </td>
        </tr>
    }
}

/// `#/components` (`only: None`) and `#/components/<tag>`.
#[component]
pub(crate) fn ComponentsPage(only: Option<String>) -> impl IntoView {
    registry::want();
    let reg = registry::signal();
    let one = only.clone();
    let reg_card = reg.clone();
    let rows = Memo::new(move |_| reg.with(|r| r.as_ref().map(|r| rows(&r.list(), one.as_deref()))));
    let title = match &only {
        Some(name) => format!("<{name}>"),
        None => "Components".to_string(),
    };
    let lede = match &only {
        Some(_) => view! { <p class="crumbs"><a href="#/components">"← every component"</a></p> }.into_any(),
        None => view! {
            <p class="none">
                "Every component a ticket defines, or once defined — a "
                <code>"```component m-name"</code>
                " block in its body — usable as a tag in any ticket."
            </p>
        }
        .into_any(),
    };
    let missing = only.clone();
    // COMP-9: the help page's docs card, framed — live examples and preview.
    let card_name = only.clone();
    let card = Memo::new(move |_| {
        let name = card_name.clone()?;
        reg_card.with(|r| r.as_ref().and_then(|r| r.list().into_iter().find(|c| c.name == name)).map(|c| help_card(&c)))
    });
    let card_view = move || {
        card.get().map(|_| {
            let html = Signal::derive(move || card.get().unwrap_or_default());
            view! { <div class="help-card"><Frame html ticket=None /></div> }
        })
    };
    view! {
        <section id="components" data-only=only.clone()>
            <h2>{title}</h2>
            {lede}
            {card_view}
            {move || match rows.get() {
                None => view! { <p class="loading">"loading the component registry…"</p> }.into_any(),
                Some(list) if list.is_empty() => match missing.clone() {
                    Some(name) => view! { <p class="none missing">{format!("<{name}> is not in the registry.")}</p> }.into_any(),
                    None => view! { <p class="none empty">"No components yet."</p> }.into_any(),
                },
                Some(_) => view! {
                    <table class="comps">
                        <thead>
                            <tr><th class="c-name">"component"</th><th class="c-owner">"owner"</th><th>"references"</th></tr>
                        </thead>
                        <tbody>
                            <For
                                each=move || rows.get().unwrap_or_default()
                                key=|row| row.clone()
                                children=row_view
                            />
                        </tbody>
                    </table>
                }
                .into_any(),
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(name: &str, source: &str, owner: Option<&str>, references: &[&str]) -> Component {
        Component {
            name: name.into(),
            source: source.into(),
            owner: owner.map(str::to_string),
            references: references.iter().map(|r| r.to_string()).collect(),
            docs: None,
            preview: Vec::new(),
        }
    }

    /// The registry's two cases: an owned component and an orphan (with docs).
    fn fixture() -> Vec<Component> {
        let mut orphan = component(
            "m-wave-summary",
            "<template>\n  <span class=\"n\"></span>\n</template>\n<script>\n  this.root;\n</script>\n",
            None,
            &["DEMO-6", "PLEXUS-2", "m-board-card"],
        );
        orphan.docs = Some(menger_tickets_api::ComponentDocs { summary: "Wave 1, counted.".into(), ..Default::default() });
        vec![
            component("m-badge", "<template><b><slot></slot></b></template>\n", Some("DEMO-7"), &[]),
            orphan,
        ]
    }

    /// COMP-6 acceptance 1: one row per component in the registry's order —
    /// the owned one with its owner, the orphan with none — each referrer a
    /// ticket or a component, the source whole and folded.
    #[test]
    fn rows_from_a_registry_fixture() {
        let rows = rows(&fixture(), None);
        assert_eq!(
            rows,
            vec![
                Row {
                    name: "m-badge".into(),
                    summary: None,
                    owner: Some("DEMO-7".into()),
                    referrers: vec![],
                    source: "<template><b><slot></slot></b></template>\n".into(),
                    open: false,
                },
                Row {
                    name: "m-wave-summary".into(),
                    summary: Some("Wave 1, counted.".into()),
                    owner: None,
                    referrers: vec![
                        Referrer::Ticket("DEMO-6".into()),
                        Referrer::Ticket("PLEXUS-2".into()),
                        Referrer::Component("m-board-card".into()),
                    ],
                    source: "<template>\n  <span class=\"n\"></span>\n</template>\n<script>\n  this.root;\n</script>\n".into(),
                    open: false,
                },
            ]
        );
    }

    /// `#/components/<tag>`: that one, its source open; a name not in the
    /// registry is no row.
    #[test]
    fn one_component_opens_with_its_source() {
        let one = rows(&fixture(), Some("m-wave-summary"));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "m-wave-summary");
        assert!(one[0].open && one[0].owner.is_none());
        assert!(rows(&fixture(), Some("m-gone")).is_empty());
        assert!(rows(&[], None).is_empty());
    }

    /// COMP-9: the help page's card is the docs card without the
    /// references link and the source (the page shows both).
    #[test]
    fn the_help_pages_card() {
        let html = help_card(&fixture()[1]);
        assert!(html.contains("data-menger-card=\"m-wave-summary\"") && html.contains("Wave 1, counted."), "{html}");
        assert!(!html.contains("mc-refs") && !html.contains("mc-source"), "{html}");
        assert!(help_card(&fixture()[0]).contains("no docs yet"));
    }

    #[test]
    fn the_labels_count_references_and_source_lines() {
        assert_eq!(count_label(0), "no references");
        assert_eq!(count_label(1), "1 reference");
        assert_eq!(count_label(3), "3 references");
        assert_eq!(source_label("<b>x</b>"), "source · 1 line");
        assert_eq!(source_label("<template>\n</template>\n<style></style>\n"), "source · 3 lines");
        assert_eq!(source_label(""), "source · 0 lines");
        assert_eq!(Referrer::of("SDLC-11"), Referrer::Ticket("SDLC-11".into()));
        assert_eq!(Referrer::of("m-x"), Referrer::Component("m-x".into()));
    }
}
