//! COMP-3: the component registry — a test per row of the ticket's table
//! (registered, source replaced, a second owner refused, orphaned, removed,
//! adopted, references gained and lost, a source's references keeping a
//! component alive, the last reference of an orphan), the code examples that
//! are not definitions and the code span that is not a reference, the
//! events and their place after a write's others, durability, and a
//! database from before the table.

use super::*;
use std::collections::BTreeMap;

/// A ` ```component ` block — its source is `source` and the newline after.
fn def(name: &str, source: &str) -> String {
    format!("```component {name}\n{source}\n```\n")
}

fn src(source: &str) -> String {
    format!("{source}\n")
}

fn ticket(epic: &str, kind: Kind, body: &str) -> NewTicket {
    NewTicket {
        epic: epic.into(),
        title: format!("a {} in {epic}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: body.into(),
    }
}

/// A store with a scope root, `C-1`, to file builds under.
async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, ticket("C", Kind::Scope, "the root")).await.unwrap();
    store
}

async fn born(store: &Store, body: &str) -> String {
    String::from(store.create(SYS, ticket("C", Kind::Build, body)).await.unwrap().id)
}

async fn amend(store: &Store, id: &str, body: &str) {
    store.amend(SYS.permit(id), None, Some(body.into())).await.unwrap();
}

fn registry(store: &Store) -> BTreeMap<String, Component> {
    store.components(SYS).into_iter().map(|c| (c.name.clone(), c)).collect()
}

fn component(name: &str, source: &str, owner: Option<&str>, references: &[&str]) -> Component {
    Component {
        name: name.into(),
        source: source.into(),
        owner: owner.map(Into::into),
        references: references.iter().map(|r| r.to_string()).collect(),
        docs: None,
        preview: Vec::new(),
    }
}

/// Everything on the channel now.
fn drain(rx: &mut broadcast::Receiver<Change>) -> Vec<Change> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// The component events of a write, as (kind, name).
fn heard(changes: &[Change]) -> Vec<(&'static str, String)> {
    changes
        .iter()
        .filter_map(|c| match c {
            Change::ComponentChanged { component } => Some(("changed", component.name.clone())),
            Change::ComponentRemoved { name } => Some(("removed", name.clone())),
            _ => None,
        })
        .collect()
}

/// Row 1: a body gains a block — the name registered, owned by the ticket,
/// its source the block's content; `component_changed` after every other
/// event of the birth.
#[tokio::test]
async fn a_body_gaining_a_block_registers_the_component() {
    let store = store().await;
    let mut rx = store.subscribe();
    let body = format!("Defines one.\n\n{}\nprose after", def("m-x", "<template><b>x</b></template>"));
    let id = born(&store, &body).await;
    let want = component("m-x", &src("<template><b>x</b></template>"), Some(&id), &[]);
    assert_eq!(store.components(SYS), vec![want.clone()]);
    let changes = drain(&mut rx);
    assert!(matches!(changes.first(), Some(Change::Created { .. })), "{changes:?}");
    match changes.last() {
        Some(Change::ComponentChanged { component }) => assert_eq!(component, &want),
        other => panic!("the component's change is the write's last event: {other:?}"),
    }
    assert_eq!(heard(&changes), [("changed", "m-x".to_string())]);
    assert_eq!(store.get(SYS, &id).await.unwrap().body, body, "the body is stored exactly as written");

    // An amend gaining one too.
    let other = born(&store, "nothing yet").await;
    amend(&store, &other, &def("m-y", "<template>y</template>")).await;
    assert_eq!(registry(&store)["m-y"].owner.as_deref(), Some(other.as_str()));
}

/// Row 2: the owner amends the block — the source replaced, once heard.
#[tokio::test]
async fn the_owner_amending_the_block_replaces_the_source() {
    let store = store().await;
    let id = born(&store, &def("m-x", "<template>one</template>")).await;
    let mut rx = store.subscribe();
    amend(&store, &id, &format!("new words\n\n{}", def("m-x", "<template>two</template>"))).await;
    assert_eq!(store.components(SYS), vec![component("m-x", &src("<template>two</template>"), Some(&id), &[])]);
    let changes = drain(&mut rx);
    assert!(matches!(changes[0], Change::Amended { .. }));
    assert_eq!(heard(&changes), [("changed", "m-x".to_string())]);

    // Words around an unchanged block move no component: no event.
    amend(&store, &id, &format!("other words\n\n{}", def("m-x", "<template>two</template>"))).await;
    let changes = drain(&mut rx);
    assert_eq!(changes.len(), 1, "the amend alone: {changes:?}");
}

