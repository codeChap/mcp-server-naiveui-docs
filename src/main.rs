mod names;
mod parse;
mod server;
mod sources;

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

use server::NaiveUiServer;
use sources::{NAIVE_UI_PINNED_REV, cache_dir};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cache = match cache_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--sync") {
        eprintln!("not implemented");
        return Ok(());
    }
    if args.iter().any(|a| a == "--rebuild") {
        eprintln!("not implemented");
        return Ok(());
    }

    eprintln!(
        "naive-ui mcp: 0 pages from {} pin {}",
        cache.display(),
        NAIVE_UI_PINNED_REV
    );

    let service = NaiveUiServer::new(cache).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
