//! V2-20's review, B1 and B2: a body is read as v1 read it, and one editor's
//! body stalls nothing. B2: a file v1 kept, with the shapes the app renders
//! as inputs, reads the same after form 17's build. B1: at the 8 MiB cap,
//! every shape that was quadratic or worse answers in bounded time, an
//! input's markup read or refused in band.

use super::*;
use crate::inputs::{read, Problem};
use crate::types::InputValue;
use std::time::{Duration, Instant};

const CAP: usize = 8 * 1024 * 1024;
/// A debug build, on a loaded machine: the bound is that it ends, not a
/// budget (perf/ringer.mjs has those).
const AT_MOST: Duration = Duration::from_secs(20);

fn new(kind: Kind, body: &str) -> NewTicket {
    NewTicket { epic: "V".into(), title: "a ticket".into(), kind, severity: None, blocked_by: Vec::new(), parent: None, path: None, body: body.into() }
}

fn text(id: &str) -> String {
    format!("<m-input id=\"{id}\" kind=\"text\"><m-question>{id}?</m-question></m-input>\n")
}

/// The bodies marks.rs misread: each declares `want`, as v1 read it.
fn shapes() -> Vec<(&'static str, String, Vec<&'static str>)> {
    vec![
        ("indented under a list item", format!("1. the first\n\n    {}\n", text("listed")), vec!["listed"]),
        ("a lone backtick before a blank line", format!("a ` tick\n\n{}\nand ` another\n", text("after")), vec!["after"]),
        ("a textarea holds text", format!("<textarea>\n{}</textarea>\n\n{}", text("inside"), text("outside")), vec!["outside"]),
        ("a blockquote", format!("> {}", text("quoted")), vec!["quoted"]),
        ("a page in a mockup", format!("<m-mockup>\n<html><body>{}</body></html>\n</m-mockup>\n\n{}", text("mocked"), text("real")), vec!["real"]),
        ("a blank line inside", "<m-input id=\"gap\" kind=\"multi\">\n<m-question>Which?</m-question>\n\n<m-choice value=\"a\">A</m-choice>\n</m-input>\n".to_string(), vec!["gap"]),
    ]
}

#[test]
fn a_body_is_read_as_v1_read_it() {
    for (what, body, want) in shapes() {
        let (found, problems) = read(&body);
        assert!(problems.is_empty(), "{what}: {problems:?}");
        assert_eq!(found.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), want, "{what}");
    }
}

/// A file as v1 left it: its bodies, its `inputs` rows, an answer on each,
/// form 15. After form 17's build every input stands as it stood, and takes
/// the next answer.
#[tokio::test]
async fn a_v1_file_reads_the_same_after_form_17() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let shapes = shapes();
    {
        let store = Store::open(&path).unwrap();
        let sys = store.system().unwrap();
        store.create(&sys, new(Kind::Scope, "root")).await.unwrap();
        for (_, body, _) in &shapes {
            store.create(&sys, new(Kind::Build, body)).await.unwrap();
        }
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DELETE FROM inputs; DELETE FROM answers;").unwrap();
    for (n, (_, _, want)) in shapes.iter().enumerate() {
        let ticket = format!("V-{}", n + 2);
        for (position, id) in want.iter().enumerate() {
            conn.execute(
                "INSERT INTO inputs (ticket, id, position, kind, options, state, asked_unix) VALUES (?1, ?2, ?3, 'text', '[]', 'answered', 1)",
                rusqlite::params![ticket, id, position as i64],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO answers (ticket, input, action, value, source, at_unix) VALUES (?1, ?2, 'answered', '{\"type\":\"text\",\"text\":\"kept\"}', 'app', 1)",
                rusqlite::params![ticket, id],
            )
            .unwrap();
        }
    }
    conn.execute_batch("UPDATE derived_state SET form = 15;").unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    let sys = store.system().unwrap();
    for (n, (what, _, want)) in shapes.iter().enumerate() {
        let ticket = format!("V-{}", n + 2);
        let inputs = store.inputs(SYS, Some(&ticket), None, None).unwrap();
        for id in want {
            let i = inputs.iter().find(|i| i.id == *id).unwrap_or_else(|| panic!("{what}: {id} gone: {inputs:?}"));
            assert_eq!((i.state, i.answer.is_some()), (InputState::Answered, true), "{what}: {id}");
        }
        assert_eq!(inputs.iter().filter(|i| i.state == InputState::Removed).count(), 0, "{what}");
        if inputs[0].kind == crate::types::InputKind::Text {
            store.answer(sys.permit(&ticket), want[0], InputValue::Text { text: "again".into() }, None, AnswerSource::Api).await.unwrap_or_else(|e| panic!("{what}: {e}"));
        }
    }
}

