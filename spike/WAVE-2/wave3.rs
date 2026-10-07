//! WAVE-3 (spike, child of WAVE-2): what does keeping every ticket's
//! placement (WAVE-6) cost, at 10² … 10⁶ tickets?
//!
//! Built as an example of `menger-tickets-api` twice — once as the store
//! is (`with`: WAVE-6's re-placement pass and its startup placement), once
//! with `--features wave3-no-replace` (`without`: neither) — and run against
//! byte-identical copies of one prepared file per magnitude. It drives the
//! store directly, as the activation does: no transport in the number.
//!
//! ```sh
//! spike/WAVE-2/run.sh 100 10000 1000000      # build both, prepare, measure, print the sheet
//! ```
//!
//! `prepare N FILE` — a store of N tickets: menger's own synthetic seed
//! (`SeedSpec`, PERF-2) for N − the shapes, then the fan-out shapes of
//! `perf/fan-out.py` and `perf/large-parent.py` written into the file beside
//! it (as those scripts write theirs): a long chain (CHN, each blocked by the
//! one before), a wide fan (FNX-2 blocking every other FNX ticket), and a
//! deep diamond (DIA: a root, then layers of `W` each blocked by all of the
//! layer before, then a sink). The derived state is marked behind the
//! tickets, so the next open builds it — once, in `prepare`.
//!
//! `measure FILE` — opens it (timed: startup, the placement pass in it),
//! reads VmRSS/VmHWM, then times writes: `set_status` across the satisfied
//! line on synthetic tickets (each flipped there and back), `create` with
//! up to two blockers among them, and the worst writes on each shape —
//! completing and reopening the chain's head, the fan's blocker, the
//! diamond's root — counting what each `placements_changed` moved. One
//! JSON line out.
//!
//! Beside each write's whole latency (commit and fsync included), the pass
//! itself: the store logs each re-placement and the startup placement on
//! the `wave6` target with their microseconds, and a small subscriber here
//! keeps those fields (the `with` build only — the other has no pass).

use menger_tickets_api::store::{Change, Kind, NewTicket, SeedSpec, Status, Store};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the store said on the `wave6` target: each pass's `(micros, placed,
/// moved)`, and each startup placement's micros.
static PASSES: Mutex<Vec<(u64, u64, u64)>> = Mutex::new(Vec::new());
static PLACE_ALL: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// The fields of one `wave6` event.
#[derive(Default)]
struct Fields {
    micros: Option<u64>,
    placed: Option<u64>,
    moved: Option<u64>,
}

impl tracing::field::Visit for Fields {
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "micros" => self.micros = Some(value),
            "placed" => self.placed = Some(value),
            "moved" => self.moved = Some(value),
            _ => {}
        }
    }
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
}

/// Keeps the `wave6` events' fields; drops everything else.
struct Grab;

