//! SYNAPSE-CC-9: every position in menger's schemas is typed. Every
//! activation `menger::build_rpc` registers, and each of their static
//! children (`tickets.commits`, `chats.inputs`, …), answers a schema with no
//! untyped site (`plexus_core::plexus::schema::untyped_sites`): no position
//! that is neither a typed schema nor the JSON type (a bare `object` or
//! `array`, a type list like `["object","boolean"]`), and no RPC method
//! without `returns`. A `serde_json::Value` is the JSON type — a type like
//! any other (the owner, 2026-10-29: nothing is opaque). synapse refuses
//! each untyped site at IR build (SYNAPSE-CC-8). SYNAPSE-CC-12 adds
//! `foreign_keywords` here.
#![cfg(feature = "server")]

use menger_tickets_api::access::Access;
use menger_tickets_api::{
    ChatsActivation, CommentsActivation, DeploymentsActivation, EpicsActivation, MediaActivation,
    MembersActivation, NotesActivation, OrgsActivation, SessionActivation, Store, ThemesActivation,
    TicketsActivation,
};
use plexus_core::plexus::schema::untyped_sites;
use plexus_core::plexus::{Activation, PluginSchema};
use std::sync::Arc;

/// `(path, schema)` for an activation and, under it, its static children.
fn with_children<A: Activation>(a: &A, out: &mut Vec<(String, PluginSchema)>) {
    let s = a.plugin_schema();
    let path = s.namespace.clone();
    for (child, c) in a.static_child_schemas() {
        out.push((format!("{path}.{child}"), c));
    }
    out.push((path, s));
}

fn every_schema() -> Vec<(String, PluginSchema)> {
    let store = Arc::new(Store::new());
    let mut out = Vec::new();
    with_children(&TicketsActivation::new(Arc::clone(&store)), &mut out);
    with_children(&CommentsActivation::new(Arc::clone(&store)), &mut out);
    with_children(&ChatsActivation::new(Arc::clone(&store)), &mut out);
    with_children(&MediaActivation::new(Arc::clone(&store)), &mut out);
    with_children(&NotesActivation::new(Arc::clone(&store)), &mut out);
    with_children(&ThemesActivation::new(Arc::clone(&store)), &mut out);
    with_children(&DeploymentsActivation::new(Arc::clone(&store)), &mut out);
    with_children(&OrgsActivation::new(Arc::clone(&store)), &mut out);
    with_children(&EpicsActivation::new(Arc::clone(&store)), &mut out);
    with_children(&MembersActivation::new(Arc::clone(&store), Access::for_tests()), &mut out);
    with_children(&SessionActivation::new(), &mut out);
    out
}

#[test]
fn no_activation_has_an_untyped_site() {
    let all = every_schema();
    let paths: Vec<&str> = all.iter().map(|(p, _)| p.as_str()).collect();
    for p in ["tickets", "tickets.commits", "tickets.nav", "chats.inputs", "members.teams", "deployments.webhook", "session"] {
        assert!(paths.contains(&p), "{p} not walked: {paths:?}");
    }
    let mut found = Vec::new();
    for (path, schema) in &all {
        for site in untyped_sites(schema) {
            found.push(format!("{path}: {site}"));
        }
    }
    assert_eq!(found, Vec::<String>::new(), "give each a type (SYNAPSE-CC-6)");
}
