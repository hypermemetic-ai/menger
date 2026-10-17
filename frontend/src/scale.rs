//! CHAT-8: a chat at scale — what the conversation fetches, mounts and keeps.
//!
//! - **Pages sized to the window** ([`page_size`]): `ceil(viewport / 24 px)
//!   × 2` messages, at least 20 and at most 200; the first from the newest
//!   end (or around the read mark, `?at=`), older ones the same size as you
//!   scroll up.
//! - **Only what is near the viewport is mounted** ([`Layout`]): the held
//!   messages within [`OVERSCAN`] viewports of the visible ones are drawn;
//!   the rest are a spacer above and below of their measured heights (an
//!   estimate until measured), so the scrollbar is honest over what is held.
//! - **What you read stays put** ([`Layout::anchor`], [`Layout::restore`]):
//!   the first visible message and its offset are kept as you scroll, and
//!   every change of the list or of a height puts the scroll back on it — a
//!   page prepended above, a message measured, a long one shown whole.
//! - **What is held is bounded by the window** ([`trim`]): past
//!   [`HELD_PAGES`] pages, the far end is dropped, to be fetched again on
//!   return.
//! - **One huge message** ([`split_long`]): past [`LONG_LINES`] lines it
//!   shows its first block-bounded 200 lines and **show all N lines**, which
//!   renders the rest in chunks of blocks across tasks, never one layout.
//!
//! All of it is pure and pinned by native tests; src/chat_pane.rs is the DOM.

use std::collections::HashMap;

/// The messages a page holds, for a viewport `h` px tall.
pub(crate) fn page_size(h: f64) -> u32 {
    let n = (h.max(0.0) / 24.0).ceil() as u32 * 2;
    n.clamp(20, 200)
}

/// How many viewports above and below the visible ones are mounted.
pub(crate) const OVERSCAN: f64 = 2.0;
/// How many pages are held before the far end is dropped.
pub(crate) const HELD_PAGES: usize = 6;
/// A message's height before it is measured.
pub(crate) const ESTIMATE: f64 = 72.0;
/// Past this many lines a message shows its head and *show all*.
pub(crate) const LONG_LINES: usize = 200;
/// How many lines *show all* renders a task.
pub(crate) const CHUNK_LINES: usize = 400;

/// The held messages' heights and where each begins.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub(crate) ids: Vec<u64>,
    /// `tops[i]` is where message `i` begins; `tops[n]` the whole height.
    pub(crate) tops: Vec<f64>,
}

impl Layout {
    pub(crate) fn new(ids: Vec<u64>, heights: &HashMap<u64, f64>) -> Self {
        let mut tops = Vec::with_capacity(ids.len() + 1);
        let mut y = 0.0;
        for id in &ids {
            tops.push(y);
            y += heights.get(id).copied().unwrap_or(ESTIMATE);
        }
        tops.push(y);
        Layout { ids, tops }
    }

    pub(crate) fn total(&self) -> f64 {
        self.tops.last().copied().unwrap_or(0.0)
    }

    pub(crate) fn top_of(&self, id: u64) -> Option<f64> {
        self.ids.iter().position(|i| *i == id).map(|i| self.tops[i])
    }

    /// The range of messages to mount for a scroller at `scroll_top`
    /// showing `view` px (relative to the list's start): those within
    /// [`OVERSCAN`] viewports of the visible ones.
    pub(crate) fn mounted(&self, scroll_top: f64, view: f64) -> (usize, usize) {
        let n = self.ids.len();
        if n == 0 {
            return (0, 0);
        }
        let lo = scroll_top - OVERSCAN * view;
        let hi = scroll_top + view + OVERSCAN * view;
        // The first whose bottom is past `lo`; the first whose top is past `hi`.
        let i0 = self.tops[1..].partition_point(|&b| b <= lo).min(n);
        let i1 = self.tops[..n].partition_point(|&t| t < hi).max(i0);
        (i0, i1)
    }