impl tracing::Subscriber for Grab {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        m.target() == "wave6"
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut f = Fields::default();
        e.record(&mut f);
        match (f.micros, f.placed, f.moved) {
            (Some(us), Some(placed), Some(moved)) => PASSES.lock().unwrap().push((us, placed, moved)),
            (Some(us), None, None) => PLACE_ALL.lock().unwrap().push(us),
            _ => {}
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// The passes logged since the last call: p50 / p99 / max of their
/// microseconds, and the most any placed and moved.
fn passes() -> serde_json::Value {
    let taken: Vec<(u64, u64, u64)> = std::mem::take(&mut *PASSES.lock().unwrap());
    let mut us: Vec<f64> = taken.iter().map(|p| p.0 as f64).collect();
    serde_json::json!({
        "n": taken.len(),
        "p50_us": quantile(&mut us.clone(), 0.5),
        "p99_us": quantile(&mut us.clone(), 0.99),
        "max_us": quantile(&mut us, 1.0),
        "placed_max": taken.iter().map(|p| p.1).max().unwrap_or(0),
        "moved_max": taken.iter().map(|p| p.2).max().unwrap_or(0),
    })
}

const WITH: bool = !cfg!(feature = "wave3-no-replace");

/// The shapes' sizes at `n` tickets.
struct Shapes {
    chain: usize,
    fan: usize,
    width: usize,
    depth: usize,
}

impl Shapes {
    fn of(n: usize) -> Self {
        // A tenth of the store each for the chain and the fan, capped at
        // fan-out.py's largest (10⁵); a diamond of width 10 and depth up to
        // 1,000 (10⁴ tickets, 10⁵ edges).
        let chain = (n / 10).clamp(3, 100_000);
        let fan = (n / 10).clamp(3, 100_000);
        let width = if n >= 10_000 { 10 } else { 3 };
        let depth = (n / 1_000).clamp(2, 1_000);
        Self { chain, fan, width, depth }
    }

    fn total(&self) -> usize {
        // Each shortcode's scope root too.
        (1 + self.chain) + (1 + 1 + self.fan) + (1 + 1 + self.width * self.depth + 1)
    }
}

fn rss() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = |key: &str| {
        status
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (kb("VmRSS:"), kb("VmHWM:"))
}

fn ids(list: &[String]) -> String {
    serde_json::to_string(list).unwrap()
}

/// The shapes, written into the file beside the seed as the perf scripts
/// write theirs: rows only, `unlocks` mirrored, nothing derived.
fn write_shapes(path: &Path, s: &Shapes) {
    let mut conn = rusqlite::Connection::open(path).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO tickets (id, epic, seq, title, status, kind, severity, blocked_by, unlocks,
                     parent, body, created_unix, updated_unix, commits, child_count, unsatisfied)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, '', 1750000000, 1750000000, '[]', 0, 0)",
            )
            .unwrap();
        let mut row = |epic: &str, seq: usize, status: &str, kind: &str, blocked_by: &[String], unlocks: &[String]| {
            let parent = (seq > 1).then(|| format!("{epic}-1"));
            insert
                .execute(rusqlite::params![
                    format!("{epic}-{seq}"),
                    epic,
                    seq as i64,
                    format!("{epic} {seq}"),
                    status,
                    kind,
                    ids(blocked_by),
                    ids(unlocks),
                    parent,
                ])
                .unwrap();
        };
        let id = |epic: &str, seq: usize| format!("{epic}-{seq}");
        // The chain: CHN-2 ← CHN-3 ← … each waits on the one before.
        row("CHN", 1, "Epic", "scope", &[], &[]);
        for i in 0..s.chain {
            let seq = i + 2;
            let blocked = if i == 0 { vec![] } else { vec![id("CHN", seq - 1)] };
            let unlocks = if i + 1 < s.chain { vec![id("CHN", seq + 1)] } else { vec![] };
            row("CHN", seq, if i == 0 { "Ready" } else { "Pending" }, "build", &blocked, &unlocks);
        }
        // The fan: FNX-2 blocks every other FNX ticket, each Ready. (Not FAN: a
        // seeded shortcode alternates consonant and vowel and could be one.)
        row("FNX", 1, "Epic", "scope", &[], &[]);
        let fanned: Vec<String> = (0..s.fan).map(|i| id("FNX", i + 3)).collect();
        row("FNX", 2, "Ready", "build", &[], &fanned);
        for i in 0..s.fan {
            row("FNX", i + 3, "Ready", "build", &[id("FNX", 2)], &[]);
        }
        // The diamond: DIA-2 the root, `depth` layers of `width`, each node
        // blocked by every node of the layer before; then a sink.
        row("DIA", 1, "Epic", "scope", &[], &[]);
        let layer = |l: usize| -> Vec<String> { (0..s.width).map(|j| id("DIA", 3 + l * s.width + j)).collect() };
        let sink = 3 + s.depth * s.width;
        row("DIA", 2, "Ready", "build", &[], &layer(0));
        for l in 0..s.depth {
            let blocked = if l == 0 { vec![id("DIA", 2)] } else { layer(l - 1) };
            let unlocks = if l + 1 < s.depth { layer(l + 1) } else { vec![id("DIA", sink)] };
            for j in 0..s.width {
                row("DIA", 3 + l * s.width + j, "Pending", "build", &blocked, &unlocks);
            }
        }
        row("DIA", sink, "Pending", "build", &layer(s.depth - 1), &[]);
    }
    // The derived state is behind the tickets now: the next open builds it.
    tx.execute("UPDATE derived_state SET stale = 1", []).unwrap();
    tx.commit().unwrap();
}

