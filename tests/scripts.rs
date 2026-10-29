//! V2-14: the script tests run under `cargo test`, so the pipeline's suite
//! holds the scripts as it holds the server. v1 ran `deploy/tests/run`,
//! `scripts/tests/cleanup-gate` and `scripts/tests/disk-check` from
//! `tests/deploy_record.rs`; V2-4's cut took that file with the records it
//! was named for, and nothing ran them since. This file runs those three and
//! the pipeline's own two (`deploy/tests/pipeline`, bash on a throwaway
//! root; `deploy/tests/runner.test.mjs`, node's test runner). Nothing here
//! touches a server, the network or the user's database.

#![cfg(unix)]

use std::process::Command;

fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Runs `program args` from the repository's root and asserts it exits 0
/// and says ` 0 failed` (every script prints `N passed, M failed`; node's
/// runner prints `fail 0` after a glyph that depends on the reporter). A missing program is named, not a bare failure.
fn passes(program: &str, args: &[&str], ok: &[&str]) {
    let out = match Command::new(program).args(args).current_dir(root()).output() {
        Ok(out) => out,
        Err(e) => panic!("{program} is needed on PATH to run {}: {e}", args.join(" ")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{program} {} failed ({}):\n{stdout}\n{stderr}", args.join(" "), out.status);
    assert!(ok.iter().any(|s| stdout.contains(s) || stderr.contains(s)), "{program} {}: no summary line:\n{stdout}\n{stderr}", args.join(" "));
}

/// DEPLOY-10: `deploy/record`'s tests — every input form against a fixture,
/// the schema against deploy/README.md's examples, `--send` against a
/// stand-in synapse. bash, git and jq.
#[test]
fn deploy_record_script() {
    passes("bash", &["deploy/tests/run"], &[" 0 failed"]);
}

/// DEPLOY-1…V2-7: `deploy/pipeline`'s tests on a throwaway root — the
/// release, the image, the retired commands, the v2 path. bash.
#[test]
fn deploy_pipeline_script() {
    passes("bash", &["deploy/tests/pipeline"], &[" 0 failed"]);
}

/// DEPLOY-37/48: the deploy runner, `--fake`, under node's test runner.
#[test]
fn deploy_runner_script() {
    passes("node", &["--test", "deploy/tests/runner.test.mjs"], &["fail 0"]);
}

/// RUN-21, CI-2: `scripts/cleanup-gate.sh` — every leftover shape removed
/// and every refusal refused, on a throwaway root the script removes.
#[test]
fn cleanup_gate_script() {
    passes("bash", &["scripts/tests/cleanup-gate"], &[" 0 failed"]);
}

/// CI-2: `scripts/disk-check.sh` — the floor and the alert, synapse a
/// stand-in, the disk said, not measured.
#[test]
fn disk_check_script() {
    passes("bash", &["scripts/tests/disk-check"], &[" 0 failed"]);
}