    /// What is being read: the first message whose bottom is below the
    /// scroll top, and how far the scroll top is into it.
    pub(crate) fn anchor(&self, scroll_top: f64) -> Option<(u64, f64)> {
        let n = self.ids.len();
        let i = self.tops[1..].partition_point(|&b| b <= scroll_top);
        (i < n).then(|| (self.ids[i], scroll_top - self.tops[i]))
    }

    /// The scroll top that puts `anchor` back where it was — `None` when
    /// its message is no longer held.
    pub(crate) fn restore(&self, anchor: (u64, f64)) -> Option<f64> {
        self.top_of(anchor.0).map(|t| (t + anchor.1).max(0.0))
    }
}

/// Which end of the held window to drop, past [`HELD_PAGES`] pages of
/// `page` — the end farther from the visible range `[v0, v1)` of `n` —
/// and how many: never within a page of what is visible.
pub(crate) fn trim(n: usize, v0: usize, v1: usize, page: usize) -> Option<(Side, usize)> {
    let max = HELD_PAGES * page;
    if n <= max {
        return None;
    }
    let above = v0;
    let below = n.saturating_sub(v1);
    let excess = n - max;
    let (side, room) = if above >= below { (Side::Older, above) } else { (Side::Newer, below) };
    let drop = excess.min(room.saturating_sub(page));
    (drop > 0).then_some((side, drop))
}

/// An end of the held window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Older,
    Newer,
}

/// A long message's parts: the head shown at first (its first
/// [`LONG_LINES`] lines, cut at a block's end outside any fence), the rest
/// in chunks of about [`CHUNK_LINES`] lines (each a run of whole blocks),
/// and its line count. A message no longer than that is all head.
pub(crate) fn split_long(body: &str) -> (&str, Vec<&str>, usize) {
    let total = body.lines().count();
    if total <= LONG_LINES {
        return (body, Vec::new(), total);
    }
    let cuts = block_ends(body);
    let head_end = cuts.iter().copied().find(|&(_, line)| line >= LONG_LINES).map_or(body.len(), |(at, _)| at);
    let mut chunks = Vec::new();
    let mut start = head_end;
    let mut start_line = cuts.iter().find(|(at, _)| *at == head_end).map_or(total, |(_, l)| *l);
    for &(at, line) in &cuts {
        if at <= start {
            continue;
        }
        if line - start_line >= CHUNK_LINES {
            chunks.push(&body[start..at]);
            start = at;
            start_line = line;
        }
    }
    if start < body.len() && !body[start..].trim().is_empty() {
        chunks.push(&body[start..]);
    }
    (&body[..head_end], chunks, total)
}