/// Row 3: another ticket defining an owned name — the write's in-band
/// error names the owner, and nothing changes: no ticket born (no id
/// spent), no body amended (title included), the registry as it was,
/// nothing heard.
#[tokio::test]
async fn another_ticket_defining_an_owned_name_is_refused_naming_the_owner() {
    let store = store().await;
    let owner = born(&store, &def("m-x", "<template>mine</template>")).await; // C-2
    let other = born(&store, "plain").await; // C-3
    let before = store.components(SYS);
    let mut rx = store.subscribe();

    let e = store.create(SYS, ticket("C", Kind::Build, &def("m-x", "<template>theirs</template>"))).await.unwrap_err();
    assert!(matches!(&e, StoreError::ComponentOwned { id: None, name, owner: o } if name == "m-x" && *o == owner), "{e:?}");
    let message = e.to_string();
    assert!(message.contains(&owner) && message.contains("m-x") && message.contains("nothing was created"), "{message}");

    let e = store
        .amend(SYS.permit(&other), Some("a new title".into()), Some(def("m-x", "<template>theirs</template>")))
        .await
        .unwrap_err();
    assert!(e.to_string().contains(&owner), "{e}");
    assert!(e.to_string().contains(&format!("nothing changed for '{other}'")), "{e}");

    assert_eq!(store.components(SYS), before);
    assert!(drain(&mut rx).is_empty(), "nothing heard");
    let t = store.get(SYS, &other).await.unwrap();
    assert_eq!((t.title.as_str(), t.body.as_str()), ("a build in C", "plain"), "title and body as they were");
    assert_eq!(born(&store, "next").await, "C-4", "no id was spent");
}

/// Row 4: the owner's body loses the block while a reference remains — an
/// orphan: owner `None`, source kept, references kept.
#[tokio::test]
async fn losing_the_block_with_references_left_orphans_it() {
    let store = store().await;
    let owner = born(&store, &def("m-x", "<template>x</template>")).await;
    let user = born(&store, "uses <m-x></m-x>").await;
    let mut rx = store.subscribe();
    amend(&store, &owner, "the block is gone").await;
    assert_eq!(store.components(SYS), vec![component("m-x", &src("<template>x</template>"), None, &[&user])]);
    let changes = drain(&mut rx);
    assert_eq!(heard(&changes), [("changed", "m-x".to_string())]);
}

/// Row 5: …and with no reference left, the component goes.
#[tokio::test]
async fn losing_the_block_with_no_references_removes_it() {
    let store = store().await;
    let owner = born(&store, &def("m-x", "<template>x</template>")).await;
    let mut rx = store.subscribe();
    amend(&store, &owner, "gone").await;
    assert!(store.components(SYS).is_empty());
    assert_eq!(heard(&drain(&mut rx)), [("removed", "m-x".to_string())]);
}

/// Row 6: a ticket defining an orphan's name adopts it — owner that ticket,
/// source its block, the references it had kept.
#[tokio::test]
async fn defining_an_orphans_name_adopts_it() {
    let store = store().await;
    let first = born(&store, &def("m-x", "<template>old</template>")).await;
    let user = born(&store, "<m-x></m-x>").await;
    amend(&store, &first, "let it go").await;
    assert_eq!(registry(&store)["m-x"].owner, None);
    let mut rx = store.subscribe();
    let adopter = born(&store, &def("m-x", "<template>new</template>")).await;
    assert_eq!(
        store.components(SYS),
        vec![component("m-x", &src("<template>new</template>"), Some(&adopter), &[&user])]
    );
    assert_eq!(heard(&drain(&mut rx)), [("changed", "m-x".to_string())]);
    // And now it is the adopter's: the first owner may not take it back.
    let e = store.amend(SYS.permit(&first), None, Some(def("m-x", "<template>mine</template>"))).await.unwrap_err();
    assert!(e.to_string().contains(&adopter), "{e}");
}

