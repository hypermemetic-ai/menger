//! DIST-21: `menger start` — the line that adds menger to an agent, the
//! bridge that line runs, and a small prompt on a terminal.
//!
//! Still never open mode (ACCESS-24). An MCP client is given a command, not
//! a URL: `claude mcp add --scope user menger -- menger mcp`. **`menger
//! mcp`** is an MCP server on stdin and stdout — plexus-transport-mcp's
//! bridge, the one `/mcp` is — whose upstream is the local menger's
//! WebSocket, opened as the local admin: a fresh key proof per connection,
//! signed with `local.key` in the data directory (0600, the person's own).
//! No token is printed, pasted or kept; a process that cannot read the key
//! reaches the auth door and nothing else, as before.
//!
//! - [`mcp_argv`] / [`mcp_line`]: the command, as the banner prints it.
//! - [`serve_mcp`]: `menger mcp`. [`LocalUpstream`] reads `local.json` at
//!   every (re)connect, so a restart or a moved port needs nothing from the
//!   agent.
//! - [`Key`], [`KEYS_LINE`], [`prompt`]: on a terminal, `o` + Enter opens the
//!   app, `a` + Enter runs the command.

use crate::first_run::{self, Running};
use plexus_rpc::client::{TransportErrorKind, WsTransport};
use plexus_rpc::transport::plexus_transport_mcp::upstream::BoxFuture;
use plexus_rpc::transport::plexus_transport_mcp::{serve_stdio, Credential, DynTransport, McpBridge, McpConfig, Upstream, UpstreamError};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The agent the command is written for, as its own CLI is called.
pub const AGENT_CLI: &str = "claude";

/// The command that adds this menger to Claude Code, as an argv:
/// `claude mcp add --scope user menger -- <menger> mcp [--data-dir DIR]`.
/// `menger`: how this binary is called (see [`called_as`]); `data_dir`: named
/// only when it is not the default, since the agent starts the bridge with
/// none of this shell's flags or environment.
pub fn mcp_argv(menger: &str, data_dir: Option<&Path>) -> Vec<String> {
    let mut argv: Vec<String> = [AGENT_CLI, "mcp", "add", "--scope", "user", "menger", "--", menger, "mcp"].map(String::from).into();
    if let Some(dir) = data_dir {
        argv.push("--data-dir".into());
        argv.push(dir.display().to_string());
    }
    argv
}

/// An argv as one shell line: a word of anything but plain characters in
/// single quotes.
pub fn shell_line(argv: &[String]) -> String {
    let plain = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=@%+,".contains(&b));
    argv.iter().map(|w| if plain(w) { w.clone() } else { format!("'{}'", w.replace('\'', "'\\''")) }).collect::<Vec<_>>().join(" ")
}

/// The banner's line.
pub fn mcp_line(argv: &[String]) -> String {
    format!("mcp:   {}", shell_line(argv))
}

/// `data_dir` when it is not the default one — what the command must name.
pub fn named_data_dir(data_dir: &Path) -> Option<PathBuf> {
    named_unless(data_dir, &first_run::default_data_dir())
}

/// `data_dir`, canonical, unless it is `default`.
fn named_unless(data_dir: &Path, default: &Path) -> Option<PathBuf> {
    let same = match (std::fs::canonicalize(data_dir), std::fs::canonicalize(default)) {
        (Ok(a), Ok(b)) => a == b,
        _ => data_dir == default,
    };
    (!same).then(|| std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf()))
}

/// The first `name` on `path` (a `PATH` value) that is a file.
pub fn on_path(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// How a command should call this binary: `menger` when that name on `PATH`
/// is this very file, else the file's own path — so the line works from the
/// agent's shell whether or not menger was installed onto `PATH`.
pub fn called_as(exe: &Path, path: Option<&std::ffi::OsStr>) -> String {
    let this = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    match on_path("menger", path).and_then(|p| std::fs::canonicalize(p).ok()) {
        Some(found) if found == this => "menger".into(),
        _ => this.display().to_string(),
    }
}

/// The local menger's WebSocket, opened as the local admin. One socket, kept
/// while it lives; a closed one is opened again — from `local.json` as it
/// then is, with a proof signed then.
pub struct LocalUpstream {
    data_dir: PathBuf,
    open: tokio::sync::Mutex<Option<WsTransport>>,
}

impl LocalUpstream {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into(), open: tokio::sync::Mutex::new(None) }
    }

    async fn connect(&self) -> Result<WsTransport, UpstreamError> {
        let running = running(&self.data_dir).map_err(UpstreamError::Unreachable)?;
        let seed = first_run::local_seed(&self.data_dir).map_err(|e| UpstreamError::Unreachable(e.to_string()))?;
        let url = format!("ws://127.0.0.1:{}", running.ws_port);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        // A proof is one upgrade's: its nonce is new each time.
        let proof = plexus_rpc::auth_core::adapters::sign_key_proof(&seed, &url, "menger-mcp", &rand::random(), now);
        let headers = [("authorization".to_string(), format!("Bearer {proof}"))];
        WsTransport::connect_with_headers(&url, "menger", &headers).await.map_err(|e| match e.kind {
            TransportErrorKind::Http { status: 401 | 403 } => UpstreamError::Unauthorized,
            _ => UpstreamError::Unreachable(format!("{} (is menger running? `menger start`)", e.message)),
        })
    }
}

