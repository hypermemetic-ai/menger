//! TICKETS-7: **what changed between two versions of a ticket's text** —
//! a line diff (Myers' O(ND), over lines interned to numbers), the words
//! within a changed line (the same diff over a line's words), cut into
//! hunks with three lines of context; and a body's visible text, for a diff
//! of what a reader sees rather than of its source ([`visible`]).
//!
//! Wasm-safe and pure: the store answers `revisions diff` with it, and the
//! app could diff with it too. **Bounded**: a diff stops looking for the
//! shortest edit past [`MAX_EDITS`] edits and says the rest of the region
//! was replaced (every line of it out, every line in) — two unrelated 4 MB
//! mockups cost a scan, never minutes; and an answer holds at most
//! [`MAX_LINES`] lines (`truncated` says so).

use serde::{Deserialize, Serialize};

/// The most edits a diff searches for the shortest script before it calls
/// the rest of the region replaced.
pub const MAX_EDITS: usize = 1000;
/// The most lines an answer's hunks hold.
pub const MAX_LINES: usize = 4000;
/// Lines of context around a change.
pub const CONTEXT: usize = 3;
/// A changed line longer than this is not diffed by its words.
const WORDS_MAX_LINE: usize = 2000;

/// One line's (or one word run's) part in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiffOp {
    /// In both.
    Same,
    /// Only in the newer.
    Add,
    /// Only in the older.
    Del,
}

/// A run of a changed line's text: in both, only the newer's, or only the
/// older's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WordRun {
    pub op: DiffOp,
    pub text: String,
}

/// One line of a hunk. `words`: for a line changed in place (a `del` beside
/// the `add` that replaced it), its runs — the `del`'s same and deleted
/// text, the `add`'s same and added; empty otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiffLine {
    pub op: DiffOp,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<WordRun>,
}

/// A region that changed, with its context: where it starts in each version
/// (1-based lines) and how many lines of each it spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// A text's line diff: lines added and removed, the hunks, and whether the
/// hunks stop short of the whole ([`MAX_LINES`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TextDiff {
    pub added: u32,
    pub removed: u32,
    pub hunks: Vec<Hunk>,
    #[serde(default)]
    pub truncated: bool,
}

/// The shortest edit script from `a` to `b` (each element compared by
/// equality), as ops in order — or, past [`MAX_EDITS`] in the region after
/// their common head and tail, that region all out and then all in.
pub fn script<T: PartialEq>(a: &[T], b: &[T], max: usize) -> Vec<DiffOp> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (ra, rb) = (&a[head..], &b[head..]);
    let tail = ra.iter().rev().zip(rb.iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&ra[..ra.len() - tail], &rb[..rb.len() - tail]);
    let mut out = vec![DiffOp::Same; head];
    match myers(ma, mb, max) {
        Some(mid) => out.extend(mid),
        None => {
            out.extend(std::iter::repeat_n(DiffOp::Del, ma.len()));
            out.extend(std::iter::repeat_n(DiffOp::Add, mb.len()));
        }
    }
    out.extend(std::iter::repeat_n(DiffOp::Same, tail));
    out
}

/// Myers' greedy forward search with its trace; `None` past `max` edits.
fn myers<T: PartialEq>(a: &[T], b: &[T], max: usize) -> Option<Vec<DiffOp>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    if n == 0 || m == 0 {
        let mut out = vec![DiffOp::Del; a.len()];
        out.extend(std::iter::repeat_n(DiffOp::Add, b.len()));
        return Some(out);
    }
    let limit = ((n + m) as usize).min(max) as isize;
    let off = limit + 1;
    let mut v = vec![0isize; (2 * off + 1) as usize];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=limit {
        for k in (-d..=d).step_by(2) {
            let ix = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[ix - 1] < v[ix + 1]) { v[ix + 1] } else { v[ix - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[ix] = x;
            if x >= n && y >= m {
                trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
                found = Some(d);
                break 'outer;
            }
        }
        trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
    }
    let d_end = found?;
    // Walk the trace back from (n, m).
    let mut ops = Vec::with_capacity((n + m) as usize);
    let (mut x, mut y) = (n, m);
    for d in (1..=d_end).rev() {
        let prev = &trace[(d - 1) as usize];
        let at = |k: isize| prev[(k + (d - 1)) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) { k + 1 } else { k - 1 };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(DiffOp::Same);
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            ops.push(DiffOp::Add);
            y -= 1;
        } else {
            ops.push(DiffOp::Del);
            x -= 1;
        }
    }
    while x > 0 && y > 0 {
        ops.push(DiffOp::Same);
        x -= 1;
        y -= 1;
    }
    ops.reverse();
    Some(ops)
}

