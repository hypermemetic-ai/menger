//! The mermaid blocks of a markdown body — by the app's own rule.
//!
//! `check` must find exactly the diagrams the ticket page draws: no more
//! (a block the page shows as code is not a diagram anyone sees) and no
//! fewer. So this is not a second reading of "what is a mermaid block". It
//! is the app's, as the app computes it (`frontend/src/md.rs`, AGS-14, and
//! the page that calls it, `frontend/src/main.rs`):
//!
//! - the body's YAML frontmatter is split off first and never parsed as
//!   markdown ([`split_frontmatter`], copied verbatim);
//! - the rest goes through pulldown-cmark — the same crate, at the same
//!   version (the two lockfiles are held equal below), with the same four
//!   options;
//! - a fenced code block is a diagram when its info string's first word is
//!   exactly `mermaid` ([`is_mermaid_fence`], copied verbatim);
//! - its source is the block's text, as the parser yields it — what the
//!   page hands to mermaid.
//!
//! The app is its own cargo unit (wasm32), so its functions cannot be
//! called from here; they are copied, and the tests below read
//! `frontend/src/md.rs` as text to check the copies are still word for word
//! what it holds and to run every case its own tests name against this
//! module (tickets-api's `tests_find.rs` does the same for the jump
//! palette's rule). A change to the app's rule fails this crate's tests
//! until it is copied here too.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// One mermaid block: its position in the body and its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 1-based, in document order.
    pub index: u32,
    /// The 1-based line, within the whole markdown (frontmatter included),
    /// of the block's first source line: the line after its opening fence.
    pub line: u32,
    /// The diagram, as the page hands it to mermaid.
    pub source: String,
}

/// Every mermaid block in `markdown`, in order. No blocks, an empty list.
pub fn mermaid_blocks(markdown: &str) -> Vec<Block> {
    let (_, body) = split_frontmatter(markdown);
    // `body` is a suffix of `markdown`: offsets into it are offsets into
    // the markdown after the frontmatter.
    let skipped = markdown.len() - body.len();
    let lines = LineStarts::of(markdown);

    let mut blocks = Vec::new();
    // Inside a mermaid fence: its first source line and the source so far.
    let mut open: Option<(u32, String)> = None;
    for (event, range) in Parser::new_ext(body, options()).into_offset_iter() {
        if let Some((_, source)) = open.as_mut() {
            match event {
                Event::Text(text) => source.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    let (line, source) = open.take().expect("checked");
                    blocks.push(Block {
                        index: blocks.len() as u32 + 1,
                        line,
                        source,
                    });
                }
                // A fenced block's body is Text only (md.rs drops anything
                // else here too).
                _ => {}
            }
            continue;
        }
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info))) = event {
            if is_mermaid_fence(info) {
                // A fence's info string cannot span lines: the source
                // starts on the line after the one the block starts on.
                open = Some((lines.line_of(skipped + range.start) + 1, String::new()));
            }
        }
    }
    blocks
}

/// The parser options `render_body` uses (md.rs), the same four.
fn options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts
}

/// Is this fence's info string a mermaid fence? The language is the first
/// word (`mermaid title=...` still counts); exact and lowercase, as
/// GitHub's renderer treats it.
fn is_mermaid_fence(info: &str) -> bool {
    info.split_whitespace().next() == Some("mermaid")
}

/// Split a leading YAML frontmatter block off a ticket body. Returns
/// `(Some(frontmatter), rest)` when the body starts with a `---` line and
/// a closing `---` line exists; otherwise `(None, body)` — an unclosed
/// fence (or a mid-document thematic break) is not frontmatter.
pub fn split_frontmatter(body: &str) -> (Option<&str>, &str) {
    let Some(after_open) = body.strip_prefix("---") else {
        return (None, body);
    };
    // The opening fence must be a line of its own ("---not-a-fence" isn't one).
    let Some(after_open) = after_open
        .strip_prefix("\r\n")
        .or_else(|| after_open.strip_prefix('\n'))
    else {
        return (None, body);
    };
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let fm = &after_open[..offset];
            let rest = &after_open[offset + line.len()..];
            return (Some(fm), rest);
        }
        offset += line.len();
    }
    (None, body)
}

