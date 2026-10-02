//! COMPILE-4 over the wire: the real Node program — the vendored bundle in
//! jsdom — behind the real service, asked through the generated
//! `MermaidClient<&WsTransport>` (and once through the raw transport, for
//! the JSON as it travels).
//!
//! Every test here needs Node and jsdom (`npm ci --prefix diagrams/node`).
//! Without them each one prints `SKIPPED …: <what is missing>` and passes —
//! `cargo test` at the workspace root must not need Node. What does not
//! need Node (the lifecycle, a missing Node) is tested in lifecycle.rs and
//! binary.rs.
#![cfg(feature = "server")]

mod support;

use futures::StreamExt;
use menger_diagrams::node::{Config, Driver};
use menger_diagrams::{MermaidClient, MermaidEvent, Verdict};
use plexus_client::{MethodRef, StreamItem, Transport, WsTransport};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use support::{connect, exits_within, kill9, shared_service, start_server};

/// The 53 diagrams on the live tracker on 2026-10-01 (COMPILE-1/3), each
/// with the verdict a real browser gave it.
const LIVE: &str = include_str!("fixtures/live-diagrams-2026-10-01.json");
/// One valid diagram of each type we could write one for, and the type
/// mermaid names it.
const EVERY_TYPE: &str = include_str!("fixtures/every-type.json");

/// The fixture records each diagram's verdict, not its line. The two the
/// browser rejected, it rejected with "Parse error on line 2" — COMPILE-3:
/// the same message and line under jsdom — and line 2 is where each is
/// broken in its source: these are those lines, recorded here.
const BROWSER_LINES: [(&str, u32); 2] = [("DEMO-3#2", 2), ("SYNAPSE-2#1 (as first filed)", 2)];

#[derive(Deserialize)]
struct Live {
    id: String,
    src: String,
    expect: String,
}

#[derive(Deserialize)]
struct Typed {
    id: String,
    src: String,
    diagram_type: String,
}

/// A service of the test's own, around a driver the test keeps — for the
/// tests that kill its program or time it.
async fn service() -> (Driver, WsTransport) {
    let driver = Driver::new(Config::default());
    let port = start_server(driver.clone()).await;
    (driver, connect(port).await)
}

/// The one service the tests that only ask questions share: one Node
/// program (~300 MB) for all of them, not one each.
async fn shared() -> (Driver, WsTransport) {
    let (port, driver) = shared_service();
    (driver, connect(port).await)
}

fn one(events: Vec<MermaidEvent>) -> MermaidEvent {
    assert_eq!(
        events.len(),
        1,
        "parse yields exactly one event: {events:?}"
    );
    events.into_iter().next().unwrap()
}

async fn parse(client: &MermaidClient<&WsTransport>, source: &str) -> MermaidEvent {
    one(client
        .parse_all(source.into())
        .await
        .expect("the call succeeds"))
}

