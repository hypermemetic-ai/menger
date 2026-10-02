//! COMPILE-4: the Node program's lifecycle, as the service handles it —
//! started on first use, kept, replaced when it dies or hangs, never
//! confused about whose answer is whose, killed when the service is done.
//!
//! These drive the [`Driver`] against `tests/fixtures/fake-node.sh`, a
//! stand-in that speaks the program's protocol and misbehaves on cue, so
//! every failure is produced on purpose and on time — and none of it needs
//! Node, so it runs everywhere `cargo test` does. tests/wire.rs runs the
//! real program over the wire.
#![cfg(feature = "server")]

mod support;

use menger_diagrams::node::{Config, Driver, Unavailable};
use menger_diagrams::Verdict;
use std::path::PathBuf;
use std::time::Duration;
use support::{alive, dies_within, exits_within, fixture, kill9};

/// A scenario for the stand-in: its "bundle", a file whose name says how
/// the program starts (see fake-node.sh).
struct Scenario {
    _dir: tempfile::TempDir,
    bundle: PathBuf,
}

fn scenario(name: &str) -> Scenario {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let bundle = dir.path().join(format!("{name}.js"));
    std::fs::write(&bundle, "// a stand-in bundle\n").unwrap();
    Scenario { _dir: dir, bundle }
}

fn fake(scenario: &Scenario, limit: Duration) -> Config {
    Config {
        node: "sh".into(),
        program: fixture("fake-node.sh"),
        bundle: scenario.bundle.clone(),
        limit,
        start_limit: Duration::from_secs(5),
    }
}

fn parsed(verdict: Result<Verdict, Unavailable>) -> String {
    match verdict {
        Ok(Verdict::Parsed { diagram_type }) => diagram_type,
        other => panic!("expected `parsed`, got {other:?}"),
    }
}

fn error(verdict: Result<Verdict, Unavailable>) -> String {
    match verdict {
        Ok(Verdict::Error { message }) => message,
        other => panic!("expected the diagram's `error`, got {other:?}"),
    }
}

fn unavailable(verdict: Result<Verdict, Unavailable>) -> String {
    match verdict {
        Err(Unavailable(message)) => message,
        other => panic!("expected a missing piece, got {other:?}"),
    }
}

// -- started once, kept --------------------------------------------------------

#[tokio::test]
async fn started_on_the_first_call_and_kept() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(2)));
    assert_eq!(driver.pid(), None, "nothing starts before the first call");
    assert_eq!(parsed(driver.parse("one".into()).await), "one");
    let pid = driver.pid().expect("started by the first call");
    for word in ["two", "three", "four"] {
        assert_eq!(parsed(driver.parse(word.into()).await), word);
    }
    assert_eq!(driver.pid(), Some(pid), "later calls reuse it");
    driver.shutdown().await;
}

// -- many at once ------------------------------------------------------------------

/// Fifty callers at once, each answered with its own diagram's answer —
/// with stray lines on the program's stdout (not the protocol; another
/// request's id) mixed in, which are skipped, never handed to a caller.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_at_once_each_get_their_own_answer() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(5)));
    let calls: Vec<_> = (0..50)
        .map(|i| {
            let driver = driver.clone();
            let word = if i % 7 == 3 {
                format!("JUNK-{i}")
            } else {
                format!("diagram-{i}")
            };
            tokio::spawn(async move { (word.clone(), driver.parse(word).await) })
        })
        .collect();
    for call in calls {
        let (word, verdict) = call.await.unwrap();
        assert_eq!(parsed(verdict), word, "each caller gets its own answer");
    }
    driver.shutdown().await;
}

// -- the limit -----------------------------------------------------------------------

/// A diagram that outlives the limit: its verdict is an `error` saying so,
/// the program is killed (not left running), and a fresh one answers the
/// next call.
#[tokio::test]
async fn a_diagram_past_the_limit_is_an_error_and_the_program_is_replaced() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_millis(400)));
    assert_eq!(parsed(driver.parse("warm".into()).await), "warm");
    let hung = driver.pid().unwrap();

    let message = error(driver.parse("HANG".into()).await);
    assert!(message.contains("took longer than 400 ms"), "{message}");
    assert!(message.contains("restarted"), "{message}");
    assert!(!alive(hung), "the hung program is killed, not left behind");

    assert_eq!(parsed(driver.parse("after".into()).await), "after");
    let fresh = driver.pid().unwrap();
    assert_ne!(fresh, hung, "a fresh program answered");
    driver.shutdown().await;
}

