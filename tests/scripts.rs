//! V2-14: the script tests run under `cargo test`, so the pipeline's suite
//! holds the scripts as it holds the server. v1 ran `deploy/tests/run`,
//! `scripts/tests/cleanup-gate` and `scripts/tests/disk-check` from
//! `tests/deploy_record.rs`; V2-4's cut took that file with the records it
//! was named for, and nothing ran them since. This file runs those three and
//! the pipeline's own two (`deploy/tests/pipeline`, bash on a throwaway
//! root; `deploy/tests/runner.test.mjs`, node's test runner). Nothing here
//! touches a server, the network or the user's database.
//!
//! V2-15: one at a time, each under a timeout, each with a `TMPDIR` of its
//! own under the target directory — the suite of 20261029T175954Z-f1b65e5
//! stalled 25 minutes in `deploy/tests/pipeline` with nothing in its log,
//! and a hang inside a script must fail with what the script's own scratch
//! logs say, not wait for the runner to be killed.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

/// The scripts share the machine's `/tmp/claude-UID/*` roots and PATH
/// stand-ins; they run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// A scratch directory for the script's temp files, under the target
/// directory (so a cleanup gate sweeps it) — the script's `mktemp -d` lands
/// in it, and its logs are read back if the script is killed.
fn scratch(name: &str) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(root()).join("target"));
    let dir = target.join("script-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory under the target");
    // Canonical: a script compares the paths it is given with the ones it
    // resolves (`/home/…` is `/var/home/…` on this machine).
    std::fs::canonicalize(&dir).expect("the scratch directory's real path")
}

/// The last lines of every file under `dir` whose name ends `.log` or `out`
/// — what a killed script was doing.
fn logs_under(dir: &Path) -> String {
    let mut out = String::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "log") || p.file_name().is_some_and(|n| n == "out") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    let tail: Vec<&str> = text.lines().rev().take(15).collect();
                    out.push_str(&format!("\n--- {} (last {} lines)\n", p.display(), tail.len()));
                    for l in tail.iter().rev() {
                        out.push_str(l);
                        out.push('\n');
                    }
                }
            }
        }
    }
    out
}

/// Runs `program args` from the repository's root, alone, under `timeout`
/// (GNU `timeout -k 10 N`), and asserts it exits 0 and prints one of `ok`
/// (every script prints `N passed, M failed`; node's runner prints `fail 0`
/// after a glyph that depends on the reporter). A missing program is named,
/// not a bare failure; a kill by the timeout (exit 124) names the script and
/// shows its scratch logs.
fn passes(name: &str, program: &str, args: &[&str], ok: &[&str], limit: Duration) {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // SCRIPT_TEST_LIMIT_SECS overrides every limit — to see the kill path.
    let limit = std::env::var("SCRIPT_TEST_LIMIT_SECS").ok().and_then(|s| s.parse().ok()).map(Duration::from_secs).unwrap_or(limit);
    let tmp = scratch(name);
    let mut cmd = Command::new("timeout");
    cmd.arg("-k").arg("10").arg(limit.as_secs().to_string()).arg(program).args(args).current_dir(root()).env("TMPDIR", &tmp);
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) => panic!("timeout (coreutils) and {program} are needed on PATH to run {}: {e}", args.join(" ")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.code() == Some(124) {
        panic!(
            "{program} {} did not finish within {} s and was killed — its scratch logs:\n{}\n--- its output:\n{stdout}\n{stderr}",
            args.join(" "),
            limit.as_secs(),
            logs_under(&tmp)
        );
    }
    assert!(out.status.success(), "{program} {} failed ({}):\n{stdout}\n{stderr}", args.join(" "), out.status);
    assert!(ok.iter().any(|s| stdout.contains(s) || stderr.contains(s)), "{program} {}: no summary line:\n{stdout}\n{stderr}", args.join(" "));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// DEPLOY-10: `deploy/record`'s tests — every input form against a fixture,
/// the schema against deploy/README.md's examples, `--send` against a
/// stand-in synapse. bash, git and jq.
#[test]
fn deploy_record_script() {
    passes("record", "bash", &["deploy/tests/run"], &[" 0 failed"], Duration::from_secs(300));
}

/// DEPLOY-1…V2-7: `deploy/pipeline`'s tests on a throwaway root — the
/// release, the image, the retired commands, the v2 path. bash. About a
/// minute; ten is a hang.
#[test]
fn deploy_pipeline_script() {
    passes("pipeline", "bash", &["deploy/tests/pipeline"], &[" 0 failed"], Duration::from_secs(600));
}

/// DEPLOY-37/48: the deploy runner, `--fake`, under node's test runner.
#[test]
fn deploy_runner_script() {
    passes("runner", "node", &["--test", "deploy/tests/runner.test.mjs"], &["fail 0"], Duration::from_secs(300));
}

/// RUN-21, CI-2: `scripts/cleanup-gate.sh` — every leftover shape removed
/// and every refusal refused, on a throwaway root the script removes.
#[test]
fn cleanup_gate_script() {
    passes("cleanup-gate", "bash", &["scripts/tests/cleanup-gate"], &[" 0 failed"], Duration::from_secs(300));
}

/// CI-2: `scripts/disk-check.sh` — the floor and the alert, synapse a
/// stand-in, the disk said, not measured.
#[test]
fn disk_check_script() {
    passes("disk-check", "bash", &["scripts/tests/disk-check"], &[" 0 failed"], Duration::from_secs(300));
}

/// RUN-34: the throwaway's work directories (`ui8-*`, `workDir()` in
/// `scripts/ui-throwaway.mjs`) are said to go — by the caller, or in a
/// scratch root (`menger-perf-scratch`, `MENGER_SCRATCH_DIRS`) by the cleanup
/// gate once idle an hour, at the next hourly sweep; below `/tmp/claude-UID/`
/// by the caller's `--path` (the sweep never takes a session's scratch). The harness's usage and
/// perf/README.md's probe section say so, and neither says the gate has no
/// shape for them any more.
#[test]
fn ui8_work_dirs_are_said_to_go_at_the_hourly_sweep() {
    let read = |p: &str| std::fs::read_to_string(Path::new(root()).join(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
    let harness = read("scripts/ui-throwaway.mjs");
    // the prose as one line: a sentence may wrap anywhere
    let flat = |t: &str| t.split_whitespace().filter(|w| *w != "//").collect::<Vec<_>>().join(" ");
    let usage = flat(&harness.lines().take_while(|l| l.starts_with("//")).collect::<Vec<_>>().join("\n"));
    let readme = read("perf/README.md");
    let probe = flat(readme.split("\n## ").find(|s| s.contains("`pages-probe.mjs` (UI-8)")).expect("perf/README.md's pages-probe section"));
    for (name, text) in [("scripts/ui-throwaway.mjs's usage", usage.as_str()), ("perf/README.md's pages-probe section", probe.as_str())] {
        for want in ["the caller removes the work directory", "once idle an hour, at the next hourly sweep", "the caller removes it by", "scripts/cleanup-gate.sh", "menger-perf-scratch", "MENGER_SCRATCH_DIRS"] {
            assert!(text.contains(want), "{name} does not say {want:?}:\n{text}");
        }
    }
    for (name, text) in [("scripts/ui-throwaway.mjs's usage", usage.as_str()), ("perf/README.md's pages-probe section", probe.as_str())] {
        assert!(!text.contains("within the hour"), "{name} promises \"within the hour\": idle an hour, then the hourly sweep, is up to two");
    }
    assert!(!harness.contains("the gate has no shape"), "scripts/ui-throwaway.mjs still says the gate has no shape for a work directory under another $TMPDIR");
}