impl Upstream for LocalUpstream {
    fn service(&self) -> &str {
        "menger"
    }

    fn transport_for(&self, _credential: &Credential) -> BoxFuture<'_, Result<DynTransport, UpstreamError>> {
        Box::pin(async move {
            // One caller, so the lock is held across the connect: two first
            // requests open one socket.
            let mut open = self.open.lock().await;
            if let Some(t) = open.as_ref().filter(|t| !t.is_closed()) {
                return Ok(Arc::new(t.clone()) as DynTransport);
            }
            let t = self.connect().await?;
            *open = Some(t.clone());
            Ok(Arc::new(t) as DynTransport)
        })
    }
}

/// The local menger running on `data_dir`, or why there is none to bridge to.
pub fn running(data_dir: &Path) -> Result<Running, String> {
    let not_running = || format!("menger isn't running on {}. Start it with `menger start`.", first_run::shown(data_dir));
    let running = Running::read(data_dir).ok_or_else(not_running)?;
    if std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], running.ws_port).into(), std::time::Duration::from_secs(2)).is_err() {
        return Err(not_running());
    }
    if !data_dir.join(first_run::KEY_FILE).exists() {
        return Err(format!(
            "the menger running on {} signs people in with a login provider: there is no local key to reach it with",
            first_run::shown(data_dir)
        ));
    }
    Ok(running)
}

/// `menger mcp`: MCP on stdin and stdout until stdin closes, every call made
/// on the local menger as its admin. Refused up front, in a sentence, when
/// no local menger runs on `data_dir`.
pub async fn serve_mcp(data_dir: &Path) -> anyhow::Result<()> {
    running(data_dir).map_err(anyhow::Error::msg)?;
    let mut config = McpConfig::default();
    config.server.name = Some("menger".into());
    config.server.version = Some(env!("CARGO_PKG_VERSION").into());
    config.server.instructions = Some(format!("{} Every menger method is a tool; each call runs as this machine's local admin.", crate::discovery::DESCRIPTION));
    serve_stdio(McpBridge::new(LocalUpstream::new(data_dir), config)).await.map_err(|e| anyhow::anyhow!("{e}"))
}

/// What the prompt says it takes, under the banner.
pub const KEYS_LINE: &str = "  keys:  o + Enter opens the app · a + Enter adds menger to Claude Code";

/// The same where the start serves no MCP (`--no-mcp`).
pub const KEYS_LINE_OPEN: &str = "  keys:  o + Enter opens the app";

/// A line typed at the prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Key {
    /// `o`: open the app.
    Open,
    /// `a`: add menger to the agent.
    Add,
    /// Anything else, an empty line too: nothing.
    Other,
}

impl Key {
    pub fn parse(line: &str) -> Key {
        match line.trim().to_ascii_lowercase().as_str() {
            "o" | "open" => Key::Open,
            "a" | "add" => Key::Add,
            _ => Key::Other,
        }
    }
}

/// What `a` does: run the command when the agent's CLI is on `path`, else
/// say the line to run where it is. What it printed.
pub fn add(argv: &[String], path: Option<&std::ffi::OsStr>) -> String {
    let Some(cli) = on_path(&argv[0], path) else {
        return format!("  {} isn't on PATH here. Run this where it is:\n    {}", argv[0], shell_line(argv));
    };
    match std::process::Command::new(cli).args(&argv[1..]).stdin(std::process::Stdio::null()).output() {
        Ok(out) => {
            let said = [out.stdout, out.stderr].concat();
            let said = String::from_utf8_lossy(&said);
            let said: Vec<&str> = said.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
            let head = if out.status.success() { "added: start a new Claude Code session to use it" } else { "Claude Code did not add it" };
            std::iter::once(format!("  {head}")).chain(said.iter().map(|l| format!("    {l}"))).collect::<Vec<_>>().join("\n")
        }
        Err(e) => format!("  can't run {}: {e}", argv[0]),
    }
}