/// The line mermaid's message names ("… on line N …").
fn line_in_message(message: &str) -> Option<u32> {
    let at = message.find("on line ")? + "on line ".len();
    let digits: String = message[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

// -- criterion 1: the browser's verdicts -------------------------------------------

#[tokio::test]
async fn the_live_diagrams_get_the_browsers_verdicts() {
    needs_node!();
    let (_driver, transport) = shared().await;
    let client = MermaidClient::new(&transport);

    let live: Vec<Live> = serde_json::from_str(LIVE).unwrap();
    assert_eq!(live.len(), 53);
    let (mut accepted, mut rejected) = (0, 0);
    for d in &live {
        match (d.expect.as_str(), parse(&client, &d.src).await) {
            ("ok", MermaidEvent::Parsed { .. }) => accepted += 1,
            ("error", MermaidEvent::ParseError { message, line }) => {
                rejected += 1;
                let recorded = BROWSER_LINES
                    .iter()
                    .find(|(id, _)| *id == d.id)
                    .expect("a recorded line")
                    .1;
                assert_eq!(line, Some(recorded), "{}: {message}", d.id);
                assert_eq!(
                    line_in_message(&message),
                    Some(recorded),
                    "{}: {message}",
                    d.id
                );
            }
            (expect, got) => panic!("{}: the browser said {expect}, the service {got:?}", d.id),
        }
    }
    assert_eq!((accepted, rejected), (51, 2));
}

/// jsdom is not a browser: a valid `pie` or `radar` was reported broken
/// until the program lent the window Node's `structuredClone`. None of the
/// 53 live diagrams is one, so this is the guard — one diagram of every
/// type we could write, each accepted as the type it is.
#[tokio::test]
async fn every_type_is_accepted_as_itself() {
    needs_node!();
    let (_driver, transport) = shared().await;
    let client = MermaidClient::new(&transport);
    let typed: Vec<Typed> = serde_json::from_str(EVERY_TYPE).unwrap();
    assert!(typed.len() >= 26);
    for d in &typed {
        assert_eq!(
            parse(&client, &d.src).await,
            MermaidEvent::Parsed {
                diagram_type: d.diagram_type.clone()
            },
            "{}",
            d.id
        );
    }
}

/// `line` is a line of the source as given — not always the number in
/// mermaid's message, which counts the text mermaid parsed after dropping
/// frontmatter, directives, `%%` comments and leading blank lines (and,
/// for a flowchart, blank lines after a `}`).
#[tokio::test]
async fn a_parse_errors_line_is_a_line_of_the_source() {
    needs_node!();
    let (_driver, transport) = shared().await;
    let client = MermaidClient::new(&transport);
    let broken = "  C -->> [[[";
    for (case, source, line, text) in [
        (
            "plain",
            format!("flowchart LR\n  A --> B\n{broken}\n"),
            3,
            broken,
        ),
        (
            "frontmatter",
            format!("---\ntitle: x\n---\nflowchart LR\n  A --> B\n{broken}\n"),
            6,
            broken,
        ),
        (
            "directive",
            format!("%%{{init: {{'theme':'dark'}}}}%%\nflowchart LR\n  A --> B\n{broken}\n"),
            4,
            broken,
        ),
        (
            "leading blank lines",
            format!("\n\nflowchart LR\n  A --> B\n{broken}\n"),
            5,
            broken,
        ),
        (
            "comments",
            format!("flowchart LR\n  %% one\n  A --> B\n\n  %% two\n{broken}\n"),
            6,
            broken,
        ),
        (
            "blank lines after a brace",
            format!("flowchart LR\n  A{{x}}\n\n\n{broken}\n"),
            5,
            broken,
        ),
        (
            "crlf",
            format!("flowchart LR\r\n  A --> B\r\n{broken}\r\n"),
            3,
            broken,
        ),
        (
            "sequence, commented",
            "sequenceDiagram\n  %% c\n  A->>B: hi\n  A-x->B: bad\n".to_string(),
            4,
            "  A-x->B: bad",
        ),
        (
            "gitGraph under frontmatter",
            "---\ntitle: x\n---\ngitGraph\n commit\n bogus".to_string(),
            6,
            " bogus",
        ),
    ] {
        assert_eq!(
            source.lines().nth(line as usize - 1),
            Some(text),
            "{case}: the case itself"
        );
        match parse(&client, &source).await {
            MermaidEvent::ParseError { line: got, message } => {
                assert_eq!(got, Some(line), "{case}: {message}")
            }
            other => panic!("{case}: expected a parse_error, got {other:?}"),
        }
    }
    // mermaid names no line for a type it does not know.
    match parse(&client, "notadiagram\n  A --> B").await {
        MermaidEvent::ParseError { line, message } => {
            assert_eq!(line, None, "{message}");
            assert!(message.contains("No diagram type detected"), "{message}");
        }
        other => panic!("expected a parse_error, got {other:?}"),
    }
}

// -- criterion 2: check ------------------------------------------------------------------

/// Three mermaid blocks — one broken — among other fenced code, under a
/// frontmatter: three items, in order, with the right index and line, and
/// exactly one error, whose line within the markdown is the broken line.
#[tokio::test]
async fn check_yields_one_diagram_per_block_in_order() {
    needs_node!();
    let (_driver, transport) = shared().await;
    let client = MermaidClient::new(&transport);
    let markdown = "\
---
status: Ready
---
# A ticket

```rust
fn main() {}
```

```mermaid
graph LR
  A --> B
```

Some prose, then text that only mentions mermaid:

```text
mermaid
```

```mermaid title=\"the broken one\"
flowchart LR
  A --> B
  C -->> [[[
```

    indented code is not a fence: ```mermaid

```mermaidx
graph TD
```

```mermaid
sequenceDiagram
  A->>B: hi
```
";
    let events = client.check_all(markdown.into()).await.expect("check");
    let diagrams: Vec<(u32, u32, Verdict)> = events
        .into_iter()
        .map(|e| match e {
            MermaidEvent::Diagram {
                index,
                line,
                result,
            } => (index, line, result),
            other => panic!("check yields diagrams only: {other:?}"),
        })
        .collect();
    assert_eq!(diagrams.len(), 3, "{diagrams:?}");
    assert_eq!(diagrams.iter().map(|d| d.0).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(
        diagrams.iter().map(|d| d.1).collect::<Vec<_>>(),
        [11, 22, 34]
    );
    assert_eq!(
        diagrams[0].2,
        Verdict::Parsed {
            diagram_type: "flowchart-v2".into()
        }
    );
    assert_eq!(
        diagrams[2].2,
        Verdict::Parsed {
            diagram_type: "sequence".into()
        }
    );
    let errors: Vec<_> = diagrams
        .iter()
        .filter(|d| !matches!(d.2, Verdict::Parsed { .. }))
        .collect();
    assert_eq!(errors.len(), 1, "exactly one error: {diagrams:?}");
    let Verdict::ParseError { line: Some(at), .. } = &diagrams[1].2 else {
        panic!("the second block is the broken one: {:?}", diagrams[1]);
    };
    let markdown_line = diagrams[1].1 + at - 1;
    assert_eq!(
        markdown.lines().nth(markdown_line as usize - 1),
        Some("  C -->> [[[")
    );

    assert!(
        client
            .check_all("no diagrams\n\n```rust\nfn main() {}\n```\n".into())
            .await
            .unwrap()
            .is_empty(),
        "no blocks, no items"
    );
}

// -- the wire, as JSON ------------------------------------------------------------------------

/// The events as they travel — the ticket's "Provides" row, as JSON.
#[tokio::test]
async fn the_events_on_the_wire() {
    needs_node!();
    let (_driver, transport) = shared().await;
    async fn data(transport: &WsTransport, method: &'static str, params: Value) -> Vec<Value> {
        let mut out = Vec::new();
        let mut stream = transport.call(MethodRef::new("mermaid", method), params);
        while let Some(item) = stream.next().await {
            match item.expect("no transport error") {
                StreamItem::Data { content, .. } => out.push(content),
                StreamItem::Done { .. } => break,
                other => panic!("unexpected stream item {other:?}"),
            }
        }
        out
    }
    assert_eq!(
        data(
            &transport,
            "parse",
            json!({"source": "graph LR\n  A --> B"})
        )
        .await,
        [json!({"type": "parsed", "diagram_type": "flowchart-v2"})]
    );
    let rejected = data(
        &transport,
        "parse",
        json!({"source": "flowchart LR\n  this is not --> valid mermaid [[[\n"}),
    )
    .await;
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0]["type"], "parse_error");
    assert_eq!(rejected[0]["line"], 2);
    assert!(rejected[0]["message"]
        .as_str()
        .unwrap()
        .starts_with("Parse error on line 2:"));
    assert_eq!(
        data(
            &transport,
            "check",
            json!({"markdown": "x\n\n```mermaid\npie\n  \"a\": 1\n```\n"})
        )
        .await,
        [
            json!({"type": "diagram", "index": 1, "line": 4, "result": {"type": "parsed", "diagram_type": "pie"}})
        ]
    );
}

