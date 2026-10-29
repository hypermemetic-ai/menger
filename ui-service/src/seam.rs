//! UI-7: **the seam** — the ui service as a client of the service it is
//! registered beside. The process's [`DynamicHub`] is built after the ui
//! activation is, so the activation holds an empty [`Seam`] that the
//! builder fills once the hub exists ([`Seam::attach`]), with the server's
//! own caller (menger's `Access::system_caller`). A push asks `tickets`
//! through [`HubTransport::as_`] as that caller — the service's own full
//! view, so a method's existence never depends on who pushes — through the
//! same route and gate a socket reaches, never a copy of the schema.

use futures::StreamExt;
use plexus_client::{MethodRef, StreamItem, Transport};
use plexus_core::plexus::{AuthContext, DynamicHub, HubTransport};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// The service a component's `uses` are read against.
pub const TICKETS: &str = "tickets";

/// Where the running service is, once the builder says. Weak: the hub owns
/// the activation that holds this.
#[derive(Clone, Default)]
pub struct Seam {
    at: Arc<OnceLock<(Weak<DynamicHub>, AuthContext)>>,
}

impl std::fmt::Debug for Seam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seam").field("attached", &self.at.get().is_some()).finish()
    }
}

impl Seam {
    /// The hub this service is registered in, called as `system`; the
    /// first attach holds.
    pub fn attach(&self, hub: &Arc<DynamicHub>, system: AuthContext) {
        let _ = self.at.set((Arc::downgrade(hub), system));
    }

    /// A transport into the running hub as the system; `None` before the
    /// builder attached it (or after the hub is gone).
    pub fn system(&self) -> Option<HubTransport> {
        let (hub, system) = self.at.get()?;
        hub.upgrade().map(|hub| HubTransport::as_(hub, system.clone()))
    }
}

/// A dotted path kept for the life of the process — only paths the
/// running schema names as children are ever made, so the set is bounded by
/// the program, not by what a push names.
fn interned(path: String) -> &'static str {
    static PATHS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut paths = PATHS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    match paths.get(path.as_str()) {
        Some(p) => p,
        None => {
            let p: &'static str = Box::leak(path.into_boxed_str());
            paths.insert(p);
            p
        }
    }
}

/// The schemas of `tickets` and its children, read through the seam once
/// per push.
pub struct Schemas {
    transport: HubTransport,
    read: BTreeMap<String, Value>,
}

impl Schemas {
    pub fn new(transport: HubTransport) -> Self {
        Self { transport, read: BTreeMap::new() }
    }

    /// The schema of the activation at `path` (`tickets`, `tickets.nav`).
    async fn at(&mut self, path: &str) -> Result<&Value, String> {
        if !self.read.contains_key(path) {
            let method = MethodRef::new(TICKETS, "schema").at(if path == TICKETS { "" } else { interned(path.to_string()) });
            let mut items = self.transport.call(method, Value::Object(Default::default()));
            let mut schema = None;
            while let Some(item) = items.next().await {
                match item.map_err(|e| format!("{path}.schema through the seam: {e}"))? {
                    StreamItem::Data { content, .. } => {
                        schema = Some(content);
                        break;
                    }
                    StreamItem::Error { message, .. } => return Err(format!("{path}.schema through the seam: {message}")),
                    _ => {}
                }
            }
            let schema = schema.ok_or_else(|| format!("{path}.schema answered nothing through the seam"))?;
            self.read.insert(path.to_string(), schema);
        }
        Ok(&self.read[path])
    }

    /// The running tickets service's hash (`tickets.schema`'s `hash`).
    pub async fn hash(&mut self) -> Result<String, String> {
        let schema = self.at(TICKETS).await?;
        schema["hash"].as_str().map(str::to_string).ok_or_else(|| "tickets.schema holds no hash".to_string())
    }

    /// Whether the running service exposes `method` (`tickets.list`, or a
    /// method under a child, `tickets.nav.tree`); `Err` when the seam fails.
    pub async fn exposes(&mut self, method: &str) -> Result<bool, String> {
        let mut parts = method.split('.');
        if parts.next() != Some(TICKETS) {
            return Ok(false);
        }
        let rest: Vec<&str> = parts.collect();
        let Some((last, children)) = rest.split_last() else { return Ok(false) };
        let mut path = TICKETS.to_string();
        for child in children {
            if !has(self.at(&path).await?, child, "static_child") {
                return Ok(false);
            }
            path = format!("{path}.{child}");
        }
        Ok(has(self.at(&path).await?, last, "rpc"))
    }
}

/// Whether `schema` lists a method `name` of role `kind`.
fn has(schema: &Value, name: &str, kind: &str) -> bool {
    schema["methods"].as_array().is_some_and(|ms| {
        ms.iter().any(|m| m["name"] == name && m["role"]["kind"].as_str().unwrap_or("rpc") == kind)
    })
}