/// Each shape at the cap: `(what, body)`. All of them name an input, so all
/// of them are read.
fn heavy() -> Vec<(&'static str, String)> {
    let fill = |unit: &str, head: &str| {
        let mut s = String::with_capacity(CAP + unit.len());
        s.push_str(head);
        while s.len() + unit.len() <= CAP {
            s.push_str(unit);
        }
        s
    };
    let open = "<m-input id=\"x\" kind=\"text\">";
    vec![
        ("nested questions", fill("<m-question>", open)),
        ("unterminated tags", fill("<m-input a ", "")),
        ("unterminated quotes", fill("<m-input a=\"", "")),
        ("ampersands", format!("{open}<m-question>{}</m-question></m-input>", fill("&", ""))),
        ("angle brackets", format!("{open}<m-question>{}</m-question></m-input>", fill("<a ", ""))),
        ("options", format!("<m-decision id=\"d\">{}</m-decision>", (0..).map(|n| format!("<m-option name=\"o{n}\"></m-option>")).scan(0, |len, o| { *len += o.len(); (*len < CAP - 100).then_some(o) }).collect::<String>())),
        ("inputs", (0..).map(|n| format!("<m-input id=\"i{n}\" kind=\"text\"></m-input>\n")).scan(0, |len, o| { *len += o.len(); (*len < CAP).then_some(o) }).collect()),
        ("blocks with a blank line inside", fill("<div>\n<p>a</p>\n\n<p>b</p>\n</div>\n\n", &format!("{open}</m-input>\n\n"))),
        ("spans never closed", fill("<span>", open)),
        ("closes that find nothing", format!("{}{}", "<span>".repeat(1024), fill("</b>", open))),
        ("paragraph-closers under spans", format!("{}{}", "<span>".repeat(1024), fill("<div></div>", open))),
        ("blank lines", fill("\n", &format!("{open}\n"))),
        ("mockups", fill("<m-mockup>a</m-mockup>\n\n", open)),
        ("blockquotes", fill(">", &format!("{open}\n"))),
        ("list items", fill("- a\n  - b\n", &format!("{open}\n"))),
        ("backticks", fill("` ", open)),
        ("brackets", fill("[", open)),
        ("stars", fill("*a ", open)),
        ("comments never closed", fill("<!-- ", open)),
    ]
}

#[test]
fn every_shape_at_the_cap_ends() {
    let mut worst = Duration::ZERO;
    for (what, body) in heavy() {
        assert!(body.len() <= CAP + 8192, "{what}: {}", body.len());
        let t = Instant::now();
        let (found, problems) = read(&body);
        let took = t.elapsed();
        eprintln!("{what}: {took:?}, {} found, {:?}", found.len(), problems.first());
        assert!(took < AT_MOST, "{what}: {took:?}");
        worst = worst.max(took);
    }
    eprintln!("the worst shape: {worst:?}");
}

/// Past what a body may hold, a write is refused in band, naming why.
#[test]
fn past_the_bounds_a_body_is_refused_by_name() {
    let many: String = (0..2_000).map(|n| text(&format!("i{n}"))).collect();
    assert!(matches!(read(&many).1.first(), Some(Problem::TooMuch { .. })), "too many inputs");
    let opts: String = (0..500).map(|n| format!("<m-option name=\"o{n}\"></m-option>")).collect();
    assert!(matches!(read(&format!("<m-decision id=\"d\">{opts}</m-decision>")).1.first(), Some(Problem::TooMuch { .. })), "too many options");
    let deep = format!("<m-input id=\"x\" kind=\"text\">{}", "<m-question>".repeat(100_000));
    assert!(matches!(read(&deep).1.first(), Some(Problem::TooMuch { .. })), "too deep");
    assert!(read(&deep).1[0].to_string().contains("nothing"), "{}", read(&deep).1[0]);
}

