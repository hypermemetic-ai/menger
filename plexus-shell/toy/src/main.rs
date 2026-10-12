//! `cargo run -p plexus-shell-toy`: the toy until Ctrl-C.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = plexus_shell_toy::start().await?;
    println!("app {} · hub {}", server.shell_url(), server.ws_url());
    tokio::signal::ctrl_c().await?;
    server.shutdown().await;
    Ok(())
}