/// Row 7: a body gaining or losing `<m-x` moves the references — a ticket
/// counts once however often it writes the tag; inline HTML and an HTML
/// block alike; a reference made before the definition counts from its
/// birth.
#[tokio::test]
async fn a_body_gaining_or_losing_a_tag_moves_the_references() {
    let store = store().await;
    let early = born(&store, "<div>\n<m-x></m-x>\n</div>\n").await; // before m-x exists
    let owner = born(&store, &def("m-x", "<template>x</template>")).await;
    let twice = born(&store, "one <m-x></m-x> and two <M-X board=PLX/>").await;
    assert_eq!(registry(&store)["m-x"].references, [early.as_str(), twice.as_str()], "early counted; twice once");

    let mut rx = store.subscribe();
    amend(&store, &early, "no longer").await;
    assert_eq!(registry(&store)["m-x"].references, [twice.as_str()]);
    assert_eq!(heard(&drain(&mut rx)), [("changed", "m-x".to_string())]);
    amend(&store, &owner, &format!("{}\nand it uses itself <m-x></m-x>", def("m-x", "<template>x</template>"))).await;
    assert_eq!(registry(&store)["m-x"].references, [owner.as_str(), twice.as_str()], "the store's order");
    assert_eq!(heard(&drain(&mut rx)), [("changed", "m-x".to_string())]);
    // A reference to a name nobody defines is no component, and says nothing.
    amend(&store, &twice, "<m-x></m-x> <m-nobody></m-nobody>").await;
    assert!(drain(&mut rx).iter().all(|c| matches!(c, Change::Amended { .. })));
    assert_eq!(registry(&store).keys().collect::<Vec<_>>(), ["m-x"]);
}

/// Row 8: a component's source holding `<m-y` — `m-x` references `m-y`, and
/// keeps it alive: `m-y`'s owner lets it go, and it stays, an orphan, while
/// `m-x` uses it.
#[tokio::test]
async fn a_sources_tag_is_a_reference_that_keeps_its_component_alive() {
    let store = store().await;
    let y = born(&store, &def("m-y", "<template>y</template>")).await;
    let x = born(&store, &def("m-x", "<template><m-y></m-y></template>")).await;
    assert_eq!(registry(&store)["m-y"].references, ["m-x"]);
    amend(&store, &y, "let m-y go").await;
    assert_eq!(registry(&store)["m-y"], component("m-y", &src("<template>y</template>"), None, &["m-x"]));
    // The source changes, the reference goes — and the orphan with it.
    let mut rx = store.subscribe();
    amend(&store, &x, &def("m-x", "<template>plain</template>")).await;
    assert_eq!(registry(&store).keys().collect::<Vec<_>>(), ["m-x"]);
    assert_eq!(
        heard(&drain(&mut rx)),
        [("changed", "m-x".to_string()), ("removed", "m-y".to_string())],
        "changed, then removed — each in name order"
    );
}

/// Row 9: an orphan whose last reference goes is removed in the same write
/// — and an orphan only another orphan kept goes with it, as do orphans
/// that only keep each other.
#[tokio::test]
async fn an_orphans_last_reference_going_removes_it_in_the_same_write() {
    let store = store().await;
    let owner = born(&store, &format!("{}{}", def("m-a", "<template><m-b></m-b></template>"), def("m-b", "<template>b</template>"))).await;
    let user = born(&store, "<m-a></m-a>").await;
    amend(&store, &owner, "both blocks gone").await;
    let orphans = registry(&store);
    assert_eq!(orphans["m-a"], component("m-a", &src("<template><m-b></m-b></template>"), None, &[&user]));
    assert_eq!(orphans["m-b"], component("m-b", &src("<template>b</template>"), None, &["m-a"]));
    let mut rx = store.subscribe();
    amend(&store, &user, "no tags").await;
    assert!(store.components(SYS).is_empty(), "m-a went, and m-b, which only m-a kept");
    let changes = drain(&mut rx);
    assert!(matches!(changes[0], Change::Amended { .. }));
    assert_eq!(heard(&changes), [("removed", "m-a".to_string()), ("removed", "m-b".to_string())]);

    // Two orphans that only reference each other are not kept by it.
    let p = born(&store, &def("m-p", "<template><m-q></m-q></template>")).await;
    let q = born(&store, &def("m-q", "<template><m-p></m-p></template>")).await;
    amend(&store, &p, "p's block gone").await;
    assert_eq!(registry(&store)["m-p"].owner, None, "m-q, owned, keeps it");
    amend(&store, &q, "q's block gone").await;
    assert!(store.components(SYS).is_empty(), "a cycle of orphans keeps nothing alive");
}