/// Each place a top-level block ends outside a fence: the byte just past a
/// blank line (or a fence's closing line), and the lines before it.
fn block_ends(body: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut at = 0;
    for (n, line) in body.split_inclusive('\n').enumerate() {
        at += line.len();
        let t = line.trim();
        let marks = t.chars().next().filter(|c| *c == '`' || *c == '~').map(|c| (c, t.chars().take_while(|x| *x == c).count()));
        match (fence, marks) {
            (None, Some((c, k))) if k >= 3 => fence = Some((c, k)),
            (Some((c, k)), Some((c2, k2))) if c == c2 && k2 >= k && t.chars().all(|x| x == c) => {
                fence = None;
                out.push((at, n + 1));
            }
            (None, _) if t.is_empty() => out.push((at, n + 1)),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page-size rule: twice the lines a viewport holds at 24 px, 20 to 200.
    #[test]
    fn a_page_is_sized_to_the_window() {
        assert_eq!(page_size(900.0), 76);
        assert_eq!(page_size(844.0), 72);
        assert_eq!(page_size(100.0), 20, "at least 20");
        assert_eq!(page_size(4000.0), 200, "at most 200");
        assert_eq!(page_size(0.0), 20);
    }

    fn layout(n: u64, h: f64) -> Layout {
        let heights = (1..=n).map(|i| (i, h)).collect();
        Layout::new((1..=n).collect(), &heights)
    }

    /// Only what is within two viewports of the visible is mounted; the
    /// same however long the chat.
    #[test]
    fn only_the_near_ones_are_mounted() {
        let l = layout(1000, 50.0);
        assert_eq!(l.total(), 50_000.0);
        // At the bottom of a 500 px view: 10 visible, 20 more above, nothing below.
        let (i0, i1) = l.mounted(49_500.0, 500.0);
        assert_eq!((i0, i1), (970, 1000));
        let mid = l.mounted(25_000.0, 500.0);
        assert_eq!(mid, (480, 530), "two viewports each side");
        let big = layout(100_000, 50.0).mounted(2_500_000.0, 500.0);
        assert_eq!(big.1 - big.0, mid.1 - mid.0, "flat across sizes");
        assert_eq!(Layout::new(Vec::new(), &HashMap::new()).mounted(0.0, 500.0), (0, 0));
    }

    /// The anchor-on-prepend rule: a page put above, or a height measured,
    /// puts the scroll back on what was being read.
    #[test]
    fn what_you_read_stays_put() {
        let mut heights: HashMap<u64, f64> = (100..=150).map(|i| (i, 40.0)).collect();
        let before = Layout::new((100..=150).collect(), &heights);
        let a = before.anchor(410.0).unwrap();
        assert_eq!(a, (110, 10.0), "message 110, 10 px into it");
        // Older messages 60..100 prepended, unmeasured (the estimate).
        let after = Layout::new((60..=150).collect(), &heights);
        assert_eq!(after.restore(a), Some(40.0 * ESTIMATE + 410.0));
        // Then they are measured at 30 px each: the scroll follows again.
        heights.extend((60..100).map(|i| (i, 30.0)));
        let measured = Layout::new((60..=150).collect(), &heights);
        assert_eq!(measured.restore(a), Some(40.0 * 30.0 + 410.0));
        // A message above the anchor grows (shown whole): the anchor holds.
        heights.insert(105, 400.0);
        let grown = Layout::new((60..=150).collect(), &heights);
        assert_eq!(grown.restore(a), Some(40.0 * 30.0 + 410.0 + 360.0));
        // Dropped from what is held: no anchor to go back to.
        assert_eq!(Layout::new((120..=150).collect(), &heights).restore(a), None);
    }

    /// What is held stays within six pages: the far end goes, never within
    /// a page of what is visible.
    #[test]
    fn what_is_held_is_bounded() {
        assert_eq!(trim(100, 80, 100, 20), None, "within six pages");
        assert_eq!(trim(160, 140, 160, 20), Some((Side::Older, 40)), "reading the newest: the oldest go");
        assert_eq!(trim(160, 0, 20, 20), Some((Side::Newer, 40)), "reading back: the newest go");
        assert_eq!(trim(130, 15, 25, 20), Some((Side::Newer, 10)));
    }

    /// One huge message: its first 200 lines (at a block's end), then the
    /// rest in chunks of blocks — every line once, no fence cut.
    #[test]
    fn show_all_renders_in_chunks() {
        let mut body = String::new();
        for i in 0..5_000 {
            body.push_str(&format!("line {i}\n"));
            if i % 10 == 9 {
                body.push('\n');
            }
            if i % 500 == 195 {
                body.push_str("```\ncode a\ncode b\n\ncode c\n```\n");
            }
        }
        let (head, chunks, total) = split_long(&body);
        assert!(total > 5_000);
        let head_lines = head.lines().count();
        assert!((LONG_LINES..LONG_LINES + 20).contains(&head_lines), "{head_lines}");
        assert_eq!(head.matches("```").count() % 2, 0, "no fence cut in the head");
        for c in &chunks {
            assert_eq!(c.matches("```").count() % 2, 0, "no fence cut in a chunk");
            assert!(c.lines().count() <= CHUNK_LINES + 20, "{}", c.lines().count());
        }
        assert!(chunks.len() >= 10);
        let whole: String = std::iter::once(head).chain(chunks.iter().copied()).collect();
        assert_eq!(whole, body, "every line once, in order");
        let short = "a\nb\n";
        assert_eq!(split_long(short), (short, Vec::new(), 2));
    }
}
