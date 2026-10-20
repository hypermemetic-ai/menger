//! APP-70: the drawing pad — drawing as a form of input (owner, 2026-10-18,
//! APP-68: *"I want the ability to draw as a form of input"*).
//!
//! - **Where**: every box that takes images ([`crate::media::Attach`]) has
//!   *draw* beside *Image*; it opens this pad inside the box, above the text
//!   ([`PadView`]). The chat's composer, a comment box, a reply, the
//!   annotation box, the tray's Request changes and an input's text reply
//!   all have it, because they all hold an `Attach`.
//! - **Tools**: pen, highlighter, arrow, box, text and eraser; the five
//!   token colours ([`Ink`]: `--ink`, `--urgent`, `--accent`, `--live`,
//!   `--ask`); undo / redo (⌘Z / ⇧⌘Z,
//!   `draw.undo` / `draw.redo` in [`Context::Draw`]) and clear. A pencil's
//!   pressure (`PointerEvent.pressure`, pointer type `pen`) thickens the pen.
//! - **Vectors**: a drawing is its strokes ([`Drawing`]), in units of a
//!   surface 1,000 wide ([`UNITS`]), each in a colour *token*, so it is
//!   painted again in whatever theme is worn — the pad on a theme change,
//!   and a sent drawing in every reader's theme ([`repaint_all`], from
//!   `scopes::repaint`). The eraser takes out whole strokes it touches
//!   ([`hits`]), one undo step per sweep.
//! - **Sending**: the box's send (or Save) turns the pad into a PNG — the
//!   drawing on the box's `--card`, at twice its size — with the strokes as
//!   JSON *inside* it, an `iTXt` chunk keyed [`KEYWORD`] ([`embed`]), and
//!   puts it with `media.put` like any pasted image (CHAT-30's spinner,
//!   retry, remove); its markdown goes in the text and the send goes when it
//!   is stored ([`crate::media::Attach::then`]). `media.put` keeps an
//!   image's bytes as they are (content-addressed), so the strokes travel
//!   with the image, in one record, and anything outside the app sees a
//!   plain PNG.
//! - **Editing again**: an image whose bytes carry strokes ([`learn`]) opens
//!   full size with *edit drawing*; that opens the pad of the nearest box
//!   with its strokes. A drawing still in the draft is taken out of it and
//!   comes back edited; a sent one is never changed — the edit is a new
//!   image.
//! - **Marking up an image** (APP-104): any image opened full size offers
//!   *mark up* ([`marker`]); it opens the nearest box's pad **over that
//!   image** — the image the pad's background at its aspect ([`Bg`]), the
//!   strokes over it with the same tools, inks, undo and pressure. The send
//!   flattens image and strokes to one PNG (scaled down until it fits
//!   [`MAX_BYTES`](menger_tickets_api::media::MAX_BYTES)), its strokes kept
//!   as before with the source image's hash ([`Source`]), so *edit drawing*
//!   on it opens the photo and the strokes apart again. A marked-up image
//!   is never re-painted for the theme: the photo is what it is.
//! - **Motion**: nothing on the pad moves but the ink under the pointer.
//!
//! What decides — the stroke model, undo and redo, the eraser's hit test,
//! the JSON and the PNG chunk — is pure and pinned by native tests; the rest
//! is the DOM.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use leptos::html::{Canvas, Input};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::bindings::{self, Context};

// ── the model ───────────────────────────────────────────────────────────

/// A drawing's width, in its own units: every point and width is in these,
/// so a drawing made on a phone and one made on a desk are drawn alike.
pub(crate) const UNITS: f64 = 1000.0;

/// The PNG text chunk's keyword the strokes travel under.
pub(crate) const KEYWORD: &str = "menger:strokes";

/// The drawing format's version.
pub(crate) const VERSION: u32 = 1;

/// A tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Tool {
    Pen,
    Highlighter,
    Arrow,
    Box,
    Text,
    Eraser,
}

impl Tool {
    pub(crate) const ALL: [Tool; 6] = [Tool::Pen, Tool::Highlighter, Tool::Arrow, Tool::Box, Tool::Text, Tool::Eraser];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tool::Pen => "pen",
            Tool::Highlighter => "highlighter",
            Tool::Arrow => "arrow",
            Tool::Box => "box",
            Tool::Text => "text",
            Tool::Eraser => "eraser",
        }
    }

    /// Its line's width (a text's size, the eraser's reach), in CSS pixels
    /// on the pad as drawn.
    pub(crate) fn px(self) -> f64 {
        match self {
            Tool::Pen | Tool::Arrow | Tool::Box => 3.0,
            Tool::Highlighter => 16.0,
            Tool::Text => 18.0,
            Tool::Eraser => 24.0,
        }
    }

    /// A tool drawn from where it lands to where it lifts.
    pub(crate) fn two_point(self) -> bool {
        matches!(self, Tool::Arrow | Tool::Box)
    }

    fn icon(self) -> &'static str {
        match self {
            Tool::Pen => r#"<path d="M4 20l4-1 11-11-3-3L5 16z"/><path d="M14 6l3 3"/>"#,
            Tool::Highlighter => r#"<path d="M9 15l-4 5h6l2-2"/><path d="M8 13l7-9 5 4-8 8z"/>"#,
            Tool::Arrow => r#"<path d="M5 19L19 5"/><path d="M10 5h9v9"/>"#,
            Tool::Box => r#"<rect x="4" y="5" width="16" height="14" rx="2"/>"#,
            Tool::Text => r#"<path d="M5 6h14"/><path d="M12 6v13"/><path d="M9 19h6"/>"#,
            Tool::Eraser => r#"<path d="M8 20h12"/><path d="M4 15l9-10 7 6-8 9H8z"/><path d="M9 10l7 6"/>"#,
        }
    }
}

/// A colour: one of the theme's tokens, never a value — a stroke is
/// painted in the theme worn when it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Ink {
    Ink,
    Urgent,
    Accent,
    Live,
    Ask,
}

impl Ink {
    pub(crate) const ALL: [Ink; 5] = [Ink::Ink, Ink::Urgent, Ink::Accent, Ink::Live, Ink::Ask];

    pub(crate) fn token(self) -> &'static str {
        match self {
            Ink::Ink => "--ink",
            Ink::Urgent => "--urgent",
            Ink::Accent => "--accent",
            Ink::Live => "--live",
            Ink::Ask => "--ask",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Ink::Ink => "ink",
            Ink::Urgent => "urgent",
            Ink::Accent => "accent",
            Ink::Live => "live",
            Ink::Ask => "ask",
        }
    }
}

/// The colour a stroke of `tool` takes when `ink` is picked: a highlighter
/// in ink would hide what it marks, so it is `--ask`'s.
pub(crate) fn ink_for(tool: Tool, ink: Ink) -> Ink {
    if tool == Tool::Highlighter && ink == Ink::Ink {
        Ink::Ask
    } else {
        ink
    }
}

/// One stroke: its tool, colour, width (a text's size) and points — `x`,
/// `y` in [`UNITS`] and a pencil's pressure (0: none) — and a text's words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Stroke {
    pub(crate) t: Tool,
    pub(crate) c: Ink,
    pub(crate) w: f64,
    pub(crate) p: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) s: Option<String>,
}

/// To a tenth of a unit (a pressure to a hundredth): the JSON stays small.
fn round(v: f64, by: f64) -> f64 {
    (v * by).round() / by
}

