//! DEPLOY-10: `deploy/record`'s own tests (`deploy/tests/run` — bash, git
//! and jq, every input form against its fixture, the schema against the
//! docs' examples, `--send` against a stand-in synapse) run under `cargo
//! test`, so the pipeline's gate holds the adapter as it holds the server.
//! Nothing here touches a server, the network or the user's database.

#![cfg(unix)]

use std::process::Command;

#[test]
fn deploy_record_adapter() {
    let run = concat!(env!("CARGO_MANIFEST_DIR"), "/deploy/tests/run");
    let out = Command::new("bash")
        .arg(run)
        .output()
        .expect("deploy/tests/run needs bash on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "deploy/tests/run failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains(" 0 failed"), "{stdout}");
}

/// RUN-21: `scripts/cleanup-gate.sh` (`scripts/tests/cleanup-gate`) — every
/// leftover shape removed and every refusal refused (CI-2: build-dirs, a
/// worktree's with it, and the sweep), on a throwaway root under
/// `/tmp/claude-UID/run21/` that the script itself removes at the end.
#[test]
fn cleanup_gate_script() {
    let run = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/tests/cleanup-gate");
    let out = Command::new("bash")
        .arg(run)
        .output()
        .expect("scripts/tests/cleanup-gate needs bash on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "scripts/tests/cleanup-gate failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains(" 0 failed"), "{stdout}");
}

/// CI-2: `scripts/disk-check.sh` (`scripts/tests/disk-check`) — the floor
/// (free GB, used %) and the alert at 90%: filed and queued once, not again
/// while that ticket is open; synapse a stand-in, the disk said, not measured.
#[test]
fn disk_check_script() {
    let run = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/tests/disk-check");
    let out = Command::new("bash")
        .arg(run)
        .output()
        .expect("scripts/tests/disk-check needs bash on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "scripts/tests/disk-check failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains(" 0 failed"), "{stdout}");
}

/// DEPLOY-12: `deploy/pipeline`'s deployment records (`deploy/tests/pipeline`)
/// — deploy test, sync, promote with the drill's rollback, rollback, a
/// tracker down or hung (the outbox), a refusal, a run that dies after
/// `started`, `status` and `backfill` — on a throwaway root: HOME a scratch
/// directory, systemctl, synapse and the rest stand-ins. No unit, port,
/// database or tracker is touched.
#[test]
fn deploy_pipeline_records() {
    let run = concat!(env!("CARGO_MANIFEST_DIR"), "/deploy/tests/pipeline");
    let out = Command::new("bash")
        .arg(run)
        .output()
        .expect("deploy/tests/pipeline needs bash on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "deploy/tests/pipeline failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains(" 0 failed"), "{stdout}");
}
