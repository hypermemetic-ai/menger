//! UI-5's goldens: `tests/golden/<name>.svelte` compiles, as `<name>`, to
//! `<name>.json` (the view tree) and `<name>.rhai` (the handlers), byte for
//! byte. `ticket-row` is the contract doc's example (UI-4); `board-layout`
//! its sections form (`<slot name … />`).

mod common;

use menger_ui_compiler::compile;
use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_component_compiles_to_the_contracts_tree() {
    for name in common::goldens() {
        let src = common::golden(&name, "svelte");
        let out = compile(&name, &src).unwrap_or_else(|r| panic!("{name}: refused at line {}: {}", r.line, r.message));
        assert_eq!(out.tree, common::golden(&name, "json"), "{name}: the tree differs from tests/golden/{name}.json");
        assert_eq!(out.handlers, common::golden(&name, "rhai"), "{name}: the handlers differ from tests/golden/{name}.rhai");
        // The tree names its handlers by their sha256, and itself by its name.
        let tree: serde_json::Value = serde_json::from_str(&out.tree).expect("the tree is JSON");
        assert_eq!(tree["handlers"], hex(&Sha256::digest(out.handlers.as_bytes())), "{name}: tree.handlers is the handlers' sha256");
        assert_eq!(tree["abi"], "engine/1", "{name}: the abi");
        assert_eq!(tree["name"], name.as_str(), "{name}: the name");
    }
}

#[test]
fn the_output_is_deterministic() {
    for name in common::goldens() {
        let src = common::golden(&name, "svelte");
        let first = compile(&name, &src).unwrap_or_else(|r| panic!("{name}: refused at line {}: {}", r.line, r.message));
        let second = compile(&name, &src).expect("the second run compiles as the first");
        assert_eq!(first, second, "{name}: two native runs differ");
        match common::run(&name, &src, common::FUEL) {
            common::Outcome::Ok(wasm) => {
                assert_eq!(wasm.tree, first.tree, "{name}: the wasm tree differs from the native one");
                assert_eq!(wasm.handlers, first.handlers, "{name}: the wasm handlers differ from the native ones");
            }
            other => panic!("{name}: the wasm run did not compile it: {other:?}"),
        }
    }
}