impl Stroke {
    pub(crate) fn start(t: Tool, c: Ink, w: f64, x: f64, y: f64, pressure: f64) -> Self {
        Stroke { t, c: ink_for(t, c), w: round(w, 10.0), p: vec![[round(x, 10.0), round(y, 10.0), round(pressure, 100.0)]], s: None }
    }

    pub(crate) fn text(c: Ink, size: f64, x: f64, y: f64, words: &str) -> Self {
        Stroke { t: Tool::Text, c, w: round(size, 10.0), p: vec![[round(x, 10.0), round(y, 10.0), 0.0]], s: Some(words.to_string()) }
    }

    /// The pointer moved to `(x, y)`: a two-point tool's end moves; a pen
    /// or highlighter takes the point, unless it is within half a unit of
    /// the last one.
    pub(crate) fn to(&mut self, x: f64, y: f64, pressure: f64) {
        let pt = [round(x, 10.0), round(y, 10.0), round(pressure, 100.0)];
        if self.t.two_point() {
            if self.p.len() < 2 {
                self.p.push(pt);
            } else {
                self.p[1] = pt;
            }
            return;
        }
        if let Some(last) = self.p.last() {
            if (last[0] - pt[0]).hypot(last[1] - pt[1]) < 0.5 {
                return;
            }
        }
        self.p.push(pt);
    }

    /// Whether it is worth keeping: an arrow or a box drawn longer than a
    /// tap, a text with words; a pen's dot is a mark.
    pub(crate) fn keeps(&self) -> bool {
        match self.t {
            Tool::Eraser => false,
            Tool::Text => self.s.as_deref().is_some_and(|s| !s.trim().is_empty()),
            t if t.two_point() => self.p.len() >= 2 && (self.p[1][0] - self.p[0][0]).hypot(self.p[1][1] - self.p[0][1]) >= 4.0,
            _ => !self.p.is_empty(),
        }
    }

    /// Whether a pencil drew it (its points carry pressure).
    pub(crate) fn pressed(&self) -> bool {
        self.t == Tool::Pen && self.p.iter().any(|p| p[2] > 0.0)
    }

    /// A text's box: its top left, and how wide and tall it is drawn.
    fn text_box(&self) -> (f64, f64, f64, f64) {
        let n = self.s.as_deref().map_or(0, |s| s.chars().count()) as f64;
        (self.p[0][0], self.p[0][1], n * self.w * 0.6, self.w * 1.25)
    }
}

/// How far `(px, py)` is from the segment `a`–`b`.
pub(crate) fn seg_dist(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

/// The eraser's test: whether a circle of radius `r` at `(x, y)` touches
/// the stroke as it is drawn — a line within its half-width, a box on its
/// edges (not its inside: what is in a box is another stroke), a text
/// anywhere in its box.
pub(crate) fn hits(s: &Stroke, x: f64, y: f64, r: f64) -> bool {
    if s.p.is_empty() {
        return false;
    }
    let reach = r + s.w / 2.0;
    match s.t {
        Tool::Text => {
            let (l, t, w, h) = s.text_box();
            x >= l - r && x <= l + w + r && y >= t - r && y <= t + h + r
        }
        Tool::Box => {
            let (a, b) = (s.p[0], *s.p.last().unwrap());
            let (l, t, rr, bb) = (a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1]));
            [(l, t, rr, t), (rr, t, rr, bb), (rr, bb, l, bb), (l, bb, l, t)].iter().any(|&(ax, ay, bx, by)| seg_dist(x, y, ax, ay, bx, by) <= reach)
        }
        Tool::Arrow => {
            let (a, b) = (s.p[0], *s.p.last().unwrap());
            seg_dist(x, y, a[0], a[1], b[0], b[1]) <= reach
        }
        _ if s.p.len() == 1 => (x - s.p[0][0]).hypot(y - s.p[0][1]) <= reach,
        _ => s.p.windows(2).any(|w| seg_dist(x, y, w[0][0], w[0][1], w[1][0], w[1][1]) <= reach),
    }
}

/// How far down the strokes reach, in units.
pub(crate) fn bottom(strokes: &[Stroke]) -> f64 {
    strokes
        .iter()
        .map(|s| match s.t {
            Tool::Text if !s.p.is_empty() => {
                let (_, t, _, h) = s.text_box();
                t + h
            }
            _ => s.p.iter().map(|p| p[1]).fold(0.0, f64::max) + s.w / 2.0,
        })
        .fold(0.0, f64::max)
}

/// The strokes on the pad, and their undo and redo: each a whole set of
/// strokes as it stood (a drawing is small; a snapshot is the simplest
/// thing that is always right).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Board {
    pub(crate) strokes: Vec<Stroke>,
    past: Vec<Vec<Stroke>>,
    future: Vec<Vec<Stroke>>,
}

impl Board {
    pub(crate) fn with(strokes: Vec<Stroke>) -> Self {
        Board { strokes, ..Board::default() }
    }

    fn mark(&mut self) {
        self.past.push(self.strokes.clone());
        self.future.clear();
    }

    /// A stroke drawn: kept if it is worth keeping, one undo step.
    pub(crate) fn add(&mut self, s: Stroke) -> bool {
        if !s.keeps() {
            return false;
        }
        self.mark();
        self.strokes.push(s);
        true
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub(crate) fn undo(&mut self) -> bool {
        let Some(prev) = self.past.pop() else { return false };
        self.future.push(std::mem::replace(&mut self.strokes, prev));
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let Some(next) = self.future.pop() else { return false };
        self.past.push(std::mem::replace(&mut self.strokes, next));
        true
    }

    /// Every stroke gone, as one step (undo brings them back).
    pub(crate) fn clear(&mut self) -> bool {
        if self.strokes.is_empty() {
            return false;
        }
        self.mark();
        self.strokes.clear();
        true
    }

    /// The eraser at `(x, y)`, reach `r`: every stroke it touches taken out
    /// at once (an undo step is [`Board::erase_end`]'s). How many went.
    pub(crate) fn erase_at(&mut self, x: f64, y: f64, r: f64) -> usize {
        let before = self.strokes.len();
        self.strokes.retain(|s| !hits(s, x, y, r));
        before - self.strokes.len()
    }

    /// Whether the eraser at `(x, y)` would take anything.
    pub(crate) fn would_erase(&self, x: f64, y: f64, r: f64) -> bool {
        self.strokes.iter().any(|s| hits(s, x, y, r))
    }

    /// A sweep of the eraser ended: one undo step back to `before`, if it
    /// took anything.
    pub(crate) fn erase_end(&mut self, before: Vec<Stroke>) -> bool {
        if before == self.strokes {
            return false;
        }
        self.past.push(before);
        self.future.clear();
        true
    }
}

/// A drawing as it is kept: its version, its height in units (its width is
/// [`UNITS`]) and its strokes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Drawing {
    pub(crate) v: u32,
    pub(crate) h: f64,
    pub(crate) strokes: Vec<Stroke>,
    /// APP-104: the image the strokes were drawn over, if any — read by an
    /// app from before it as a plain drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) bg: Option<Source>,
}

/// APP-104: the image a mark-up was drawn over: its hash (`media.get`) and
/// its size in pixels, so its aspect is the drawing's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Source {
    pub(crate) hash: String,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// APP-104: the widest a flattened mark-up is made, in pixels.
pub(crate) const MARKUP_MAX_PX: f64 = 2400.0;

