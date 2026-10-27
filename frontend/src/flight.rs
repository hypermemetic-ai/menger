//! DEPLOY-30: what we're working on, by where it will land — the model of
//! `#/deploy` (src/deploy_view.rs draws it).
//!
//! **The work in flight** of a project, toward one destination (an
//! environment nothing promotes from — menger's `prod`): what is being
//! written (`Coding`), what is finished and waits for the next deploy (`In
//! Review` — DEPLOY-22: a merge leaves it there until the deploy to test
//! completes it), what is Complete and runs nowhere yet (`waiting`), and
//! what runs on an environment short of the destination (`ready for` the
//! next one). One filtered `list` asks for all of it ([`membership`]):
//! FILTER-3's cells, so a membership watch says when a ticket enters or
//! leaves — a deploy, a promote, a status change.
//!
//! **Where each sits** ([`node_of`]): a lane — coding, in review, waiting,
//! or on an environment of the destination's track ([`track`]) — and its
//! state, the stage it is in on the way to the next environment
//! (deploying and failed first: they are what to look at). Shipped work (its
//! current version on the destination) is no node.
//!
//! **Clusters are areas**: a shortcode's tickets together ([`clusters`]),
//! the biggest first, each summarised by its states' counts.
//!
//! **The canvas** ([`lay_out`]): a column per lane, left to right in the
//! order work moves, the destination at the right; a band per cluster, its
//! nodes stacked in their lane's column and wrapped in the hull of their
//! corners ([`hull`]); blockers are arrows between the nodes drawn.
//!
//! Everything here is pure and pinned by native tests.

use std::collections::{BTreeMap, HashMap};

use menger_tickets_api::deployments::DeliveryStage;
use menger_tickets_api::{Environment, Kind, Status, TicketSummary};

use crate::deploys::Mark;

/// Where a ticket is on its way to the destination.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Lane {
    /// Being written (`Coding`).
    Coding,
    /// Finished, waiting for the next deploy (`In Review`).
    Review,
    /// Complete, its current version on no environment of the track.
    Waiting,
    /// Its current version runs on this environment of the track — the
    /// furthest such — and not yet on the destination.
    On(String),
}

impl Lane {
    /// The lane's key, for a DOM attribute: `coding`, `review`, `waiting`,
    /// `on-test`.
    pub(crate) fn key(&self) -> String {
        match self {
            Lane::Coding => "coding".into(),
            Lane::Review => "review".into(),
            Lane::Waiting => "waiting".into(),
            Lane::On(e) => format!("on-{e}"),
        }
    }

    /// The lane's heading: `coding`, `in review`, `waiting`, `on test`.
    pub(crate) fn words(&self) -> String {
        match self {
            Lane::Coding => "coding".into(),
            Lane::Review => "in review".into(),
            Lane::Waiting => "waiting".into(),
            Lane::On(e) => format!("on {e}"),
        }
    }

    /// Where the lane's work lands, under its heading: `→ test at the next
    /// deploy`, `→ prod at the next promote`.
    pub(crate) fn lands(&self, track: &[String]) -> String {
        let first = track.first().map(String::as_str).unwrap_or("the first environment");
        match self {
            Lane::Coding => "→ in review, then its merge".into(),
            Lane::Review => format!("→ {first} at the next deploy"),
            Lane::Waiting => format!("Complete, on no environment → {first}"),
            Lane::On(e) => match next_after(track, e) {
                Some(n) => format!("→ {n} at the next promote"),
                None => "shipped".into(),
            },
        }
    }
}

/// The environment of `track` after `env`.
fn next_after<'a>(track: &'a [String], env: &str) -> Option<&'a str> {
    let i = track.iter().position(|e| e == env)?;
    track.get(i + 1).map(String::as_str)
}

/// A node's state: the stage it is in on its way to the next environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum State {
    Coding,
    Review,
    Waiting,
    Ready,
    Deploying,
    Failed,
}

impl State {
    /// In the order work moves, the legend's.
    pub(crate) const ALL: [State; 6] = [State::Coding, State::Review, State::Waiting, State::Ready, State::Deploying, State::Failed];

    /// Its class: `coding`, `review`, `waiting`, `ready`, `deploying`,
    /// `failed`.
    pub(crate) const fn class(self) -> &'static str {
        match self {
            State::Coding => "coding",
            State::Review => "review",
            State::Waiting => "waiting",
            State::Ready => "ready",
            State::Deploying => "deploying",
            State::Failed => "failed",
        }
    }

    /// The state a class names (DEPLOY-39: `?stage=ready`).
    pub(crate) fn of_class(class: &str) -> Option<State> {
        State::ALL.into_iter().find(|s| s.class() == class)
    }

    /// The legend's word for it.
    pub(crate) const fn words(self) -> &'static str {
        match self {
            State::Coding => "coding",
            State::Review => "in review",
            State::Waiting => "waiting for a deploy",
            State::Ready => "ready to promote",
            State::Deploying => "deploying",
            State::Failed => "failed",
        }
    }

    /// The glyph in its node's dot.
    pub(crate) const fn glyph(self) -> &'static str {
        match self {
            State::Coding => "✎",
            State::Review => "◐",
            State::Waiting => "⏸",
            State::Ready => "→",
            State::Deploying => "↻",
            State::Failed => "×",
        }
    }
}