/// A definition inside an indented code block, or a fence of another
/// language, is that example's text — not a definition, and its tags no
/// references.
#[tokio::test]
async fn a_definition_in_a_code_example_is_not_one() {
    let store = store().await;
    let fenced = format!("How to define one:\n\n````markdown\n{}````\n", def("m-x", "<template><m-y></m-y></template>"));
    let indented = "How to define one:\n\n    ```component m-x\n    <template><m-y></m-y></template>\n    ```\n";
    let a = born(&store, &fenced).await;
    let b = born(&store, indented).await;
    assert!(store.components(SYS).is_empty());
    // Nothing waits on m-y either: defined now, nothing references it.
    born(&store, &def("m-y", "<template>y</template>")).await;
    assert_eq!(registry(&store)["m-y"].references, Vec::<String>::new());
    // And neither ticket owns m-x: a third may define it.
    let c = born(&store, &def("m-x", "<template>x</template>")).await;
    assert_eq!(registry(&store)["m-x"].owner.as_deref(), Some(c.as_str()));
    let _ = (a, b);
}

/// A tag inside a code span is text, not a reference.
#[tokio::test]
async fn a_tag_in_a_code_span_is_not_a_reference() {
    let store = store().await;
    born(&store, &def("m-x", "<template>x</template>")).await;
    born(&store, "write `<m-x></m-x>` to use it").await;
    born(&store, "```html\n<m-x></m-x>\n```\n").await;
    born(&store, "<!-- <m-x></m-x> -->\n").await;
    assert_eq!(registry(&store)["m-x"].references, Vec::<String>::new());
}

/// A malformed block refuses the write that carries it — a create or a
/// body change — in-band, and nothing changes; a title-only amend, and the
/// same body again, are not body changes.
#[tokio::test]
async fn a_malformed_block_refuses_a_body_change() {
    let store = store().await;
    for (body, needle) in [
        ("```component Chart\nx\n```\n", "'Chart' is not a component name"),
        ("```component chart\nx\n```\n", "'chart' is not a component name"),
        ("```component\nx\n```\n", "names none"),
        (&*format!("{}{}", def("m-x", "1"), def("m-x", "2")), "twice"),
    ] {
        let e = store.create(SYS, ticket("C", Kind::Build, body)).await.unwrap_err();
        assert!(e.to_string().contains(needle) && e.to_string().contains("nothing was created"), "{body}: {e}");
    }
    let id = born(&store, "plain").await;
    assert_eq!(id, "C-2", "no id spent");
    let e = store.amend(SYS.permit(&id), None, Some("```component Chart\nx\n```\n".into())).await.unwrap_err();
    assert!(e.to_string().contains(&format!("nothing changed for '{id}'")), "{e}");
    assert_eq!(store.get(SYS, &id).await.unwrap().body, "plain");
    assert!(store.components(SYS).is_empty());
}

/// The registry is durable: a reopened file holds it as acknowledged —
/// orphans and references included — and keeps moving it.
#[tokio::test]
async fn the_registry_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("components.db");
    let before = {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket("C", Kind::Scope, "root")).await.unwrap();
        let owner = born(&store, &format!("{}{}", def("m-x", "<template><m-y></m-y></template>"), def("m-y", "y"))).await;
        born(&store, "<m-x></m-x>").await;
        amend(&store, &owner, &def("m-y", "y")).await; // m-x orphaned, kept by C-3
        store.components(SYS)
    };
    assert_eq!(before.len(), 2);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.components(SYS), before);
    amend(&store, "C-3", "no more").await;
    assert_eq!(store.components(SYS).into_iter().map(|c| c.name).collect::<Vec<_>>(), ["m-y"]);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.components(SYS), vec![component("m-y", "y\n", Some("C-2"), &[])]);
}

