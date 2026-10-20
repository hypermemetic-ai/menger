//! DIST-14: `menger` with no flags — the first run, which just works.
//!
//! Still never open mode (ACCESS-24). With no login provider and every
//! listener on loopback, menger signs its one person in by itself, the way
//! Jupyter does:
//!
//! - a **local admin key** is made on the first start, in the data
//!   directory (`local.key`, 0600, synapse's format — a base64 Ed25519
//!   seed), and its principal `key:<hex>` is the server's `--admin`; the
//!   `key` provider is on, so synapse signs in with it too;
//! - each start prints a **one-time sign-in link**,
//!   `http://127.0.0.1:<shell>/login?once=<token>` — single use, ten
//!   minutes, taken only from loopback ([`plexus_login::once`]); opening it
//!   signs the browser in as the local admin;
//! - `menger login` prints a fresh one for the server running on that data
//!   directory.
//!
//! The links are files, so the server and `menger login` share them without
//! a method on the wire: `once/<sha256 of the token>` in the data directory
//! (0700), each holding its expiry. Taking one is removing its file, which
//! succeeds once. The server writes where it listens to `local.json`, which
//! `menger login` reads.
//!
//! Beyond loopback (`--bind`, a `--public-url` off this machine) with no
//! provider, menger refuses, saying which flag to add ([`refusal`]).

use crate::login::Provider;
use plexus_core::plexus::AuthContext;
use sha2::Digest;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// How long a one-time link lasts.
pub const ONCE_SECS: i64 = 10 * 60;

/// The local admin key's file in the data directory.
pub const KEY_FILE: &str = "local.key";

/// The database a first run keeps, in the data directory. Never prod's
/// `tickets.db`: an install of menger on a machine that runs prod does not
/// reach its data.
pub const DB_FILE: &str = "local.db";

/// Where the running server says it listens.
const RUNNING_FILE: &str = "local.json";

/// The default data directory: `~/.local/share/menger` on Linux
/// (`$XDG_DATA_HOME/menger`), the platform's data directory elsewhere.
pub fn default_data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("menger")
}

/// How this start signs people in.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// The providers the flags name.
    Providers,
    /// None named, on loopback: the local admin and one-time links.
    Local,
}

/// Whether a server bound at `bind`, reached at `public_url`, is reached
/// from beyond this machine.
pub fn beyond_loopback(bind: IpAddr, public_url: Option<&str>) -> bool {
    let off = public_url
        .and_then(|u| url::Url::parse(u).ok())
        .and_then(|u| {
            u.host().map(|h| match h {
                url::Host::Domain(d) => !d.eq_ignore_ascii_case("localhost"),
                url::Host::Ipv4(ip) => !ip.is_loopback(),
                url::Host::Ipv6(ip) => !ip.is_loopback(),
            })
        })
        .unwrap_or(false);
    !bind.is_loopback() || off
}

/// How this start signs people in, or why it does not start: with a
/// provider (or `--identity`), the providers; with none, on loopback, the
/// local admin; with none, reached beyond this machine, a refusal naming
/// the flag to add. `--stdio` is refused either way.
pub fn mode(providers: &[Provider], identity: bool, stdio: bool, bind: IpAddr, public_url: Option<&str>) -> Result<Mode, String> {
    // ACCESS-24: stdio carries no credential.
    if stdio {
        return Err("--stdio is not supported: nothing on stdio can prove who is calling.\n\
                    Reach menger over its WebSocket instead, e.g. `synapse -P <port> menger`."
            .into());
    }
    if !providers.is_empty() || identity {
        return Ok(Mode::Providers);
    }
    if beyond_loopback(bind, public_url) {
        // ACCESS-24: no open mode — beyond this machine, someone must be
        // able to sign in, and the local link is this machine's alone.
        let what = match public_url {
            Some(u) if bind.is_loopback() => format!("--public-url {u}"),
            _ => format!("--bind {bind}"),
        };
        return Err(format!(
            "can't be reached from other machines ({what}) without a way to sign in.\n\
             Add a login provider, for example:\n\
             \x20 menger {what} --auth-provider workos=<client-id> --admin <your principal>\n\
             or leave out {} to run on this machine only (it then signs you in with a one-time link).",
            what.split(' ').next().unwrap_or("--bind")
        ));
    }
    Ok(Mode::Local)
}

