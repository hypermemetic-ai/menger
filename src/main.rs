use clap::Parser;
use menger::store::{SeedShape, SeedSpec, Store, Verdict};
use menger::{build_rpc_seeded, default_db_path, shell};
use plexus_core::plexus::DynamicHub;
use plexus_transport::{RestHttpConfig, RouteFn, TransportServer};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "menger")]
#[command(about = "A plan is a program whose leaves are prose — ticketing on Plexus RPC")]
struct Args {
    /// WebSocket port (4440-4459 is the synapse scan range)
    #[arg(short, long, default_value = "4455")]
    port: u16,

    /// Web shell port
    #[arg(long, default_value = "4456")]
    shell_port: u16,

    /// REST HTTP port (POST /rest/{namespace}/{method}; streaming methods
    /// respond as SSE)
    #[arg(long, default_value = "4457")]
    rest_port: u16,

    /// Disable the REST HTTP listener
    #[arg(long)]
    no_rest: bool,

    /// Directory holding the built Leptos app (frontend/dist), served at
    /// /app/ on the shell port. Missing output degrades to a "frontend
    /// not built" page, never an error.
    #[arg(long, env = "MENGER_APP_DIST", value_name = "DIR")]
    app_dist: Option<PathBuf>,

    /// MFE-23: the environment's name (`prod`, `test`, …); the deploy env
    /// files set `MENGER_ENV`. `test` titles the browser tab `menger.test`,
    /// anything else (or none) `menger`.
    #[arg(long, env = "MENGER_ENV", value_name = "NAME")]
    env: Option<String>,

    /// Run in stdio mode (MCP-compatible); implies no web shell
    #[arg(long)]
    stdio: bool,

    /// Disable the web shell
    #[arg(long)]
    no_shell: bool,

    /// Database file (the system of record). Defaults to a per-user data
    /// directory; a fresh location starts empty.
    #[arg(long, env = "MENGER_DB_PATH", value_name = "PATH")]
    db_path: Option<PathBuf>,

    /// PERF-2: seed N synthetic tickets into the database before serving
    /// (bulk — a few transactions, no per-row fsync; deterministic per
    /// --seed-value). Refuses a database that already holds tickets
    /// unless --seed-into-existing.
    #[arg(long, env = "MENGER_SEED_SYNTHETIC", value_name = "N")]
    seed_synthetic: Option<usize>,

    /// RNG seed for --seed-synthetic: the same value reproduces the same
    /// tickets.
    #[arg(long, env = "MENGER_SEED_VALUE", default_value = "1", value_name = "U64")]
    seed_value: u64,

    /// FILTER-13: where --seed-synthetic hangs its tickets: `flat` (every
    /// ticket under its shortcode's root, PERF-2's) or `nested` (FILTER-3
    /// §6.5's tree: executions under the root, builds under executions,
    /// docs under builds).
    #[arg(long, env = "MENGER_SEED_SHAPE", default_value = "flat", value_name = "SHAPE")]
    seed_shape: SeedShape,

    /// Let --seed-synthetic add to a database that already holds tickets
    /// (ids continue after the existing ones). As an env var, 1/true/yes
    /// enable it.
    #[arg(
        long,
        env = "MENGER_SEED_INTO_EXISTING",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    seed_into_existing: bool,

