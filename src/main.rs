use clap::Parser;
use menger::store::SeedSpec;
use menger::{build_rpc_with, default_db_path, shell};
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
    let seed = args.seed_synthetic.map(|count| SeedSpec {
        count,
        seed: args.seed_value,
        into_existing: args.seed_into_existing,
    });
    let hub = build_rpc_with(&db_path, seed).await?;

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
