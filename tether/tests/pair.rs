//! TETHER-11: `tether pair` against a throwaway menger — the machine signs
//! in with its key, joins the waiting list under its name, prints the code
//! and the #/pair URL (the public key and the name, nothing secret); an
//! admin grants it; tether sees the grant, says so, and writes tether.env.

mod support;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;
use support::*;

/// The machine's key — made beforehand, so the test knows its principal.
const MACHINE: [u8; 32] = [0x66; 32];

/// The next line `f` takes, every line read kept in `said`.
async fn next_until(lines: &mut tokio::sync::mpsc::UnboundedReceiver<String>, log: &std::path::Path, what: &str, f: &dyn Fn(&str) -> bool, said: &mut Vec<String>) -> String {
    loop {
        match tokio::time::timeout(Duration::from_secs(60), lines.recv()).await {
            Ok(Some(l)) => {
                said.push(l.clone());
                if f(&l) {
                    return l;
                }
            }
            _ => panic!("never said {what}; said: {said:#?}; log: {}", std::fs::read_to_string(log).unwrap_or_default()),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pair_waits_for_the_grant_an_admin_gives_and_writes_tether_env() {
    let tmp = scratch();
    let dir = tmp.path();
    let (_menger, mp) = menger(dir).await;
    let owner = signed_in(&OWNER, mp).await;
    one(&owner, "menger", "tickets.create", json!({ "epic": "TT", "title": "tether's tests", "kind": "scope" })).await;

    use base64::Engine as _;
    use std::os::unix::fs::PermissionsExt;
    let key = dir.join("keys/test-box.key");
    std::fs::create_dir_all(key.parent().unwrap()).unwrap();
    let seed = base64::engine::general_purpose::STANDARD.encode(MACHINE);
    std::fs::write(&key, &seed).unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let env = dir.join("config/tether.env");
    let principal = principal_of(&MACHINE);
    let remote = format!("ws://127.0.0.1:{mp}");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tether"));
    cmd.env("PLEXUS_NO_REGISTRY", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .args(["pair", "--remote", &remote, "--app", "http://127.0.0.1:9", "--name", "test-box", "--no-browser"])
        .args(["--poll-secs", "1", "--wait-secs", "90", "--key"])
        .arg(&key)
        .arg("--env-file")
        .arg(&env);
    let argv: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(!argv.iter().any(|a| a.contains(&seed)), "the seed is never on argv");
    assert!(cmd.get_envs().all(|(_, v)| v.is_none_or(|v| !v.to_string_lossy().contains(&seed))), "nor in the environment");
    let mut child = cmd.stdout(Stdio::piped()).stderr(std::fs::File::create(dir.join("pair.log")).unwrap()).spawn().expect("tether pair starts");
    let stdout = child.stdout.take().unwrap();
    let mut pair = Proc(child);
    let (tx, mut lines) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(l);
        }
    });
    let mut said = Vec::new();
    let log = dir.join("pair.log");
    // 1. It says who it is, the code and the URL — the public key and the name only.
    let code = menger_tickets_api::pairing::code(&principal).unwrap();
    let head = next_until(&mut lines, &log, "who it is", &|l| l.starts_with("tether pair:"), &mut said).await;
    assert!(head.contains(&principal) && head.contains("test-box") && !head.contains("made just now"), "{head}");
    let c = next_until(&mut lines, &log, "the code", &|l| l.trim_start().starts_with("code"), &mut said).await;
    assert_eq!(c.split_whitespace().last(), Some(code.as_str()));
    // TETHER-12: the QR code came before it, the key after it.
    assert!(said.iter().filter(|l| l.contains('█') || l.contains('▀')).count() >= 20, "a QR code: {said:#?}");
    let k = next_until(&mut lines, &log, "the key", &|l| l.trim_start().starts_with("key "), &mut said).await;
    assert_eq!(k.split_whitespace().last(), Some(principal.as_str()));
    let url = next_until(&mut lines, &log, "the URL", &|l| l.trim_start().starts_with("open"), &mut said).await;
    assert_eq!(url.split_whitespace().last().unwrap(), format!("http://127.0.0.1:9/#/pair?principal={principal}&name=test-box"));
    let any = next_until(&mut lines, &log, "any one works", &|l| l.starts_with("Any one of them works"), &mut said).await;
    assert!(any.contains("scan, or go to 127.0.0.1:9/#/pair and type the code"), "{any}");
    assert!(!said.iter().any(|l| l.contains('\x1b')), "no colours into a pipe");
    let how = next_until(&mut lines, &log, "what to do", &|l| l.contains("then Pair"), &mut said).await;
    assert!(how.starts_with("Open it on any device"), "--no-browser opens nothing: {how}");
    next_until(&mut lines, &log, "waiting", &|l| l.starts_with("Waiting for the grant"), &mut said).await;

    // 2. The remote learned of it: waiting, under its name.
    let waiting = until("the key in members.requests", || async {
        let w = call(&owner, "menger", "members.requests", json!({})).await.ok()?;
        w.into_iter().find(|e| e["principal"] == principal.as_str())
    })
    .await;
    assert_eq!(waiting["name"], "test-box", "{waiting}");

    // 3. An admin grants it; tether sees the grant and says so.
    one(&owner, "menger", "members.grant", json!({ "principal": principal, "scope": "TT", "role": "editor" })).await;
    let paired = next_until(&mut lines, &log, "paired", &|l| l.starts_with("paired:"), &mut said).await;
    assert_eq!(paired, "paired: test-box → editor on hypermemetic:TT");
    let status = tokio::task::spawn_blocking(move || pair.0.wait()).await.unwrap().unwrap();
    assert!(status.success(), "{said:#?}");

    // 4. tether.env made, with the remote and the agent in it; the key untouched.
    let text = std::fs::read_to_string(&env).unwrap();
    assert!(text.contains(&format!("\nTETHER_REMOTE={remote}\n")) && text.contains("\nTETHER_REMOTE_AS=test-box\n"), "{text}");
    assert!(!text.contains(&seed));
    assert_eq!(std::fs::read_to_string(&key).unwrap(), seed);

    // 5. Paired already: it says so at once and leaves a set file alone.
    let out = Command::new(env!("CARGO_BIN_EXE_tether"))
        .env("PLEXUS_NO_REGISTRY", "1")
        .args(["pair", "--remote", &remote, "--name", "test-box", "--no-browser", "--key"])
        .arg(&key)
        .arg("--env-file")
        .arg(&env)
        .output()
        .unwrap();
    let again = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && again.contains("already holds a grant") && again.contains("paired: test-box → editor on hypermemetic:TT"), "{again}");
    assert!(again.contains("kept  TETHER_REMOTE_AS=test-box"), "{again}");
    assert_eq!(std::fs::read_to_string(&env).unwrap(), text, "nothing rewritten");
}