/// The data directory, made (0700) when missing.
pub fn ensure_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !dir.is_dir() {
        std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("can't make the data directory {}: {e}", dir.display()))?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// The local admin's principal, `key:<hex>`: the key in `dir/local.key`,
/// made on the first start (a fresh seed, base64 as synapse writes it,
/// 0600, made new — never over another).
pub fn local_admin(dir: &Path) -> anyhow::Result<String> {
    use base64::Engine as _;
    use std::os::unix::fs::OpenOptionsExt;
    ensure_dir(dir)?;
    let file = dir.join(KEY_FILE);
    let b64 = base64::engine::general_purpose::STANDARD;
    let seed: [u8; 32] = if file.exists() {
        let text = std::fs::read_to_string(&file).map_err(|e| anyhow::anyhow!("can't read {}: {e}", file.display()))?;
        b64.decode(text.trim())
            .ok()
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| anyhow::anyhow!("{} is not a key (32 bytes, base64); move it aside and start again for a new one", file.display()))?
    } else {
        let seed: [u8; 32] = rand::random();
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .map_err(|e| anyhow::anyhow!("can't write {}: {e}", file.display()))?;
        f.write_all(b64.encode(seed).as_bytes())?;
        f.write_all(b"\n")?;
        seed
    };
    let public = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    Ok(plexus_auth_core::adapters::key_principal(&public).to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn hash(token: &str) -> String {
    sha2::Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The one-time links of a data directory, signing in `principal`.
pub struct OnceLinks {
    dir: PathBuf,
    principal: String,
}

impl OnceLinks {
    /// The links in `data_dir/once`, each signing in `principal`.
    pub fn new(data_dir: &Path, principal: String) -> Self {
        Self { dir: data_dir.join("once"), principal }
    }

    /// A fresh token, good once for [`ONCE_SECS`]; the expired ones are
    /// cleared on the way.
    pub fn mint(&self) -> anyhow::Result<String> {
        mint_in(&self.dir)
    }
}

/// A fresh token in `dir` (made 0700 when missing).
fn mint_in(dir: &Path) -> anyhow::Result<String> {
    use base64::Engine as _;
    use std::os::unix::fs::OpenOptionsExt;
    ensure_dir(dir)?;
    let t = now();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let stale = std::fs::read_to_string(e.path()).ok().and_then(|s| s.trim().parse::<i64>().ok()).is_none_or(|exp| exp <= t);
            if stale {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(dir.join(hash(&token)))?;
    write!(f, "{}", t + ONCE_SECS)?;
    Ok(token)
}

impl plexus_login::once::OnceLinks for OnceLinks {
    fn redeem(&self, token: &str) -> Option<AuthContext> {
        // A token is URL-safe base64 of 32 bytes; anything else names no file.
        if token.len() != 43 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return None;
        }
        let file = self.dir.join(hash(token));
        let expires = std::fs::read_to_string(&file).ok()?.trim().parse::<i64>().ok()?;
        // Taking it is removing it: of two at once, one succeeds.
        std::fs::remove_file(&file).ok()?;
        if expires <= now() {
            return None;
        }
        let meta = serde_json::json!({ "principal": self.principal, "name": "local admin", "provider": "local", "auth_method": "once" });
        Some(AuthContext::new(self.principal.clone(), "once".into(), vec![], meta))
    }

    fn spent(&self) -> String {
        "This sign-in link was already used or has expired. Run `menger login` for a new one.".into()
    }

    fn hint(&self) -> String {
        "Sign in with the link menger printed when it started, or run `menger login` for a new one.".into()
    }
}

/// Where a running server listens, as `local.json` keeps it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Running {
    pub pid: u32,
    pub ws_port: u16,
    pub shell_port: u16,
}

impl Running {
    /// Written to `dir/local.json` when the server listens.
    pub fn write(&self, dir: &Path) -> anyhow::Result<()> {
        let tmp = dir.join(format!("{RUNNING_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, dir.join(RUNNING_FILE))?;
        Ok(())
    }

    /// What `dir/local.json` says, if anything.
    pub fn read(dir: &Path) -> Option<Running> {
        serde_json::from_slice(&std::fs::read(dir.join(RUNNING_FILE)).ok()?).ok()
    }
}

/// The one-time link to `shell_port`'s login page.
pub fn link(shell_port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{shell_port}/login?once={token}")
}

/// `menger login`: a fresh link for the server running on `data_dir`, or
/// why there is none.
pub fn login(data_dir: &Path) -> Result<String, String> {
    let not_running = || format!("menger isn't running on {}. Start it with `menger`.", shown(data_dir));
    let running = Running::read(data_dir).ok_or_else(not_running)?;
    if std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], running.shell_port).into(), std::time::Duration::from_secs(2)).is_err() {
        return Err(not_running());
    }
    if !data_dir.join(KEY_FILE).exists() {
        return Err(format!(
            "the menger running on {} signs people in with a login provider, not a one-time link: open http://127.0.0.1:{}/",
            shown(data_dir),
            running.shell_port
        ));
    }
    let token = mint_in(&data_dir.join("once")).map_err(|e| format!("can't make a sign-in link in {}: {e}", shown(data_dir)))?;
    Ok(link(running.shell_port, &token))
}

