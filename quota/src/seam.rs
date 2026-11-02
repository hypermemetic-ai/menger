//! **The seam** — the quota service as a client of the services beside it,
//! and they of it (V2-1: "each reaching the others as a client through the
//! seam"; media's and ui's `seam.rs` are the pattern).
//!
//! [`Seam`] is what the quota service asks the others, as the server's own
//! caller (the builder attaches it with `Access::system_caller`): a team's
//! members (`members.teams.show`), an organization's principals
//! (`members.list`), and each store's own count of what a principal keeps
//! (`tickets.revisions.usage`, `media.usage`, `docs.usage`) for `recount`.
//! Whether a principal is exempt — an admin of `*`, the operator — is the
//! gate's word, asked of the running service. It never opens another file.
//!
//! [`QuotaSeam`] is the other way: a [`Meter`] a writing service holds,
//! asking `quota.reserve`/`commit`/`release`/`credit` through the running
//! service as the server's own caller.

use crate::meter::{Hold, Meter, Refusal};
use crate::vocab::QuotaEvent;
use crate::QuotaClient;
use futures::StreamExt;
use plexus_core::plexus::{AuthContext, DynamicHub, HubTransport, PlexusStreamItem};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// A grant only an admin holds on `*` among the quota service's — the
/// exemption's and the admin's question.
pub const ADMIN_GRANT: &str = "quota.set";

/// How long a team's members are trusted before they are asked again.
const TEAM_TTL: Duration = Duration::from_secs(30);

type Attached = (Weak<DynamicHub>, AuthContext);

/// Where the running service is, and who the quota service asks as.
#[derive(Clone, Default)]
pub struct Seam {
    at: Arc<OnceLock<Attached>>,
    teams: Arc<Mutex<HashMap<String, (Instant, Vec<String>)>>>,
}

impl std::fmt::Debug for Seam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seam").field("attached", &self.at.get().is_some()).finish()
    }
}

/// `principal` as an identity the gate can be asked about.
pub fn identity(principal: &str) -> AuthContext {
    AuthContext::new(principal.to_string(), "quota".into(), vec![], json!({ "principal": principal }))
}

/// Every data item of `method` called on `hub` as `ctx`, or the error said.
pub async fn call(hub: &DynamicHub, ctx: &AuthContext, method: &str, params: Value) -> Result<Vec<Value>, String> {
    let mut stream = hub.route(method, params, Some(ctx)).await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            PlexusStreamItem::Data { content, .. } => out.push(content.value().clone()),
            PlexusStreamItem::Error { message, .. } => return Err(message),
            _ => {}
        }
    }
    Ok(out)
}

/// What one store counts for one principal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counted {
    pub count: Option<u64>,
    pub bytes: Option<u64>,
}

impl Seam {
    /// The service this one is registered in, and the caller it asks as; the first attach holds.
    pub fn attach(&self, service: &Arc<DynamicHub>, system: AuthContext) {
        let _ = self.at.set((Arc::downgrade(service), system));
    }

    fn service(&self) -> Option<(Arc<DynamicHub>, AuthContext)> {
        self.at.get().and_then(|(w, c)| w.upgrade().map(|h| (h, c.clone())))
    }

    /// Whether `ctx` holds [`ADMIN_GRANT`] on `*` — the gate's word now.
    /// A service with no gate: everyone is.
    pub fn admin_everywhere(&self, ctx: &AuthContext) -> bool {
        let Some((service, _)) = self.service() else { return false };
        match service.gate() {
            None => true,
            Some(gate) => gate.held_on(ctx, &plexus_auth_core::grants::Grant::new(ADMIN_GRANT)).iter().any(|s| s.is_everywhere()),
        }
    }

