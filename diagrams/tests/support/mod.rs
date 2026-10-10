//! What the diagrams service's test binaries share: a port, a server, a
//! connection, Node's presence, and a look at a process from outside.
#![allow(dead_code)] // each test binary uses its own part of this

use menger_diagrams::node::{Config, Driver};
use menger_diagrams::{build_hub, no_browser_origins, HUB};
use plexus_client::WsTransport;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// A port no other test in this binary has been handed (menger's MNG-19
/// rule: binding `:0` only suggests one, and the kernel can suggest it
/// twice).
pub async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// The service, in process, on its own port, around `driver` — which the
/// test keeps, to see and kill the Node program. Never registered with a
/// Plexus registry.
pub async fn start_server(driver: Driver) -> u16 {
    let port = free_port().await;
    let hub = build_hub(driver);
    let conv = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"))
    };
    tokio::spawn(async move {
        TransportServer::builder(hub, conv)
            .with_websocket(port)
            .with_allowed_origins(no_browser_origins())
            .without_registry()
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    port
}

/// One service for every test in the binary that only asks it things —
/// one Node program for all of them instead of one each. It lives on a
/// thread and runtime of its own for the binary's life (a test's runtime
/// ends with its test); at exit the program sees its stdin close and
/// exits. Tests reach it over the WebSocket only — the driver is returned
/// for its `pid()`, never to call from another runtime.
pub fn shared_service() -> (u16, Driver) {
    static SHARED: OnceLock<(u16, Driver)> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("a runtime for the shared service");
                runtime.block_on(async move {
                    let driver = Driver::new(Config::default());
                    let port = start_server(driver.clone()).await;
                    tx.send((port, driver)).expect("the test is waiting");
                    std::future::pending::<()>().await;
                });
            });
            rx.recv().expect("the shared service starts")
        })
        .clone()
}

pub async fn connect(port: u16) -> WsTransport {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..200 {
        if let Ok(transport) = WsTransport::connect(&url, HUB).await {
            return transport;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the service never came up on {url}");
}

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Node and jsdom, as the real program needs them — or why not. A test
/// that needs them prints this and returns: `cargo test` never needs Node
/// (the ticket's "must not change").
pub fn node_missing() -> Option<String> {
    if let Some(why) = node_binary_missing() {
        return Some(why);
    }
    let jsdom = Config::default_node_dir()
        .join("node_modules")
        .join("jsdom")
        .join("package.json");
    if !jsdom.is_file() {
        return Some(format!(
            "jsdom is not installed (`npm ci --prefix {}`)",
            Config::default_node_dir().display()
        ));
    }
    None
}

/// Node alone — for a test of what the program says when the rest is
/// missing.
pub fn node_binary_missing() -> Option<String> {
    match std::process::Command::new("node").arg("--version").output() {
        Ok(out) if out.status.success() => None,
        _ => Some("`node` is not on PATH".into()),
    }
}

/// Skips the calling test, saying so, when Node or jsdom is missing
/// (`needs_node!(without_jsdom)`: when Node is).
#[macro_export]
macro_rules! needs_node {
    () => {
        if let Some(why) = $crate::support::node_missing() {
            $crate::support::skipped(&why);
            return;
        }
    };
    (without_jsdom) => {
        if let Some(why) = $crate::support::node_binary_missing() {
            $crate::support::skipped(&why);
            return;
        }
    };
}

/// The skip notice, on the process's own stderr: libtest captures what
/// `eprintln!` writes from a passing test and shows none of it, and a skip
/// nobody sees is a pass nobody earned. The test thread carries the test's
/// name.
pub fn skipped(why: &str) {
    use std::io::Write;
    let name = std::thread::current()
        .name()
        .unwrap_or("a test")
        .to_string();
    let _ = writeln!(std::io::stderr(), "SKIPPED {name}: {why}");
}

/// Is `pid` a live process — not gone, not a zombie waiting to be reaped?
pub fn alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // The state is the first field after the parenthesized name.
        Ok(stat) => stat
            .rsplit_once(") ")
            .is_some_and(|(_, rest)| !rest.starts_with('Z')),
        Err(_) => false,
    }
}

/// Waits (up to `limit`) for `pid` to stop being alive.
pub async fn dies_within(pid: u32, limit: Duration) -> bool {
    let began = Instant::now();
    while began.elapsed() < limit {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    !alive(pid)
}

/// Has our child `pid` exited for good — every thread of it, so it can be
/// reaped? Looked at with `WNOWAIT`, which leaves it for whoever reaps it
/// (the driver's `try_wait`). A multi-threaded program (Node) shows as a
/// zombie in /proc while its other threads are still being torn down, and
/// until they are, nobody can reap it — "dead" in /proc is not yet "gone".
/// Already reaped by someone counts as gone.
pub fn exited_child(pid: u32) -> bool {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if r != 0 {
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD);
    }
    let exited = unsafe { info.si_pid() };
    exited == pid as libc::pid_t
}

/// Waits (up to `limit`) until our child `pid` has exited for good.
pub async fn exits_within(pid: u32, limit: Duration) -> bool {
    let began = Instant::now();
    while began.elapsed() < limit {
        if exited_child(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    exited_child(pid)
}

/// SIGKILL, from outside — as an operator, or the OOM killer, would.
pub fn kill9(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill");
    assert!(status.success(), "kill -9 {pid}");
}

/// The direct children of `pid` (Linux: every thread's `children` list).
pub fn children_of(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    if let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) {
        for task in tasks.flatten() {
            if let Ok(list) = std::fs::read_to_string(task.path().join("children")) {
                out.extend(
                    list.split_whitespace()
                        .filter_map(|p| p.parse::<u32>().ok()),
                );
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}