/// A diagram that keeps the program busy only up to the limit is fine:
/// the limit is per diagram, from the moment it is written — time spent
/// waiting behind other callers does not count against it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn waiting_in_line_does_not_count_against_the_limit() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_millis(500)));
    let calls: Vec<_> = (0..6)
        .map(|i| {
            let driver = driver.clone();
            tokio::spawn(async move { driver.parse(format!("SLOW-{i}")).await })
        })
        .collect();
    // Six times 200 ms is far past one limit; each diagram is well inside it.
    for (i, call) in calls.into_iter().enumerate() {
        assert_eq!(parsed(call.await.unwrap()), format!("SLOW-{i}"));
    }
    driver.shutdown().await;
}

// -- death -------------------------------------------------------------------------------

/// The program dies with a diagram in flight: that call answers an
/// `error` (saying how it ended, with its last words on stderr), and the
/// next call is answered by a fresh one.
#[tokio::test]
async fn dying_mid_call_is_that_calls_error_and_the_next_call_is_fresh() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(2)));
    assert_eq!(parsed(driver.parse("warm".into()).await), "warm");
    let first = driver.pid().unwrap();

    let message = error(driver.parse("DIE".into()).await);
    assert!(message.contains("exited (exit status: 3)"), "{message}");
    assert!(
        message.contains("dying on request"),
        "its stderr is quoted: {message}"
    );
    assert!(
        message.contains("the next call starts a fresh one"),
        "{message}"
    );

    assert_eq!(parsed(driver.parse("after".into()).await), "after");
    assert_ne!(driver.pid(), Some(first));
    driver.shutdown().await;
}

/// Killed between two calls (an operator, the OOM killer): the second
/// call notices before it writes, and succeeds on a fresh program.
#[tokio::test]
async fn killed_between_calls_the_second_call_succeeds() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(2)));
    assert_eq!(parsed(driver.parse("before".into()).await), "before");
    let pid = driver.pid().unwrap();
    kill9(pid);
    // Between the calls means dead and gone before the second is made.
    assert!(exits_within(pid, Duration::from_secs(5)).await);

    assert_eq!(parsed(driver.parse("after".into()).await), "after");
    assert_ne!(driver.pid(), Some(pid));
    driver.shutdown().await;
}

// -- stderr and stdout -----------------------------------------------------------------

/// The program's stderr is drained as it comes, never read as the
/// protocol: 300 KB of it — far past a pipe's buffer, which would stall a
/// program nobody reads — and the answer still arrives, inside the limit.
#[tokio::test]
async fn stderr_is_drained_and_never_mistaken_for_an_answer() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(3)));
    assert_eq!(parsed(driver.parse("NOISE".into()).await), "NOISE");
    assert_eq!(parsed(driver.parse("quiet".into()).await), "quiet");
    driver.shutdown().await;
}

// -- a caller that leaves ----------------------------------------------------------------

/// A caller that goes away mid-call (a client unsubscribing) leaves the
/// request to run to its end: the next caller gets its own answer, not
/// the abandoned one, and the program is not replaced over it.
#[tokio::test]
async fn a_caller_that_leaves_does_not_unsettle_the_next() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(2)));
    assert_eq!(parsed(driver.parse("warm".into()).await), "warm");
    let pid = driver.pid().unwrap();
    let abandoned = tokio::time::timeout(
        Duration::from_millis(50),
        driver.parse("SLOW-abandoned".into()),
    )
    .await;
    assert!(abandoned.is_err(), "the caller gave up first");
    assert_eq!(parsed(driver.parse("next".into()).await), "next");
    assert_eq!(driver.pid(), Some(pid), "the same program, still in step");
    driver.shutdown().await;
}

// -- missing pieces -------------------------------------------------------------------------

