//! TETHER-11: `tether pair` — enrol this machine's key with a remote menger,
//! so no one copies a `key:<hex>` around by hand. A subcommand of the
//! binary, run before the server ever is:
//!
//! 1. the machine's Ed25519 key — synapse's file store,
//!    `~/.plexus/keys/<name>.key` (0600), made when there is none; the seed
//!    never leaves the file;
//! 2. a sign-in to the remote with it and one `members.mine`: a key that
//!    holds nothing joins the remote's waiting list (`members.requests`)
//!    under its name (`principal_names`, CHAT-14);
//! 3. the pairing code ([`menger_tickets_api::pairing`]) and the URL of the
//!    app's `#/pair` — the public key and the name, nothing secret — printed,
//!    and opened in the browser only with `--open` (DIST-14) and a display;
//! 4. `members.mine` again every few seconds until a grant of its own
//!    arrives (at most `--wait-secs`), then `paired: <name> → <role> on
//!    <scope>`, and `TETHER_REMOTE` / `TETHER_REMOTE_AS` written into
//!    `tether.env` where they are missing.
//!
//! TETHER-12: beside the URL, the URL as a QR code drawn in the terminal
//! ([`qr_lines`]), for a phone's camera — and the code and the key, for
//! one without: any one of the three pairs it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use menger_tickets_api::vocab::Grantee;
use menger_tickets_api::{Grant, MembersClient, MembersEvent};

use crate::remote::Remote;

/// Where the remote's socket and its app are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    /// The socket tether signs in to (`wss://app.menger.sh/ws`).
    pub ws: String,
    /// The app's origin (`https://app.menger.sh`), where `#/pair` is.
    pub app: String,
}

/// The remote as given — the app's `https://` origin (its socket is
/// `/ws`), or the socket itself (`wss://…/ws`; its app the same host) —
/// and `--app` when the app is elsewhere (a local menger's shell port).
pub fn endpoints(remote: &str, app: Option<&str>) -> Result<Endpoints, String> {
    let r = remote.trim().trim_end_matches('/');
    let (ws, derived) = if let Some(host) = r.strip_prefix("https://") {
        (format!("wss://{host}/ws"), format!("https://{host}"))
    } else if let Some(host) = r.strip_prefix("http://") {
        (format!("ws://{host}/ws"), format!("http://{host}"))
    } else if let Some(rest) = r.strip_prefix("wss://") {
        (r.to_string(), format!("https://{}", rest.strip_suffix("/ws").unwrap_or(rest)))
    } else if let Some(rest) = r.strip_prefix("ws://") {
        (r.to_string(), format!("http://{}", rest.strip_suffix("/ws").unwrap_or(rest)))
    } else {
        return Err(format!("the remote {r}: give the app's https:// address (https://app.menger.sh) or its socket (wss://…/ws)"));
    };
    let app = match app {
        Some(a) => {
            let a = a.trim().trim_end_matches('/');
            if !(a.starts_with("https://") || a.starts_with("http://")) {
                return Err(format!("--app {a}: an http(s):// origin"));
            }
            a.to_string()
        }
        None => derived,
    };
    Ok(Endpoints { ws, app })
}

/// A machine's name: what it signs in as (synapse's `--as`), its key's file
/// name, and what the app shows — letters, digits, `-`, `_`, `.`, at most
/// 64, so it goes in a URL and a file name as it is.
pub fn check_name(name: &str) -> Result<String, String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with(['.', '-']);
    ok.then(|| name.to_string()).ok_or_else(|| format!("the name `{name}`: letters, digits, - _ . (at most 64; not starting with . or -)"))
}

/// The default name: this machine's host name, made a [`check_name`] one.
pub fn default_name() -> String {
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::process::Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()))
        .unwrap_or_default();
    let name: String = host.trim().split('.').next().unwrap_or("").chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' }).take(64).collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        "tether".into()
    } else {
        name
    }
}

/// The page that grants it: the public key and the name, nothing else.
pub fn pair_url(app: &str, principal: &str, name: &str) -> String {
    format!("{app}/#/pair?principal={principal}&name={name}")
}

