//! UI-6: **the router** — `manifest.routes`: a hash route (`#/ticket/T-1`)
//! to the component drawn (a layout too) and its props, each `:name`
//! segment of the pattern a prop. The most specific pattern wins: one with
//! fewer `:` segments over one with more (`#/ticket/new` over
//! `#/ticket/:id`).

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

fn segments(path: &str) -> Vec<&str> {
    let path = path.trim_start_matches('#').trim_start_matches('/');
    let path = path.split(['?']).next().unwrap_or("");
    path.split('/').filter(|s| !s.is_empty()).collect()
}

impl Router {
    /// Over `manifest.routes`.
    pub fn new(routes: &BTreeMap<String, String>) -> Self {
        Self { routes: routes.clone() }
    }

    /// The route `hash` (`#/ticket/T-1`; an empty hash is `#/`) names.
    pub fn resolve(&self, hash: &str) -> Option<Route> {
        let want = segments(hash);
        let mut best: Option<(usize, Route)> = None;
        for (pattern, component) in &self.routes {
            let pat = segments(pattern);
            if pat.len() != want.len() {
                continue;
            }
            let mut props = Map::new();
            let mut params = 0;
            let fits = pat.iter().zip(&want).all(|(p, w)| match p.strip_prefix(':') {
                Some(name) => {
                    params += 1;
                    props.insert(name.to_string(), Value::String(w.to_string()));
                    true
                }
                None => p == w,
            });
            if fits && best.as_ref().is_none_or(|(n, _)| params < *n) {
                best = Some((params, Route { component: component.clone(), props }));
            }
        }
        best.map(|(_, r)| r)
    }
}

#[cfg(test)]
mod tests;
