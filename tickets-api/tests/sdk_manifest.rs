//! COMP-10: the SDK's drift guard. `menger.tickets.*` inside a frame is the
//! client hub-codegen generated from this activation's schema — a snapshot,
//! committed in `frontend/sdk/generated/` by `scripts/sdk-codegen.sh`, which
//! writes beside it `manifest.json`: each method of the activation and its
//! parameter names, as the generator read them. This test holds the
//! activation to that manifest, so a method or parameter added, removed or
//! renamed fails `cargo test --workspace` until the client is generated
//! again — never silently shipping a frame SDK that calls what is gone.
//! (Events and types are not compared: a renamed field already fails the
//! wasm app's build, MFE-11, and the script regenerates them all at once.)
#![cfg(feature = "server")]

use menger_tickets_api::{Store, Tickets};
use plexus_core::plexus::Activation;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Each method's parameter names, sorted, by method name.
type Surface = BTreeMap<String, Vec<String>>;

/// The activation's surface, from its own schema.
fn activation() -> Surface {
    let schema = Tickets::new(Arc::new(Store::new())).plugin_schema();
    assert_eq!(schema.namespace, "tickets");
    schema
        .methods
        .iter()
        .map(|m| {
            let mut params: Vec<String> = m
                .params
                .as_ref()
                .and_then(|p| p.as_object())
                .and_then(|o| o.get("properties"))
                .and_then(|p| p.as_object())
                .map(|props| props.keys().cloned().collect())
                .unwrap_or_default();
            params.sort();
            (m.name.clone(), params)
        })
        .collect()
}

/// The generated client's surface, from the manifest the script wrote.
fn generated() -> Surface {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../frontend/sdk/generated/manifest.json");
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{path}: {e} — run scripts/sdk-codegen.sh to generate the SDK's client"));
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("manifest.json is JSON");
    assert_eq!(manifest["namespace"], "tickets");
    serde_json::from_value(manifest["methods"].clone()).expect("manifest.json's methods: { name: [param, …] }")
}

/// What differs, one line each.
fn drift(activation: &Surface, generated: &Surface) -> Vec<String> {
    let mut out = Vec::new();
    for (name, params) in activation {
        match generated.get(name) {
            None => out.push(format!("method `{name}` is not in the generated client")),
            Some(g) if g != params => out.push(format!("`{name}` takes {params:?}; the generated client sends {g:?}")),
            Some(_) => {}
        }
    }
    for name in generated.keys().filter(|n| !activation.contains_key(*n)) {
        out.push(format!("the generated client has `{name}`, which the activation no longer has"));
    }
    out
}

#[test]
fn the_generated_sdk_matches_the_activation() {
    let problems = drift(&activation(), &generated());
    assert!(
        problems.is_empty(),
        "the tickets activation's surface changed since the SDK's client was generated — run scripts/sdk-codegen.sh \
         (and commit frontend/sdk/):\n  {}",
        problems.join("\n  ")
    );
}

/// The comparison itself: a renamed parameter, a new method and a gone one
/// are each named.
#[test]
fn drift_names_what_moved() {
    let surface = |pairs: &[(&str, &[&str])]| -> Surface {
        pairs.iter().map(|(m, ps)| (m.to_string(), ps.iter().map(|p| p.to_string()).collect())).collect()
    };
    let now = surface(&[("waves", &["epics", "kinds"]), ("health", &[]), ("fresh", &[])]);
    let then = surface(&[("waves", &["epic", "kinds"]), ("health", &[]), ("gone", &["x"])]);
    let problems = drift(&now, &then);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].contains("`fresh` is not in the generated client"), "{problems:?}");
    assert!(problems[1].contains("`waves` takes [\"epics\", \"kinds\"]"), "{problems:?}");
    assert!(problems[2].contains("`gone`, which the activation no longer has"), "{problems:?}");
    assert!(drift(&now, &now).is_empty());
}