/// `watch { epic }` passes a component's change whatever the epic: a
/// component is in no shortcode.
#[tokio::test]
async fn component_events_pass_any_epic_filter() {
    let store = store().await;
    born(&store, &def("m-x", "x")).await;
    let changed = Change::ComponentChanged { component: component("m-x", "x\n", Some("C-2"), &[]) };
    assert!(changed.clone().in_epic(Some(&ShortcodeId::from("OTHER"))).is_some());
    assert!(Change::ComponentRemoved { name: "m-x".into() }.in_epic(Some(&ShortcodeId::from("OTHER"))).is_some());
    assert_eq!(changed.in_epic(None).map(|c| heard(&[c])), Some(vec![("changed", "m-x".to_string())]));
}

/// A database without the table — this build's own file with it taken
/// away, as prod's is — gains it on open, durably, once, with the registry
/// built from every body: a name two bodies define is the earlier one's
/// (the later is refused when it next changes its body), references made
/// before and after the definition count, code examples do not, a
/// malformed block is skipped. Every ticket as it was.
#[tokio::test]
async fn a_database_without_the_table_gains_it_built_from_the_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket("C", Kind::Scope, "root <m-x></m-x>")).await.unwrap(); // C-1: references before
        born(&store, &def("m-x", "<template><m-y></m-y></template>")).await; // C-2: owns m-x
        born(&store, "plain").await; // C-3: will define m-x behind the server's back
        born(&store, "plain").await; // C-4: will hold a malformed block, written the same way
        born(&store, &def("m-y", "y")).await; // C-5: owns m-y
    }
    let before = {
        // As a binary from before COMP-3 would leave it: the table gone, and
        // bodies written without the registry hearing — C-3 now defines m-x,
        // C-4 holds a block no write would take and a tag in a code span.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE components").unwrap();
        for (id, body) in [
            ("C-3", format!("{}<m-y></m-y>", def("m-x", "theirs"))),
            ("C-4", "```component Bad\nx\n```\n`<m-x>` in code".to_string()),
        ] {
            conn.execute("UPDATE tickets SET body = ?1 WHERE id = ?2", [body, id.to_string()]).unwrap();
        }
        conn.query_row("SELECT count(*) FROM tickets", [], |r| r.get::<_, i64>(0)).unwrap()
    };
    let store = Store::open(&path).unwrap();
    assert_eq!(store.count(SYS) as i64, before, "every ticket kept");
    assert_eq!(
        store.components(SYS),
        vec![
            component("m-x", &src("<template><m-y></m-y></template>"), Some("C-2"), &["C-1"]),
            component("m-y", "y\n", Some("C-5"), &["C-3", "m-x"]),
        ]
    );
    // C-3 does not own m-x, and its next body change that still defines it is refused.
    let e = store.amend(SYS.permit("C-3"), None, Some(def("m-x", "still theirs"))).await.unwrap_err();
    assert!(e.to_string().contains("C-2"), "{e}");
    amend(&store, "C-3", "gave it up").await;
    assert_eq!(registry(&store)["m-y"].references, ["m-x"]);
    drop(store);

    // Once: the table is there, and the next open reads it.
    let mut conn = Connection::open(&path).unwrap();
    assert!(!components::migrate(&mut conn).unwrap(), "the next open builds nothing");
    let stored: i64 = conn.query_row("SELECT count(*) FROM components", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 2);
    drop(conn);
    let store = Store::open(&path).unwrap();
    assert_eq!(registry(&store)["m-y"].references, ["m-x"]);
}