/// Where each line of a text begins, so an offset becomes a 1-based line.
/// A line ends at `\n` (so `\r\n` once) — where pulldown-cmark ends one: a
/// lone `\r` does not start a line for it, so no fence can follow one.
struct LineStarts(Vec<usize>);

impl LineStarts {
    fn of(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineStarts(starts)
    }

    fn line_of(&self, offset: usize) -> u32 {
        // The number of line starts at or before `offset`.
        self.0.partition_point(|&start| start <= offset) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's renderer and the page that calls it, as text. The app is
    /// its own cargo unit; this reads it, it does not build it.
    const MD_RS: &str = include_str!("../../frontend/src/md.rs");
    const APP_MAIN_RS: &str = include_str!("../../frontend/src/main.rs");
    /// WAVE-11: where the page's body view lives now — shared by the ticket
    /// page and the board's annotation.
    const APP_NOTES_RS: &str = include_str!("../../frontend/src/notes.rs");
    const THIS_RS: &str = include_str!("blocks.rs");
    /// The two lockfiles: the app's, and the workspace's this crate builds in.
    const APP_LOCK: &str = include_str!("../../frontend/Cargo.lock");
    const WORKSPACE_LOCK: &str = include_str!("../../Cargo.lock");

    fn sources(markdown: &str) -> Vec<String> {
        mermaid_blocks(markdown)
            .into_iter()
            .map(|b| b.source)
            .collect()
    }

    fn lines(markdown: &str) -> Vec<u32> {
        mermaid_blocks(markdown)
            .into_iter()
            .map(|b| b.line)
            .collect()
    }

    // -- the app's rule, word for word ------------------------------------

    /// The two copied functions are what md.rs holds, verbatim — and what
    /// this file holds.
    #[test]
    fn the_copies_are_the_apps_code_word_for_word() {
        let is_mermaid_fence =
            "/// Is this fence's info string a mermaid fence? The language is the first
/// word (`mermaid title=...` still counts); exact and lowercase, as
/// GitHub's renderer treats it.
fn is_mermaid_fence(info: &str) -> bool {
    info.split_whitespace().next() == Some(\"mermaid\")
}
";
        let split_frontmatter = "pub fn split_frontmatter(body: &str) -> (Option<&str>, &str) {
    let Some(after_open) = body.strip_prefix(\"---\") else {
        return (None, body);
    };
    // The opening fence must be a line of its own (\"---not-a-fence\" isn't one).
    let Some(after_open) = after_open
        .strip_prefix(\"\\r\\n\")
        .or_else(|| after_open.strip_prefix('\\n'))
    else {
        return (None, body);
    };
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\\n') {
        if line.trim_end_matches(['\\r', '\\n']) == \"---\" {
            let fm = &after_open[..offset];
            let rest = &after_open[offset + line.len()..];
            return (Some(fm), rest);
        }
        offset += line.len();
    }
    (None, body)
}
";
        for (name, code) in [
            ("is_mermaid_fence", is_mermaid_fence),
            ("split_frontmatter", split_frontmatter),
        ] {
            assert!(
                MD_RS.contains(code),
                "md.rs no longer reads {name} as copied here — re-copy it"
            );
            assert!(
                THIS_RS.contains(code),
                "blocks.rs's {name} is not md.rs's — re-copy it"
            );
        }
    }