/// The read of a body happens before the write-serial lock: a heavy amend
/// does not hold another's write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_heavy_body_does_not_hold_the_write_lock() {
    let store = std::sync::Arc::new(Store::new());
    store.create(SYS, new(Kind::Scope, "root")).await.unwrap();
    store.create(SYS, new(Kind::Build, "plain")).await.unwrap();
    let body = format!("<m-input id=\"x\" kind=\"text\"></m-input>\n\n{}", "<div>\n<p>a</p>\n\n<p>b</p>\n</div>\n\n".repeat(200_000));
    let s = store.clone();
    let heavy = tokio::spawn(async move { s.amend(SYS.permit("V-2"), None, Some(body)).await });
    tokio::time::sleep(Duration::from_millis(5)).await;
    let t = Instant::now();
    store.amend(SYS.permit("V-1"), None, Some("a line".into())).await.unwrap();
    let beside = t.elapsed();
    let _ = heavy.await.unwrap();
    assert!(beside < Duration::from_millis(500), "a small write waited {beside:?}");
}

/// Prose, as a report's sections hold it: `paragraphs` of about 500 bytes,
/// a blank line between them, inside one `<section>`.
fn section(n: usize, paragraphs: usize) -> String {
    let line = "The kiln held its temperature through the second soak, and the glaze came out even across the shelf. ".repeat(5);
    let body: String = (0..paragraphs).map(|_| format!("<p>{line}</p>\n\n")).collect();
    format!("<section id=\"s{n}\">\n<p>Its first words.</p>\n\n{body}<p>Its last.</p>\n</section>\n\n")
}

/// A report: 1 MB in 60 sections, a blank line inside each, and its question.
fn report() -> String {
    let parts: String = (0..60).map(|n| section(n, 32)).collect();
    format!("{parts}{}", text("verdict"))
}

/// 3 MB: 12 pages drawn whole and 12 sections between them, and its question.
fn mocked() -> String {
    let page = format!("<m-mockup>\n<html><body>{}</body></html>\n</m-mockup>\n\n", "<div class=\"row\"><span>a cell</span></div>\n".repeat(4_000));
    let parts: String = (0..12).map(|n| format!("{page}{}", section(n, 150))).collect();
    format!("{parts}{}", text("verdict"))
}

/// V2-20's re-review, N3: bodies v1 took are read, and the budget still bounds
/// the wall time at the cap.
#[test]
fn large_bodies_v1_took_are_read() {
    for (what, body, size) in [("a report", report(), 900_000), ("pages and sections", mocked(), 2_500_000)] {
        assert!(body.len() > size, "{what}: {}", body.len());
        let t = Instant::now();
        let (found, problems) = read(&body);
        eprintln!("{what}: {} bytes, {:?}", body.len(), t.elapsed());
        assert!(problems.is_empty(), "{what}: {problems:?}");
        assert_eq!(found.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["verdict"], "{what}");
    }
}

/// A refusal says which limit it reached.
#[test]
fn a_refusal_names_the_limit_it_reached() {
    let blocks = format!("{}{}", text("x"), "<div>\n<p>a</p>\n\n<p>b</p>\n</div>\n\n".repeat(200_000));
    let closes = format!("{}{}{}", text("x"), "<span>".repeat(1024), "</b>".repeat(2_000_000));
    for (what, body, says) in [("blocks", blocks, "blank line"), ("closes", closes, "open elements")] {
        let (_, problems) = read(&body);
        let why = problems.first().map(ToString::to_string).unwrap_or_default();
        assert!(why.contains(says), "{what}: {why}");
        assert!(!why.contains("fewer tags, or fewer blank lines"), "{what}: {why}");
    }
}