/// The columns a QR code may take: a terminal's 80.
pub const QR_COLUMNS: usize = 80;

/// How [`qr_lines`] paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// Black on white, said in ANSI colours: right on any terminal.
    Ansi,
    /// No colours: the light modules drawn in the terminal's ink, for the
    /// usual light-on-dark terminal (a log file reads it inverted).
    Plain,
}

/// TETHER-12: `text` as a QR code in Unicode half blocks (`▀ ▄ █`), two
/// rows of modules to a line and one column each, with the four-module
/// quiet zone a camera needs drawn in. Medium error correction, down to Low
/// when that is what fits [`QR_COLUMNS`]; an error when nothing does.
pub fn qr_lines(text: &str, paint: Paint) -> Result<Vec<String>, String> {
    use qrcode::{Color, EcLevel, QrCode};
    const QUIET: usize = 4;
    let code = [EcLevel::M, EcLevel::L]
        .into_iter()
        .filter_map(|ec| QrCode::with_error_correction_level(text, ec).ok())
        .find(|c| c.width() + 2 * QUIET <= QR_COLUMNS)
        .ok_or_else(|| format!("{} characters do not fit a QR code {QR_COLUMNS} columns wide", text.len()))?;
    let (w, colors) = (code.width(), code.to_colors());
    let side = w + 2 * QUIET;
    let dark = |x: usize, y: usize| -> bool {
        (QUIET..QUIET + w).contains(&x) && (QUIET..QUIET + w).contains(&y) && colors[(y - QUIET) * w + (x - QUIET)] == Color::Dark
    };
    // What the ink draws: the dark modules on white, the light ones on a dark terminal.
    let ink = |x: usize, y: usize| y < side && (dark(x, y) == (paint == Paint::Ansi));
    Ok((0..side.div_ceil(2))
        .map(|row| {
            let line: String = (0..side)
                .map(|x| match (ink(x, 2 * row), ink(x, 2 * row + 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                })
                .collect();
            match paint {
                Paint::Ansi => format!("\x1b[30;107m{line}\x1b[0m"),
                Paint::Plain => line,
            }
        })
        .collect())
}

/// `app.menger.sh/#/pair` — where the code is typed, as a person reads it.
pub fn typing_place(app: &str) -> String {
    let host = app.split_once("://").map_or(app, |(_, h)| h);
    format!("{host}/#/pair")
}

/// What opens a URL in the browser here, if anything can: `open` on macOS,
/// the URL handler on Windows (not `cmd /c start`, which reads `&` in the
/// URL as its own), `xdg-open` elsewhere — and nothing on a Linux or BSD
/// with no display (SSH, headless), where the URL is only printed. `env`
/// reads a variable (the process's, or a test's).
pub fn opener(os: &str, env: impl Fn(&str) -> Option<String>) -> Option<(String, Vec<String>)> {
    match os {
        "macos" => Some(("open".into(), vec![])),
        "windows" => Some(("rundll32".into(), vec!["url.dll,FileProtocolHandler".into()])),
        _ => {
            let shown = |v: &str| env(v).is_some_and(|s| !s.trim().is_empty());
            (shown("DISPLAY") || shown("WAYLAND_DISPLAY")).then(|| ("xdg-open".into(), vec![]))
        }
    }
}

/// DIST-14: what opens the pairing page — nothing unless `--open` asked,
/// then [`opener`]'s choice.
pub fn opener_for(asked: bool, os: &str, env: impl Fn(&str) -> Option<String>) -> Option<(String, Vec<String>)> {
    asked.then(|| opener(os, env)).flatten()
}

/// Open `url` with `opener`; whether it started. The child is reaped on a
/// thread of its own, its output dropped.
fn open(opener: &(String, Vec<String>), url: &str) -> bool {
    let child = std::process::Command::new(&opener.0)
        .args(&opener.1)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match child {
        Ok(mut c) => {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// The key in `file`, made when there is none: a fresh seed written as
/// synapse writes one (base64, 0600, made new — never over another).
/// Whether it was made.
pub fn ensure_key(file: &Path) -> anyhow::Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;
    if file.exists() {
        crate::remote::read_seed(file)?;
        return Ok(false);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let mut seed = [0u8; 32];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut seed)?;
    use base64::Engine as _;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(file).map_err(|e| anyhow::anyhow!("the key {}: {e}", file.display()))?;
    f.write_all(base64::engine::general_purpose::STANDARD.encode(seed).as_bytes())?;
    f.write_all(b"\n")?;
    Ok(true)
}

/// What [`fill_env`] did with each variable.
#[derive(Debug, PartialEq, Eq)]
pub enum EnvLine {
    /// Written, as it was missing.
    Wrote(String),
    /// Already set there (to what), and left.
    Kept(String, String),
}

/// `vars` written into the env file at `path` where they are missing (an
/// uncommented `NAME=` line is there); a file that does not exist is made
/// from `deploy/tether.env.example` (0600), as `install.sh` makes it, with
/// these values in it.
pub fn fill_env(path: &Path, vars: &[(&str, &str)], home: Option<&str>) -> anyhow::Result<Vec<EnvLine>> {
    let set = |text: &str, name: &str| -> Option<String> {
        text.lines().find_map(|l| l.trim().strip_prefix(name).and_then(|r| r.strip_prefix('=')).map(str::to_string))
    };
    if !path.exists() {
        let mut text = include_str!("../deploy/tether.env.example").to_string();
        if let Some(h) = home {
            text = text.replace("/home/hypermemetic", h);
        }
        let mut out = String::new();
        for line in text.lines() {
            let replaced = vars.iter().find(|(n, _)| line.strip_prefix(n).is_some_and(|r| r.starts_with('='))).map(|(n, v)| format!("{n}={v}"));
            out.push_str(&replaced.unwrap_or_else(|| line.to_string()));
            out.push('\n');
        }
        for (n, v) in vars {
            if set(&out, n).is_none() {
                out.push_str(&format!("{n}={v}\n"));
            }
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?.write_all(out.as_bytes())?;
        return Ok(vars.iter().map(|(n, _)| EnvLine::Wrote(n.to_string())).collect());
    }
    let text = std::fs::read_to_string(path)?;
    let mut add = String::new();
    let mut said = Vec::new();
    for (n, v) in vars {
        match set(&text, n) {
            Some(was) => said.push(EnvLine::Kept(n.to_string(), was)),
            None => {
                add.push_str(&format!("{n}={v}\n"));
                said.push(EnvLine::Wrote(n.to_string()));
            }
        }
    }
    if !add.is_empty() {
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        if !text.is_empty() && !text.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        f.write_all(format!("# TETHER-11: tether pair\n{add}").as_bytes())?;
    }
    Ok(said)
}

/// `~/.config/menger/tether.env`.
pub fn default_env_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/menger/tether.env"))
}

/// A pairing, as `tether pair` is given it.
#[derive(Clone, Debug)]
pub struct Config {
    pub endpoints: Endpoints,
    /// The machine's name: its agent on the remote and its key's name.
    pub name: String,
    /// Its key's file (`~/.plexus/keys/<name>.key`).
    pub key_file: PathBuf,
    /// DIST-14: open the browser too (`--open`); the URL is printed either way.
    pub open: bool,
    /// How long to wait for the grant.
    pub wait: Duration,
    /// How often to ask whether it came.
    pub poll: Duration,
    /// Where `TETHER_REMOTE` / `TETHER_REMOTE_AS` go; none: nowhere.
    pub env_file: Option<PathBuf>,
    /// How the QR code is painted (TETHER-12): ANSI on a terminal.
    pub paint: Paint,
}

/// The grants a `members.mine` names for the machine itself — its own and
/// its teams', never everyone's (ACCESS-57 grants every signed-in key,
/// paired or not).
fn own_grants(events: Vec<MembersEvent>) -> Result<Vec<Grant>, String> {
    let mut grants = Vec::new();
    for e in events {
        match e {
            MembersEvent::Grant { grant } if !matches!(grant.principal, Grantee::Everyone) => grants.push(grant),
            MembersEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(grants)
}

async fn mine(remote: &Remote) -> Result<Vec<Grant>, String> {
    let t = remote.connect().await?;
    own_grants(MembersClient::new(t).mine_all().await.map_err(|e| e.to_string())?)
}

/// `paired: <name> → editor on PROJ, viewer on DOCS`.
pub fn paired_line(name: &str, grants: &[Grant]) -> String {
    let held: Vec<String> = grants.iter().map(|g| format!("{} on {}", g.role, g.scope)).collect();
    format!("paired: {name} → {}", held.join(", "))
}

/// Pair: everything the module's head says, written to `out`. Returns the
/// grants the machine holds once paired.
pub async fn run(cfg: &Config, out: &mut (dyn std::io::Write + Send)) -> anyhow::Result<Vec<Grant>> {
    let made = ensure_key(&cfg.key_file)?;
    let remote = Remote::new(&cfg.endpoints.ws, &cfg.name, &cfg.key_file)?;
    let principal = remote.principal().to_string();
    let code = menger_tickets_api::pairing::code(&principal).ok_or_else(|| anyhow::anyhow!("{principal} is not a key"))?;
    writeln!(out, "tether pair: this machine is {} ({principal}){}", cfg.name, if made { ", its key made just now" } else { "" })?;
    writeln!(out, "  file  {}", cfg.key_file.display())?;

    let already = mine(&remote).await.map_err(|e| anyhow::anyhow!("signing in to {}: {e}", cfg.endpoints.ws))?;
    let grants = if !already.is_empty() {
        writeln!(out, "{} already holds a grant there.", cfg.name)?;
        already
    } else {
        let url = pair_url(&cfg.endpoints.app, &principal, &cfg.name);
        match qr_lines(&url, cfg.paint) {
            Ok(qr) => {
                writeln!(out)?;
                for l in qr {
                    writeln!(out, "  {l}")?;
                }
                writeln!(out)?;
            }
            Err(e) => writeln!(out, "  (no QR code: {e})")?,
        }
        writeln!(out, "  code  {code}")?;
        writeln!(out, "  key   {principal}")?;
        writeln!(out, "  open  {url}")?;
        writeln!(out, "Any one of them works: scan, or go to {} and type the code (or paste the key).", typing_place(&cfg.endpoints.app))?;
        let how = opener_for(cfg.open, std::env::consts::OS, |v| std::env::var(v).ok());
        match how {
            Some(o) if open(&o, &url) => writeln!(out, "Opened in your browser: check the code there is {code}, then Pair.")?,
            _ => {
                if cfg.open {
                    writeln!(out, "(No browser to open here.)")?;
                }
                writeln!(out, "Open it on any device signed in to menger, check the code there is {code}, then Pair.")?
            }
        }
        writeln!(out, "Waiting for the grant (up to {} min)…", cfg.wait.as_secs().div_ceil(60))?;
        out.flush()?;
        let until = tokio::time::Instant::now() + cfg.wait;
        loop {
            tokio::time::sleep(cfg.poll).await;
            match mine(&remote).await {
                Ok(g) if !g.is_empty() => break g,
                Ok(_) => {}
                Err(e) => tracing::debug!("TETHER-11: asking again after: {e}"),
            }
            if tokio::time::Instant::now() >= until {
                anyhow::bail!("no grant came in {} s: run tether pair again when someone can grant it", cfg.wait.as_secs());
            }
        }
    };
    writeln!(out, "{}", paired_line(&cfg.name, &grants))?;
    if let Some(env) = &cfg.env_file {
        let home = std::env::var("HOME").ok();
        let said = fill_env(env, &[("TETHER_REMOTE", &cfg.endpoints.ws), ("TETHER_REMOTE_AS", &cfg.name)], home.as_deref())?;
        for s in said {
            match s {
                EnvLine::Wrote(n) => writeln!(out, "  wrote {n} to {}", env.display())?,
                EnvLine::Kept(n, v) => writeln!(out, "  kept  {n}={v} in {} (it was set)", env.display())?,
            }
        }
    }
    out.flush()?;
    Ok(grants)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    /// DIST-14: printed by default; opened only with `--open`.
    #[test]
    fn the_browser_opens_only_when_asked() {
        let display = env(&[("DISPLAY", ":0")]);
        assert!(opener_for(false, "linux", &display).is_none(), "the default prints and opens nothing");
        assert!(opener_for(false, "macos", env(&[])).is_none());
        assert_eq!(opener_for(true, "linux", &display).unwrap().0, "xdg-open");
        assert!(opener_for(true, "linux", env(&[])).is_none(), "asked, but no display: printed only");
    }

    #[test]
    fn the_browser_opens_only_where_there_is_one() {
        assert_eq!(opener("linux", env(&[("DISPLAY", ":0")])).unwrap().0, "xdg-open");
        assert_eq!(opener("linux", env(&[("WAYLAND_DISPLAY", "wayland-0")])).unwrap().0, "xdg-open");
        assert!(opener("linux", env(&[])).is_none(), "headless: print only");
        assert!(opener("linux", env(&[("SSH_CONNECTION", "1 2 3 4"), ("DISPLAY", "")])).is_none(), "over SSH, no display");
        assert!(opener("freebsd", env(&[])).is_none());
        assert_eq!(opener("macos", env(&[])).unwrap().0, "open");
        let (w, args) = opener("windows", env(&[])).unwrap();
        assert_eq!((w.as_str(), args), ("rundll32", vec!["url.dll,FileProtocolHandler".to_string()]));
    }

    #[test]
    fn the_remote_is_the_app_or_its_socket() {
        let e = endpoints("https://app.menger.sh/", None).unwrap();
        assert_eq!((e.ws.as_str(), e.app.as_str()), ("wss://app.menger.sh/ws", "https://app.menger.sh"));
        let e = endpoints("wss://app.menger.sh/ws", None).unwrap();
        assert_eq!((e.ws.as_str(), e.app.as_str()), ("wss://app.menger.sh/ws", "https://app.menger.sh"));
        let e = endpoints("ws://127.0.0.1:4460", Some("http://127.0.0.1:4461/")).unwrap();
        assert_eq!((e.ws.as_str(), e.app.as_str()), ("ws://127.0.0.1:4460", "http://127.0.0.1:4461"));
        assert!(endpoints("app.menger.sh", None).is_err());
        assert!(endpoints("https://a", Some("a")).is_err());
    }

    #[test]
    fn the_url_holds_the_public_key_and_the_name_only() {
        let p = "key:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        assert_eq!(pair_url("https://app.menger.sh", p, "alice-laptop"), format!("https://app.menger.sh/#/pair?principal={p}&name=alice-laptop"));
        // The code tether prints and #/pair shows, for the fixture both pin.
        assert_eq!(menger_tickets_api::pairing::code(p).as_deref(), Some("MMG4-2KLG"));
    }

    /// A drawn QR code read back as a camera would: each character two
    /// modules, scaled up, decoded by `rqrr`.
    fn scan(lines: &[String], paint: Paint) -> String {
        let plain: Vec<Vec<char>> = lines.iter().map(|l| l.trim_start_matches("\x1b[30;107m").trim_end_matches("\x1b[0m").chars().collect()).collect();
        let (w, h) = (plain[0].len(), plain.len() * 2);
        let ink = |x: usize, y: usize| match plain[y / 2][x] {
            '█' => true,
            '▀' => y % 2 == 0,
            '▄' => y % 2 == 1,
            _ => false,
        };
        const PX: usize = 6;
        let mut img = rqrr::PreparedImage::prepare_from_greyscale(w * PX, h * PX, |x, y| {
            let dark = ink(x / PX, y / PX) == (paint == Paint::Ansi);
            if dark { 0 } else { 255 }
        });
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1, "one code in the picture");
        grids[0].decode().expect("it decodes").1
    }

    #[test]
    fn the_qr_code_is_the_pair_url_and_fits_80_columns() {
        let p = "key:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let longest = "x".repeat(64);
        for (app, name) in [("https://app.menger.sh", "alice-laptop"), ("https://app.menger.sh", longest.as_str()), ("https://app-test.menger.sh", longest.as_str()), ("http://127.0.0.1:4461", "b")] {
            let url = pair_url(app, p, name);
            for paint in [Paint::Ansi, Paint::Plain] {
                let lines = qr_lines(&url, paint).unwrap();
                let cols = lines.iter().map(|l| l.trim_start_matches("\x1b[30;107m").trim_end_matches("\x1b[0m").chars().count()).max().unwrap();
                // Printed two in: the indent and the code inside 80.
                assert!(cols + 2 <= QR_COLUMNS, "{cols} columns for {url}");
                assert!(lines.len() <= 40, "{} lines", lines.len());
                assert_eq!(scan(&lines, paint), url, "{paint:?}");
            }
        }
        assert!(qr_lines(&"x".repeat(2000), Paint::Plain).is_err(), "too long to fit is said, never drawn wider");
    }

    #[test]
    fn nothing_secret_is_in_the_qr_code() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("m.key");
        ensure_key(&f).unwrap();
        let seed = crate::remote::read_seed(&f).unwrap();
        let remote = Remote::new("wss://app.menger.sh/ws", "m", &f).unwrap();
        let url = pair_url("https://app.menger.sh", remote.principal(), "m");
        let read = scan(&qr_lines(&url, Paint::Plain).unwrap(), Paint::Plain);
        use base64::Engine as _;
        let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!read.contains(&hex) && !read.contains(&base64::engine::general_purpose::STANDARD.encode(seed)), "{read}");
        assert_eq!(read, format!("https://app.menger.sh/#/pair?principal={}&name=m", remote.principal()), "the public key and the name only");
        assert_eq!(typing_place("https://app.menger.sh"), "app.menger.sh/#/pair");
    }

    #[test]
    fn a_name_goes_in_a_url_and_a_file_name_as_it_is() {
        assert!(check_name("alice-laptop").is_ok());
        assert!(check_name("build.box_2").is_ok());
        for bad in ["", "-x", ".x", "a b", "a/b", "a&b=c", &"x".repeat(65)] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_name(&default_name()).is_ok());
    }

    #[test]
    fn a_key_is_made_once_private_and_never_over_another() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("keys/m.key");
        assert!(ensure_key(&f).unwrap());
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        let seed = crate::remote::read_seed(&f).unwrap();
        assert!(!ensure_key(&f).unwrap(), "kept, not made again");
        assert_eq!(crate::remote::read_seed(&f).unwrap(), seed);
    }

    #[test]
    fn the_env_file_gets_what_it_misses() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("menger/tether.env");
        let vars = [("TETHER_REMOTE", "wss://m/ws"), ("TETHER_REMOTE_AS", "alice-laptop")];
        // None there: made from the example, these in it, the rest as install.sh writes it.
        assert_eq!(fill_env(&f, &vars, Some("/home/alice")).unwrap(), vec![EnvLine::Wrote("TETHER_REMOTE".into()), EnvLine::Wrote("TETHER_REMOTE_AS".into())]);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("\nTETHER_REMOTE=wss://m/ws\n") && text.contains("\nTETHER_REMOTE_AS=alice-laptop\n"), "{text}");
        assert!(text.contains("TETHER_WORK_DIR=/home/alice/") && !text.contains("TETHER_REMOTE_AS=builder"));
        // Set already: kept.
        std::fs::write(&f, "TETHER_PORT=4486\n# TETHER_REMOTE=commented\nTETHER_REMOTE_AS=builder").unwrap();
        let said = fill_env(&f, &vars, None).unwrap();
        assert_eq!(said, vec![EnvLine::Wrote("TETHER_REMOTE".into()), EnvLine::Kept("TETHER_REMOTE_AS".into(), "builder".into())]);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.ends_with("TETHER_REMOTE_AS=builder\n# TETHER-11: tether pair\nTETHER_REMOTE=wss://m/ws\n"), "{text}");
    }
}