/// APP-104: the width a mark-up of an image `w` px wide is flattened at
/// on the `n`-th try (0 first): the image's own, at most
/// [`MARKUP_MAX_PX`], three quarters again each try — a try past the cap
/// is one smaller — and never under 240.
pub(crate) fn markup_px(w: f64, n: u32) -> f64 {
    (w.min(MARKUP_MAX_PX) * 0.75f64.powi(n as i32)).max(240.0).round()
}

impl Drawing {
    /// The strokes on a surface `h` units tall — taller, if they reach
    /// further down.
    pub(crate) fn of(strokes: Vec<Stroke>, h: f64) -> Self {
        let h = round(h.max(bottom(&strokes) + 8.0).max(40.0), 10.0);
        Drawing { v: VERSION, h, strokes, bg: None }
    }

    /// APP-104: the strokes over the image `src` — the drawing its aspect,
    /// whatever reaches past its edge cut off with it.
    pub(crate) fn over(strokes: Vec<Stroke>, src: Source) -> Self {
        let h = UNITS * f64::from(src.h.max(1)) / f64::from(src.w.max(1));
        Drawing { v: VERSION, h: (h * 10.0).round() / 10.0, strokes, bg: Some(src) }
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A drawing read back, or none: not JSON, not a drawing, a version
    /// this app does not know.
    pub(crate) fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str::<Drawing>(text).ok().filter(|d| d.v == VERSION && d.h.is_finite() && d.h > 0.0)
    }
}

// ── the PNG chunk ───────────────────────────────────────────────────────

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The CRC a PNG chunk ends with (ISO 3309, as zlib's `crc32`).
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c ^ 0xFFFF_FFFF
}

/// Each chunk of a PNG: where it starts, its type, its data.
fn chunks(png: &[u8]) -> Option<Vec<(usize, [u8; 4], &[u8])>> {
    if png.get(..8)? != PNG_SIG {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let ty: [u8; 4] = png[i + 4..i + 8].try_into().ok()?;
        let data = png.get(i + 8..i + 8 + len)?;
        out.push((i, ty, data));
        i += 12 + len;
        if &ty == b"IEND" {
            return Some(out);
        }
    }
    None
}

/// `png` with `json` in an `iTXt` chunk keyed [`KEYWORD`] (UTF-8, not
/// compressed), before its `IEND` — any such chunk it held before taken
/// out. None for bytes that are no whole PNG.
pub(crate) fn embed(png: &[u8], json: &str) -> Option<Vec<u8>> {
    let list = chunks(png)?;
    let mut out = PNG_SIG.to_vec();
    for (at, ty, data) in &list {
        if &ty[..] == b"IEND" {
            let mut body = b"iTXt".to_vec();
            body.extend_from_slice(KEYWORD.as_bytes());
            // NUL, not compressed, method 0, no language, no translated keyword.
            body.extend_from_slice(&[0, 0, 0, 0, 0]);
            body.extend_from_slice(json.as_bytes());
            out.extend_from_slice(&((body.len() - 4) as u32).to_be_bytes());
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
        }
        if &ty[..] == b"iTXt" && data.starts_with(KEYWORD.as_bytes()) && data.get(KEYWORD.len()) == Some(&0) {
            continue;
        }
        out.extend_from_slice(&png[*at..*at + 12 + data.len()]);
    }
    Some(out)
}

/// The JSON a PNG carries under [`KEYWORD`], if it carries any.
pub(crate) fn extract(png: &[u8]) -> Option<String> {
    chunks(png)?.into_iter().find_map(|(_, ty, data)| {
        if &ty != b"iTXt" {
            return None;
        }
        let rest = data.strip_prefix(KEYWORD.as_bytes())?.strip_prefix(&[0u8, 0, 0][..])?;
        // No language, no translated keyword: two NULs.
        let lang_end = rest.iter().position(|b| *b == 0)?;
        let rest = &rest[lang_end + 1..];
        let tr_end = rest.iter().position(|b| *b == 0)?;
        String::from_utf8(rest[tr_end + 1..].to_vec()).ok()
    })
}

/// The drawing an image's bytes carry, if any.
pub(crate) fn drawing_in(png: &[u8]) -> Option<Drawing> {
    extract(png).as_deref().and_then(Drawing::from_json)
}

// ── painting ────────────────────────────────────────────────────────────

/// The theme's values a drawing is painted with, read off an element.
struct Paint {
    inks: [String; 5],
    card: String,
    face: String,
}

fn paint_of(el: &web_sys::Element) -> Paint {
    let style = crate::dom::window().get_computed_style(el).ok().flatten();
    let get = |name: &str| style.as_ref().and_then(|s| s.get_property_value(name).ok()).map(|v| v.trim().to_string()).unwrap_or_default();
    Paint { inks: Ink::ALL.map(|i| get(i.token())), card: get("--card"), face: get("--sans") }
}

impl Paint {
    fn ink(&self, i: Ink) -> &str {
        &self.inks[Ink::ALL.iter().position(|x| *x == i).unwrap_or(0)]
    }
}

