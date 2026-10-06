use super::{Kind, SeedSpec, Severity, Status, Ticket};
use std::collections::{HashMap, HashSet};

/// Rows per write transaction: a few fsyncs for a million rows, and a
/// WAL that gets checkpointed between chunks instead of growing to
/// the size of the whole seed.
pub(super) const ROWS_PER_TRANSACTION: usize = 50_000;

/// SplitMix64 — tiny, well-distributed, and dependency-free; the
/// point is reproducibility, not cryptography.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        (self.next_u64() % n as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
    fn weighted<T: Copy>(&mut self, table: &[(T, u32)]) -> T {
        let total: u32 = table.iter().map(|(_, w)| w).sum();
        let mut roll = self.below(total as usize) as u32;
        for (v, w) in table {
            if roll < *w {
                return *v;
            }
            roll -= w;
        }
        table[table.len() - 1].0
    }
}

// A realistic mix (weights are percentages of non-root tickets).
const STATUSES: [(Status, u32); 7] = [
    (Status::Pending, 28),
    (Status::Ready, 18),
    (Status::Blocked, 8),
    (Status::Complete, 34),
    (Status::Idea, 6),
    (Status::Superseded, 4),
    (Status::Epic, 2),
];
const KINDS: [(Kind, u32); 5] = [
    (Kind::Scope, 4),
    (Kind::Execution, 14),
    (Kind::Build, 52),
    (Kind::Gap, 16),
    (Kind::Spike, 14),
];
const SEVERITIES: [(Option<Severity>, u32); 5] = [
    (None, 40),
    (Some(Severity::Medium), 24),
    (Some(Severity::High), 16),
    (Some(Severity::Low), 14),
    (Some(Severity::Critical), 6),
];

const VERBS: [&str; 20] = [
    "Migrate", "Harden", "Wire", "Measure", "Split", "Cache", "Index", "Paginate",
    "Stream", "Validate", "Restyle", "Document", "Refuse", "Retry", "Bound", "Persist",
    "Render", "Resolve", "Throttle", "Audit",
];
const OBJECTS: [&str; 20] = [
    "the watch stream", "id allocation", "the nav tree", "the board render",
    "startup load", "the REST bridge", "cycle detection", "the ticket page",
    "unlocks symmetry", "the seed path", "SSE reconnects", "the list payload",
    "the status control", "epic rollups", "the proxy preamble", "frontmatter folding",
    "diagram mounting", "the write-serial lock", "WAL checkpoints", "the health probe",
];
const QUALIFIERS: [&str; 8] = [
    "under load", "at scale", "for the shell", "before serving",
    "on reconnect", "per epic", "without fsync", "in one transaction",
];
const PROBLEMS: [&str; 6] = [
    "Today this path does the work one row at a time, which is fine at ten and hopeless at a million.",
    "The current behavior is correct but unobservable: nothing logs how long it took or how much it moved.",
    "A client that opens this before the snapshot lands can miss an event; the gap is small but real.",
    "The shape is right and the cost is wrong — every call re-derives what the previous call already knew.",
    "Two readers disagree about the order, and the tie-break lives in a comment nobody reads.",
    "The failure mode is a silent hang rather than a clear refusal, so the operator learns nothing.",
];
const BEHAVIORS: [&str; 8] = [
    "One call, one transaction, one log line with count and elapsed time.",
    "Reads stay on the cache; nothing on this path touches storage.",
    "A refusal names the offending row and leaves the file untouched.",
    "The same input twice produces byte-identical output.",
    "Timeouts are recorded as failures, never as hangs.",
    "The wire surface does not grow; no new method, no new field.",
    "Every edge added is mirrored on the other side before the commit.",
    "The startup log reports what was loaded and how long it took.",
];
const CRITERIA: [&str; 6] = [
    "`cargo test` passes with the new cases included.",
    "A sampled check agrees with the query's answer on a seeded store.",
    "The log line carries the count and the elapsed milliseconds.",
    "A second open of the same file finds the same rows.",
    "The measurement completes inside its budget at the target magnitude.",
    "The operator can reproduce the run from the recorded seed value.",
];

/// Number of epics for `count` tickets: a few for a hundred, a couple
/// of hundred for a million — a Zipf-weighted spread below, so the
/// biggest epic is a real stress for "expand the largest group".
fn epic_count(count: usize) -> usize {
    let k = (count as f64).powf(0.4).ceil() as usize;
    k.clamp(3, 300).min(count.max(1))
}

/// Pronounceable 3–4 letter uppercase codes (consonant/vowel
/// alternation), unique within the run.
fn epic_code(rng: &mut Rng, taken: &HashSet<String>) -> String {
    const C: &[u8] = b"BCDFGHKLMNPRSTVZ";
    const V: &[u8] = b"AEIOU";
    loop {
        let len = 3 + rng.below(2);
        let code: String = (0..len)
            .map(|i| {
                let set = if i % 2 == 0 { C } else { V };
                set[rng.below(set.len())] as char
            })
            .collect();
        if !taken.contains(&code) {
            return code;
        }
    }
}

fn title(rng: &mut Rng) -> String {
    let mut t = format!("{} {}", rng.pick(&VERBS), rng.pick(&OBJECTS));
    if rng.chance(40) {
        t.push(' ');
        t.push_str(rng.pick(&QUALIFIERS));
    }
    t
}