/// A seed writes no registry: synthetic bodies hold no tag and no block —
/// held to every body a seed generates.
#[test]
fn synthetic_bodies_say_nothing_about_components() {
    let generated = synthetic::generate(
        SeedSpec {
            count: 2_000,
            seed: 7,
            into_existing: false,
        },
        &HashMap::new(),
        SeedShape::Flat,
    );
    for t in &generated {
        assert_eq!(components::read(&t.body), (components::Uses::default(), vec![]), "{}", t.id);
    }
}

// ---------------------------------------------------------------------------
// COMP-8: a component's docs and its preview line.

/// The ticket's own example, defined with a preview line.
const DOCUMENTED: &str = r#"```component m-wave-summary board=RETURNS label="Platform team"
<template><slot name="title"></slot> <b id="n"></b></template>
<docs>
  <summary>How many of a board's tickets are workable now (wave 1).</summary>
  <attr name="board" type="shortcode" required>The shortcode to count, e.g. RETURNS.</attr>
  <attr name="label" type="string">Shown before the count.</attr>
  <attr name="size" type="enum(s|m|l)">How big.</attr>
  <attr name="limit" type="number">At most this many.</attr>
  <attr name="compact" type="boolean">Tighter.</attr>
  <attr name="focus" type="ticket">A ticket to highlight.</attr>
  <slot name="title">Optional title markup.</slot>
  <slot>The rest.</slot>
  <example><m-wave-summary board="RETURNS"></m-wave-summary></example>
  <example>
    <m-wave-summary board="PLEXUS" label="Platform">
      <b slot="title">Platform</b>
    </m-wave-summary>
  </example>
  Longer help, as **markdown** prose.

  - counts wave 1