/// A text's lines, without their endings.
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n').unwrap_or(text).split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// Lines as numbers — equal lines, equal numbers — so the search compares
/// integers.
fn interned<'a>(a: &[&'a str], b: &[&'a str]) -> (Vec<u32>, Vec<u32>) {
    let mut ids: std::collections::HashMap<&'a str, u32> = std::collections::HashMap::new();
    let mut id = |l: &'a str| {
        let next = ids.len() as u32;
        *ids.entry(l).or_insert(next)
    };
    let a: Vec<u32> = a.iter().map(|l| id(l)).collect();
    let b: Vec<u32> = b.iter().map(|l| id(l)).collect();
    (a, b)
}

/// Only the counts: lines added and removed from `a` to `b`.
pub fn counts(a: &str, b: &str) -> (u32, u32) {
    let (la, lb) = (split_lines(a), split_lines(b));
    let (ia, ib) = interned(&la, &lb);
    let ops = script(&ia, &ib, MAX_EDITS);
    let added = ops.iter().filter(|o| **o == DiffOp::Add).count() as u32;
    let removed = ops.iter().filter(|o| **o == DiffOp::Del).count() as u32;
    (added, removed)
}

/// A line's words: runs of letters and digits, of spaces, and each other
/// character alone.
fn words(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let class = |c: char| if c.is_alphanumeric() || c == '_' { 0 } else if c.is_whitespace() { 1 } else { 2 };
    let mut prev: Option<u8> = None;
    for (i, c) in line.char_indices() {
        let k = class(c);
        if let Some(p) = prev {
            if p != k || k == 2 {
                out.push(&line[start..i]);
                start = i;
            }
        }
        prev = Some(k);
    }
    if start < line.len() {
        out.push(&line[start..]);
    }
    out
}

/// The runs of a line changed in place: the older's (same and deleted) and
/// the newer's (same and added).
fn word_runs(old: &str, new: &str) -> (Vec<WordRun>, Vec<WordRun>) {
    let (wa, wb) = (words(old), words(new));
    let ops = script(&wa, &wb, 200);
    let (mut ra, mut rb): (Vec<WordRun>, Vec<WordRun>) = (Vec::new(), Vec::new());
    let push = |runs: &mut Vec<WordRun>, op: DiffOp, w: &str| match runs.last_mut() {
        Some(last) if last.op == op => last.text.push_str(w),
        _ => runs.push(WordRun { op, text: w.to_string() }),
    };
    let (mut i, mut j) = (0, 0);
    for op in ops {
        match op {
            DiffOp::Same => {
                push(&mut ra, DiffOp::Same, wa[i]);
                push(&mut rb, DiffOp::Same, wb[j]);
                i += 1;
                j += 1;
            }
            DiffOp::Del => {
                push(&mut ra, DiffOp::Del, wa[i]);
                i += 1;
            }
            DiffOp::Add => {
                push(&mut rb, DiffOp::Add, wb[j]);
                j += 1;
            }
        }
    }
    (ra, rb)
}