    /// Whether `principal` is exempt from every cap: an admin of `*` (the
    /// operator's `--admin` among them), as the gate says now. Not attached,
    /// or no gate: no one is.
    pub fn exempt(&self, principal: &str) -> bool {
        let Some((service, system)) = self.service() else { return false };
        if system.principal().is_some_and(|p| p.to_string() == principal) {
            return true;
        }
        match service.gate() {
            None => false,
            Some(gate) => gate.held_on(&identity(principal), &plexus_auth_core::grants::Grant::new(ADMIN_GRANT)).iter().any(|s| s.is_everywhere()),
        }
    }

    /// Which of `teams` `principal` is in — each team's members asked of
    /// `members.teams.show` as the server, kept for a few seconds.
    pub async fn teams_of(&self, principal: &str, teams: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for team in teams {
            if self.members_of(team).await.iter().any(|m| m == principal) {
                out.push(team.clone());
            }
        }
        out
    }

    async fn members_of(&self, team: &str) -> Vec<String> {
        if let Some((at, members)) = self.teams.lock().unwrap_or_else(|p| p.into_inner()).get(team) {
            if at.elapsed() < TEAM_TTL {
                return members.clone();
            }
        }
        let Some((service, system)) = self.service() else { return Vec::new() };
        let members: Vec<String> = match call(&service, &system, "members.teams.show", json!({ "team": team })).await {
            Ok(items) => items.iter().filter(|v| v["type"] == "team_member").filter_map(|v| v["member"]["principal"].as_str().map(str::to_string)).collect(),
            Err(e) => {
                tracing::warn!("V2-31: members.teams.show {team} through the seam: {e}");
                Vec::new()
            }
        };
        self.teams.lock().unwrap_or_else(|p| p.into_inner()).insert(team.to_string(), (Instant::now(), members.clone()));
        members
    }

    /// The principals granted a role in organization `org` — on it or on a
    /// shortcode in it (`members.list`, a grant's scope `<org>:…`) — and
    /// the members of its teams granted one.
    pub async fn principals_in(&self, org: &str) -> Result<Vec<String>, String> {
        let (service, system) = self.service().ok_or("the quota service is not attached to a running service")?;
        let prefix = format!("{org}:");
        let items = call(&service, &system, "members.list", json!({})).await?;
        let items: Vec<&Value> = items.iter().filter(|v| v["type"] != "grant" || v["grant"]["scope"].as_str().is_some_and(|s| s.starts_with(&prefix))).collect();
        let mut out = std::collections::BTreeSet::new();
        for v in items {
            if let Some(m) = v["message"].as_str().filter(|_| v["type"] == "error") {
                return Err(m.to_string());
            }
            if let Some(p) = v["grant"]["principal"].as_str() {
                match p.strip_prefix("team:") {
                    Some(team) => out.extend(self.members_of(team).await),
                    None if p != "everyone" => {
                        out.insert(p.to_string());
                    }
                    None => {}
                }
            }
        }
        Ok(out.into_iter().collect())
    }

    /// `method` (a store's usage read: `tickets.revisions.usage`,
    /// `media.usage`, `docs.usage`) asked as the server with `params` (every
    /// principal): each `usage { principal, count? | files?, bytes? }` it yields.
    pub async fn counted(&self, method: &str, params: &str) -> Result<HashMap<String, Counted>, String> {
        let (service, system) = self.service().ok_or("the quota service is not attached to a running service")?;
        let params: Value = serde_json::from_str(params).map_err(|e| e.to_string())?;
        let items = call(&service, &system, method, params).await?;
        let mut out = HashMap::new();
        for v in &items {
            match v["type"].as_str() {
                Some("usage") => {
                    if let Some(p) = v["principal"].as_str() {
                        out.insert(p.to_string(), Counted { count: v["count"].as_u64().or(v["files"].as_u64()), bytes: v["bytes"].as_u64() });
                    }
                }
                Some("error") => return Err(v["message"].as_str().unwrap_or("refused").to_string()),
                _ => {}
            }
        }
        Ok(out)
    }
}