/// Every missing piece is named, with what provides it — and nothing is
/// kept, so the next call looks again.
#[tokio::test]
async fn each_missing_piece_is_named_with_what_provides_it() {
    let s = scenario("ok");

    let bare = Config {
        node: "no-such-node-binary-compile-4".into(),
        ..fake(&s, Duration::from_secs(1))
    };
    let driver = Driver::new(bare);
    let message = unavailable(driver.parse("x".into()).await);
    assert!(message.contains("Node.js is not installed"), "{message}");
    assert!(message.contains("not on PATH"), "{message}");
    assert!(message.contains("--node"), "{message}");
    assert_eq!(driver.pid(), None, "nothing kept");

    let pathed = Config {
        node: "/nonexistent/compile-4/node".into(),
        ..fake(&s, Duration::from_secs(1))
    };
    let message = unavailable(Driver::new(pathed).parse("x".into()).await);
    assert!(
        message.contains("Node.js is not at /nonexistent/compile-4/node"),
        "{message}"
    );

    let no_bundle = Config {
        bundle: "/nonexistent/compile-4/mermaid.min.js".into(),
        ..fake(&s, Duration::from_secs(1))
    };
    let message = unavailable(Driver::new(no_bundle).parse("x".into()).await);
    assert!(
        message.contains("the mermaid bundle is not at /nonexistent/compile-4/mermaid.min.js"),
        "{message}"
    );
    assert!(message.contains("--bundle"), "{message}");
    assert!(
        message.contains("git checkout -- frontend/vendor/mermaid"),
        "{message}"
    );

    let no_program = Config {
        program: "/nonexistent/compile-4/mermaid-parse.js".into(),
        ..fake(&s, Duration::from_secs(1))
    };
    let message = unavailable(Driver::new(no_program).parse("x".into()).await);
    assert!(
        message.contains("Node program is not at /nonexistent/compile-4/mermaid-parse.js"),
        "{message}"
    );
    assert!(message.contains("--node-dir"), "{message}");
}

/// What the program itself says is missing (jsdom, in the real one) is
/// passed on word for word.
#[tokio::test]
async fn the_programs_own_fatal_is_passed_on() {
    let s = scenario("fatal");
    let driver = Driver::new(fake(&s, Duration::from_secs(1)));
    let message = unavailable(driver.parse("x".into()).await);
    assert_eq!(
        message,
        "jsdom is not installed (fake) — run `npm ci --prefix fake`"
    );
    assert_eq!(driver.pid(), None);
}

#[tokio::test]
async fn a_program_that_exits_before_ready_is_named_with_its_last_words() {
    let s = scenario("exits");
    let message = unavailable(
        Driver::new(fake(&s, Duration::from_secs(1)))
            .parse("x".into())
            .await,
    );
    assert!(
        message.contains("exited (exit status: 9) before it was ready"),
        "{message}"
    );
    assert!(message.contains("exiting before ready"), "{message}");
}

#[tokio::test]
async fn a_program_that_never_says_ready_is_given_up_on() {
    let s = scenario("silent");
    let config = Config {
        start_limit: Duration::from_millis(300),
        ..fake(&s, Duration::from_secs(1))
    };
    let message = unavailable(Driver::new(config).parse("x".into()).await);
    assert!(
        message.contains("did not say it was ready within 300 ms"),
        "{message}"
    );
}

/// A piece that arrives while the service runs is picked up by the next
/// call — no restart.
#[tokio::test]
async fn a_piece_installed_later_is_found_by_the_next_call() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let bundle = dir.path().join("ok.js");
    let config = Config {
        node: "sh".into(),
        program: fixture("fake-node.sh"),
        bundle: bundle.clone(),
        limit: Duration::from_secs(1),
        start_limit: Duration::from_secs(5),
    };
    let driver = Driver::new(config);
    assert!(unavailable(driver.parse("x".into()).await).contains("bundle"));
    std::fs::write(&bundle, "// arrived\n").unwrap();
    assert_eq!(parsed(driver.parse("x".into()).await), "x");
    driver.shutdown().await;
}

// -- the end ------------------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_kills_the_program_and_starts_no_other() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(1)));
    assert_eq!(parsed(driver.parse("x".into()).await), "x");
    let pid = driver.pid().unwrap();
    driver.shutdown().await;
    assert!(!alive(pid), "killed and reaped by shutdown");
    assert_eq!(driver.pid(), None);
    assert!(unavailable(driver.parse("y".into()).await).contains("shutting down"));
}

#[tokio::test]
async fn dropping_the_driver_kills_the_program() {
    let s = scenario("ok");
    let driver = Driver::new(fake(&s, Duration::from_secs(1)));
    assert_eq!(parsed(driver.parse("x".into()).await), "x");
    let pid = driver.pid().unwrap();
    drop(driver);
    assert!(
        dies_within(pid, Duration::from_secs(5)).await,
        "kill_on_drop"
    );
}
