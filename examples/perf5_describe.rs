//! PERF-5 (7): where `_info` / `{ns}.hash` / `{ns}.schema` spend their
//! 150–520 ms (POLYGLOT-13), in process, on menger's own service over a
//! fresh database: each step timed apart (the caller's view, each
//! activation's `plugin_schema`, the whole hub's hash, the routed answer),
//! then 30 views sampled into a flamegraph.
//!
//!   cargo run --release --example perf5_describe -- /tmp/menger-PERF-5-describe.db spike/PERF-5/describe

use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use menger_tickets_api::access::Access;
use plexus_core::plexus::{Activation, AuthContext};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn med(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = std::path::PathBuf::from(args.next().unwrap_or("/tmp/menger-PERF-5-describe.db".into()));
    let out = args.next().unwrap_or("spike/PERF-5/describe".into());
    let owner = format!("key:{}", "ab".repeat(32));
    let sessions = Arc::new(menger::sessions::Sessions::new(3600, &menger::login_names()));
    let table = menger::session_table(&sessions);
    let access = Access::with_login(table.clone(), vec![owner.clone()]).minting(table);
    let hub = menger::build_rpc(&db, access).await?;
    let admin = AuthContext::for_principal(&owner.parse().unwrap(), "s1", vec![], serde_json::json!({}));
    let n = 20;
    let time = |f: &dyn Fn()| -> f64 { med((0..n).map(|_| { let t = Instant::now(); f(); ms(t) }).collect()) };
    let mut rows = Vec::new();
    rows.push(("plugin_schema of the hub itself", time(&|| { std::hint::black_box(Activation::plugin_schema(&*hub)); })));
    rows.push(("compute_hash (the whole hub)", time(&|| { std::hint::black_box(hub.compute_hash()); })));
    rows.push(("view(admin) — the caller's view", time(&|| { std::hint::black_box(hub.view(Some(&admin))); })));
    rows.push(("view(anonymous)", time(&|| { std::hint::black_box(hub.view(None)); })));
    rows.push(("info(admin) — _info's content", time(&|| { std::hint::black_box(hub.info(Some(&admin))); })));
    let route = |m: &'static str| {
        let hub = hub.clone();
        let admin = admin.clone();
        async move {
            let mut v = Vec::new();
            for _ in 0..n {
                let t = Instant::now();
                let mut s = hub.route(m, serde_json::json!({}), Some(&admin)).await.unwrap();
                while let Some(i) = s.next().await {
                    std::hint::black_box(i);
                }
                v.push(ms(t));
            }
            med(v)
        }
    };
    let hash_ns: &'static str = Box::leak(format!("{}.hash", hub.runtime_namespace()).into_boxed_str());
    rows.push(("route menger.hash (admin), drained", route(hash_ns).await));
    rows.push(("route tickets.schema (admin), drained", route("tickets.schema").await));
    rows.push(("route tickets.health (admin), drained — the baseline", route("tickets.health").await));
    for (k, v) in &rows {
        println!("{}", serde_json::json!({"step": k, "median_ms": v, "n": n}));
    }
    std::fs::create_dir_all(std::path::Path::new(&out).parent().unwrap())?;
    let guard = pprof::ProfilerGuardBuilder::default().frequency(1999).build()?;
    for _ in 0..30 {
        std::hint::black_box(hub.view(Some(&admin)));
        std::hint::black_box(hub.compute_hash());
    }
    let report = guard.report().build()?;
    let mut opts = pprof::flamegraph::Options::default();
    opts.title = "describe: view(admin) + compute_hash, ×30".into();
    report.flamegraph_with_options(std::fs::File::create(format!("{out}.svg"))?, &mut opts)?;
    let mut folded = String::new();
    for (frames, count) in report.data.iter() {
        let names: Vec<String> = frames.frames.iter().rev().flat_map(|f| f.iter().rev().map(|s| s.name().replace(';', ":"))).collect();
        folded.push_str(&format!("{} {}\n", names.join(";"), count));
    }
    std::fs::write(format!("{out}.folded"), folded)?;
    Ok(())
}