</docs>
<script>this.root.querySelector('#n').textContent = this.attr('board')</script>
```
"#;

/// COMP-8 acceptance 1, every row of the table: `<summary>`, each `<attr>`
/// kind, `required`, `<slot>` named and default, `<example>` inner HTML, the
/// remaining text as markdown help — and the preview line, quoted and bare.
#[tokio::test]
async fn docs_are_parsed_into_the_registry_row_by_row() {
    let store = store().await;
    let id = born(&store, DOCUMENTED).await;
    let c = registry(&store).remove("m-wave-summary").expect("registered");
    assert_eq!(c.owner.as_deref(), Some(id.as_str()));
    assert_eq!(c.preview, vec![("board".to_string(), "RETURNS".to_string()), ("label".to_string(), "Platform team".to_string())]);
    let docs = c.docs.expect("docs");
    assert_eq!(docs.summary, "How many of a board's tickets are workable now (wave 1).");
    let attrs: Vec<(&str, String, bool, &str)> = docs
        .attrs
        .iter()
        .map(|a| (a.name.as_str(), a.kind.to_string(), a.required, a.description.as_str()))
        .collect();
    assert_eq!(
        attrs,
        vec![
            ("board", "shortcode".into(), true, "The shortcode to count, e.g. RETURNS."),
            ("label", "string".into(), false, "Shown before the count."),
            ("size", "enum(s|m|l)".into(), false, "How big."),
            ("limit", "number".into(), false, "At most this many."),
            ("compact", "boolean".into(), false, "Tighter."),
            ("focus", "ticket".into(), false, "A ticket to highlight."),
        ]
    );
    assert_eq!(
        docs.attrs[2].kind,
        crate::types::AttrKind::Enum { values: vec!["s".into(), "m".into(), "l".into()] }
    );
    let slots: Vec<(Option<&str>, &str)> = docs.slots.iter().map(|s| (s.name.as_deref(), s.description.as_str())).collect();
    assert_eq!(slots, vec![(Some("title"), "Optional title markup."), (None, "The rest.")]);
    assert_eq!(
        docs.examples,
        vec![
            r#"<m-wave-summary board="RETURNS"></m-wave-summary>"#.to_string(),
            "<m-wave-summary board=\"PLEXUS\" label=\"Platform\">\n  <b slot=\"title\">Platform</b>\n</m-wave-summary>".to_string(),
        ]
    );
    assert_eq!(docs.help, "Longer help, as **markdown** prose.\n\n- counts wave 1");
    // The source is still the block's content, exactly — <docs> included.
    assert!(c.source.contains("<docs>") && c.source.ends_with("</script>\n"));
    // The example's own tags are inside the definition's block: no reference.
    assert_eq!(c.references, Vec::<String>::new());
}

/// A component without `<docs>` has `docs: None`, and no preview line an
/// empty preview.
#[tokio::test]
async fn a_component_without_docs_has_none() {
    let store = store().await;
    born(&store, &def("m-x", "<template>x</template>")).await;
    let c = &registry(&store)["m-x"];
    assert_eq!((c.docs.as_ref(), c.preview.len()), (None, 0));
    assert_eq!(c, &component("m-x", &src("<template>x</template>"), Some("C-2"), &[]));
}

/// COMP-8 acceptance 1: each malformed `<docs>` refuses the write in-band,
/// naming the component and the problem — a create (no id spent) and an
/// amend (title included) alike; nothing changes, nothing is heard.
#[tokio::test]
async fn a_malformed_docs_refuses_the_write_naming_the_problem() {
    let store = store().await;
    let id = born(&store, &def("m-x", "<template>x</template>")).await; // C-2
    let before = store.components(SYS);
    let mut rx = store.subscribe();
    for (docs, needle) in [
        ("<docs><attr type=\"number\">no name</attr></docs>", "an <attr> without a name"),
        ("<docs><attr name=\"n\" type=\"date\">x</attr></docs>", "unknown type 'date'"),
        ("<docs><summary>one</summary><summary>two</summary></docs>", "two <summary> elements"),
        ("<docs><attr name=\"n\" type=\"enum()\">x</attr></docs>", "names no value"),
        ("<docs></docs>\n<docs></docs>", "two <docs> elements"),
        ("<docs><summary>never closed</summary>", "<docs> is never closed"),
    ] {
        let body = def("m-y", &format!("<template>y</template>\n{docs}"));
        let e = store.create(SYS, ticket("C", Kind::Build, &body)).await.unwrap_err();
        assert!(matches!(&e, StoreError::ComponentDocs { id: None, name, .. } if name == "m-y"), "{e:?}");
        let message = e.to_string();
        assert!(
            message.contains("'m-y' has a malformed <docs>") && message.contains(needle) && message.contains("nothing was created"),
            "{docs}: {message}"
        );
        let e = store.amend(SYS.permit(&id), Some("new title".into()), Some(body.clone())).await.unwrap_err();
        assert!(e.to_string().contains(needle) && e.to_string().contains(&format!("nothing changed for '{id}'")), "{e}");
    }
    assert_eq!(store.components(SYS), before);
    assert!(drain(&mut rx).is_empty(), "nothing heard");
    assert_eq!(store.get(SYS, &id).await.unwrap().title, "a build in C", "the title as it was");
    assert_eq!(born(&store, "next").await, "C-3", "no id was spent");
}

/// The preview line is part of the definition: changing only it moves the
/// component (one `component_changed`), quoted and bare values alike.
#[tokio::test]
async fn the_preview_line_parses_and_moves_the_component() {
    let store = store().await;
    let id = born(&store, "```component m-x board=RETURNS label=\"Platform team\" compact\n<template>x</template>\n```\n").await;
    assert_eq!(
        registry(&store)["m-x"].preview,
        vec![
            ("board".to_string(), "RETURNS".to_string()),
            ("label".to_string(), "Platform team".to_string()),
            ("compact".to_string(), String::new()),
        ]
    );
    let mut rx = store.subscribe();
    amend(&store, &id, "```component m-x board='PLEXUS'\n<template>x</template>\n```\n").await;
    assert_eq!(registry(&store)["m-x"].preview, vec![("board".to_string(), "PLEXUS".to_string())]);
    let changes = drain(&mut rx);
    assert_eq!(heard(&changes), [("changed", "m-x".to_string())]);
    match changes.last() {
        Some(Change::ComponentChanged { component }) => assert_eq!(component.preview[0].1, "PLEXUS"),
        other => panic!("{other:?}"),
    }
}