    /// SCALE-28: check the database (--db-path, or the default) instead of
    /// serving it: rebuild its derived state — shortcode runs, dangling
    /// forward references, roll-ups, children and child_count, and
    /// (SCALE-32) each ticket's unsatisfied blockers, the ready set and
    /// every shortcode's status counts, and (FILTER-4) the cell table with
    /// every shortcode's bitmap and count of each cell, and (FILTER-13) the
    /// kept tree's tallies and holders — by a scan
    /// of its tickets, compare with what it stores, print each difference
    /// or "consistent", and exit 0 when consistent, 1 on any difference
    /// (or none stored), 2 when it cannot be read. Read-only: changes
    /// nothing, migrates nothing, serves nothing.
    #[arg(long)]
    verify_derived: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = if args.stdio {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,menger=debug".into())
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let db_path = args.db_path.unwrap_or_else(default_db_path);
    if args.verify_derived {
        std::process::exit(verify_derived(db_path).await);
    }
    let seed = args.seed_synthetic.map(|count| SeedSpec {
        count,
        seed: args.seed_value,
        into_existing: args.seed_into_existing,
    });
    let hub = build_rpc_seeded(&db_path, seed, args.seed_shape).await?;

    let activations = hub.list_activations_info();
    tracing::info!("Activations ({}):", activations.len());
    for a in &activations {
        tracing::info!("  {} v{} - {}", a.namespace, a.version, a.description);
        for method in &a.methods {
            tracing::info!("    - {}.{}", a.namespace, method);
        }
    }

    let rpc_converter = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("rpc module build failed: {e}"))
    };

    let mut builder = TransportServer::builder(hub.clone(), rpc_converter);
    if args.stdio {
        builder = builder.with_stdio();
        tracing::info!("Starting stdio transport");
    } else {
        builder = builder.with_websocket(args.port);
        tracing::info!("WebSocket: ws://127.0.0.1:{}", args.port);
        if !args.no_rest {
            let route_hub = hub.clone();
            let route_fn: RouteFn = Arc::new(move |method, params| {
                let hub = route_hub.clone();
                Box::pin(async move { hub.route(&method, params, None).await })
            });
            builder = builder
                .with_rest_http_config(
                    RestHttpConfig::new(args.rest_port)
                        .with_server_name("menger".into())
                        .with_server_version(env!("CARGO_PKG_VERSION").into()),
                )
                .with_flat_schemas(hub.list_plugin_schemas()) // registers /rest/tickets/*
                .with_route_fn(route_fn); // dispatches via hub.route()
            tracing::info!("REST:      http://127.0.0.1:{}/rest", args.rest_port);
        }
        if !args.no_shell {
            let cfg = shell::ShellConfig {
                port: args.shell_port,
                ws_port: args.port,
                // MFE-2: /rest on the shell port reverse-proxies to the
                // REST port (same-origin for the browser app; the REST
                // transport emits no CORS headers).
                rest_port: (!args.no_rest).then_some(args.rest_port),
                app_dist: args.app_dist.unwrap_or_else(shell::default_app_dist),
                env: args.env.clone(),
            };
            tokio::spawn(async move {
                if let Err(e) = shell::serve(cfg).await {
                    tracing::error!("shell server exited: {e}");
                }
            });
        }
    }

    builder.build().await?.serve().await
}

/// SCALE-28: `--verify-derived` — the verdict on stdout, one difference a
/// line, and the exit code it means. Run off the async runtime: it reads
/// and scans the whole file.
async fn verify_derived(db_path: PathBuf) -> i32 {
    let path = db_path.clone();
    let verdict = tokio::task::spawn_blocking(move || Store::verify_derived(&path))
        .await
        .map_err(anyhow::Error::from)
        .and_then(|v| v);
    match verdict {
        Ok(Verdict::Consistent {
            tickets,
            runs,
            refs,
            groups,
            parents,
            blocked,
            ready,
            cells,
            bitmaps,
            tallies,
            holders,
        }) => {
            println!(
                "consistent: {} — {tickets} tickets; {runs} shortcode runs, {refs} dangling forward \
                 references, {groups} roll-up groups, the children and child_count of {parents} \
                 parents, the unsatisfied blockers of {blocked} tickets, a ready set of {ready}, \
                 every shortcode's status counts, {cells} cells' {bitmaps} bitmaps and counts, \
                 and the kept tree's {tallies} tally entries and {holders} holder containers \
                 are what a scan of the tickets gives",
                db_path.display()
            );
            0
        }
        Ok(Verdict::Differs(differences)) => {
            for d in &differences {
                println!("{d}");
            }
            println!("{}: {} difference(s) between the stored derived state and a scan", db_path.display(), differences.len());
            1
        }
        Ok(Verdict::NotStored(why)) => {
            println!("{}: {why}", db_path.display());
            1
        }
        Err(e) => {
            eprintln!("cannot verify {}: {e:#}", db_path.display());
            2
        }
    }
}