/// One ticket in flight: where it sits and what it waits for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    pub(crate) id: String,
    pub(crate) epic: String,
    pub(crate) title: String,
    pub(crate) status: Status,
    pub(crate) lane: Lane,
    pub(crate) state: State,
    /// The environment it lands on next.
    pub(crate) next: Option<String>,
    pub(crate) blocked_by: Vec<String>,
    pub(crate) unlocks: Vec<String>,
    pub(crate) commits: Vec<String>,
}

impl Node {
    /// Its state in words, with where: `ready for prod`, `in review → test`,
    /// `deploying to test`.
    pub(crate) fn state_words(&self) -> String {
        let next = self.next.as_deref();
        match (self.state, next) {
            (State::Coding, _) => "coding".into(),
            (State::Review, Some(n)) => format!("in review → {n}"),
            (State::Waiting, Some(n)) => format!("waiting for {n}"),
            (State::Ready, Some(n)) => format!("ready for {n}"),
            (State::Deploying, Some(n)) => format!("deploying to {n}"),
            (State::Failed, Some(n)) => format!("failed on {n}"),
            (s, None) => s.words().into(),
        }
    }
}

/// The environments of `project` nothing promotes from — where a track
/// ends — in display order.
pub(crate) fn destinations(envs: &[Environment], project: &str) -> Vec<String> {
    let mine: Vec<&Environment> = envs.iter().filter(|e| e.project == project).collect();
    let mut out: Vec<&Environment> = mine.iter().copied().filter(|e| !mine.iter().any(|x| x.from.as_deref() == Some(e.name.as_str()))).collect();
    out.sort_by_key(|e| (e.order, e.name.clone()));
    out.into_iter().map(|e| e.name.clone()).collect()
}

/// The destination a page opens on: of those that promote from another,
/// the one deployed to last (DEPLOY-40: `app-prod` once prod moved to
/// app.menger.sh, RUN-13), `prod` on a tie; with none deployed, `prod` when
/// there is one, else the one that runs the most tickets.
pub(crate) fn default_destination(envs: &[Environment], project: &str) -> Option<String> {
    let dests = destinations(envs, project);
    let last_at = |d: &str| envs.iter().find(|e| e.project == project && e.name == d && e.from.is_some()).and_then(|e| e.last_at);
    if let Some(d) = dests.iter().filter(|d| last_at(d).is_some()).max_by_key(|d| (last_at(d), *d == "prod")) {
        return Some(d.clone());
    }
    if dests.iter().any(|d| d == "prod") {
        return Some("prod".into());
    }
    dests
        .into_iter()
        .max_by_key(|d| envs.iter().find(|e| e.project == project && &e.name == d).map_or(0, |e| e.tickets))
}

/// DEPLOY-43: the destinations another has taken the role of. A destination
/// other than the page's default is retired when its last deploy is older than
/// the default's (or it never had one) and it sits later in the order than
/// the default that took its place (`prod`'s 40 after `app-prod`'s 25). Read
/// from the deployments data, never a name.
pub(crate) fn retired(envs: &[Environment], project: &str) -> Vec<String> {
    let Some(cur) = default_destination(envs, project) else { return Vec::new() };
    let of = |n: &str| envs.iter().find(|e| e.project == project && e.name == n);
    let (Some(cur_env), cur_track) = (of(&cur), track(envs, project, &cur)) else { return Vec::new() };
    let Some(cur_at) = cur_env.last_at else { return Vec::new() };
    if cur_track.len() < 2 {
        return Vec::new();
    }
    destinations(envs, project)
        .into_iter()
        .filter(|d| *d != cur)
        .filter(|d| {
            let Some(e) = of(d) else { return false };
            let older = e.last_at.is_none_or(|t| t < cur_at);
            older && e.order > cur_env.order
        })
        .collect()
}

/// The track that ends at `dest`: it and every environment it promotes
/// from, first to last (`["test", "prod"]`).
pub(crate) fn track(envs: &[Environment], project: &str, dest: &str) -> Vec<String> {
    let mut out = vec![dest.to_string()];
    let mut at = dest.to_string();
    while let Some(from) = envs.iter().find(|e| e.project == project && e.name == at).and_then(|e| e.from.clone()) {
        if out.contains(&from) {
            break; // a cycle the server refuses; never loop on one
        }
        out.push(from.clone());
        at = from;
    }
    out.reverse();
    out
}