// -- parsed, never run ---------------------------------------------------------------------------

/// Sources that would do something if rendered loosely — click handlers
/// calling functions, `javascript:` links, a directive asking for
/// `securityLevel: loose`, markup in labels — are parsed, answered like any
/// other, and the program is still the one it was.
#[tokio::test]
async fn hostile_sources_are_only_parsed() {
    needs_node!();
    let (driver, transport) = shared().await;
    let client = MermaidClient::new(&transport);
    assert!(matches!(
        parse(&client, "graph LR\n  A --> B").await,
        MermaidEvent::Parsed { .. }
    ));
    let pid = driver.pid().unwrap();
    for source in [
        "%%{init: {\"securityLevel\": \"loose\"}}%%\nflowchart LR\n  A --> B\n  click A call process.exit(7)\n",
        "flowchart LR\n  A[\"<img src=x onerror=process.exit(7)>\"] --> B\n  click B href \"javascript:process.exit(7)\"\n",
        "flowchart LR\n  A[\"<script>process.exit(7)</script>\"] --> B\n",
        "sequenceDiagram\n  A->>B: <script>process.exit(7)</script>\n",
    ] {
        let answer = parse(&client, source).await;
        assert!(
            matches!(answer, MermaidEvent::Parsed { .. } | MermaidEvent::ParseError { .. }),
            "{source:?}: {answer:?}"
        );
    }
    assert!(matches!(
        parse(&client, "graph LR\n  A --> B").await,
        MermaidEvent::Parsed { .. }
    ));
    assert_eq!(
        driver.pid(),
        Some(pid),
        "the same program answered throughout"
    );
}