    /// The same parser options: every `opts.insert` render_body makes, and
    /// no other.
    #[test]
    fn the_options_are_render_bodys() {
        let theirs: Vec<&str> = MD_RS
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("opts.insert("))
            .collect();
        let start = THIS_RS.find("fn options() -> Options {").unwrap();
        let ours: Vec<&str> = THIS_RS[start..]
            .lines()
            .take_while(|l| *l != "}")
            .map(str::trim)
            .filter(|l| l.starts_with("opts.insert("))
            .collect();
        assert_eq!(ours.len(), 4, "{ours:?}");
        assert_eq!(
            theirs, ours,
            "render_body's options changed — copy them here"
        );
    }

    /// The same parser: one pulldown-cmark version in both lockfiles.
    #[test]
    fn the_parser_is_the_apps_version() {
        fn version(lock: &str) -> &str {
            let at = lock
                .find("name = \"pulldown-cmark\"\nversion = \"")
                .expect("the lockfile holds pulldown-cmark");
            let rest = &lock[at + "name = \"pulldown-cmark\"\nversion = \"".len()..];
            &rest[..rest.find('"').unwrap()]
        }
        assert_eq!(
            version(WORKSPACE_LOCK),
            version(APP_LOCK),
            "the service and the app parse markdown with different pulldown-cmark versions — \
             `cargo update -p pulldown-cmark --precise <the app's>`"
        );
    }

    /// The page splits the frontmatter off before it renders — so the
    /// frontmatter is never searched for diagrams here either. WAVE-11: the
    /// ticket page draws its body through `notes::body_view`, which splits
    /// it and renders the rest, with or without its wave references.
    #[test]
    fn the_page_splits_the_frontmatter_first() {
        assert!(APP_MAIN_RS.contains("notes::body_view(&raw, Some(refs))"));
        assert!(APP_NOTES_RS.contains("let (fm, md_src) = md::split_frontmatter(raw);"));
        assert!(APP_NOTES_RS.contains("md::render_body_with(md_src, "));
        assert!(APP_NOTES_RS.contains("md::render_body(md_src)"));
    }

    // -- the app's own cases -------------------------------------------------

    /// One string literal at the start of `s`, unescaped (`\"`, `\\`, `\n`,
    /// `\t` — all md.rs's tests use), and how many bytes it spans.
    fn literal(s: &str) -> (String, usize) {
        let mut out = String::new();
        let mut chars = s.char_indices();
        assert_eq!(chars.next().map(|(_, c)| c), Some('"'));
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return (out, i + 1),
                '\\' => match chars.next().expect("an escape").1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    other => out.push(other),
                },
                other => out.push(other),
            }
        }
        panic!("an unclosed literal: {s:.40}")
    }

    /// The first integer after the next comma.
    fn count_after_comma(s: &str) -> usize {
        let s = s[s.find(',').unwrap() + 1..].trim_start();
        s[..s.find(|c: char| !c.is_ascii_digit()).unwrap()]
            .parse()
            .unwrap()
    }

    /// Every case md.rs's tests name: `(markdown, diagrams)` for each
    /// `render_body(..)` whose `.diagrams` a test asserts — inline
    /// (`render_body("…").diagrams, N`) or through `let r = render_body(…)`
    /// and a later `r.diagrams, N` in the same test.
    fn the_apps_cases() -> Vec<(String, usize)> {
        let tests = &MD_RS[MD_RS.find("#[cfg(test)]").expect("md.rs has tests")..];
        let flow = {
            let at = tests.find("const FLOW: &str =").expect("FLOW");
            let rest = tests[at..].split_once('=').unwrap().1.trim_start();
            literal(rest).0
        };
        let mut cases = Vec::new();
        let mut from = 0;
        while let Some(at) = tests[from..].find("render_body(") {
            let call = from + at;
            let arg = tests[call + "render_body(".len()..].trim_start();
            from = call + 1;
            let (markdown, after) = if arg.starts_with('"') {
                let (lit, len) = literal(arg);
                (lit, &arg[len..])
            } else if let Some(after) = arg.strip_prefix("FLOW") {
                (flow.clone(), after)
            } else {
                continue; // `render_body(src)` — the helper, not a case
            };
            let after = after.trim_start().trim_start_matches(',').trim_start();
            let after = after
                .strip_prefix(')')
                .expect("render_body takes one argument");
            let end_of_test = after.find("#[test]").unwrap_or(after.len());
            let expected = if let Some(rest) = after.strip_prefix(".diagrams") {
                count_after_comma(rest)
            } else if tests[..call].trim_end().ends_with("let r =") {
                match after[..end_of_test].find("r.diagrams") {
                    Some(at) => count_after_comma(&after[at..]),
                    None => continue,
                }
            } else {
                continue; // `.html` only
            };
            cases.push((markdown, expected));
        }
        cases
    }

    #[test]
    fn every_case_the_apps_tests_name_counts_the_same() {
        let cases = the_apps_cases();
        assert!(cases.len() >= 9, "read md.rs's cases: {cases:?}");
        for (markdown, expected) in &cases {
            assert_eq!(mermaid_blocks(markdown).len(), *expected, "{markdown:?}");
        }
    }

    /// The source is the block's text, as md.rs collects it for the page —
    /// markup and all, never interpreted here.
    #[test]
    fn the_source_is_what_the_page_hands_to_mermaid() {
        assert_eq!(
            sources(
                "# Title\n\n```mermaid\nflowchart LR\n  A[start] --> B{ok?}\n```\n\ntail text\n"
            ),
            ["flowchart LR\n  A[start] --> B{ok?}\n"]
        );
        assert_eq!(
            sources("intro\n\n```mermaid\nflowchart LR\n  A -->"),
            ["flowchart LR\n  A -->"],
            "an unclosed fence runs to the end, as on the page"
        );
        assert_eq!(
            sources("> ```mermaid\n> graph TD\n> A-->B\n> ```\n"),
            ["graph TD\nA-->B\n"]
        );
        assert_eq!(
            sources("~~~mermaid\npie\n~~~\n"),
            ["pie\n"],
            "a tilde fence is a fence"
        );
        assert!(sources("").is_empty());
        assert!(sources("no diagrams here\n\n```rust\nfn main() {}\n```\n").is_empty());
    }

    // -- lines ---------------------------------------------------------------

    #[test]
    fn line_is_the_blocks_first_source_line() {
        assert_eq!(lines("```mermaid\ngraph TD\n```\n"), [2]);
        assert_eq!(lines("intro\n\n```mermaid\ngraph TD\n```\n"), [4]);
        assert_eq!(
            lines("a\n\n```mermaid\ngraph TD\n```\n\n```rust\nx\n```\n\n```mermaid title=\"t\"\npie\n```\n"),
            [4, 12]
        );
        assert_eq!(
            lines("- item\n\n  ```mermaid\n  graph TD\n  ```\n"),
            [4],
            "in a list item"
        );
        assert_eq!(
            lines("> quote\n> ```mermaid\n> graph TD\n> ```\n"),
            [3],
            "in a block quote"
        );
    }

    #[test]
    fn lines_count_crlf_once() {
        assert_eq!(lines("a\r\n\r\n```mermaid\r\ngraph TD\r\n```\r\n"), [4]);
        // The parser's own reading of a lone `\r`: not a line ending, so
        // not a fence either — the page draws nothing here, and neither
        // does `check` find anything.
        assert!(lines("a\r\r```mermaid\rgraph TD\r```\r").is_empty());
    }

    /// The frontmatter is not searched, but it is counted: `line` is a line
    /// of the markdown as given.
    #[test]
    fn frontmatter_is_counted_not_searched() {
        let body = "---\nstatus: Ready\nnote: |\n  ```mermaid\n  graph TD\n  ```\n---\n# T\n\n```mermaid\ngraph LR\n```\n";
        let blocks = mermaid_blocks(body);
        assert_eq!(
            blocks.len(),
            1,
            "the fence inside the frontmatter is not a diagram: {blocks:?}"
        );
        assert_eq!(blocks[0].line, 11);
        assert_eq!(blocks[0].source, "graph LR\n");
        assert_eq!(
            lines("---\r\na: 1\r\n---\r\n```mermaid\r\npie\r\n```\r\n"),
            [5]
        );
    }

    /// The indices count mermaid blocks only, from 1, in order.
    #[test]
    fn indices_count_diagrams_from_one() {
        let blocks = mermaid_blocks("```mermaid\na\n```\n```text\nb\n```\n```mermaid\nc\n```\n");
        assert_eq!(blocks.iter().map(|b| b.index).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(
            sources("```mermaid\na\n```\n```text\nb\n```\n```mermaid\nc\n```\n"),
            ["a\n", "c\n"]
        );
    }
}
