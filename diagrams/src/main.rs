use clap::Parser;
use menger_diagrams::node::{Config, Driver};
use menger_diagrams::{build_hub, DEFAULT_PORT};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "menger-diagrams")]
#[command(
    about = "The diagrams service — mermaid parsed by the bundle the app serves, under Node, on Plexus RPC"
)]
struct Args {
    /// WebSocket port (4440-4459 is the synapse scan range)
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Run in stdio mode (MCP-compatible) instead of on the WebSocket
    #[arg(long)]
    stdio: bool,

    /// The mermaid bundle the Node program loads — the file the app
    /// serves. Defaults to this checkout's
    /// frontend/vendor/mermaid/mermaid-11.17.2.min.js.
    #[arg(long, env = "MENGER_DIAGRAMS_BUNDLE", value_name = "PATH")]
    bundle: Option<PathBuf>,

    /// The Node.js binary; a bare name is looked up on PATH.
    #[arg(
        long,
        env = "MENGER_DIAGRAMS_NODE",
        default_value = "node",
        value_name = "PATH"
    )]
    node: PathBuf,

    /// The directory holding the Node program (mermaid-parse.js) and its
    /// node_modules — `npm ci --prefix <DIR>` installs them. Defaults to
    /// this checkout's diagrams/node.
    #[arg(long, env = "MENGER_DIAGRAMS_NODE_DIR", value_name = "DIR")]
    node_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Node's own stderr is logged at debug under `menger_diagrams::node`.
    let filter = if args.stdio {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let config = Config::new(
        args.node,
        args.node_dir.unwrap_or_else(Config::default_node_dir),
        args.bundle.unwrap_or_else(Config::default_bundle),
    );
    tracing::info!(
        "node:    {} (started on the first call)",
        config.node.display()
    );
    tracing::info!("program: {}", config.program.display());
    tracing::info!("bundle:  {}", config.bundle.display());
    // Missing pieces never stop the service (every call names them in its
    // answer); saying so here too spares a reader the first call.
    for (what, path) in [("program", &config.program), ("bundle", &config.bundle)] {
        if !path.is_file() {
            tracing::warn!(
                "the {what} is not at {} — every call will answer an error until it is",
                path.display()
            );
        }
    }

    let driver = Driver::new(config);
    let hub = build_hub(driver.clone());
    for a in hub.list_activations_info() {
        tracing::info!("  {} v{} - {}", a.namespace, a.version, a.description);
        for method in &a.methods {
            tracing::info!("    - {}.{}", a.namespace, method);
        }
    }

    let rpc_converter = |arc: Arc<DynamicHub>| {
        DynamicHub::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("rpc module build failed: {e}"))
    };
    let mut builder = TransportServer::builder(hub, rpc_converter);
    if args.stdio {
        builder = builder.with_stdio();
        tracing::info!("Starting stdio transport");
    } else {
        builder = builder.with_websocket(args.port);
        tracing::info!("WebSocket: ws://127.0.0.1:{}", args.port);
    }
    let served = builder.build().await?.serve().await;

    // The transport has stopped (its shutdown signal, or stdio's end): the
    // Node program goes with it. A service killed outright never gets here
    // — the program sees its stdin close and exits on its own.
    driver.shutdown().await;
    served
}