/// Docs and preview survive a reopen (the preview stored, the docs parsed
/// from the stored source) — an orphan's included.
#[tokio::test]
async fn docs_and_preview_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docs.db");
    let before = {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket("C", Kind::Scope, "root")).await.unwrap();
        let owner = born(&store, DOCUMENTED).await;
        born(&store, "<m-wave-summary board=RETURNS></m-wave-summary>").await;
        born(&store, &format!("{}{}", def("m-y", "<docs><summary>y</summary></docs>"), "")).await;
        amend(&store, &owner, "let it go").await; // m-wave-summary orphaned, docs kept
        store.components(SYS)
    };
    assert!(before.iter().all(|c| c.docs.is_some()), "{before:?}");
    assert_eq!(before[0].owner, None);
    assert_eq!(before[0].preview.len(), 2);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.components(SYS), before);
}

/// A `components` table from before COMP-8 gains `preview` on open,
/// durably, once — each owned component's read from its owner's body, an
/// orphan's empty; the docs come from the stored sources.
#[tokio::test]
async fn a_table_without_preview_gains_it_from_the_owners_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("before-comp8.db");
    {
        let store = Store::open(&path).unwrap();
        store.create(SYS, ticket("C", Kind::Scope, "root")).await.unwrap();
        born(&store, DOCUMENTED).await; // C-2
        let gone = born(&store, "```component m-o board=X\n<docs><summary>o</summary></docs>\n```\n").await; // C-3
        born(&store, "<m-o></m-o>").await; // C-4
        amend(&store, &gone, "let it go").await; // m-o orphaned
    }
    {
        // As a binary from before COMP-8 left it: no preview column.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("ALTER TABLE components DROP COLUMN preview").unwrap();
    }
    let store = Store::open(&path).unwrap();
    let r = registry(&store);
    assert_eq!(r["m-wave-summary"].preview.len(), 2, "read from C-2's body");
    assert_eq!(r["m-wave-summary"].docs.as_ref().unwrap().attrs.len(), 6);
    assert_eq!((r["m-o"].owner.as_deref(), r["m-o"].preview.len()), (None, 0), "an orphan's preview is empty");
    assert_eq!(r["m-o"].docs.as_ref().unwrap().summary, "o");
    drop(store);
    let mut conn = Connection::open(&path).unwrap();
    assert!(!components::migrate_preview(&mut conn).unwrap(), "once");
}

/// COMP-14: a cite is a reference, never a copy. The citing body holds only
/// its `<m-cite>` tag — so the cited section's components count as the
/// cited ticket's references, its inputs as the cited ticket's, and a word
/// in it is found in the cited ticket alone; the citer references `m-cite`.
#[tokio::test]
async fn a_cite_counts_as_its_tag_and_nothing_it_shows() {
    let store = store().await;
    let house = born(&store, &format!("{}{}", def("m-field", "<slot></slot>"), def("m-cite", "<docs><summary>a window</summary></docs>"))).await;
    let cited = born(
        &store,
        "<m-field data-field=\"findings\">The zanzibar finding.</m-field>\n\n\
         <m-decision data-field=\"decision\" id=\"landing\"><m-question>Ship?</m-question>\
         <m-option name=\"Yes\"></m-option><m-option name=\"No\"></m-option></m-decision>\n",
    )
    .await;
    let citer = born(&store, &format!("<m-cite ref=\"{cited}\" field=\"findings\"></m-cite>\n\n<m-cite ref=\"{cited}#landing\"></m-cite>\n")).await;
    let reg = registry(&store);
    assert_eq!(reg["m-field"].references, [cited.as_str()], "the cited section's component is the cited ticket's");
    assert_eq!(reg["m-cite"].references, [citer.as_str()]);
    assert_eq!(reg["m-cite"].owner.as_deref(), Some(house.as_str()));
    let inputs = store.inputs(SYS, Some(&citer), None, None).unwrap();
    assert!(inputs.is_empty(), "{inputs:?}");
    let inputs = store.inputs(SYS, Some(&cited), None, None).unwrap();
    assert_eq!(inputs.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["landing"]);
    let page = store.search(SYS, "zanzibar", None, None, None).await.unwrap();
    assert_eq!(page.hits.iter().map(|h| h.summary.id.as_str()).collect::<Vec<_>>(), [cited.as_str()]);
}