/// The line diff from `a` to `b`, cut into hunks with [`CONTEXT`] lines of
/// context; a line changed in place carries its words.
pub fn text(a: &str, b: &str) -> TextDiff {
    let (la, lb) = (split_lines(a), split_lines(b));
    let (ia, ib) = interned(&la, &lb);
    let ops = script(&ia, &ib, MAX_EDITS);
    // Each op with the line it stands at in each version.
    let mut at = Vec::with_capacity(ops.len());
    let (mut i, mut j) = (0usize, 0usize);
    for op in &ops {
        at.push((*op, i, j));
        match op {
            DiffOp::Same => {
                i += 1;
                j += 1;
            }
            DiffOp::Del => i += 1,
            DiffOp::Add => j += 1,
        }
    }
    let added = ops.iter().filter(|o| **o == DiffOp::Add).count() as u32;
    let removed = ops.iter().filter(|o| **o == DiffOp::Del).count() as u32;
    let mut out = TextDiff { added, removed, hunks: Vec::new(), truncated: false };
    let changed: Vec<usize> = at.iter().enumerate().filter(|(_, (op, ..))| *op != DiffOp::Same).map(|(k, _)| k).collect();
    if changed.is_empty() {
        return out;
    }
    // The ranges of ops each hunk spans: a change and its context, merged
    // with the next where their contexts touch.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for k in changed {
        let (s, e) = (k.saturating_sub(CONTEXT), (k + CONTEXT + 1).min(at.len()));
        match ranges.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => ranges.push((s, e)),
        }
    }
    let mut lines_out = 0usize;
    for (s, e) in ranges {
        if lines_out >= MAX_LINES {
            out.truncated = true;
            break;
        }
        let (_, oi, nj) = at[s];
        let mut hunk = Hunk { old_start: oi as u32 + 1, old_lines: 0, new_start: nj as u32 + 1, new_lines: 0, lines: Vec::new() };
        let mut k = s;
        while k < e {
            if lines_out >= MAX_LINES {
                out.truncated = true;
                break;
            }
            let (op, oi, _) = at[k];
            match op {
                DiffOp::Same => {
                    hunk.lines.push(DiffLine { op, text: la[oi].to_string(), words: Vec::new() });
                    hunk.old_lines += 1;
                    hunk.new_lines += 1;
                    lines_out += 1;
                    k += 1;
                }
                _ => {
                    // A run of changes: its deletions, then its additions, each
                    // deletion paired with the addition at its place.
                    let run_end = (k..e).find(|&x| at[x].0 == DiffOp::Same).unwrap_or(e);
                    let dels: Vec<usize> = (k..run_end).filter(|&x| at[x].0 == DiffOp::Del).map(|x| at[x].1).collect();
                    let adds: Vec<usize> = (k..run_end).filter(|&x| at[x].0 == DiffOp::Add).map(|x| at[x].2).collect();
                    let paired = dels.len().min(adds.len());
                    let mut del_lines = Vec::new();
                    let mut add_lines = Vec::new();
                    for (n, &d) in dels.iter().enumerate() {
                        let words = if n < paired && la[d].len() <= WORDS_MAX_LINE && lb[adds[n]].len() <= WORDS_MAX_LINE {
                            word_runs(la[d], lb[adds[n]]).0
                        } else {
                            Vec::new()
                        };
                        del_lines.push(DiffLine { op: DiffOp::Del, text: la[d].to_string(), words });
                    }
                    for (n, &a_) in adds.iter().enumerate() {
                        let words = if n < paired && la[dels[n]].len() <= WORDS_MAX_LINE && lb[a_].len() <= WORDS_MAX_LINE {
                            word_runs(la[dels[n]], lb[a_]).1
                        } else {
                            Vec::new()
                        };
                        add_lines.push(DiffLine { op: DiffOp::Add, text: lb[a_].to_string(), words });
                    }
                    hunk.old_lines += del_lines.len() as u32;
                    hunk.new_lines += add_lines.len() as u32;
                    for l in del_lines.into_iter().chain(add_lines) {
                        if lines_out >= MAX_LINES {
                            out.truncated = true;
                            break;
                        }
                        hunk.lines.push(l);
                        lines_out += 1;
                    }
                    k = run_end;
                }
            }
        }
        out.hunks.push(hunk);
    }
    out
}

/// A body's text as a reader sees it, line by line: no frontmatter, no
/// markup (`<…>` tags, and whatever a `<script>` or `<style>` holds), no
/// markdown markers at a line's start, no blank lines — what a diff "of the
/// rendered text" compares.
pub fn visible(body: &str) -> String {
    let (_, rest) = crate::fields::split_frontmatter(body);
    let mut out = String::with_capacity(rest.len() / 2);
    let mut skipping: Option<&'static str> = None;
    for line in rest.lines() {
        let mut text = String::new();
        let mut s = line;
        loop {
            if let Some(end) = skipping {
                match s.to_ascii_lowercase().find(end) {
                    Some(i) => {
                        s = &s[i + end.len()..];
                        skipping = None;
                    }
                    None => break,
                }
            }
            match s.find('<') {
                None => {
                    text.push_str(s);
                    break;
                }
                Some(i) => {
                    text.push_str(&s[..i]);
                    let tag = &s[i..];
                    let lower = tag.to_ascii_lowercase();
                    if lower.starts_with("<script") {
                        skipping = Some("</script>");
                    } else if lower.starts_with("<style") {
                        skipping = Some("</style>");
                    }
                    match tag.find('>') {
                        Some(j) => {
                            text.push(' ');
                            s = &tag[j + 1..];
                        }
                        None => break,
                    }
                }
            }
        }
        let t = text.trim();
        let t = t.trim_start_matches(['#', '>']).trim_start();
        let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
        let t = t.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("**", "").replace('`', "");
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() && t.chars().any(|c| c.is_alphanumeric()) {
            out.push_str(&t);
            out.push('\n');
        }
    }
    out
}