/// A short markdown body in the house shape (Problem / Required
/// behavior / Acceptance criteria), ~400–600 bytes.
fn body(rng: &mut Rng, id: &str, blocked_by: &[String]) -> String {
    let mut b = String::with_capacity(640);
    b.push_str("## Problem\n\n");
    b.push_str(rng.pick(&PROBLEMS));
    if let Some(first) = blocked_by.first() {
        b.push_str(&format!(" Blocked on {first} until it lands."));
    }
    b.push_str("\n\n## Required behavior\n\n");
    let n = 2 + rng.below(2);
    for _ in 0..n {
        b.push_str("- ");
        b.push_str(rng.pick(&BEHAVIORS));
        b.push('\n');
    }
    b.push_str("\n## Acceptance criteria\n\n");
    let n = 2 + rng.below(2);
    for i in 0..n {
        b.push_str(&format!("{}. {}\n", i + 1, rng.pick(&CRITERIA)));
    }
    b.push_str(&format!("\n_{id}: synthetic (PERF-2)._\n"));
    b
}

/// Generates `spec.count` tickets in creation order. `next_seq` is the
/// per-epic maximum sequence already in the store (empty for a fresh
/// one): a generated epic that collides with an existing one
/// continues its numbering, as `create` would.
pub(super) fn generate(spec: SeedSpec, existing_max: &HashMap<String, u32>) -> Vec<Ticket> {
    let mut rng = Rng(spec.seed ^ 0x5EED_0000_0000_0000);
    let count = spec.count;
    let mut out: Vec<Ticket> = Vec::with_capacity(count);
    if count == 0 {
        return out;
    }

    // Epics: Zipf-weighted (the biggest holds ~1/H(K) of everything).
    let k = epic_count(count);
    let mut codes: Vec<String> = Vec::with_capacity(k);
    let mut taken: HashSet<String> = HashSet::new();
    for _ in 0..k {
        let c = epic_code(&mut rng, &taken);
        taken.insert(c.clone());
        codes.push(c);
    }
    let mut cumulative: Vec<f64> = Vec::with_capacity(k);
    let mut acc = 0.0;
    for i in 0..k {
        acc += 1.0 / (i as f64 + 1.0);
        cumulative.push(acc);
    }
    let pick_epic = |rng: &mut Rng| -> usize {
        let r = rng.unit() * acc;
        cumulative.partition_point(|c| *c <= r).min(k - 1)
    };

    let mut seq: Vec<u32> = codes
        .iter()
        .map(|c| existing_max.get(c).copied().unwrap_or(0))
        .collect();
    // Per-epic creation-order index lists (for local blocker choice).
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); k];
    let mut index: HashMap<String, usize> = HashMap::with_capacity(count);

    const BASE_UNIX: i64 = 1_750_000_000; // 2025-06-15T13:20Z
    const STEP: i64 = 60;

    for i in 0..count {
        let e = pick_epic(&mut rng);
        seq[e] += 1;
        let epic = codes[e].clone();
        let id = format!("{epic}-{}", seq[e]);
        let root = members[e].is_empty();

        let (kind, status, severity, title_s) = if root {
            (
                Kind::Scope,
                Status::Epic,
                None,
                format!("{epic}: {}", title(&mut rng).to_lowercase()),
            )
        } else {
            (
                rng.weighted(&KINDS),
                rng.weighted(&STATUSES),
                rng.weighted(&SEVERITIES),
                title(&mut rng),
            )
        };

        // A modest fraction carries blockers: mostly recent siblings
        // in the same epic, sometimes anything created earlier
        // anywhere. Always earlier in creation order — acyclic.
        let mut blocked_by: Vec<String> = Vec::new();
        if !root && rng.chance(25) {
            let want = 1 + usize::from(rng.chance(30)) + usize::from(rng.chance(10));
            for _ in 0..want {
                let candidate = if rng.chance(90) {
                    let mine = &members[e];
                    let window = mine.len().min(40);
                    mine.get(mine.len() - window + rng.below(window)).copied()
                } else if i > 0 {
                    Some(rng.below(i))
                } else {
                    None
                };
                if let Some(c) = candidate {
                    let bid = out[c].id.clone();
                    if !blocked_by.contains(&bid) {
                        blocked_by.push(bid);
                    }
                }
            }
        }

        let created = BASE_UNIX + i as i64 * STEP;
        let updated = if status == Status::Pending {
            created
        } else {
            created + rng.below(30 * 86_400) as i64
        };
        let body_s = body(&mut rng, &id, &blocked_by);

        for bid in &blocked_by {
            let bi = index[bid];
            out[bi].unlocks.push(id.clone());
            out[bi].updated_unix = out[bi].updated_unix.max(created);
        }
        // MNG-14: the shape `create` would give it — every non-root
        // hangs under its epic's scope root. SCALE-4: the root's row is
        // not touched by the birth (the cache counts the children when the
        // seed lands), as `create` leaves a parent's row alone.
        let parent = if root {
            None
        } else {
            Some(out[members[e][0]].id.clone())
        };
        members[e].push(i);
        index.insert(id.clone(), i);
        out.push(Ticket {
            id,
            epic,
            seq: seq[e],
            title: title_s,
            status,
            kind,
            severity,
            blocked_by,
            unlocks: Vec::new(),
            parent,
            child_count: 0,
            // SDLC-9: none — a commit is linked after birth, as `create`
            // leaves it; the generator stays byte-identical per seed value.
            commits: Vec::new(),
            // WAVE-10: no address — a seed makes work, not menger's documents.
            path: None,
            body: body_s,
            created_unix: created,
            updated_unix: updated,
        });
    }
    out
}