/// The filter that admits the work in flight toward the track's end — in
/// FILTER-3's language, so a membership watch keeps it:
/// `under MENGER and not kind doc and (status in (Coding, "In Review") or
/// waiting menger/prod or ready for menger/prod or deploying … or failed …)`.
pub(crate) fn membership(project: &str, track: &[String]) -> String {
    let p = project.to_ascii_lowercase();
    let mut any = vec!["status in (Coding, \"In Review\")".to_string()];
    if let Some(dest) = track.last() {
        any.push(format!("waiting {p}/{dest}"));
    }
    for (i, e) in track.iter().enumerate() {
        if i > 0 {
            any.push(format!("ready for {p}/{e}"));
        }
        any.push(format!("deploying {p}/{e}"));
        any.push(format!("failed {p}/{e}"));
    }
    format!("under {project} and not kind doc and ({})", any.join(" or "))
}

/// Where `t` sits, from its row and its marks — `None` for shipped work
/// (its current version on the destination) and for a doc.
pub(crate) fn node_of(t: &TicketSummary, marks: &[Mark], project: &str, track: &[String]) -> Option<Node> {
    if t.kind == Kind::Doc || track.is_empty() {
        return None;
    }
    let mark = |env: &str| marks.iter().find(|m| m.project == project && m.env == env);
    let current = |env: &str| mark(env).is_some_and(|m| m.stage.is_none() && m.behind.is_empty());
    let staged = |stage: DeliveryStage| track.iter().find(|e| mark(e).is_some_and(|m| m.stage == Some(stage))).cloned();
    let last = track.len() - 1;
    let furthest = track[..last].iter().rposition(|e| current(e));
    let shipped = current(&track[last]);
    let lane = match t.status {
        Status::Coding => Lane::Coding,
        Status::InReview => Lane::Review,
        _ if shipped => return None,
        _ => match furthest {
            Some(i) => Lane::On(track[i].clone()),
            None => Lane::Waiting,
        },
    };
    let next = match furthest {
        Some(i) => track.get(i + 1).cloned(),
        None => track.first().cloned(),
    };
    let (state, next) = if let Some(e) = staged(DeliveryStage::Deploying) {
        (State::Deploying, Some(e))
    } else if let Some(e) = staged(DeliveryStage::Failed) {
        (State::Failed, Some(e))
    } else {
        let state = match &lane {
            Lane::Coding => State::Coding,
            Lane::Review => State::Review,
            Lane::Waiting => State::Waiting,
            Lane::On(_) => State::Ready,
        };
        (state, next)
    };
    Some(Node {
        id: t.id.to_string(),
        epic: t.epic.to_string(),
        title: t.title.clone(),
        status: t.status,
        lane,
        state,
        next,
        blocked_by: t.blocked_by.iter().map(ToString::to_string).collect(),
        unlocks: t.unlocks.iter().map(ToString::to_string).collect(),
        commits: t.commits.iter().map(ToString::to_string).collect(),
    })
}

/// An area's work in flight.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cluster {
    pub(crate) code: String,
    pub(crate) nodes: Vec<Node>,
}