/// Any text is one request with one answer: line and paragraph separators
/// (U+2028, U+2029 — which JSON carries unescaped, and which Node's
/// readline would have split a request at), carriage returns, NUL, an emoji,
/// a 200 KB label. None of them costs the program its place.
#[tokio::test]
async fn any_text_is_one_request_with_one_answer() {
    needs_node!();
    let (driver, transport) = shared().await;
    let client = MermaidClient::new(&transport);
    assert!(matches!(
        parse(&client, "graph LR\n  A --> B").await,
        MermaidEvent::Parsed { .. }
    ));
    let pid = driver.pid().unwrap();
    let long = format!("graph LR\n  A[\"{}\"] --> B", "x".repeat(200_000));
    for source in [
        "graph LR\n  A[\"x\u{2028}y\u{2029}z\"] --> B",
        "graph LR\r\n  A --> B\r",
        "graph LR\n  A[\"\u{0} \u{1F600}\"] --> B",
        long.as_str(),
    ] {
        let began = Instant::now();
        assert_eq!(
            parse(&client, source).await,
            MermaidEvent::Parsed {
                diagram_type: "flowchart-v2".into()
            },
            "{:.60?}",
            source
        );
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "answered, not timed out"
        );
    }
    assert_eq!(driver.pid(), Some(pid), "the same program throughout");
}

// -- criterion 3: killed between calls ----------------------------------------------------------

#[tokio::test]
async fn killed_between_calls_the_second_call_succeeds() {
    needs_node!();
    let (driver, transport) = service().await;
    let client = MermaidClient::new(&transport);
    assert!(matches!(
        parse(&client, "graph LR\n  A --> B").await,
        MermaidEvent::Parsed { .. }
    ));
    let pid = driver.pid().expect("started by the first call");
    kill9(pid);
    // Between the calls means dead and gone before the second is made.
    assert!(exits_within(pid, Duration::from_secs(5)).await);
    assert_eq!(
        parse(&client, "graph LR\n  A --> B").await,
        MermaidEvent::Parsed {
            diagram_type: "flowchart-v2".into()
        }
    );
    assert_ne!(driver.pid(), Some(pid), "a fresh program answered");
}

// -- criteria 5 and 6: many at once; warm, a round trip ------------------------------------------

