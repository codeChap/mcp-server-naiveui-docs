mod catalog;
mod clip;
mod names;
mod parse;
mod server;
mod sources;
mod sync;
mod theme;

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

use catalog::Catalog;
use server::NaiveUiServer;
use sources::{cache_dir, resolve_rev};
use sync::ensure_sources;

fn resolved_rev_label() -> String {
    resolve_rev(std::env::var("NAIVE_UI_MCP_REV").ok().as_deref())
        .unwrap_or_else(|| "HEAD".to_string())
}

fn print_counts(catalog: &Catalog, cache: &std::path::Path) {
    eprintln!(
        "naiveui-docs mcp: {} pages ({} components, {} demos, {} docs, {} gotchas) from {} pin {}",
        catalog.pages.len(),
        catalog.component_count(),
        catalog.demo_count(),
        catalog.doc_count(),
        catalog.gotchas_count(),
        cache.display(),
        resolved_rev_label()
    );
}

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
        let force = args.iter().any(|a| a == "--force");
        match ensure_sources(&cache, force) {
            Ok(log) => println!("{log}"),
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
        }
        let catalog = Catalog::load(&cache);
        println!(
            "indexed {} pages, {} demos",
            catalog.pages.len(),
            catalog.demo_count()
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--rebuild") {
        let catalog = Catalog::load(&cache);
        println!(
            "rebuilt {} pages ({} components, {} demos, {} docs, {} gotchas) from {}",
            catalog.pages.len(),
            catalog.component_count(),
            catalog.demo_count(),
            catalog.doc_count(),
            catalog.gotchas_count(),
            cache.display()
        );
        return Ok(());
    }

    if std::env::var("NAIVE_UI_MCP_SYNC_ON_START").ok().as_deref() == Some("1")
        && let Err(e) = ensure_sources(&cache, false)
    {
        eprintln!("naiveui-docs mcp sync on start failed: {e:#}");
    }

    let catalog = Catalog::load(&cache);
    print_counts(&catalog, &cache);

    let service = NaiveUiServer::new(cache, catalog).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