impl Cluster {
    /// How many of its nodes are in each state, in the legend's order — only
    /// the states it has.
    pub(crate) fn tally(&self) -> Vec<(State, usize)> {
        State::ALL
            .iter()
            .map(|s| (*s, self.nodes.iter().filter(|n| n.state == *s).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// Its summary: the loudest state as a chip, and its counts in words —
    /// `(failed, "1 failed · 2 ready")`, `(ready, "all 3 ready")`.
    pub(crate) fn summary(&self) -> (State, String) {
        let tally = self.tally();
        let loudest = if tally.iter().any(|(s, _)| *s == State::Failed) {
            State::Failed
        } else if tally.iter().any(|(s, _)| *s == State::Deploying) {
            State::Deploying
        } else {
            tally.iter().map(|(s, _)| *s).max().unwrap_or(State::Coding)
        };
        let words = match tally.as_slice() {
            [(s, n)] if *n > 1 => format!("all {n} {}", short(*s)),
            _ => tally.iter().map(|(s, n)| format!("{n} {}", short(*s))).collect::<Vec<_>>().join(" · "),
        };
        (loudest, words)
    }
}

/// A state as a cluster's summary counts it.
fn short(s: State) -> &'static str {
    match s {
        State::Coding => "coding",
        State::Review => "in review",
        State::Waiting => "waiting",
        State::Ready => "ready",
        State::Deploying => "deploying",
        State::Failed => "failed",
    }
}

/// The nodes by area: one cluster per shortcode, the most work first (then
/// by name); in each, the nodes in lane order, then by id's number.
pub(crate) fn clusters(nodes: Vec<Node>) -> Vec<Cluster> {
    let mut by: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for n in nodes {
        by.entry(n.epic.clone()).or_default().push(n);
    }
    let mut out: Vec<Cluster> = by
        .into_iter()
        .map(|(code, mut nodes)| {
            nodes.sort_by(|a, b| a.lane.cmp(&b.lane).then_with(|| seq(&a.id).cmp(&seq(&b.id))).then_with(|| a.id.cmp(&b.id)));
            Cluster { code, nodes }
        })
        .collect();
    out.sort_by(|a, b| b.nodes.len().cmp(&a.nodes.len()).then_with(|| a.code.cmp(&b.code)));
    out
}

/// DEPLOY-39: the nodes a stage pill admits — all of them when none is on.
pub(crate) fn only(nodes: Vec<Node>, stage: Option<State>) -> Vec<Node> {
    match stage {
        Some(s) => nodes.into_iter().filter(|n| n.state == s).collect(),
        None => nodes,
    }
}

/// An id's number: `APP-64` → 64.
fn seq(id: &str) -> u32 {
    id.rsplit_once('-').and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
}

/// The lanes a track draws, in the order work moves.
pub(crate) fn lanes(track: &[String]) -> Vec<Lane> {
    let mut out = vec![Lane::Coding, Lane::Review, Lane::Waiting];
    if let Some((_, before)) = track.split_last() {
        out.extend(before.iter().cloned().map(Lane::On));
    }
    out
}

/// A node's size and the gaps between them, in canvas units.
pub(crate) const NW: f64 = 176.0;
pub(crate) const NH: f64 = 44.0;
const GAP_X: f64 = 64.0;
const GAP_Y: f64 = 10.0;
/// The lanes' headings above the bands.
pub(crate) const HEAD: f64 = 72.0;
/// Room above a band's first row for its label (the hull's pad included).
const LABEL: f64 = 58.0;
/// A hull's pad around its nodes (half its outline's width).
pub(crate) const PAD: f64 = 22.0;

/// A cluster laid out: where its label goes and the hull around it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Band {
    pub(crate) code: String,
    pub(crate) label: (f64, f64),
    pub(crate) hull: Vec<(f64, f64)>,
}

/// The canvas: every node's top-left, the bands, each lane's column, the
/// destination's column and the whole size.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Laid {
    pub(crate) pos: HashMap<String, (f64, f64)>,
    pub(crate) bands: Vec<Band>,
    pub(crate) columns: Vec<(Lane, f64)>,
    pub(crate) dest_x: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// A lane's column's left edge.
fn column_x(i: usize) -> f64 {
    PAD + i as f64 * (NW + GAP_X)
}

/// Lay `clusters` out over `lanes`: a column per lane, a band per cluster,
/// its nodes stacked in their lane's column.
pub(crate) fn lay_out(clusters: &[Cluster], lanes: &[Lane]) -> Laid {
    let mut laid = Laid { columns: lanes.iter().enumerate().map(|(i, l)| (l.clone(), column_x(i))).collect(), ..Laid::default() };
    let mut y = HEAD;
    for c in clusters {
        let top = y + LABEL;
        let mut rows: HashMap<usize, usize> = HashMap::new();
        let mut corners = Vec::new();
        let mut left = f64::MAX;
        for n in &c.nodes {
            let Some(i) = lanes.iter().position(|l| *l == n.lane) else { continue };
            let r = rows.entry(i).or_insert(0);
            let (x, ny) = (column_x(i), top + *r as f64 * (NH + GAP_Y));
            *r += 1;
            laid.pos.insert(n.id.clone(), (x, ny));
            corners.extend([(x, ny), (x + NW, ny), (x, ny + NH), (x + NW, ny + NH)]);
            left = left.min(x);
        }
        let deepest = rows.values().copied().max().unwrap_or(0);
        if deepest == 0 {
            continue;
        }
        laid.bands.push(Band { code: c.code.clone(), label: (left - 4.0, top - PAD - 22.0), hull: hull(corners) });
        y = top + deepest as f64 * (NH + GAP_Y) - GAP_Y + PAD;
    }
    laid.dest_x = column_x(lanes.len());
    laid.width = laid.dest_x + 168.0 + PAD;
    laid.height = y + PAD;
    laid
}

/// The convex hull of `pts`, counter-clockwise from the leftmost (Andrew's
/// monotone chain).
pub(crate) fn hull(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A hull as an SVG path.
pub(crate) fn path_of(pts: &[(f64, f64)]) -> String {
    let mut d = String::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        d.push_str(&format!("{}{x:.0} {y:.0}", if i == 0 { "M" } else { "L" }));
    }
    d.push('Z');
    d
}

/// A blocker's arrow from `a`'s node to `b`'s (top-lefts): right side to
/// left side across columns, bottom to top within one.
pub(crate) fn edge_path(a: (f64, f64), b: (f64, f64)) -> String {
    if (a.0 - b.0).abs() < 1.0 {
        let (x, y1, y2) = (a.0 + NW - 14.0, a.1 + NH, b.1);
        let (y1, y2) = if y1 <= y2 { (y1, y2) } else { (a.1, b.1 + NH) };
        return format!("M{x:.0} {y1:.0}C{:.0} {y1:.0} {:.0} {y2:.0} {x:.0} {y2:.0}", x + 18.0, x + 18.0);
    }
    let (s, e) = if a.0 < b.0 { ((a.0 + NW, a.1 + NH / 2.0), (b.0, b.1 + NH / 2.0)) } else { ((a.0, a.1 + NH / 2.0), (b.0 + NW, b.1 + NH / 2.0)) };
    let dx = (e.0 - s.0) / 2.0;
    format!("M{:.0} {:.0}C{:.0} {:.0} {:.0} {:.0} {:.0} {:.0}", s.0, s.1, s.0 + dx, s.1, e.0 - dx, e.1, e.0, e.1)
}

/// A title cut to fit a node: at most `n` characters, an ellipsis when cut.
pub(crate) fn clip(title: &str, n: usize) -> String {
    if title.chars().count() <= n {
        return title.to_string();
    }
    let mut s: String = title.chars().take(n.saturating_sub(1)).collect();
    s = s.trim_end().to_string();
    s.push('…');
    s
}

/// The fit that shows the canvas's whole width in a view `w` × `h` wide,
/// its top at the top — never larger than 100%, never under 30%: `(k, tx,
/// ty)`.
pub(crate) fn fit_width(laid: &Laid, w: f64) -> (f64, f64, f64) {
    let k = ((w - 24.0) / laid.width).clamp(0.3, 1.0);
    let tx = ((w - laid.width * k) / 2.0).max(12.0);
    (k, tx, 0.0)
}

/// The fit that shows all of it: `(k, tx, ty)`.
pub(crate) fn fit_all(laid: &Laid, w: f64, h: f64) -> (f64, f64, f64) {
    let k = ((w - 24.0) / laid.width).min((h - 24.0) / laid.height).clamp(0.1, 1.0);
    (k, (w - laid.width * k) / 2.0, ((h - laid.height * k) / 2.0).max(0.0))
}

/// A zoom by `f` about the point `(cx, cy)` of the view, from `(k, tx,
/// ty)`, held between 10% and 300%.
pub(crate) fn zoom_at((k, tx, ty): (f64, f64, f64), f: f64, cx: f64, cy: f64) -> (f64, f64, f64) {
    let nk = (k * f).clamp(0.1, 3.0);
    (nk, cx - (cx - tx) * nk / k, cy - (cy - ty) * nk / k)
}

/// DEPLOY-38: a pinch from `v0` (the view when the two fingers came down
/// at `a0` and `b0`) to the fingers now at `a1` and `b1`: the zoom is the
/// spread's ratio, about the fingers' midpoint, and the midpoint's move
/// pans — what was under the fingers stays under them. Held as `zoom_at`
/// holds it.
pub(crate) fn pinch(v0: (f64, f64, f64), a0: (f64, f64), b0: (f64, f64), a1: (f64, f64), b1: (f64, f64)) -> (f64, f64, f64) {
    let (k, tx, ty) = v0;
    let d0 = (a0.0 - b0.0).hypot(a0.1 - b0.1).max(1.0);
    let d1 = (a1.0 - b1.0).hypot(a1.1 - b1.1).max(1.0);
    let nk = (k * d1 / d0).clamp(0.1, 3.0);
    let (m0, m1) = (((a0.0 + b0.0) / 2.0, (a0.1 + b0.1) / 2.0), ((a1.0 + b1.0) / 2.0, (a1.1 + b1.1) / 2.0));
    let (wx, wy) = ((m0.0 - tx) / k, (m0.1 - ty) / k);
    (nk, m1.0 - wx * nk, m1.1 - wy * nk)
}

/// DEPLOY-38: whether a tap at `at` (ms) and `p` is the second of a
/// double-tap after one at `prev` — within 300 ms and 30 px.
pub(crate) fn double_tap(prev: Option<(f64, f64, f64)>, at: f64, p: (f64, f64)) -> bool {
    prev.is_some_and(|(t, x, y)| at - t <= 300.0 && (p.0 - x).hypot(p.1 - y) <= 30.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::filter::Filter;

    fn env(name: &str, from: Option<&str>, order: u32, tickets: u64) -> Environment {
        Environment {
            project: "MENGER".into(),
            name: name.into(),
            order,
            revision: None,
            release: None,
            since: None,
            deployment: None,
            last: None,
            last_state: None,
            last_at: None,
            tickets,
            from: from.map(str::to_string),
        }
    }

    /// menger's own: test → prod, test → app-test → app-prod, and edge on
    /// its own track.
    fn menger() -> Vec<Environment> {
        vec![
            env("test", None, 20, 271),
            env("app-test", Some("test"), 25, 22),
            env("app-prod", Some("app-test"), 25, 10),
            env("edge", None, 25, 87),
            env("prod", Some("test"), 40, 251),
        ]
    }

    fn row(id: &str, status: Status) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            archived: None,
            frozen: false,
            hidden: false,
            labels: Vec::new(),
            assignees: Vec::new(),
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic),
            shortcode: menger_tickets_api::ShortcodeId::new(epic),
            address: String::new(),
            seq: seq.parse().expect("seq"),
            title: format!("the title of {id}"),
            status,
            kind: Kind::Build,
            severity: None,
            parent: None,
            child_count: 0,
            commits: Vec::new(),
            path: None,
            placement: None,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn mark(env: &str, stage: Option<DeliveryStage>, behind: bool) -> Mark {
        Mark {
            project: "MENGER".into(),
            env: env.into(),
            since: 0,
            deployment: 1,
            behind: if behind { vec!["abc1234".into()] } else { Vec::new() },
            version: None,
            stage,
        }
    }

    fn tr() -> Vec<String> {
        vec!["test".into(), "prod".into()]
    }

    #[test]
    fn destinations_and_tracks() {
        let envs = menger();
        assert_eq!(destinations(&envs, "MENGER"), ["app-prod", "edge", "prod"]);
        assert_eq!(default_destination(&envs, "MENGER").as_deref(), Some("prod"));
        assert_eq!(track(&envs, "MENGER", "prod"), ["test", "prod"]);
        assert_eq!(track(&envs, "MENGER", "app-prod"), ["test", "app-test", "app-prod"]);
        assert_eq!(track(&envs, "MENGER", "edge"), ["edge"]);
        // DEPLOY-40: the destination deployed to last, of those on a track.
        let at = |envs: Vec<Environment>, name: &str, t: i64| -> Vec<Environment> {
            envs.into_iter().map(|mut e| {
                if e.name == name {
                    e.last_at = Some(t);
                }
                e
            }).collect()
        };
        let envs2 = at(at(at(menger(), "prod", 100), "app-prod", 200), "edge", 300);
        assert_eq!(default_destination(&envs2, "MENGER").as_deref(), Some("app-prod"), "edge promotes from nothing: not a track's end");
        let envs2 = at(envs2, "prod", 200);
        assert_eq!(default_destination(&envs2, "MENGER").as_deref(), Some("prod"), "a tie is prod's");
        // No prod: the destination that runs the most.
        let envs: Vec<Environment> = menger().into_iter().filter(|e| e.name != "prod").collect();
        assert_eq!(default_destination(&envs, "MENGER").as_deref(), Some("edge"));
        assert_eq!(default_destination(&envs, "OTHER"), None);
    }

    /// DEPLOY-43: prod, no longer deployed to once app-prod took its role, is
    /// retired; edge (its own track, same order as app-prod) never is.
    #[test]
    fn a_destination_another_took_over_is_retired() {
        let at = |t: [(&str, i64); 3]| -> Vec<Environment> {
            menger().into_iter().map(|mut e| { e.last_at = t.iter().find(|(n, _)| *n == e.name).map(|x| x.1); e }).collect()
        };
        let envs = at([("prod", 100), ("app-prod", 200), ("edge", 50)]);
        assert_eq!(retired(&envs, "MENGER"), ["prod"]);
        assert!(retired(&menger(), "MENGER").is_empty(), "nothing deployed: nothing retired");
        let live = at([("prod", 300), ("app-prod", 200), ("edge", 50)]);
        assert_eq!(default_destination(&live, "MENGER").as_deref(), Some("prod"));
        assert!(retired(&live, "MENGER").is_empty());
    }

    /// The filter is the language's: it parses, and names every cell of the
    /// track.
    #[test]
    fn the_membership_filter_parses() {
        let f = membership("MENGER", &tr());
        assert_eq!(
            f,
            "under MENGER and not kind doc and (status in (Coding, \"In Review\") or waiting menger/prod or deploying menger/test or failed menger/test or ready for menger/prod or deploying menger/prod or failed menger/prod)"
        );
        Filter::parse(&f).expect("parses");
        Filter::parse(&membership("MENGER", &["edge".to_string()])).expect("one environment parses");
        Filter::parse(&membership("MENGER", &["test".into(), "app-test".into(), "app-prod".into()])).expect("three parse");
    }

    #[test]
    fn where_each_ticket_sits() {
        let t = tr();
        let at = |s: Status, ms: &[Mark]| node_of(&row("APP-1", s), ms, "MENGER", &t).map(|n| (n.lane, n.state, n.next));
        // Work in progress lands on the first environment.
        assert_eq!(at(Status::Coding, &[]), Some((Lane::Coding, State::Coding, Some("test".into()))));
        assert_eq!(at(Status::InReview, &[]), Some((Lane::Review, State::Review, Some("test".into()))));
        // Complete on nothing: waiting for test.
        assert_eq!(at(Status::Complete, &[]), Some((Lane::Waiting, State::Waiting, Some("test".into()))));
        // On test, ready for prod.
        let on_test = [mark("test", None, false), mark("prod", Some(DeliveryStage::ReadyFor), false)];
        assert_eq!(at(Status::Complete, &on_test), Some((Lane::On("test".into()), State::Ready, Some("prod".into()))));
        // Behind on test is not on it.
        assert_eq!(at(Status::Complete, &[mark("test", None, true)]).map(|x| x.0), Some(Lane::Waiting));
        // Shipped: no node.
        assert_eq!(at(Status::Complete, &[mark("test", None, false), mark("prod", None, false)]), None);
        // Deploying and failed are said first, with where.
        let going = [mark("test", None, false), mark("prod", Some(DeliveryStage::Deploying), false)];
        assert_eq!(at(Status::Complete, &going), Some((Lane::On("test".into()), State::Deploying, Some("prod".into()))));
        let failed = [mark("test", Some(DeliveryStage::Failed), false)];
        assert_eq!(at(Status::InReview, &failed), Some((Lane::Review, State::Failed, Some("test".into()))));
        // Another project's marks are not this one's.
        let mut other = mark("prod", None, false);
        other.project = "KILN".into();
        assert_eq!(at(Status::Complete, &[other]).map(|x| x.0), Some(Lane::Waiting));
        // A doc is never in flight.
        let mut doc = row("APP-2", Status::Doc);
        doc.kind = Kind::Doc;
        assert_eq!(node_of(&doc, &[], "MENGER", &t), None);
    }

    #[test]
    fn a_nodes_words() {
        let n = node_of(&row("APP-1", Status::InReview), &[], "MENGER", &tr()).expect("node");
        assert_eq!(n.state_words(), "in review → test");
        let n = node_of(&row("APP-1", Status::Complete), &[mark("test", None, false)], "MENGER", &tr()).expect("node");
        assert_eq!(n.state_words(), "ready for prod");
        assert_eq!(Lane::On("test".into()).lands(&tr()), "→ prod at the next promote");
        assert_eq!(Lane::Review.lands(&tr()), "→ test at the next deploy");
        assert_eq!(lanes(&tr()), [Lane::Coding, Lane::Review, Lane::Waiting, Lane::On("test".into())]);
        assert_eq!(lanes(&["edge".to_string()]), [Lane::Coding, Lane::Review, Lane::Waiting]);
    }

    fn nodes(ids: &[(&str, Status, &[Mark])]) -> Vec<Node> {
        ids.iter().filter_map(|(id, s, ms)| node_of(&row(id, *s), ms, "MENGER", &tr())).collect()
    }

    #[test]
    fn clusters_are_areas_biggest_first() {
        let on_test = [mark("test", None, false)];
        let cs = clusters(nodes(&[
            ("CHAT-30", Status::InReview, &[]),
            ("APP-65", Status::Complete, &on_test),
            ("APP-9", Status::Coding, &[]),
            ("APP-64", Status::Complete, &on_test),
            ("CI-5", Status::Coding, &[]),
        ]));
        let codes: Vec<&str> = cs.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["APP", "CHAT", "CI"]);
        let ids: Vec<&str> = cs[0].nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["APP-9", "APP-64", "APP-65"], "lane order, then by number");
        assert_eq!(cs[0].summary(), (State::Ready, "1 coding · 2 ready".to_string()));
        assert_eq!(cs[1].summary(), (State::Review, "1 in review".to_string()));
        let all = clusters(nodes(&[("APP-1", Status::Complete, &on_test), ("APP-2", Status::Complete, &on_test)]));
        assert_eq!(all[0].summary(), (State::Ready, "all 2 ready".to_string()));
        let loud = clusters(nodes(&[("APP-1", Status::Complete, &on_test), ("APP-2", Status::InReview, &[mark("test", Some(DeliveryStage::Failed), false)])]));
        assert_eq!(loud[0].summary(), (State::Failed, "1 ready · 1 failed".to_string()));
    }

    /// No two nodes overlap, each sits in its lane's column, and each band's
    /// hull holds its own nodes and no other band's.
    #[test]
    fn the_layout_never_overlaps() {
        let on_test = [mark("test", None, false)];
        let cs = clusters(nodes(&[
            ("APP-1", Status::Coding, &[]),
            ("APP-2", Status::Coding, &[]),
            ("APP-3", Status::Complete, &on_test),
            ("CHAT-1", Status::InReview, &[]),
            ("CHAT-2", Status::Complete, &[]),
            ("CI-1", Status::Complete, &on_test),
        ]));
        let ls = lanes(&tr());
        let laid = lay_out(&cs, &ls);
        assert_eq!(laid.pos.len(), 6);
        let boxes: Vec<(&String, (f64, f64))> = laid.pos.iter().map(|(k, v)| (k, *v)).collect();
        for (i, (a, pa)) in boxes.iter().enumerate() {
            for (b, pb) in &boxes[i + 1..] {
                let apart = pa.0 + NW <= pb.0 || pb.0 + NW <= pa.0 || pa.1 + NH <= pb.1 || pb.1 + NH <= pa.1;
                assert!(apart, "{a} and {b} overlap");
            }
        }
        for c in &cs {
            for n in &c.nodes {
                let col = laid.columns.iter().find(|(l, _)| *l == n.lane).expect("a column").1;
                assert_eq!(laid.pos[&n.id].0, col, "{} in its lane", n.id);
            }
        }
        // Bands stack: each band's top below the one before's bottom.
        let tops: Vec<f64> = laid.bands.iter().map(|b| b.hull.iter().map(|p| p.1).fold(f64::MAX, f64::min)).collect();
        let bottoms: Vec<f64> = laid.bands.iter().map(|b| b.hull.iter().map(|p| p.1).fold(f64::MIN, f64::max)).collect();
        for i in 1..tops.len() {
            assert!(tops[i] > bottoms[i - 1] + PAD * 2.0, "band {i} clears the one above with its pad");
        }
        assert!(laid.width > laid.dest_x && laid.height > *bottoms.last().expect("a band"));
    }

    #[test]
    fn the_hull_and_the_view() {
        let h = hull(vec![(0.0, 0.0), (2.0, 0.0), (1.0, 1.0), (2.0, 2.0), (0.0, 2.0), (1.0, 0.0)]);
        assert_eq!(h, [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        assert_eq!(path_of(&h), "M0 0L2 0L2 2L0 2Z");
        assert_eq!(clip("a short one", 20), "a short one");
        assert_eq!(clip("a title much too long for a node", 12), "a title muc…");
        let laid = Laid { width: 1000.0, height: 2000.0, ..Laid::default() };
        assert_eq!(fit_width(&laid, 1424.0), (1.0, 212.0, 0.0));
        assert_eq!(fit_width(&laid, 524.0).0, 0.5);
        let (k, _, _) = fit_all(&laid, 1024.0, 1024.0);
        assert_eq!(k, 0.5);
        let (k, tx, ty) = zoom_at((1.0, 0.0, 0.0), 2.0, 100.0, 50.0);
        assert_eq!((k, tx, ty), (2.0, -100.0, -50.0));
    }

    #[test]
    fn a_pinch_zooms_about_the_fingers_and_pans_with_them() {
        let v0 = (1.0, 10.0, 20.0);
        // Spread from 100 px to 200 px about (100, 100): twice, the midpoint held.
        let v = pinch(v0, (50.0, 100.0), (150.0, 100.0), (0.0, 100.0), (200.0, 100.0));
        assert!((v.0 - 2.0).abs() < 1e-9);
        let world = ((100.0 - v0.1) / v0.0, (100.0 - v0.2) / v0.0);
        assert!((world.0 * v.0 + v.1 - 100.0).abs() < 1e-9 && (world.1 * v.0 + v.2 - 100.0).abs() < 1e-9);
        // Two fingers moved together, apart the same: a pan, no zoom.
        let p = pinch(v0, (50.0, 100.0), (150.0, 100.0), (80.0, 140.0), (180.0, 140.0));
        assert_eq!(p, (1.0, 40.0, 60.0));
        // Held between 10% and 300%; fingers on one point do not divide by zero.
        assert_eq!(pinch(v0, (0.0, 0.0), (10.0, 0.0), (0.0, 0.0), (1000.0, 0.0)).0, 3.0);
        assert!(pinch(v0, (0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)).0.is_finite());
    }

    #[test]
    fn a_double_tap_is_two_taps_close_in_time_and_place() {
        assert!(double_tap(Some((1000.0, 50.0, 50.0)), 1200.0, (60.0, 55.0)));
        assert!(!double_tap(Some((1000.0, 50.0, 50.0)), 1400.0, (50.0, 50.0)));
        assert!(!double_tap(Some((1000.0, 50.0, 50.0)), 1100.0, (150.0, 50.0)));
        assert!(!double_tap(None, 1100.0, (50.0, 50.0)));
    }

    /// DEPLOY-39: a stage pill leaves only its tickets; an area with none is no cluster.
    #[test]
    fn a_stage_filters_the_nodes_and_the_clusters() {
        let node = |id: &str, state| {
            let mut n = node_of(&row(id, Status::Complete), &[], "MENGER", &["test".to_string(), "prod".to_string()]).expect("a node");
            n.state = state;
            n
        };
        let nodes = vec![node("APP-1", State::Ready), node("APP-2", State::Waiting), node("DEPLOY-1", State::Waiting)];
        assert_eq!(only(nodes.clone(), None).len(), 3);
        let cs = clusters(only(nodes.clone(), Some(State::Ready)));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].summary().1, "1 ready".to_string());
        assert_eq!(clusters(only(nodes.clone(), Some(State::Waiting))).len(), 2);
        assert!(clusters(only(nodes, Some(State::Failed))).is_empty());
        assert_eq!(State::of_class("ready"), Some(State::Ready));
    }
}
