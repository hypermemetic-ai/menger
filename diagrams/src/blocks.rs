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
//!   page hands to mermaid;
//! - ART-5: a body holding HTML is drawn in a frame (COMP-4), where a
//!   diagram may also be written as HTML — ought's form, mermaid source in a
//!   `<pre class="mermaid">`, alone or inside a field element or a component
//!   (`<m-dag>`) — in an HTML block. The frame's renderer
//!   (`render_body_framed`) gathers each HTML block's lines and turns each
//!   such `<pre>` into a fence's placeholder ([`html_diagrams`], copied
//!   verbatim), so the page draws it; so it is a diagram here too, its
//!   source what md.rs puts in the placeholder and its `line` the line that
//!   source starts on. (The rule applies to every body: a `<pre>` in an HTML
//!   block is HTML, and HTML frames a body.) A fence inside a field element
//!   or a component, with blank lines around it, is markdown: a fence like
//!   any other.
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
    /// of the block's first source line: the line after its opening fence —
    /// or, for a diagram written as HTML (ART-5), the line its source starts
    /// on (the open tag's, or the next when a newline follows the tag).
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
    // ART-5: inside an HTML block — its lines gathered as md.rs gathers them
    // for the frame, and where each starts, in the gathered text and in the
    // markdown (a line in a block quote or a list item has its prefix
    // dropped, so the two drift).
    let mut html: Option<(String, Vec<(usize, usize)>)> = None;
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
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info))) if is_mermaid_fence(info) => {
                // A fence's info string cannot span lines: the source
                // starts on the line after the one the block starts on.
                open = Some((lines.line_of(skipped + range.start) + 1, String::new()));
            }
            Event::Start(Tag::HtmlBlock) => html = Some((String::new(), Vec::new())),
            Event::Html(text) => {
                if let Some((block, starts)) = html.as_mut() {
                    starts.push((block.len(), skipped + range.start));
                    block.push_str(&text);
                }
            }
            Event::End(TagEnd::HtmlBlock) => {
                let Some((block, starts)) = html.take() else { continue };
                for (_, _, from, source) in html_diagrams(&block) {
                    let k = starts.partition_point(|s| s.0 <= from).max(1) - 1;
                    blocks.push(Block {
                        index: blocks.len() as u32 + 1,
                        line: lines.line_of(starts[k].1 + (from - starts[k].0)),
                        source,
                    });
                }
            }
            _ => {}
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