/// A path as a person reads it: under the home directory, `~/…`.
pub fn shown(path: &Path) -> String {
    match dirs::home_dir().and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// A port to listen on at `bind`: `asked` when given (taken or not —
/// the caller's bind says), else the first free one from `default`, never
/// one in `taken`. Whether it moved off the default.
pub fn pick_port(bind: IpAddr, asked: Option<u16>, default: u16, taken: &[u16]) -> anyhow::Result<(u16, bool)> {
    if let Some(p) = asked {
        return Ok((p, false));
    }
    for p in default..default.saturating_add(100) {
        if !taken.contains(&p) && std::net::TcpListener::bind((bind, p)).is_ok() {
            return Ok((p, p != default));
        }
    }
    anyhow::bail!("no free port from {default} to {}: give one with --port / --shell-port", default.saturating_add(99))
}

/// What a start prints, in this order: that it runs, where to open it
/// (`once`: a one-time link), the data, how to stop it — and a port that
/// moved, said.
pub fn banner(version: &str, url: &str, once: bool, db: &Path, moved: &[String]) -> String {
    let note = if once { "   (one-time; `menger login` for another)" } else { "" };
    let mut out = format!("menger {version} — running\n  open:  {url}{note}\n  data:  {}\n  stop:  Ctrl-C\n", shown(db));
    for m in moved {
        out.push_str(&format!("  note:  {m}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexus_login::once::OnceLinks as _;

    fn lo() -> IpAddr {
        "127.0.0.1".parse().unwrap()
    }

    #[test]
    fn no_provider_on_loopback_is_the_local_mode_and_beyond_it_a_refusal_naming_the_flag() {
        assert_eq!(mode(&[], false, false, lo(), None), Ok(Mode::Local));
        assert_eq!(mode(&[], false, false, lo(), Some("http://localhost:4456")), Ok(Mode::Local));
        let key: Vec<Provider> = vec!["key".parse().unwrap()];
        assert_eq!(mode(&key, false, false, "0.0.0.0".parse().unwrap(), None), Ok(Mode::Providers));
        assert_eq!(mode(&[], true, false, lo(), None), Ok(Mode::Providers), "--identity is a provider");

        let bound = mode(&[], false, false, "0.0.0.0".parse().unwrap(), None).unwrap_err();
        assert!(bound.contains("--bind 0.0.0.0") && bound.contains("--auth-provider"), "{bound}");
        let public = mode(&[], false, false, lo(), Some("https://tickets.example")).unwrap_err();
        assert!(public.contains("--public-url https://tickets.example") && public.contains("--auth-provider"), "{public}");
        let stdio = mode(&key, false, true, lo(), None).unwrap_err();
        assert!(stdio.contains("--stdio"), "{stdio}");
        for m in [bound, public, stdio] {
            assert!(!["ACCESS-", "DIST-", "RUN-"].iter().any(|id| m.contains(id)), "no ticket ids: {m}");
        }
    }

    #[test]
    fn the_local_admin_key_is_made_once_0600_and_read_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("menger");
        let a = local_admin(&data).unwrap();
        assert!(a.starts_with("key:") && a.len() == 4 + 64, "{a}");
        assert_eq!(std::fs::metadata(data.join(KEY_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&data).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(local_admin(&data).unwrap(), a, "the same key every start");
        std::fs::write(data.join(KEY_FILE), "not a key").unwrap();
        assert!(local_admin(&data).unwrap_err().to_string().contains("is not a key"));
    }

    #[test]
    fn a_link_signs_in_once_and_never_after_it_expires() {
        let dir = tempfile::tempdir().unwrap();
        let admin = format!("key:{}", "ab".repeat(32));
        let links = OnceLinks::new(dir.path(), admin.clone());
        let t = links.mint().unwrap();
        let ctx = links.redeem(&t).expect("the first use signs in");
        assert_eq!(ctx.principal().map(|p| p.to_string()), Some(admin));
        assert!(links.redeem(&t).is_none(), "a second use is refused");
        assert!(links.redeem("../../local.key").is_none());
        assert!(links.redeem("").is_none());

        let old = links.mint().unwrap();
        std::fs::write(dir.path().join("once").join(hash(&old)), (now() - 1).to_string()).unwrap();
        assert!(links.redeem(&old).is_none(), "expired");
        let stale = links.mint().unwrap();
        std::fs::write(dir.path().join("once").join(hash(&stale)), (now() - 1).to_string()).unwrap();
        links.mint().unwrap();
        assert!(!dir.path().join("once").join(hash(&stale)).exists(), "a mint clears the expired");
    }

    #[test]
    fn login_needs_a_running_server_on_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(login(dir.path()).unwrap_err().contains("isn't running"));
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        Running { pid: 1, ws_port: 1, shell_port: port }.write(dir.path()).unwrap();
        assert!(login(dir.path()).unwrap_err().contains("login provider"), "no local key: a provider's server");
        local_admin(dir.path()).unwrap();
        let link = login(dir.path()).unwrap();
        assert!(link.starts_with(&format!("http://127.0.0.1:{port}/login?once=")), "{link}");
        drop(l);
    }

    #[test]
    fn a_port_given_is_kept_and_a_default_that_is_taken_moves_on() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = held.local_addr().unwrap().port();
        assert_eq!(pick_port(lo(), Some(busy), 1, &[]).unwrap(), (busy, false), "asked: kept, the bind says");
        let (p, moved) = pick_port(lo(), None, busy, &[]).unwrap();
        assert!(p > busy && moved, "{p}");
        let (q, _) = pick_port(lo(), None, busy, &[p]).unwrap();
        assert!(q != p && q != busy);
    }

    #[test]
    fn the_banner_says_it_runs_the_link_the_data_and_how_to_stop() {
        let b = banner("0.6.1", "http://127.0.0.1:4456/login?once=x", true, Path::new("/data/local.db"), &[]);
        let lines: Vec<&str> = b.lines().collect();
        assert_eq!(lines[0], "menger 0.6.1 — running");
        assert!(lines[1].trim_start().starts_with("open:  http://127.0.0.1:4456/login?once=x"));
        assert!(lines[2].contains("data:  /data/local.db"));
        assert!(lines[3].contains("stop:  Ctrl-C"));
        assert!(lines[1].contains("`menger login` for another"));
        let p = banner("0.6.1", "https://t.example/", false, Path::new("/d.db"), &["port 4455 was taken: the WebSocket is on 4457".into()]);
        assert!(p.contains("open:  https://t.example/\n") && p.contains("note:  port 4455 was taken"), "{p}");
    }
}
