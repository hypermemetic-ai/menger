//! TETHER-9: the remote menger, reached as a client. tether dials out with
//! an agent's key — the seed synapse keeps (`~/.plexus/keys/<name>.key`,
//! 0600, the 32-byte Ed25519 seed in base64) — signing a fresh key proof
//! for each connection's upgrade (`Authorization: Bearer …`, IDENTITY-40's
//! wire, as synapse and the chat bridge sign in). The seed is read from the
//! file, never from argv or the environment, and never said back.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use plexus_client::WsTransport;

/// The service a menger serves.
pub const SERVICE: &str = "menger";

/// A remote menger and the key tether signs in to it with.
#[derive(Clone)]
pub struct Remote {
    url: String,
    agent: String,
    seed: Arc<[u8; 32]>,
    principal: String,
}

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote").field("url", &self.url).field("agent", &self.agent).field("principal", &self.principal).finish_non_exhaustive()
    }
}

/// `~/.plexus/keys/<agent>.key`.
pub fn default_key_file(agent: &str) -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".plexus/keys").join(format!("{agent}.key")))
}

/// The seed in a key file: base64 (either alphabet) of 32 bytes. Refused
/// when anyone but its owner may read it.
pub fn read_seed(file: &Path) -> anyhow::Result<[u8; 32]> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(file).map_err(|e| anyhow::anyhow!("the key {}: {e}", file.display()))?;
    if meta.permissions().mode() & 0o077 != 0 {
        anyhow::bail!("the key {} is readable by others: chmod 600 it", file.display());
    }
    let text = std::fs::read_to_string(file).map_err(|e| anyhow::anyhow!("the key {}: {e}", file.display()))?;
    let t: String = text.trim().chars().map(|c| match c { '-' => '+', '_' => '/', c => c }).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(t.trim_end_matches('='))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(t.trim_end_matches('=')))
        .map_err(|_| anyhow::anyhow!("the key {} is not a base64 seed", file.display()))?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| anyhow::anyhow!("the key {} is not a 32-byte seed", file.display()))
}

fn nonce() -> [u8; 32] {
    let mut n = [0u8; 32];
    n[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    n[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    n
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Remote {
    /// The remote at `url` (`ws://` or `wss://`), signed in to as `agent`
    /// with the key in `key_file`.
    pub fn new(url: &str, agent: &str, key_file: &Path) -> anyhow::Result<Self> {
        let url = url.trim().trim_end_matches('/').to_string();
        if !(url.starts_with("ws://") || url.starts_with("wss://")) {
            anyhow::bail!("the remote {url}: a menger's socket is ws:// or wss://");
        }
        let seed = read_seed(key_file)?;
        let public = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key();
        let principal = key_principal(public.as_bytes()).to_string();
        Ok(Self { url, agent: agent.to_string(), seed: Arc::new(seed), principal })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The agent name it signs in as (synapse's `--as`).
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Who tether is on the remote: `key:<hex>`.
    pub fn principal(&self) -> &str {
        &self.principal
    }

    /// One signed-in connection; every client on it shares it.
    pub async fn connect(&self) -> Result<WsTransport, String> {
        let proof = sign_key_proof(&self.seed, &self.url, &self.agent, &nonce(), now());
        let headers = [("Authorization".to_string(), format!("Bearer {proof}"))];
        WsTransport::connect_with_headers(&self.url, SERVICE, &headers).await.map_err(|e| e.to_string())
    }

    /// Post `body` to `chat` as tether's key (a run's report).
    pub async fn post(&self, chat: u64, body: String) -> Result<(), String> {
        use menger_tickets_api::{ChatsClient, ChatsEvent};
        let t = self.connect().await?;
        let body = body.parse().map_err(|e: String| e)?;
        let said = ChatsClient::new(t).post_all(menger_tickets_api::vocab::ChatId::new(chat), body).await.map_err(|e| e.to_string())?;
        match said.into_iter().next() {
            Some(ChatsEvent::Error { message }) => Err(message),
            Some(_) => Ok(()),
            None => Err("menger answered nothing".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_file_is_a_private_base64_seed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("k.key");
        std::fs::write(&f, format!("{}\n", base64::engine::general_purpose::STANDARD.encode([7u8; 32]))).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_seed(&f).unwrap_err().to_string().contains("chmod 600"));
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_seed(&f).unwrap(), [7u8; 32]);
        let r = Remote::new("ws://127.0.0.1:1/", "tether", &f).unwrap();
        assert_eq!(r.url(), "ws://127.0.0.1:1");
        assert!(r.principal().starts_with("key:"));
        assert!(!format!("{r:?}").contains("seed"), "the seed is never said");
        assert!(Remote::new("http://x", "tether", &f).is_err());
        std::fs::write(&f, "c2hvcnQ=").unwrap();
        assert!(read_seed(&f).is_err());
    }
}