/// The prompt: each line of `input` until it ends, `o` answered by `open`
/// and `a` by `add`, what each says written to `out`. Blocking — the caller
/// gives it a thread.
pub fn prompt(input: impl std::io::BufRead, mut out: impl std::io::Write, mut open: impl FnMut() -> String, mut add: impl FnMut() -> String) {
    for line in input.lines().map_while(Result::ok) {
        let said = match Key::parse(&line) {
            Key::Open => open(),
            Key::Add => add(),
            Key::Other => continue,
        };
        if writeln!(out, "{said}").and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_adds_the_bridge_for_every_project_and_names_a_data_dir_that_is_not_the_default() {
        let argv = mcp_argv("menger", None);
        assert_eq!(mcp_line(&argv), "mcp:   claude mcp add --scope user menger -- menger mcp");
        let argv = mcp_argv("/opt/my tools/menger", Some(Path::new("/srv/menger")));
        assert_eq!(mcp_line(&argv), "mcp:   claude mcp add --scope user menger -- '/opt/my tools/menger' mcp --data-dir /srv/menger");
        assert_eq!(shell_line(&["it's".into()]), r"'it'\''s'");
    }

    #[test]
    fn the_binary_is_called_menger_only_when_that_name_on_path_is_this_file() {
        let dir = tempfile::tempdir().unwrap();
        let (bin, other) = (dir.path().join("bin"), dir.path().join("other"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(bin.join("menger"), "").unwrap();
        std::fs::write(other.join("menger"), "").unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        assert_eq!(called_as(&bin.join("menger"), Some(&path)), "menger");
        let elsewhere = called_as(&other.join("menger"), Some(&path));
        assert_eq!(elsewhere, std::fs::canonicalize(other.join("menger")).unwrap().display().to_string());
        assert_eq!(called_as(&other.join("menger"), None), elsewhere, "no PATH: the file's own path");
    }

    #[test]
    fn the_default_data_dir_is_not_named_and_another_is() {
        // The default is given, not read: HOME is another test's to set.
        let (default, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        assert_eq!(named_unless(default.path(), default.path()), None);
        assert_eq!(named_unless(&default.path().join("not-made-yet"), &default.path().join("not-made-yet")), None);
        assert_eq!(named_unless(other.path(), default.path()), Some(std::fs::canonicalize(other.path()).unwrap()));
    }

    #[test]
    fn the_prompt_takes_o_and_a_and_ignores_the_rest() {
        assert_eq!(Key::parse("o\n"), Key::Open);
        assert_eq!(Key::parse("  A "), Key::Add);
        assert_eq!(Key::parse("open"), Key::Open);
        for other in ["", "q", "oa", "yes"] {
            assert_eq!(Key::parse(other), Key::Other, "{other:?}");
        }
        let mut out = Vec::new();
        prompt(&b"\nx\no\na\n"[..], &mut out, || "  opened".into(), || "  added".into());
        assert_eq!(String::from_utf8(out).unwrap(), "  opened\n  added\n");
    }

    #[test]
    fn adding_without_the_agent_s_cli_says_the_line_to_run() {
        let argv = mcp_argv("menger", None);
        let empty = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([empty.path()]).unwrap();
        let said = add(&argv, Some(&path));
        assert!(said.contains("claude isn't on PATH") && said.ends_with("claude mcp add --scope user menger -- menger mcp"), "{said}");
    }

    #[test]
    fn no_server_on_the_data_dir_is_said_naming_menger_start() {
        let dir = tempfile::tempdir().unwrap();
        let why = running(dir.path()).unwrap_err();
        assert!(why.contains("isn't running") && why.contains("`menger start`"), "{why}");
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        Running { pid: 1, ws_port: l.local_addr().unwrap().port(), shell_port: 1 }.write(dir.path()).unwrap();
        assert!(running(dir.path()).unwrap_err().contains("login provider"), "no local key: a provider's server");
        first_run::local_admin(dir.path()).unwrap();
        assert!(running(dir.path()).is_ok());
    }
}