/// The quota service as a [`Meter`]: what a writing service (tickets,
/// media, docs, the shell's `/media/<hash>`) holds once the builder
/// attaches the running service.
#[derive(Clone, Default)]
pub struct QuotaSeam {
    at: Arc<OnceLock<Attached>>,
}

impl std::fmt::Debug for QuotaSeam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaSeam").field("attached", &self.at.get().is_some()).finish()
    }
}

impl QuotaSeam {
    /// Asks `quota.*` on `service` as `system` (the server's own caller).
    pub fn attach(&self, service: &Arc<DynamicHub>, system: AuthContext) {
        let _ = self.at.set((Arc::downgrade(service), system));
    }

    fn client(&self) -> Result<QuotaClient<HubTransport>, Refusal> {
        let (w, ctx) = self.at.get().ok_or_else(|| Refusal::Failed("not attached to a running service".into()))?;
        let hub = w.upgrade().ok_or_else(|| Refusal::Failed("the service has stopped".into()))?;
        Ok(QuotaClient::new(HubTransport::as_(hub, ctx.clone())))
    }

    fn principal(p: &str) -> Result<crate::vocab::PrincipalName, Refusal> {
        p.parse().map_err(Refusal::Failed)
    }

    fn source(s: &str) -> Result<crate::vocab::SourceName, Refusal> {
        s.parse().map_err(|_| Refusal::UnknownSource { source: s.to_string() })
    }

    fn ticket(t: &str) -> Option<crate::vocab::HoldTicket> {
        t.parse().ok()
    }
}

#[async_trait::async_trait]
impl Meter for QuotaSeam {
    async fn reserve(&self, principal: &str, source: &str, amount: u64) -> Result<Hold, Refusal> {
        let (p, s) = (Self::principal(principal)?, Self::source(source)?);
        let events = self.client()?.reserve_all(p, s, amount).await.map_err(|e| Refusal::Failed(format!("quota.reserve: {e}")))?;
        match events.as_slice() {
            [QuotaEvent::Reserved { ticket, remaining, .. }] => Ok(Hold { ticket: Some(ticket.clone()), remaining: *remaining }),
            [QuotaEvent::OverAllowance { source, used, cap, asked, .. }] => Err(Refusal::OverAllowance { source: source.clone(), used: *used, cap: *cap, asked: *asked }),
            [QuotaEvent::TooManyAtOnce { source, open, max, .. }] => Err(Refusal::TooManyAtOnce { source: source.clone(), open: *open, max: *max }),
            [QuotaEvent::BoxFull { used_percent, full_percent, .. }] => Err(Refusal::BoxFull { used_percent: *used_percent, full_percent: *full_percent }),
            [QuotaEvent::Error { message }] => Err(Refusal::Failed(message.clone())),
            other => Err(Refusal::Failed(format!("quota.reserve answered {other:?}"))),
        }
    }

    async fn commit(&self, hold: Hold, amount: Option<u64>) {
        let Some(t) = hold.ticket.as_deref().and_then(Self::ticket) else { return };
        match self.client() {
            Ok(c) => match c.commit_all(t, amount).await.as_deref() {
                Ok([QuotaEvent::Committed { .. }]) => {}
                other => tracing::warn!("V2-31: quota.commit answered {other:?}"),
            },
            Err(e) => tracing::warn!("V2-31: quota.commit: {e}"),
        }
    }

    async fn release(&self, hold: Hold) {
        let Some(t) = hold.ticket.as_deref().and_then(Self::ticket) else { return };
        if let Ok(c) = self.client() {
            if let Err(e) = c.release_all(t).await {
                tracing::warn!("V2-31: quota.release: {e}");
            }
        }
    }

    async fn credit(&self, principal: &str, source: &str, amount: u64) {
        let (Ok(p), Ok(s)) = (Self::principal(principal), Self::source(source)) else { return };
        if let Ok(c) = self.client() {
            if let Err(e) = c.credit_all(p, s, amount).await {
                tracing::warn!("V2-31: quota.credit: {e}");
            }
        }
    }
}
