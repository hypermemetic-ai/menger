//! tether takes callers as substrate does (PLEXUS-6) and menger does
//! (ACCESS-24): through `plexus-login`, with its global-role rules
//! ([`SimpleAccess`]). No open mode: `key` (agents' Ed25519 key proofs,
//! synapse's `_login tether --key NAME`) at least; `--admin` names the first
//! admins, `--grant ROLE=PRINCIPAL` the standing grants. An anonymous socket
//! reaches only the auth door.

use std::str::FromStr;

use plexus_core::plexus::{Activation, AuthSetup, DynamicHub};
use plexus_login::login::Provider;
use plexus_login::simple::{SimpleAccess, SimpleRole};
use plexus_login::{Branding, LoginAccess, LoginOptions, Wired, Wiring};
use plexus_transport::TransportServerBuilder;

/// The app as the login names it.
pub const BRANDING: Branding = Branding { app: "tether", version: env!("CARGO_PKG_VERSION"), mark: "t", motto: "" };

/// A standing grant, `ROLE=PRINCIPAL` (e.g. `editor=key:54a9…`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartGrant {
    pub role: SimpleRole,
    pub principal: String,
}

impl FromStr for StartGrant {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (role, principal) = s
            .split_once('=')
            .ok_or_else(|| format!("`{s}` is not ROLE=PRINCIPAL (e.g. editor=key:<hex>)"))?;
        let principal = principal.trim();
        principal
            .parse::<plexus_auth_core::identity::Principal>()
            .map_err(|e| format!("`{principal}` is not a principal ({e})"))?;
        Ok(Self { role: role.parse()?, principal: principal.to_string() })
    }
}

/// How tether takes callers.
#[derive(Clone, Debug, Default)]
pub struct LoginConfig {
    /// `--auth-provider`s; at least one.
    pub providers: Vec<Provider>,
    /// `--admin` principals.
    pub admins: Vec<String>,
    /// `--grant ROLE=PRINCIPAL`s.
    pub grants: Vec<StartGrant>,
    /// The WebSocket's port: a key proof names this socket.
    pub ws_port: u16,
}

/// Why this start is refused, if it is: there is no open mode.
pub fn refusal(providers: &[Provider]) -> Option<String> {
    providers.is_empty().then(|| {
        "tether has no open mode: give it a login provider with --auth-provider <SPEC> \
         (TETHER_AUTH_PROVIDERS) — `--auth-provider key` admits agents' Ed25519 keys, \
         with `--admin key:<hex>` (synapse _key show NAME) for you"
            .into()
    })
}

/// The login, wired.
pub struct Login {
    pub wired: Wired<SimpleAccess>,
    pub access: SimpleAccess,
}

impl Login {
    pub async fn new(cfg: &LoginConfig) -> anyhow::Result<Self> {
        if let Some(why) = refusal(&cfg.providers) {
            anyhow::bail!(why);
        }
        let mut opts = LoginOptions::new(BRANDING, cfg.ws_port, cfg.ws_port);
        opts.providers = cfg.providers.clone();
        opts.page = false;
        let wiring = Wiring::new(opts)?;
        let access = SimpleAccess::new(wiring.sessions(), &cfg.admins)?;
        for g in &cfg.grants {
            access.dev_grant(&g.principal, "*", g.role).await.map_err(anyhow::Error::msg)?;
        }
        let wired = wiring.finish(access.clone(), None)?;
        Ok(Self { wired, access })
    }

    /// What tether's service's `auth` activation and gate are built from.
    pub fn auth_setup(&self) -> AuthSetup {
        self.access.auth_setup(self.wired.backend.clone())
    }

    /// The upgrade's half on a transport builder.
    pub fn apply<H: Activation>(&self, builder: TransportServerBuilder<H>) -> TransportServerBuilder<H> {
        self.wired.apply(builder)
    }

    /// Seed the roles `service`'s activations declare.
    pub fn seed(&self, service: &DynamicHub) {
        self.access.seed_roles(service);
    }
}