/// Fifty `parse` calls at once — five connections, ten each, the live
/// diagrams both broken ones included — each gets its own diagram's
/// verdict: the type a call alone gets, or the error and its line. Then,
/// warm, 200 round trips one after another: the median under 20 ms.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_at_once_then_warm_round_trips() {
    needs_node!();
    let driver = Driver::new(Config::default());
    let port = start_server(driver.clone()).await;
    let live: Vec<Live> = serde_json::from_str(LIVE).unwrap();
    let chosen: Vec<&Live> = live.iter().cycle().skip(5).take(50).collect();
    assert!(chosen.iter().any(|d| d.expect == "error"));

    // Each diagram alone, first: what the concurrent answers must equal.
    let transport = connect(port).await;
    let client = MermaidClient::new(&transport);
    let mut alone = Vec::new();
    for d in &chosen {
        alone.push(parse(&client, &d.src).await);
    }

    let mut connections = Vec::new();
    for _ in 0..5 {
        connections.push(connect(port).await);
    }
    let started = Instant::now();
    let calls = chosen.iter().enumerate().map(|(i, d)| {
        let transport = &connections[i % 5];
        async move {
            MermaidClient::new(transport)
                .parse_all(d.src.clone())
                .await
                .expect("the call succeeds")
        }
    });
    let answers = futures::future::join_all(calls).await;
    let elapsed = started.elapsed();
    for ((d, answer), expected) in chosen.iter().zip(answers).zip(&alone) {
        assert_eq!(&one(answer), expected, "{}", d.id);
    }
    eprintln!(
        "fifty concurrent parse calls: {:.0} ms in all",
        elapsed.as_secs_f64() * 1000.0
    );

    let source = "graph LR\n  A[\"a write\"] --> B[\"compiled\"]\n  B --> C[\"diagnostics\"]";
    let mut times = Vec::new();
    for _ in 0..200 {
        let began = Instant::now();
        assert!(matches!(
            parse(&client, source).await,
            MermaidEvent::Parsed { .. }
        ));
        times.push(began.elapsed());
    }
    times.sort();
    let (median, p99) = (times[100], times[197]);
    eprintln!(
        "warm parse round trip over the WebSocket, 200 calls: median {:.2} ms, p99 {:.2} ms",
        median.as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0
    );
    assert!(median < Duration::from_millis(20), "median {median:?}");
}

// -- missing pieces, as the real program names them ------------------------------------------------

/// The program without its `node_modules` (no `npm ci` yet): every call
/// is one `error` naming jsdom and the exact command — and the service is
/// still there to answer it.
#[tokio::test]
async fn without_jsdom_each_call_names_it_and_the_command() {
    needs_node!(without_jsdom);
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::copy(
        Config::default_node_dir().join("mermaid-parse.js"),
        dir.path().join("mermaid-parse.js"),
    )
    .unwrap();
    let driver = Driver::new(Config::new("node", dir.path(), Config::default_bundle()));
    let port = start_server(driver.clone()).await;
    let transport = connect(port).await;
    let client = MermaidClient::new(&transport);
    for _ in 0..2 {
        let events = client
            .parse_all("graph LR\n  A --> B".into())
            .await
            .unwrap();
        let [MermaidEvent::Error { message }] = events.as_slice() else {
            panic!("one in-band error: {events:?}");
        };
        assert!(message.starts_with("jsdom is not installed"), "{message}");
        assert!(
            message.contains(&format!("npm ci --prefix {}", dir.path().display())),
            "{message}"
        );
    }
    assert_eq!(
        driver.pid(),
        None,
        "nothing kept: the next call looks again"
    );
}

/// A bundle that is not mermaid's IIFE build is named as such.
#[tokio::test]
async fn a_bundle_that_is_not_mermaid_is_named() {
    needs_node!();
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let bundle = dir.path().join("mermaid-11.17.2.min.js");
    std::fs::write(&bundle, "globalThis.notMermaid = 1;\n").unwrap();
    let driver = Driver::new(Config::new("node", Config::default_node_dir(), &bundle));
    match driver.parse("graph LR\n  A --> B".into()).await {
        Err(menger_diagrams::node::Unavailable(message)) => {
            assert!(message.contains("defined no `mermaid.parse`"), "{message}")
        }
        other => panic!("expected a missing piece, got {other:?}"),
    }
}