/// ART-5: the diagrams an HTML block writes as HTML — ought's form, mermaid
/// source in a `<pre class="mermaid">`: each `<pre>` whose class list holds
/// `mermaid`, to the first `</pre>` after it in the same block (one the
/// block does not close is no diagram). For each, where the element starts
/// and ends in `html`, where its source starts, and the source: the
/// element's content, the newline right after its open tag dropped (as
/// HTML's parser drops it), character references decoded (`&lt;` `&gt;`
/// `&amp;` `&quot;` `&apos;` `&nbsp;`, numeric ones) and the rest as
/// written — a `<br/>` in a label stays a `<br/>`, as mermaid's own `run`
/// reads a `.mermaid` element. Never in a comment, nor in the text of a
/// `script`, `style`, `textarea` or `title`.
fn html_diagrams(html: &str) -> Vec<(usize, usize, usize, String)> {
    /// Whether the tag name at `at` in `low` is `name`, whole.
    fn named(low: &str, at: usize, name: &str) -> bool {
        low[at..].starts_with(name)
            && low[at + name.len()..].chars().next().is_none_or(|c| c.is_ascii_whitespace() || c == '/' || c == '>')
    }
    /// The first `</name` at or after `from`: where it starts, and just past
    /// its `>`.
    fn close(low: &str, name: &str, from: usize) -> Option<(usize, usize)> {
        let mut i = from;
        while let Some(off) = low[i..].find("</") {
            let lt = i + off;
            if named(low, lt + 2, name) {
                return Some((lt, low[lt..].find('>').map_or(low.len(), |j| lt + j + 1)));
            }
            i = lt + 2;
        }
        None
    }
    /// The first `class` among a tag's attributes (what follows its name).
    fn class(attrs: &str) -> Option<&str> {
        let mut rest = attrs;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
            if rest.is_empty() {
                return None;
            }
            let end = rest.find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '/').unwrap_or(rest.len()).max(1);
            let name = &rest[..end];
            rest = rest[end..].trim_start();
            let mut value = "";
            if let Some(v) = rest.strip_prefix('=') {
                let v = v.trim_start();
                (value, rest) = match v.chars().next() {
                    Some(q @ ('"' | '\'')) => match v[1..].find(q) {
                        Some(j) => (&v[1..1 + j], &v[2 + j..]),
                        None => (&v[1..], ""),
                    },
                    _ => v.split_at(v.find(|c: char| c.is_ascii_whitespace()).unwrap_or(v.len())),
                };
            }
            if name.eq_ignore_ascii_case("class") {
                return Some(value);
            }
        }
    }
    /// Character references decoded: the named few, and numeric ones.
    fn decode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            rest = &rest[amp..];
            let decoded = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
                let c = match &rest[1..j] {
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "amp" => Some('&'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    n => n
                        .strip_prefix('#')
                        .and_then(|n| match n.strip_prefix(['x', 'X']) {
                            Some(hex) => u32::from_str_radix(hex, 16).ok(),
                            None => n.parse::<u32>().ok(),
                        })
                        .and_then(char::from_u32),
                };
                c.map(|c| (c, j + 1))
            });
            match decoded {
                Some((c, len)) => {
                    out.push(c);
                    rest = &rest[len..];
                }
                None => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
        }
        out.push_str(rest);
        out
    }
    // ASCII lowercasing keeps every offset.
    let low = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = low[i..].find('<') {
        let lt = i + off;
        if low[lt..].starts_with("<!--") {
            i = low[lt + 4..].find("-->").map_or(low.len(), |j| lt + 4 + j + 3);
            continue;
        }
        if !low[lt + 1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            i = lt + 1;
            continue;
        }
        let name_end = low[lt + 1..]
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .map_or(low.len(), |j| lt + 1 + j);
        // The open tag's `>`, outside quoted values.
        let mut quote = None;
        let Some(gt) = low[name_end..]
            .char_indices()
            .find(|&(_, c)| match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                    false
                }
                None if c == '"' || c == '\'' => {
                    quote = Some(c);
                    false
                }
                None => c == '>',
            })
            .map(|(j, _)| name_end + j + 1)
        else {
            break;
        };
        i = match &low[lt + 1..name_end] {
            name @ ("script" | "style" | "textarea" | "title") => close(&low, name, gt).map_or(low.len(), |(_, after)| after),
            "pre" if class(&html[name_end..gt - 1]).is_some_and(|c| c.split_ascii_whitespace().any(|w| w == "mermaid")) => {
                match close(&low, "pre", gt) {
                    Some((end, after)) => {
                        let nl = if low[gt..].starts_with("\r\n") { 2 } else { usize::from(low[gt..].starts_with('\n')) };
                        let from = gt + nl;
                        out.push((lt, after, from, decode(&html[from..end.max(from)])));
                        after
                    }
                    None => gt,
                }
            }
            _ => gt,
        };
    }
    out
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
    /// ART-5: the frame's script — what it draws.
    const SDK_JS: &str = include_str!("../../frontend/sdk/menger-sdk.js");
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
        // ART-5: html_diagrams, doc comment and all, read from md.rs.
        let start = MD_RS
            .find("/// ART-5: the diagrams an HTML block writes as HTML")
            .expect("md.rs holds html_diagrams");
        let end = start + MD_RS[start..].find("\nfn html_diagrams(").expect("its fn");
        let end = end + MD_RS[end..].find("\n}\n").expect("its end") + 3;
        assert!(
            THIS_RS.contains(&MD_RS[start..end]),
            "blocks.rs's html_diagrams is not md.rs's — re-copy it"
        );
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
    /// COMP-4: through `notes::BodyView`, which renders an unframed body
    /// with `body_view` and a framed one (a body holding HTML) in a frame —
    /// split first all the same, and drawn from the same placeholders
    /// (`render_body_framed` is `render`'s fence rule too), so the set of
    /// diagrams a body shows is unchanged.
    #[test]
    fn the_page_splits_the_frontmatter_first() {
        assert!(APP_MAIN_RS.contains("<notes::BodyView body="));
        assert!(APP_NOTES_RS.contains("md::is_framed(md::split_frontmatter(b).1)"));
        assert!(APP_NOTES_RS.contains("body.with(|b| body_view(b, refs))"));
        assert!(APP_NOTES_RS.contains("let (fm, md_src) = md::split_frontmatter(raw);"));
        assert!(APP_NOTES_RS.contains("md::render_body_with(md_src, "));
        assert!(APP_NOTES_RS.contains("md::render_body(md_src)"));
        assert!(APP_NOTES_RS.contains("let src = md::split_frontmatter(b).1;"));
        assert!(APP_NOTES_RS.contains("md::render_body_framed(src, "));
        // COMP-9: `render`'s last argument, `cards`, turns a definition block
        // into its docs card — a fence that is not mermaid either way.
        assert!(MD_RS.contains("pub fn render_body_framed(") && MD_RS.contains("render(src, Some(resolve), true, true)"));
    }

    /// ART-5: in a frame, a diagram written as HTML is drawn because md.rs
    /// turns it into a fence's placeholder — every HTML block gathered whole
    /// and handed to `html_diagrams` — and the frame draws placeholders and
    /// nothing else. If either changes (the SDK drawing `pre.mermaid` itself,
    /// say), what `check` finds must be looked at again.
    #[test]
    fn the_frame_draws_html_diagrams_through_md_rs() {
        assert!(MD_RS.contains("        if framed {\n            match &ev {\n                Event::Start(Tag::HtmlBlock) => {"));
        assert!(MD_RS.contains("return Some(Event::Html(with_diagrams(&block, &mut diagrams).into()));"));
        assert!(MD_RS.contains("let found = html_diagrams(block);"));
        assert!(MD_RS.contains("out.push_str(&mermaid_placeholder(&source));"));
        assert!(SDK_JS.contains("doc.querySelectorAll('.mermaid[data-state=\"pending\"]')"));
        assert_eq!(SDK_JS.matches("querySelectorAll('.mermaid").count(), 2, "pending, and done (a redraw)");
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
        // ART-5: the framed renderer's cases too — `render_body_framed("…",
        // &|_| None)`, a second argument skipped (and `render_body_with`'s).
        for fun in ["render_body(", "render_body_with(", "render_body_framed("] {
            let mut from = 0;
            while let Some(at) = tests[from..].find(fun) {
                let call = from + at;
                let arg = tests[call + fun.len()..].trim_start();
                from = call + 1;
                let (markdown, after) = if arg.starts_with('"') {
                    let (lit, len) = literal(arg);
                    (lit, &arg[len..])
                } else if let Some(after) = arg.strip_prefix("FLOW") {
                    (flow.clone(), after)
                } else {
                    continue; // `render_body(src)` — the helper, not a case
                };
                let after = after.trim_start();
                let after = match after.strip_prefix(',') {
                    // A trailing comma closes a one-argument call; anything
                    // else after one is the second argument, up to the `)`.
                    Some(rest) if !rest.trim_start().starts_with(')') => &rest[rest.find(')').expect("the call ends")..],
                    Some(rest) => rest.trim_start(),
                    None => after,
                };
                let after = after
                    .strip_prefix(')')
                    .expect("the call ends after its arguments");
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
        }
        cases
    }

    #[test]
    fn every_case_the_apps_tests_name_counts_the_same() {
        let cases = the_apps_cases();
        // render_body's (nine and more), and ART-5's thirteen framed ones.
        assert!(cases.len() >= 9 + 13, "read md.rs's cases: {cases:?}");
        // ART-5: md.rs's own cases of diagrams written as HTML are among them.
        assert!(cases.iter().filter(|(m, _)| m.contains("class=\"mermaid\"")).count() >= 8, "{cases:?}");
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

    // -- diagrams written as HTML (ART-5) --------------------------------------

    /// ought's form: the source is what md.rs puts in the placeholder — the
    /// newline after the open tag dropped, references decoded, a `<br/>`
    /// kept — and `line` is the line that source starts on.
    #[test]
    fn a_pre_mermaid_is_a_diagram_its_source_and_line_md_rss() {
        let body = "# T\n\n<pre class=\"mermaid\">\ngraph LR\n  A[\"a<br/>b\"] --&gt; B &amp; C\n</pre>\n";
        let blocks = mermaid_blocks(body);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source, "graph LR\n  A[\"a<br/>b\"] --> B & C\n");
        assert_eq!(blocks[0].line, 4);
        // On one line, the source starts on the tag's line.
        assert_eq!(lines("intro\n\n<pre class='mermaid'>graph TD; A-->B</pre>\n"), [3]);
        assert_eq!(sources("<pre class='mermaid'>graph TD; A-->B</pre>\n"), ["graph TD; A-->B"]);
    }

    /// Inside a field element and a component, beside fences, in a block
    /// quote and a list item: in order, each `line` the markdown's line its
    /// source starts on.
    #[test]
    fn diagrams_inside_html_bodies_are_found_in_order_with_their_lines() {
        let body = "\
---
status: Pending
---
<p data-field=\"confidence\"><em>confidence: medium</em></p>

<figure data-field=\"dag\">
  <pre class=\"mermaid\">
graph TD
  B1 --> B2
  </pre>
  <figcaption>The DAG.</figcaption>
</figure>

<section data-field=\"shape\">

```mermaid
graph LR
  A --> B
```

</section>

<m-dag data-field=\"lens\">
<pre class=\"mermaid\">pie
  \"a\" : 1</pre>
</m-dag>

<m-dag>

~~~mermaid
sequenceDiagram
  A->>B: hi
~~~

</m-dag>

> <pre class=\"mermaid\">
> graph TD
> </pre>

- item

  <pre class=\"mermaid\">
  graph LR
  </pre>
";
        let blocks = mermaid_blocks(body);
        let at = |needle: &str| body[..body.find(needle).unwrap()].matches('\n').count() as u32 + 1;
        assert_eq!(
            blocks.iter().map(|b| (b.index, b.line)).collect::<Vec<_>>(),
            [
                (1, at("graph TD\n  B1")),
                (2, at("graph LR\n  A --> B")),
                (3, at("<pre class=\"mermaid\">pie")),
                (4, at("sequenceDiagram")),
                (5, at("> graph TD")),
                (6, at("  graph LR\n  </pre>")),
            ]
        );
        assert_eq!(blocks[0].source, "graph TD\n  B1 --> B2\n  ");
        assert_eq!(blocks[2].source, "pie\n  \"a\" : 1");
        assert_eq!(blocks[4].source, "graph TD\n", "the quote's prefix is not the source's");
        assert_eq!(blocks[5].source, "graph LR\n");
    }

    /// Where a `<pre class="mermaid">` is no diagram — the page does not
    /// draw it, so `check` does not find it.
    #[test]
    fn a_pre_mermaid_the_page_does_not_draw_is_not_found() {
        for body in [
            "Write `<pre class=\"mermaid\">graph LR</pre>` to draw one.\n",
            "```html\n<pre class=\"mermaid\">graph LR</pre>\n```\n",
            "    <pre class=\"mermaid\">graph LR</pre>\n",
            "<!--\n<pre class=\"mermaid\">graph LR</pre>\n-->\n",
            "<script>\nconst d = '<pre class=\"mermaid\">graph LR</pre>';\n</script>\n",
            "<pre class=\"not-mermaid\">graph LR</pre>\n<pre>graph LR</pre>\n",
            "text <pre class=\"mermaid\">graph LR</pre> inline\n",
            "<section>\n<pre class=\"mermaid\">\ngraph LR\n\n  A --> B\n</pre>\n</section>\n",
            "---\nnote: <pre class=\"mermaid\">graph LR</pre>\n---\nbody\n",
        ] {
            assert!(mermaid_blocks(body).is_empty(), "{body:?}");
        }
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