async fn prepare(n: usize, path: &Path) {
    let shapes = Shapes::of(n);
    let base = n.saturating_sub(shapes.total()).max(10);
    {
        let store = Store::open(path).unwrap();
        let report = store.seed_synthetic(SeedSpec { count: base, seed: 1, into_existing: false }).unwrap();
        eprintln!("seeded {} synthetic tickets in {:?}", report.count, report.elapsed);
    }
    write_shapes(path, &shapes);
    let started = Instant::now();
    let store = Store::open(path).unwrap();
    eprintln!(
        "built the derived state over {} tickets in {:?} (chain {}, fan {}, diamond {}x{})",
        store.count(),
        started.elapsed(),
        shapes.chain,
        shapes.fan,
        shapes.width,
        shapes.depth
    );
    drop(store);
    // Everything in the one file, so a copy of it is the whole store.
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if v.is_empty() {
        return 0.0;
    }
    let i = ((v.len() as f64 - 1.0) * q).round() as usize;
    v[i]
}

/// How many placements the write just made moved — `placements_changed`'s
/// changes, read off `watch` (everything the write said is in the channel
/// before it is acknowledged).
fn moved(rx: &mut tokio::sync::broadcast::Receiver<Change>) -> usize {
    let mut n = 0;
    while let Ok(c) = rx.try_recv() {
        if let Change::PlacementsChanged { changes } = c {
            n += changes.len();
        }
    }
    n
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

async fn measure(path: &Path, samples: usize) {
    let (rss0, _) = rss();
    let started = Instant::now();
    let store = Store::open(path).unwrap();
    let open = started.elapsed();
    let (rss1, hwm1) = rss();
    let place_all_ms = PLACE_ALL.lock().unwrap().last().map(|us| *us as f64 / 1000.0);
    passes();
    let n = store.count();
    let shapes = Shapes::of(n);

    // Synthetic tickets to write to: every epic but the shapes', each a
    // non-root, non-doc ticket in the ready lifecycle.
    let epics: Vec<(String, u32)> = store
        .extents()
        .into_iter()
        .filter(|(e, _)| !["CHN", "FNX", "DIA"].contains(&e.as_str()))
        .collect();
    let mut rng = Rng(42);
    let mut rx = store.subscribe();

    // set_status across the satisfied line, there and back: two samples a
    // ticket. The ticket's own status decides the direction.
    let mut status_ms = Vec::new();
    let mut status_moved = 0usize;
    let mut status_max = 0usize;
    for _ in 0..samples / 2 {
        let (epic, last) = &epics[rng.below(epics.len())];
        let id = format!("{epic}-{}", 2 + rng.below((*last as usize).saturating_sub(1).max(1)));
        let Ok(t) = store.get(&id).await else { continue };
        if !t.kind.has_lifecycle() {
            continue;
        }
        let was = t.status;
        let flip = if matches!(was, Status::Complete | Status::Superseded) { Status::Pending } else { Status::Complete };
        for to in [flip, was] {
            let t0 = Instant::now();
            store.set_status(&id, to).await.unwrap();
            status_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            let m = moved(&mut rx);
            status_moved += m;
            status_max = status_max.max(m);
        }
    }
    let status_pass = passes();

    // create: a build in a synthetic epic, blocked by up to two tickets
    // among the last forty of it (the seed's own shape).
    let mut create_ms = Vec::new();
    for _ in 0..samples {
        let (epic, last) = &epics[rng.below(epics.len())];
        let mut blocked_by = Vec::new();
        for _ in 0..rng.below(3) {
            let seq = (*last as usize).saturating_sub(rng.below(40)).max(2);
            blocked_by.push(format!("{epic}-{seq}"));
        }
        blocked_by.dedup();
        let t0 = Instant::now();
        let r = store
            .create(NewTicket {
                epic: epic.clone(),
                title: "wave3".into(),
                kind: Kind::Build,
                severity: None,
                blocked_by,
                parent: None,
                path: None,
                body: String::new(),
            })
            .await;
        if r.is_ok() {
            create_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        moved(&mut rx);
    }
    let create_pass = passes();

    // The worst writes: each shape's head completed, then reopened.
    let mut worst = Vec::new();
    for (shape, id) in [("chain", "CHN-2"), ("fan", "FNX-2"), ("diamond", "DIA-2")] {
        for (to, back) in [(Status::Complete, "complete"), (Status::Ready, "reopen")] {
            let mut ms = Vec::new();
            let mut pass_us = Vec::new();
            let mut m = 0;
            for rep in 0..3 {
                passes();
                let t0 = Instant::now();
                store.set_status(id, to).await.unwrap();
                ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                m = moved(&mut rx);
                if let Some(us) = passes()["max_us"].as_f64().filter(|_| WITH) {
                    pass_us.push(us);
                }
                // Back the other way, untimed, to repeat the same write.
                if rep < 2 {
                    let undo = if to == Status::Complete { Status::Ready } else { Status::Complete };
                    store.set_status(id, undo).await.unwrap();
                    moved(&mut rx);
                }
            }
            worst.push(serde_json::json!({
                "shape": shape, "write": back, "median_ms": quantile(&mut ms, 0.5), "max_ms": quantile(&mut ms, 1.0), "moved": m,
                "pass_median_us": quantile(&mut pass_us, 0.5)
            }));
        }
    }
    let (rss2, hwm2) = rss();
    let line = serde_json::json!({
        "variant": if WITH { "with" } else { "without" },
        "tickets": n,
        "shapes": {"chain": shapes.chain, "fan": shapes.fan, "diamond": [shapes.width, shapes.depth]},
        "open_ms": open.as_secs_f64() * 1000.0,
        "place_all_ms": place_all_ms,
        "rss_kb": {"before_open": rss0, "after_open": rss1, "hwm_after_open": hwm1, "end": rss2, "hwm_end": hwm2},
        "set_status": {"n": status_ms.len(), "p50_ms": quantile(&mut status_ms.clone(), 0.5), "p99_ms": quantile(&mut status_ms, 0.99),
                       "moved_total": status_moved, "moved_max": status_max, "pass": status_pass},
        "create": {"n": create_ms.len(), "p50_ms": quantile(&mut create_ms.clone(), 0.5), "p99_ms": quantile(&mut create_ms, 0.99),
                   "pass": create_pass},
        "worst": worst,
    });
    println!("{line}");
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing::subscriber::set_global_default(Grab).expect("the one subscriber");
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("prepare") => prepare(args[2].parse().unwrap(), Path::new(&args[3])).await,
        Some("measure") => {
            let samples = args.get(3).map_or(400, |s| s.parse().unwrap());
            measure(Path::new(&args[2]), samples).await
        }
        Some("open") => {
            // Startup alone, a few times: the median open.
            let mut ms = Vec::new();
            for _ in 0..args.get(3).map_or(3, |s| s.parse().unwrap()) {
                let t0 = Instant::now();
                let store = Store::open(Path::new(&args[2])).unwrap();
                ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                drop(store);
                std::thread::sleep(Duration::from_millis(200));
            }
            let mut place_all: Vec<f64> = PLACE_ALL.lock().unwrap().iter().map(|us| *us as f64 / 1000.0).collect();
            println!(
                "{}",
                serde_json::json!({
                    "variant": if WITH { "with" } else { "without" },
                    "open_ms": ms.clone(), "median_ms": quantile(&mut ms, 0.5),
                    "place_all_ms": place_all.clone(), "place_all_median_ms": quantile(&mut place_all, 0.5),
                })
            );
        }
        _ => eprintln!("usage: wave3 prepare N FILE | measure FILE [SAMPLES] | open FILE [RUNS]"),
    }
}