/// One stroke on `ctx`, at `k` pixels a unit.
fn stroke_on(ctx: &web_sys::CanvasRenderingContext2d, s: &Stroke, k: f64, paint: &Paint) {
    if s.p.is_empty() {
        return;
    }
    let col = paint.ink(s.c);
    let lw = (s.w * k).max(1.0);
    let pts: Vec<(f64, f64, f64)> = s.p.iter().map(|p| (p[0] * k, p[1] * k, p[2])).collect();
    ctx.save();
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    ctx.set_stroke_style_str(col);
    ctx.set_fill_style_str(col);
    ctx.set_line_width(lw);
    if s.t == Tool::Highlighter {
        ctx.set_global_alpha(0.32);
    }
    let (a, b) = (pts[0], *pts.last().unwrap());
    match s.t {
        Tool::Text => {
            ctx.set_font(&format!("{:.1}px {}", s.w * k, paint.face));
            ctx.set_text_baseline("top");
            let _ = ctx.fill_text(s.s.as_deref().unwrap_or_default(), a.0, a.1);
        }
        Tool::Box => {
            let (x, y, w, h) = (a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs());
            let r = (8.0_f64).min(w / 2.0).min(h / 2.0);
            ctx.begin_path();
            ctx.move_to(x + r, y);
            let _ = ctx.arc_to(x + w, y, x + w, y + h, r);
            let _ = ctx.arc_to(x + w, y + h, x, y + h, r);
            let _ = ctx.arc_to(x, y + h, x, y, r);
            let _ = ctx.arc_to(x, y, x + w, y, r);
            ctx.close_path();
            ctx.stroke();
        }
        Tool::Arrow => {
            let ang = (b.1 - a.1).atan2(b.0 - a.0);
            let hl = (lw * 4.5).max(12.0);
            ctx.begin_path();
            ctx.move_to(a.0, a.1);
            ctx.line_to(b.0 - ang.cos() * hl * 0.6, b.1 - ang.sin() * hl * 0.6);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(b.0, b.1);
            ctx.line_to(b.0 - hl * (ang - 0.45).cos(), b.1 - hl * (ang - 0.45).sin());
            ctx.line_to(b.0 - hl * (ang + 0.45).cos(), b.1 - hl * (ang + 0.45).sin());
            ctx.close_path();
            ctx.fill();
        }
        _ if pts.len() == 1 => {
            ctx.begin_path();
            let _ = ctx.arc(a.0, a.1, lw / 2.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }
        _ if s.pressed() => {
            for w in pts.windows(2) {
                ctx.begin_path();
                ctx.set_line_width(lw * (0.35 + w[1].2 * 1.3));
                ctx.move_to(w[0].0, w[0].1);
                ctx.line_to(w[1].0, w[1].1);
                ctx.stroke();
            }
        }
        _ => {
            ctx.begin_path();
            ctx.move_to(a.0, a.1);
            for i in 1..pts.len().saturating_sub(1) {
                let (m0, m1) = ((pts[i].0 + pts[i + 1].0) / 2.0, (pts[i].1 + pts[i + 1].1) / 2.0);
                ctx.quadratic_curve_to(pts[i].0, pts[i].1, m0, m1);
            }
            ctx.line_to(b.0, b.1);
            ctx.stroke();
        }
    }
    ctx.restore();
}

fn context(canvas: &web_sys::HtmlCanvasElement) -> Option<web_sys::CanvasRenderingContext2d> {
    canvas.get_context("2d").ok().flatten()?.dyn_into().ok()
}

/// A drawing as a PNG `px` wide, on the theme's `--card` (APP-104: over
/// `bg`, when it is a mark-up), in the theme read off `el` — the PNG's
/// bytes.
fn render_png(d: &Drawing, px: f64, el: &web_sys::Element) -> Option<Vec<u8>> {
    render_over(d, None, px, el)
}

fn render_over(d: &Drawing, bg: Option<&web_sys::HtmlImageElement>, px: f64, el: &web_sys::Element) -> Option<Vec<u8>> {
    let doc = crate::dom::window().document()?;
    let canvas: web_sys::HtmlCanvasElement = doc.create_element("canvas").ok()?.dyn_into().ok()?;
    let k = px / UNITS;
    canvas.set_width(px.round() as u32);
    canvas.set_height((d.h * k).ceil() as u32);
    let ctx = context(&canvas)?;
    let paint = paint_of(el);
    if let Some(img) = bg {
        ctx.draw_image_with_html_image_element_and_dw_and_dh(img, 0.0, 0.0, f64::from(canvas.width()), f64::from(canvas.height())).ok()?;
    } else if !paint.card.is_empty() {
        ctx.set_fill_style_str(&paint.card);
        ctx.fill_rect(0.0, 0.0, f64::from(canvas.width()), f64::from(canvas.height()));
    }
    for s in &d.strokes {
        stroke_on(&ctx, s, k, &paint);
    }
    let url = canvas.to_data_url_with_type("image/png").ok()?;
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(url.split_once(',')?.1).ok()
}

// ── the drawings the page knows ─────────────────────────────────────────

thread_local! {
    /// Each kept image known to carry a drawing: its strokes.
    static DRAWINGS: RefCell<HashMap<String, Drawing>> = RefCell::new(HashMap::new());
    /// Bumped on every repaint of the theme: a pad paints again.
    static THEME: ArcRwSignal<u32> = ArcRwSignal::new(0);
    /// The boxes that can draw, by their attachments' element: an edit
    /// opens in the one nearest the image.
    static BOXES: RefCell<Vec<(web_sys::Element, crate::media::Attach)>> = const { RefCell::new(Vec::new()) };
}

/// The image `hash`'s bytes, read: kept if they carry a drawing. Whether
/// they do.
pub(crate) fn learn(hash: &str, bytes: &[u8]) -> bool {
    let Some(d) = drawing_in(bytes) else { return false };
    DRAWINGS.with(|m| m.borrow_mut().insert(hash.to_string(), d));
    true
}

/// Whether the image `hash` is known to be a drawing.
pub(crate) fn is_drawing(hash: &str) -> bool {
    DRAWINGS.with(|m| m.borrow().contains_key(hash))
}

/// A known drawing painted in the theme worn now, as a `blob:` URL.
pub(crate) fn themed_url(hash: &str) -> Option<String> {
    let d = DRAWINGS.with(|m| m.borrow().get(hash).cloned()).filter(|d| d.bg.is_none())?;
    let root = crate::dom::window().document()?.document_element()?;
    let png = render_png(&d, 1200.0, &root)?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(&png[..]));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

/// THEME-7: the theme moved — every pad and every drawing on the page
/// painted again in it (from `scopes::repaint`).
pub(crate) fn repaint_all() {
    THEME.with(|t| t.update(|n| *n += 1));
    let hashes: Vec<String> = DRAWINGS.with(|m| m.borrow().keys().cloned().collect());
    for h in hashes {
        if let Some(u) = themed_url(&h) {
            crate::media::redraw(&h, u);
        }
    }
}

fn theme_gen() -> u32 {
    THEME.with(|t| t.get())
}

/// A box that can draw: its attachments' element, for as long as it is up.
pub(crate) fn register(el: web_sys::Element, a: crate::media::Attach) {
    BOXES.with(|b| {
        let mut b = b.borrow_mut();
        b.retain(|(e, _)| e.is_connected());
        b.push((el, a));
    });
}

/// The box an edit of a drawing at `near` opens in: the nearest one up its
/// ancestors, else the last one up.
fn box_near(near: &web_sys::Element) -> Option<crate::media::Attach> {
    BOXES.with(|b| {
        let mut b = b.borrow_mut();
        b.retain(|(e, _)| e.is_connected());
        let mut at = Some(near.clone());
        while let Some(el) = at {
            if let Some((_, a)) = b.iter().find(|(e, _)| el.contains(Some(e))) {
                return Some(*a);
            }
            at = el.parent_element();
        }
        b.last().map(|(_, a)| *a)
    })
}

/// What *edit drawing* on the image `hash` (drawn at `near`) does, if it is
/// a drawing and a box is up to draw in.
pub(crate) fn editor(hash: &str, near: &web_sys::Element) -> Option<Rc<dyn Fn()>> {
    let d = DRAWINGS.with(|m| m.borrow().get(hash).cloned())?;
    let a = box_near(near)?;
    let hash = hash.to_string();
    let Some(src) = d.bg.clone() else { return Some(Rc::new(move || a.edit_drawing(&hash, d.strokes.clone()))) };
    // APP-104: a mark-up opens over its photo again, the strokes apart.
    Some(Rc::new(move || {
        let (hash, src, strokes) = (hash.clone(), src.clone(), d.strokes.clone());
        wasm_bindgen_futures::spawn_local(async move {
            match source_url(&src.hash).await {
                Some(url) => match Bg::load(&src.hash, &url).await {
                    Some(bg) => a.mark_up(bg, strokes, Some(&hash)),
                    None => leptos::logging::log!("mark up: {} would not load", src.hash),
                },
                None => leptos::logging::log!("mark up: {} is not available", src.hash),
            }
        });
    }))
}

/// APP-104: what *mark up* on the image `hash` (drawn at `near` from
/// `url`) does: the nearest box's pad opened over it — none when no box is
/// up to draw in.
pub(crate) fn marker(hash: &str, url: &str, near: &web_sys::Element) -> Option<Rc<dyn Fn()>> {
    let a = box_near(near)?;
    let (hash, url) = (hash.to_string(), url.to_string());
    Some(Rc::new(move || {
        let (hash, url) = (hash.clone(), url.clone());
        wasm_bindgen_futures::spawn_local(async move {
            match Bg::load(&hash, &url).await {
                Some(bg) => a.mark_up(bg, Vec::new(), None),
                None => leptos::logging::log!("mark up: {hash} would not load"),
            }
        });
    }))
}

/// The image `hash`'s URL, once the page has it (asked for if it is not
/// yet) — none if it is not to be had.
async fn source_url(hash: &str) -> Option<String> {
    for _ in 0..100 {
        if let Some(u) = crate::media::url(hash) {
            return Some(u);
        }
        if crate::media::missing_now(hash) {
            return None;
        }
        crate::dom::sleep_ms(100).await;
    }
    None
}

/// APP-104: a pad's background — the image, loaded, and what it is.
#[derive(Clone)]
pub(crate) struct Bg {
    pub(crate) src: Source,
    img: web_sys::HtmlImageElement,
}

impl Bg {
    /// The image at `url` (the image `hash`), decoded — none if it will not.
    pub(crate) async fn load(hash: &str, url: &str) -> Option<Self> {
        let img = web_sys::HtmlImageElement::new().ok()?;
        img.set_src(url);
        wasm_bindgen_futures::JsFuture::from(img.decode()).await.ok()?;
        let (w, h) = (img.natural_width(), img.natural_height());
        (w > 0 && h > 0).then(|| Bg { src: Source { hash: hash.to_string(), w, h }, img })
    }
}

// ── the pad ─────────────────────────────────────────────────────────────

/// One box's pad: open or not, its strokes and their history, the tool and
/// colour picked, and the stroke under the pointer.
#[derive(Clone, Copy)]
pub(crate) struct Pad {
    pub(crate) open: RwSignal<bool>,
    pub(crate) board: RwSignal<Board>,
    tool: RwSignal<Tool>,
    ink: RwSignal<Ink>,
    /// The text tool's point, while its words are typed.
    typing: RwSignal<Option<(f64, f64)>>,
    live: StoredValue<Option<Stroke>>,
    erasing: StoredValue<Option<Vec<Stroke>>>,
    pointer: StoredValue<Option<i32>>,
    canvas: NodeRef<Canvas>,
    /// APP-104: the image the pad draws over, when it marks one up.
    bg: RwSignal<Option<Bg>, LocalStorage>,
}

impl Pad {
    pub(crate) fn new() -> Self {
        Pad {
            open: RwSignal::new(false),
            board: RwSignal::new(Board::default()),
            tool: RwSignal::new(Tool::Pen),
            ink: RwSignal::new(Ink::Urgent),
            typing: RwSignal::new(None),
            live: StoredValue::new(None),
            erasing: StoredValue::new(None),
            pointer: StoredValue::new(None),
            canvas: NodeRef::new(),
            bg: RwSignal::new_local(None),
        }
    }

    /// Whether the pad holds a drawing to send (tracked).
    pub(crate) fn drawn(self) -> bool {
        self.open.get() && self.board.with(|b| !b.strokes.is_empty())
    }

    /// The pad opened on `strokes` (an edit), its history fresh.
    pub(crate) fn open_with(self, strokes: Vec<Stroke>) {
        self.bg.set(None);
        self.board.set(Board::with(strokes));
        self.typing.set(None);
        self.open.set(true);
    }

    /// APP-104: the pad opened over `bg`, with `strokes` on it.
    pub(crate) fn open_over(self, bg: Bg, strokes: Vec<Stroke>) {
        self.open_with(strokes);
        self.bg.set(Some(bg));
    }

    /// Put away, its strokes gone.
    pub(crate) fn discard(self) {
        self.bg.set(None);
        self.board.set(Board::default());
        self.typing.set(None);
        self.open.set(false);
    }

    /// The pad as a PNG carrying its strokes — then put away. None when
    /// it is shut or holds nothing.
    pub(crate) fn take_png(self) -> Option<Vec<u8>> {
        self.commit_text(None);
        if !self.open.get_untracked() || self.board.with_untracked(|b| b.strokes.is_empty()) {
            return None;
        }
        let el: web_sys::Element = match self.canvas.get_untracked() {
            Some(c) => c.into(),
            None => crate::dom::window().document()?.document_element()?,
        };
        let strokes = self.board.get_untracked().strokes;
        let bytes = if let Some(bg) = self.bg.get_untracked() {
            // APP-104: the image and the strokes, one PNG under the cap.
            let d = Drawing::over(strokes, bg.src.clone());
            let json = d.to_json();
            let max = menger_tickets_api::media::MAX_BYTES;
            (0..8).find_map(|n| {
                let px = markup_px(f64::from(bg.src.w), n);
                embed(&render_over(&d, Some(&bg.img), px, &el)?, &json).filter(|b| b.len() <= max)
            })?
        } else {
            let r = el.get_bounding_client_rect();
            let (w, h) = if r.width() > 0.0 { (r.width(), r.height()) } else { (780.0, 300.0) };
            let d = Drawing::of(strokes, h * UNITS / w);
            let png = render_png(&d, (w * 2.0).clamp(480.0, 2000.0), &el)?;
            embed(&png, &d.to_json())?
        };
        self.discard();
        Some(bytes)
    }

    /// APP-104: whether the pad is marking up an image.
    pub(crate) fn over(self) -> bool {
        self.bg.with_untracked(Option::is_some)
    }

    /// The pad's canvas, while it is up.
    pub(crate) fn canvas_el(self) -> Option<web_sys::HtmlCanvasElement> {
        self.canvas.get_untracked()
    }

    /// Pixels a unit, the canvas as laid out now.
    fn scale(self) -> f64 {
        self.canvas.get_untracked().map(|c| c.get_bounding_client_rect().width()).filter(|w| *w > 0.0).map_or(1.0, |w| w / UNITS)
    }

    /// The pad painted: fitted to its box at the screen's density, its
    /// strokes and the one under the pointer in the theme worn.
    pub(crate) fn paint(self) {
        let Some(c) = self.canvas.get_untracked() else { return };
        let r = c.get_bounding_client_rect();
        if r.width() <= 0.0 {
            return;
        }
        let dpr = crate::dom::window().device_pixel_ratio().max(1.0);
        let (pw, ph) = ((r.width() * dpr).round() as u32, (r.height() * dpr).round() as u32);
        if c.width() != pw || c.height() != ph {
            c.set_width(pw);
            c.set_height(ph);
        }
        let Some(ctx) = context(&c) else { return };
        let _ = ctx.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        ctx.clear_rect(0.0, 0.0, f64::from(pw), f64::from(ph));
        let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
        self.bg.with_untracked(|bg| {
            if let Some(bg) = bg {
                let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(&bg.img, 0.0, 0.0, r.width(), r.height());
            }
        });
        let k = r.width() / UNITS;
        let paint = paint_of(&c);
        self.board.with_untracked(|b| {
            for s in &b.strokes {
                stroke_on(&ctx, s, k, &paint);
            }
        });
        self.live.with_value(|l| {
            if let Some(s) = l {
                stroke_on(&ctx, s, k, &paint);
            }
        });
    }

    /// Where a pointer is on the pad, in units, and its pressure (a
    /// pencil's; 0 for a finger or a mouse).
    fn at(self, ev: &web_sys::PointerEvent) -> Option<(f64, f64, f64)> {
        let c = self.canvas.get_untracked()?;
        let r = c.get_bounding_client_rect();
        if r.width() <= 0.0 {
            return None;
        }
        let u = UNITS / r.width();
        let pressure = if ev.pointer_type() == "pen" { f64::from(ev.pressure()).clamp(0.01, 1.0) } else { 0.0 };
        Some(((f64::from(ev.client_x()) - r.left()) * u, (f64::from(ev.client_y()) - r.top()) * u, pressure))
    }

    fn down(self, ev: web_sys::PointerEvent) {
        let Some((x, y, pr)) = self.at(&ev) else { return };
        ev.prevent_default();
        if let Some(c) = self.canvas.get_untracked() {
            let _ = c.set_pointer_capture(ev.pointer_id());
        }
        let u = 1.0 / self.scale();
        let tool = self.tool.get_untracked();
        match tool {
            Tool::Text => {
                // A text being typed is kept first; then words go here.
                self.commit_text(None);
                self.typing.set(Some((x, y)));
                return;
            }
            Tool::Eraser => {
                self.erasing.set_value(Some(self.board.with_untracked(|b| b.strokes.clone())));
                self.erase(x, y, tool.px() / 2.0 * u);
            }
            _ => {
                if let Some(c) = self.canvas.get_untracked() {
                    let _ = c.focus();
                }
                self.live.set_value(Some(Stroke::start(tool, self.ink.get_untracked(), tool.px() * u, x, y, pr)));
                self.paint();
            }
        }
        self.pointer.set_value(Some(ev.pointer_id()));
    }

    fn erase(self, x: f64, y: f64, r: f64) {
        if self.board.with_untracked(|b| b.would_erase(x, y, r)) {
            self.board.update(|b| {
                b.erase_at(x, y, r);
            });
        }
    }

    fn moved(self, ev: web_sys::PointerEvent) {
        if self.pointer.get_value() != Some(ev.pointer_id()) {
            return;
        }
        let Some((x, y, pr)) = self.at(&ev) else { return };
        ev.prevent_default();
        if self.tool.get_untracked() == Tool::Eraser {
            self.erase(x, y, Tool::Eraser.px() / 2.0 / self.scale());
            return;
        }
        self.live.update_value(|l| {
            if let Some(s) = l {
                s.to(x, y, pr);
            }
        });
        self.paint();
    }

    fn up(self, ev: web_sys::PointerEvent) {
        if self.pointer.get_value() != Some(ev.pointer_id()) {
            return;
        }
        self.pointer.set_value(None);
        if let Some(before) = self.erasing.try_update_value(Option::take).flatten() {
            self.board.update(|b| {
                b.erase_end(before);
            });
            return;
        }
        if let Some(s) = self.live.try_update_value(Option::take).flatten() {
            let kept = self.board.try_update(|b| b.add(s)).unwrap_or(false);
            if !kept {
                self.paint();
            }
        }
    }

    /// The text being typed kept as a stroke (`words`, else the field's).
    fn commit_text(self, words: Option<String>) {
        let Some((x, y)) = self.typing.try_get_untracked().flatten() else { return };
        self.typing.set(None);
        let words = words.unwrap_or_default();
        if words.trim().is_empty() {
            return;
        }
        let size = Tool::Text.px() / self.scale();
        let s = Stroke::text(self.ink.get_untracked(), size, x, y, words.trim());
        self.board.update(|b| {
            b.add(s);
        });
    }

    fn undo(self) {
        self.board.update(|b| {
            b.undo();
        });
    }

    fn redo(self) {
        self.board.update(|b| {
            b.redo();
        });
    }
}

fn svg(paths: &'static str) -> impl IntoView {
    view! { <svg viewBox="0 0 24 24" aria-hidden="true" inner_html=paths></svg> }
}

const UNDO: &str = r#"<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>"#;
const REDO: &str = r#"<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>"#;

/// The pencil on the *draw* button.
pub(crate) const PENCIL: &str = r#"<path d="M4 20l4-1 11-11-3-3L5 16z"/><path d="M14 6l3 3"/>"#;

/// The pad, inside its box above the text — drawn while it is open.
#[component]
pub(crate) fn PadView(a: crate::media::Attach) -> impl IntoView {
    let pad = a.pad();
    move || pad.open.get().then(|| untrack(|| pad_body(pad, a)))
}

fn pad_body(pad: Pad, a: crate::media::Attach) -> impl IntoView {
    // Painted again when its strokes move and when the theme does.
    Effect::new(move |_| {
        pad.board.track();
        pad.bg.track();
        theme_gen();
        // After the surface takes the image's aspect.
        wasm_bindgen_futures::spawn_local(async move {
            crate::dom::sleep_ms(0).await;
            pad.paint();
        });
    });
    pad.canvas.on_load(move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            crate::dom::sleep_ms(0).await;
            pad.paint();
        });
    });
    let resize = window_event_listener(leptos::ev::resize, move |_| pad.paint());
    on_cleanup(move || resize.remove());
    let keys = move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Draw) {
        Some("draw.undo") => {
            ev.prevent_default();
            ev.stop_propagation();
            pad.undo();
        }
        Some("draw.redo") => {
            ev.prevent_default();
            ev.stop_propagation();
            pad.redo();
        }
        _ => {}
    };
    let tools = Tool::ALL
        .iter()
        .map(|&t| {
            view! {
                <button type="button" class="dbtn" data-tool=t.name() aria-label=t.name() title=t.name()
                    aria-pressed=move || (pad.tool.get() == t).to_string()
                    on:click=move |_| {
                        pad.commit_text(None);
                        pad.tool.set(t);
                    }>
                    {svg(t.icon())}
                </button>
            }
        })
        .collect_view();
    let swatches = Ink::ALL
        .iter()
        .map(|&i| {
            view! {
                <button type="button" class="dsw" data-ink=i.name() aria-label=i.name() title=i.name()
                    style=format!("--c: var({})", i.token())
                    aria-pressed=move || (pad.ink.get() == i).to_string()
                    on:click=move |_| {
                        pad.ink.set(i);
                        if pad.tool.get_untracked() == Tool::Eraser {
                            pad.tool.set(Tool::Pen);
                        }
                    }>
                </button>
            }
        })
        .collect_view();
    let text_field = move || {
        let (x, y) = pad.typing.get()?;
        let k = pad.scale();
        let field = NodeRef::<Input>::new();
        field.on_load(|el| {
            let _ = el.focus();
        });
        let ink = pad.ink.get_untracked();
        Some(view! {
            <input node_ref=field class="dtext" type="text" aria-label="the words to write on the drawing" placeholder="type, then Enter"
                data-ink=ink.name()
                style=format!("left: {:.1}px; top: {:.1}px; font-size: {:.1}px; --c: var({})", x * k, y * k, Tool::Text.px(), ink.token())
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    match bindings::hit(&ev, Context::Draw) {
                        Some("draw.text-done") => {
                            ev.prevent_default();
                            pad.commit_text(Some(event_target_value(&ev)));
                        }
                        Some("draw.text-cancel") => {
                            ev.prevent_default();
                            pad.typing.set(None);
                        }
                        _ => {}
                    }
                    // The field's own keys (its undo too) stay its own.
                    ev.stop_propagation();
                }
                // A blur comes after the field is gone too: its words are
                // read off the event, never off a ref that went with it.
                on:blur=move |ev| pad.commit_text(Some(event_target_value(&ev)))
            />
        })
    };
    view! {
        <div class="dpad" role="group" aria-label="drawing pad" on:keydown=keys>
            <div class="dtop">
                <span class="dlbl">{move || if pad.bg.with(Option::is_some) { "mark-up · strokes over the image" } else { "drawing · kept as strokes" }}</span>
                // APP-104: a mark-up goes into the box as an image (words may
                // follow it); the box's send takes it too.
                {move || pad.bg.with(Option::is_some).then(|| view! {
                    <button type="button" class="dtxt dattach" data-act="attach" title="put the marked-up image in the box"
                        disabled=move || pad.board.with(|b| b.strokes.is_empty())
                        on:click=move |_| { a.flush(); }>"attach"</button>
                })}
                <button type="button" class="dtxt" data-act="discard" title="put the drawing away" on:click=move |_| pad.discard()>"discard"</button>
            </div>
            <div class="dsurf" class:over=move || pad.bg.with(Option::is_some)
                style=move || pad.bg.with(|b| b.as_ref().map(|b| format!("--arw: {}; --arh: {}", b.src.w, b.src.h)).unwrap_or_default())>
                <canvas node_ref=pad.canvas class="dink" tabindex="0" aria-label="drawing pad: draw with a finger, a pencil or the mouse"
                    on:pointerdown=move |ev| pad.down(ev)
                    on:pointermove=move |ev| pad.moved(ev)
                    on:pointerup=move |ev| pad.up(ev)
                    on:pointercancel=move |ev| pad.up(ev)
                ></canvas>
                {move || (pad.board.with(|b| b.strokes.is_empty()) && pad.typing.with(Option::is_none) && pad.bg.with(Option::is_none)).then(|| view! {
                    <span class="dempty">"draw here: a finger, a pencil or the mouse"</span>
                })}
                {text_field}
            </div>
            <div class="dtools" role="toolbar" aria-label="drawing tools">
                {tools}
                <span class="dsep"></span>
                {swatches}
                <span class="dsep"></span>
                <button type="button" class="dbtn" data-act="undo" aria-label="undo" title="undo (⌘Z)" disabled=move || !pad.board.with(Board::can_undo) on:click=move |_| pad.undo()>{svg(UNDO)}</button>
                <button type="button" class="dbtn" data-act="redo" aria-label="redo" title="redo (⇧⌘Z)" disabled=move || !pad.board.with(Board::can_redo) on:click=move |_| pad.redo()>{svg(REDO)}</button>
                <button type="button" class="dtxt" data-act="clear" title="clear the drawing" disabled=move || pad.board.with(|b| b.strokes.is_empty())
                    on:click=move |_| pad.board.update(|b| {
                        b.clear();
                    })>"clear"</button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen(pts: &[(f64, f64)]) -> Stroke {
        let mut s = Stroke::start(Tool::Pen, Ink::Urgent, 3.0, pts[0].0, pts[0].1, 0.0);
        for &(x, y) in &pts[1..] {
            s.to(x, y, 0.0);
        }
        s
    }

    fn two(t: Tool, a: (f64, f64), b: (f64, f64)) -> Stroke {
        let mut s = Stroke::start(t, Ink::Accent, 3.0, a.0, a.1, 0.0);
        s.to(b.0, b.1, 0.0);
        s
    }

    #[test]
    fn a_stroke_keeps_its_points_and_a_two_point_tool_its_ends() {
        let s = pen(&[(10.0, 10.0), (10.2, 10.1), (20.0, 20.0), (30.0, 25.0)]);
        // A move within half a unit of the last point is no point.
        assert_eq!(s.p.len(), 3);
        let mut a = two(Tool::Arrow, (0.0, 0.0), (50.0, 0.0));
        a.to(80.0, 40.0, 0.0);
        assert_eq!(a.p, vec![[0.0, 0.0, 0.0], [80.0, 40.0, 0.0]], "an arrow is where it landed and where it is now");
        assert!(a.keeps());
        assert!(!two(Tool::Box, (10.0, 10.0), (11.0, 11.0)).keeps(), "a tap with the box is no box");
        assert!(!Stroke::text(Ink::Ink, 18.0, 0.0, 0.0, "  ").keeps(), "a text with no words");
        assert!(Stroke::text(Ink::Ink, 18.0, 0.0, 0.0, "kiln").keeps());
        assert!(Stroke::start(Tool::Pen, Ink::Ink, 3.0, 1.0, 1.0, 0.0).keeps(), "a dot is a mark");
        assert!(!Stroke::start(Tool::Eraser, Ink::Ink, 3.0, 1.0, 1.0, 0.0).keeps(), "the eraser draws nothing");
    }

    #[test]
    fn a_highlighter_in_ink_is_ask_and_a_pencil_presses() {
        assert_eq!(Stroke::start(Tool::Highlighter, Ink::Ink, 16.0, 0.0, 0.0, 0.0).c, Ink::Ask);
        assert_eq!(Stroke::start(Tool::Highlighter, Ink::Live, 16.0, 0.0, 0.0, 0.0).c, Ink::Live);
        assert_eq!(Stroke::start(Tool::Pen, Ink::Ink, 3.0, 0.0, 0.0, 0.0).c, Ink::Ink);
        let mut p = Stroke::start(Tool::Pen, Ink::Ink, 3.0, 0.0, 0.0, 0.42);
        p.to(10.0, 0.0, 0.9);
        assert!(p.pressed());
        assert!(!pen(&[(0.0, 0.0), (10.0, 0.0)]).pressed(), "a finger or a mouse has no pressure");
    }

    #[test]
    fn the_five_colours_are_tokens() {
        let tokens: Vec<&str> = Ink::ALL.iter().map(|i| i.token()).collect();
        assert_eq!(tokens, ["--ink", "--urgent", "--accent", "--live", "--ask"]);
        let contract = include_str!("../../tickets-api/src/tokens.toml");
        for t in tokens {
            assert!(contract.contains(&format!("\"{t}\"")) || contract.contains(t.trim_start_matches("--")), "{t} is not in the token contract");
        }
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut b = Board::default();
        assert!(!b.can_undo() && !b.can_redo());
        assert!(b.add(pen(&[(0.0, 0.0), (10.0, 10.0)])));
        assert!(b.add(two(Tool::Box, (20.0, 20.0), (60.0, 50.0))));
        assert!(!b.add(two(Tool::Arrow, (1.0, 1.0), (1.0, 2.0))), "a stroke not worth keeping is no step");
        assert_eq!(b.strokes.len(), 2);
        assert!(b.undo());
        assert_eq!(b.strokes.len(), 1);
        assert!(b.undo());
        assert!(b.strokes.is_empty());
        assert!(!b.undo(), "nothing before the first stroke");
        assert!(b.redo() && b.redo());
        assert_eq!(b.strokes.len(), 2);
        assert!(!b.redo());
        // A new stroke after an undo drops what could be redone.
        b.undo();
        b.add(pen(&[(5.0, 5.0)]));
        assert!(!b.can_redo());
        assert_eq!(b.strokes.len(), 2);
        // Clear is one step, undone.
        assert!(b.clear());
        assert!(b.strokes.is_empty());
        assert!(!b.clear(), "nothing to clear");
        assert!(b.undo());
        assert_eq!(b.strokes.len(), 2);
    }

    #[test]
    fn the_eraser_hits_what_it_touches_and_nothing_else() {
        let line = pen(&[(0.0, 0.0), (100.0, 0.0)]);
        assert!(hits(&line, 50.0, 5.0, 4.0), "within its reach plus the line's half-width");
        assert!(!hits(&line, 50.0, 20.0, 4.0));
        assert!(!hits(&line, 120.0, 0.0, 4.0), "past its end");
        let dot = pen(&[(10.0, 10.0)]);
        assert!(hits(&dot, 13.0, 10.0, 2.0));
        assert!(!hits(&dot, 20.0, 10.0, 2.0));
        let bx = two(Tool::Box, (0.0, 0.0), (100.0, 100.0));
        assert!(hits(&bx, 100.0, 50.0, 3.0), "on its edge");
        assert!(!hits(&bx, 50.0, 50.0, 3.0), "inside a box is not the box");
        let ar = two(Tool::Arrow, (0.0, 0.0), (100.0, 100.0));
        assert!(hits(&ar, 50.0, 52.0, 3.0));
        assert!(!hits(&ar, 50.0, 80.0, 3.0));
        let tx = Stroke::text(Ink::Ink, 20.0, 10.0, 10.0, "four");
        // four letters at 20 units: 48 wide, 25 tall.
        assert!(hits(&tx, 40.0, 20.0, 1.0));
        assert!(!hits(&tx, 80.0, 20.0, 1.0));
        assert!(!hits(&tx, 30.0, 50.0, 1.0));
    }

    #[test]
    fn a_sweep_of_the_eraser_is_one_undo_step() {
        let mut b = Board::with(vec![pen(&[(0.0, 0.0), (100.0, 0.0)]), pen(&[(0.0, 50.0), (100.0, 50.0)]), pen(&[(0.0, 90.0), (100.0, 90.0)])]);
        let before = b.strokes.clone();
        assert!(b.would_erase(10.0, 0.0, 5.0));
        assert!(!b.would_erase(10.0, 25.0, 5.0));
        assert_eq!(b.erase_at(10.0, 0.0, 5.0), 1);
        assert_eq!(b.erase_at(20.0, 50.0, 5.0), 1);
        assert_eq!(b.erase_at(20.0, 70.0, 5.0), 0);
        assert!(b.erase_end(before.clone()));
        assert_eq!(b.strokes.len(), 1);
        assert!(b.undo());
        assert_eq!(b.strokes, before, "one undo brings the sweep's strokes back");
        // A sweep that took nothing is no step.
        let mut c = Board::with(before.clone());
        assert!(!c.erase_end(before));
        assert!(!c.can_undo());
    }

    #[test]
    fn a_drawing_round_trips_through_json() {
        let strokes = vec![
            pen(&[(1.25, 2.0), (30.0, 40.0)]),
            two(Tool::Arrow, (0.0, 0.0), (100.0, 100.0)),
            Stroke::text(Ink::Live, 18.0, 10.0, 200.0, "the hold at 1240 °C — “tall” pieces"),
            Stroke::start(Tool::Highlighter, Ink::Ink, 16.0, 5.0, 5.0, 0.0),
        ];
        let d = Drawing::of(strokes, 300.0);
        let json = d.to_json();
        assert!(json.contains("\"t\":\"highlighter\"") && json.contains("\"c\":\"ask\""), "{json}");
        assert!(!json.contains("\"s\":null"), "a stroke with no words says none");
        assert_eq!(Drawing::from_json(&json), Some(d));
        assert_eq!(Drawing::from_json("{\"v\":2,\"h\":10,\"strokes\":[]}"), None, "a version this app does not know");
        assert_eq!(Drawing::from_json("not json"), None);
    }

    #[test]
    fn a_drawing_is_as_tall_as_its_strokes_reach() {
        let d = Drawing::of(vec![pen(&[(0.0, 0.0), (10.0, 500.0)])], 300.0);
        assert!(d.h >= 501.5, "{}", d.h);
        let d = Drawing::of(vec![pen(&[(0.0, 0.0), (10.0, 20.0)])], 300.0);
        assert_eq!(d.h, 300.0, "the pad's own height when the strokes are inside it");
    }

    /// A 1x1 PNG, as a canvas makes one.
    fn tiny_png() -> Vec<u8> {
        let mut out = PNG_SIG.to_vec();
        let mut chunk = |ty: &[u8], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = ty.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
        };
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        chunk(b"IDAT", &[0x78, 0x9c, 0x63, 0x60, 0, 0, 0, 2, 0, 1]);
        chunk(b"IEND", &[]);
        out
    }

    #[test]
    fn crc32_is_zlibs() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082, "the CRC every PNG ends with");
    }

    #[test]
    fn the_strokes_travel_inside_the_png_and_the_image_stays_an_image() {
        let png = tiny_png();
        assert_eq!(extract(&png), None, "a plain PNG carries nothing");
        let d = Drawing::of(vec![pen(&[(0.0, 0.0), (10.0, 10.0)]), Stroke::text(Ink::Ink, 18.0, 0.0, 0.0, "ünïcode ✓")], 300.0);
        let with = embed(&png, &d.to_json()).expect("embedded");
        assert_eq!(drawing_in(&with), Some(d.clone()));
        // The server's own rule still takes it: a PNG, 1x1.
        let sniffed = menger_tickets_api::media::check("image/png", &with).expect("still a PNG menger keeps");
        assert_eq!((sniffed.width, sniffed.height), (1, 1));
        // Every chunk's CRC holds, and IEND is last.
        let list = chunks(&with).unwrap();
        for (at, _, data) in &list {
            let body = &with[at + 4..at + 8 + data.len()];
            let crc = u32::from_be_bytes(with[at + 8 + data.len()..at + 12 + data.len()].try_into().unwrap());
            assert_eq!(crc32(body), crc);
        }
        assert_eq!(&list.last().unwrap().1, b"IEND");
        // Embedding again replaces the strokes, never doubles them.
        let d2 = Drawing::of(vec![pen(&[(5.0, 5.0)])], 100.0);
        let again = embed(&with, &d2.to_json()).unwrap();
        assert_eq!(drawing_in(&again), Some(d2));
        assert_eq!(chunks(&again).unwrap().iter().filter(|c| &c.1 == b"iTXt").count(), 1);
        assert_eq!(embed(b"not a png", "{}"), None);
        assert_eq!(embed(&png[..png.len() - 4], "{}"), None, "a truncated PNG");
    }

    /// APP-104: a mark-up's strokes travel with the hash of the image they
    /// were drawn over, and are learned apart from it.
    #[test]
    fn a_mark_up_keeps_its_photo_and_its_strokes_apart() {
        let src = Source { hash: "ab".repeat(32), w: 1170, h: 2532 };
        let d = Drawing::over(vec![pen(&[(100.0, 100.0), (400.0, 900.0)])], src.clone());
        assert_eq!(d.h, 2164.1, "the image's aspect, in units");
        let with = embed(&tiny_png(), &d.to_json()).expect("embedded");
        assert_eq!(drawing_in(&with), Some(d.clone()), "the strokes and the source come back");
        assert!(learn("cd", &with), "a mark-up is a drawing the page knows");
        assert!(is_drawing("cd"));
        let back = DRAWINGS.with(|m| m.borrow().get("cd").cloned()).unwrap();
        assert_eq!(back.bg, Some(src), "with the photo's hash, to open over it again");
        assert_eq!(back.strokes, d.strokes);
        // A drawing from before APP-104 reads as it did, with no image.
        let old = r#"{"v":1,"h":300,"strokes":[]}"#;
        assert_eq!(Drawing::from_json(old).map(|d| d.bg), Some(None));
        assert!(!Drawing::of(Vec::new(), 300.0).to_json().contains("bg"), "a plain drawing writes no bg");
    }

    /// APP-104: a mark-up is flattened at the photo's width, at most
    /// 2,400 px, a quarter smaller each try past the cap.
    #[test]
    fn a_mark_up_shrinks_to_fit_the_cap() {
        assert_eq!(markup_px(1170.0, 0), 1170.0);
        assert_eq!(markup_px(4032.0, 0), 2400.0);
        assert_eq!(markup_px(4032.0, 1), 1800.0);
        assert_eq!(markup_px(4032.0, 2), 1350.0);
        assert_eq!(markup_px(300.0, 5), 240.0, "never under 240");
    }
}
