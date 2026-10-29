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

/// UI-8: a segment percent-decoded — the browser keeps the hash encoded
/// (`#/f/status%20Ready`), and a prop is the text (`status Ready`); a `%`
/// that starts no escape, or bytes that are not UTF-8, are kept as written.
pub fn decode(segment: &str) -> String {
    let b = segment.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| segment.to_string())
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
                    props.insert(name.to_string(), Value::String(decode(w)));
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
