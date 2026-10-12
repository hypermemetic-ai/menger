//! AUTH-22: plexus-idp as an [`IdentityBackend`] — the local stand-in
//! (AUTH-14: "plexus idp … should be used only for local testing").
//!
//! plexus-idp speaks OAuth's token endpoint with the `password` and
//! `refresh_token` grants and publishes discovery and a JWKS; it sends no
//! email and federates nothing. So this backend offers what it has:
//!
//! | capability | how |
//! | --- | --- |
//! | password | `POST <token_endpoint>` grant `password` (username, password, audience) |
//! | refresh | grant `refresh_token` (rotated: the new one replaces it) |
//! | sign-out | nothing to revoke at plexus-idp; the app's session ends |
//!
//! The page then draws only a username/password form. Tokens are verified
//! by the same plexus-idp OIDC adapter the gate uses (`idp:<uuid>`).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use plexus_auth_core::identity::{Authenticator, Presentation};
use serde_json::Value;

use crate::identity::{Capabilities, IdResult, IdentityBackend, IdentityError, RequestInfo, Secret, SignedIn, TokenSet, UserInfo};
use crate::login::Provider;
use crate::sessions::now;

/// The plexus-idp backend. See the module docs.
pub struct PlexusIdp {
    issuer: String,
    audience: String,
    provider: Provider,
    verifier: Arc<dyn Authenticator>,
    http: reqwest::Client,
    token_endpoint: Mutex<Option<String>>,
}

impl PlexusIdp {
    /// plexus-idp at `issuer`, its tokens minted for `audience`.
    pub fn new(issuer: &str, audience: &str) -> anyhow::Result<Self> {
        let provider: Provider = format!("idp={issuer},{audience}").parse().map_err(anyhow::Error::msg)?;
        let verifier = provider.adapter()?;
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(15)).build()?;
        Ok(Self { issuer: issuer.trim_end_matches('/').into(), audience: audience.into(), provider, verifier, http, token_endpoint: Mutex::new(None) })
    }

    async fn endpoint(&self) -> IdResult<String> {
        if let Some(e) = self.token_endpoint.lock().unwrap().clone() {
            return Ok(e);
        }
        let doc: Value = self
            .http
            .get(format!("{}/.well-known/openid-configuration", self.issuer))
            .send()
            .await
            .map_err(|_| IdentityError::Unavailable("plexus-idp is unreachable".into()))?
            .json()
            .await
            .map_err(|_| IdentityError::Unavailable("plexus-idp's discovery is unreadable".into()))?;
        let e = doc.get("token_endpoint").and_then(Value::as_str).ok_or_else(|| IdentityError::Unavailable("plexus-idp names no token endpoint".into()))?.to_string();
        *self.token_endpoint.lock().unwrap() = Some(e.clone());
        Ok(e)
    }

    async fn grant(&self, form: &[(&str, &str)], method: &str) -> IdResult<SignedIn> {
        let endpoint = self.endpoint().await?;
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let resp = self
            .http
            .post(endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|_| IdentityError::Unavailable("plexus-idp is unreachable".into()))?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let Some(access) = s("access_token") else {
            return Err(match (status.as_u16(), s("error").as_deref()) {
                (429, _) => IdentityError::RateLimited { retry_after: None },
                (_, Some("invalid_grant" | "invalid_request")) | (400 | 401 | 403, _) => IdentityError::InvalidCode,
                (_, Some("invalid_client" | "invalid_target")) => IdentityError::Misconfigured("plexus-idp refuses this audience".into()),
                (code, _) => IdentityError::Unavailable(format!("plexus-idp answered HTTP {code}")),
            });
        };
        let ctx = self
            .verifier
            .authenticate(&Presentation::Bearer { token: access.clone() })
            .await
            .map(|a| a.into_auth_context())
            .map_err(|_| IdentityError::Unavailable("plexus-idp's token did not verify".into()))?;
        let expires_at = ctx.metadata.get("expires_at").and_then(Value::as_i64).unwrap_or_else(|| now() + 300);
        let subject = ctx.user_id.clone();
        let name = ctx.get_metadata_string("name");
        Ok(SignedIn {
            user: UserInfo { subject, email: ctx.get_metadata_string("email"), name },
            tokens: Some(TokenSet { access: Secret(access), refresh: s("refresh_token").map(Secret), session: None, expires_at }),
            ctx,
            method: method.into(),
        })
    }
}

#[async_trait]
impl IdentityBackend for PlexusIdp {
    fn name(&self) -> &str {
        "plexus-idp"
    }

    fn for_development(&self) -> bool {
        true
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { password: true, refresh: true, ..Capabilities::default() }
    }

    fn gate_provider(&self) -> Option<Provider> {
        Some(self.provider.clone())
    }

    async fn password(&self, username: &str, password: &str, _req: &RequestInfo) -> IdResult<SignedIn> {
        self.grant(&[("grant_type", "password"), ("username", username), ("password", password), ("audience", &self.audience), ("scope", "openid")], "password").await
    }

    async fn refresh(&self, tokens: &TokenSet, _req: &RequestInfo) -> IdResult<SignedIn> {
        let Some(r) = &tokens.refresh else { return Err(IdentityError::InvalidCode) };
        self.grant(&[("grant_type", "refresh_token"), ("refresh_token", r.expose())], "refresh").await
    }
}