/// A frontmatter's keys and their values, in order — a key's continuation
/// lines (indented, or list items) with it.
pub fn frontmatter(body: &str) -> Vec<(String, String)> {
    let (Some(fm), _) = crate::fields::split_frontmatter(body) else { return Vec::new() };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in fm.lines() {
        let continued = line.starts_with(' ') || line.starts_with('\t') || line.starts_with('-');
        match (continued, line.split_once(':')) {
            (false, Some((k, v))) if !k.trim().is_empty() => out.push((k.trim().to_string(), v.trim().to_string())),
            _ => {
                if let Some(last) = out.last_mut() {
                    if !last.1.is_empty() {
                        last.1.push('\n');
                    }
                    last.1.push_str(line.trim_end());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(a: &[&str], b: &[&str], ops: &[DiffOp]) -> Vec<String> {
        // Replaying the script on `a` gives `b`.
        let (mut i, mut j, mut out) = (0, 0, Vec::new());
        for op in ops {
            match op {
                DiffOp::Same => {
                    assert_eq!(a[i], b[j]);
                    out.push(a[i].to_string());
                    i += 1;
                    j += 1;
                }
                DiffOp::Del => i += 1,
                DiffOp::Add => {
                    out.push(b[j].to_string());
                    j += 1;
                }
            }
        }
        assert_eq!((i, j), (a.len(), b.len()));
        out
    }

    #[test]
    fn a_script_turns_one_into_the_other_and_is_shortest() {
        let a = ["a", "b", "c", "a", "b", "b", "a"];
        let b = ["c", "b", "a", "b", "a", "c"];
        let ops = script(&a, &b, MAX_EDITS);
        assert_eq!(apply(&a, &b, &ops), b.to_vec());
        // Myers' example: D = 5.
        assert_eq!(ops.iter().filter(|o| **o != DiffOp::Same).count(), 5);
        assert_eq!(script::<&str>(&[], &[], 10), vec![]);
        assert_eq!(script(&["x"], &[], 10), vec![DiffOp::Del]);
    }

    #[test]
    fn past_the_bound_the_region_is_replaced() {
        let a: Vec<u32> = (0..50).collect();
        let b: Vec<u32> = (100..150).collect();
        let ops = script(&a, &b, 10);
        assert_eq!(ops.iter().filter(|o| **o == DiffOp::Del).count(), 50);
        assert_eq!(ops.iter().filter(|o| **o == DiffOp::Add).count(), 50);
    }

    #[test]
    fn counts_and_hunks_with_words() {
        let a = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
        let b = "one\ntwo\nthree\nfour\nFIVE five\nsix\nseven\neight\nnine\nten\neleven\n";
        assert_eq!(counts(a, b), (2, 1));
        let d = text(a, b);
        assert_eq!((d.added, d.removed), (2, 1));
        assert_eq!(d.hunks.len(), 1, "{d:?}");
        let h = &d.hunks[0];
        assert_eq!((h.old_start, h.new_start), (2, 2));
        let del = h.lines.iter().find(|l| l.op == DiffOp::Del).unwrap();
        assert_eq!(del.text, "five");
        let add = h.lines.iter().find(|l| l.op == DiffOp::Add).unwrap();
        assert_eq!(add.words.first().map(|w| (w.op, w.text.as_str())), Some((DiffOp::Add, "FIVE ")));
        assert!(text(a, a).hunks.is_empty());
    }

    #[test]
    fn far_apart_changes_are_two_hunks() {
        let a: String = (0..40).map(|i| format!("l{i}\n")).collect();
        let b = a.replace("l2\n", "x\n").replace("l30\n", "y\n");
        assert_eq!(text(&a, &b).hunks.len(), 2);
    }

    #[test]
    fn the_visible_text_drops_markup_and_scripts() {
        let body = "---\ntheme: x\n---\n# Title\n\n<m-field data-field=\"a\">**Hello** &amp; bye</m-field>\n<script>\nlet x = 1;\n</script>\n- item\n";
        assert_eq!(visible(body), "Title\nHello & bye\nitem\n");
    }

    #[test]
    fn frontmatter_keys() {
        let body = "---\ntheme: ocean\nwidths:\n  - 390\n  - 1440\n---\nbody";
        assert_eq!(frontmatter(body), vec![("theme".into(), "ocean".into()), ("widths".into(), "  - 390\n  - 1440".into())]);
        assert!(frontmatter("no front").is_empty());
    }
}
