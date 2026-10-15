//! THEME-5: the parts a theme may skin. A component's `<docs>` declares them
//! (`<part name="label">`); a documented part its `<template>` does not
//! carry is refused by the write that holds it, so the registry only ever
//! documents parts a component exposes. And the house's own bodies — kept
//! in the tracker, not here — are checked the same way on demand:
//! `MENGER_HOUSE_BODIES=a.md:b.md cargo test -p menger-tickets-api
//! house_bodies` files each body and asks that every component it defines
//! documents at least one part (one that renders nothing aside).

use super::*;

fn ticket(kind: Kind, body: &str) -> NewTicket {
    NewTicket {
        epic: "H".into(),
        title: format!("a {}", kind.as_str()),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        path: None,
        body: body.into(),
    }
}

async fn store() -> Store {
    let store = Store::new();
    store.create(SYS, ticket(Kind::Scope, "the root")).await.unwrap();
    store
}

const FIELD: &str = "```component m-x\n<template><section part=\"section\"><span class=\"l\" part=\"label\"></span><slot></slot></section></template>\n\
<docs><summary>A field.</summary><part name=\"section\">The section.</part><part name=\"label\">The label.</part>\
<part name=\"go\" scripted>A button the script makes.</part></docs>\n```\n";

/// Documented parts reach the registry in order, scripted ones marked.
#[tokio::test]
async fn documented_parts_are_registered() {
    let store = store().await;
    store.create(SYS, ticket(Kind::Doc, FIELD)).await.unwrap();
    let c = store.components(SYS).into_iter().find(|c| c.name == "m-x").expect("m-x");
    let parts: Vec<(String, bool)> = c.docs.expect("docs").parts.into_iter().map(|p| (p.name, p.scripted)).collect();
    assert_eq!(parts, vec![("section".into(), false), ("label".into(), false), ("go".into(), true)]);
}

/// A part the template lacks is refused — create and amend alike — naming
/// the part, and nothing is written.
#[tokio::test]
async fn a_part_the_template_lacks_is_refused() {
    let store = store().await;
    let bad = FIELD.replace("part=\"label\"", "");
    let e = store.create(SYS, ticket(Kind::Doc, &bad)).await.expect_err("refused");
    assert!(e.to_string().contains("the part 'label' is documented but no element of the <template> carries part=\"label\""), "{e}");
    assert!(store.components(SYS).is_empty());
    let id = store.create(SYS, ticket(Kind::Doc, FIELD)).await.unwrap().id;
    let e = store.amend(SYS.permit(&id), None, Some(bad)).await.expect_err("refused");
    assert!(e.to_string().contains("the part 'label'"), "{e}");
    let c = store.components(SYS).into_iter().find(|c| c.name == "m-x").unwrap();
    assert_eq!(c.docs.unwrap().parts.len(), 3, "the amend wrote nothing");
}

/// On demand: the house's bodies, as they will be filed, each component in
/// them documenting the parts it exposes.
#[tokio::test]
async fn house_bodies_expose_their_documented_parts() {
    let Ok(paths) = std::env::var("MENGER_HOUSE_BODIES") else { return };
    let store = store().await;
    let mut seen = 0;
    for path in paths.split(':').filter(|p| !p.is_empty()) {
        let body = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let id = store.create(SYS, ticket(Kind::Doc, &body)).await.unwrap_or_else(|e| panic!("{path}: refused: {e}")).id;
        // A component that renders nothing (m-house, the shared code: an empty template) has nothing to skin.
        let renders = |c: &Component| !c.source.contains("<template></template>");
        for c in store.components(SYS).into_iter().filter(|c| c.owner.as_deref() == Some(id.as_str()) && renders(c)) {
            let docs = c.docs.unwrap_or_else(|| panic!("{path}: {} has no docs", c.name));
            assert!(!docs.parts.is_empty(), "{path}: {} documents no part a theme may skin", c.name);
            seen += 1;
        }
    }
    eprintln!("house bodies: {seen} components, every one documenting its parts");
    assert!(seen > 0, "MENGER_HOUSE_BODIES named no body defining a component");
}
