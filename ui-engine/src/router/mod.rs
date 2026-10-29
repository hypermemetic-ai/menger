//! UI-6: **the router** — `manifest.routes`: a hash route (`#/ticket/T-1`)
//! to the component drawn (a layout too) and its props, each `:name`
//! segment of the pattern a prop. The most specific pattern wins: one
//! with fewer `:` segments over one with more.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A resolved route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub component: String,
    pub props: Map<String, Value>,
}

/// The routes of the live version.
#[derive(Debug, Clone, Default)]
pub struct Router {
    routes: BTreeMap<String, String>,
}

impl Router {
    /// Over `manifest.routes`.
    pub fn new(routes: &BTreeMap<String, String>) -> Self {
        Self { routes: routes.clone() }
    }

    /// The route `hash` (`#/ticket/T-1`; an empty hash is `#/`) names.
    pub fn resolve(&self, hash: &str) -> Option<Route> {
        let _ = (hash, &self.routes);
        todo!("UI-6: Router::resolve")
    }
}
