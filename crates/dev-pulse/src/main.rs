//! `dev-pulse` — top-level binary. Stage 1 wires
//! `starter_observability::tracing::init` and a clap skeleton so the
//! later phases can hang `migrate`, `serve`, `fetch-now`, `backfill`,
//! and `claim` off it. No subcommand implementations live here yet.

use anyhow::Result;
use clap::Command;
use starter_observability::tracing::Format;

#[tokio::main]
async fn main() -> Result<()> {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let _tracing = starter_observability::tracing::init(&filter, Format::Pretty)
        .map_err(|e| anyhow::anyhow!("init tracing: {e}"))?;

    let app = Command::new("dev-pulse")
        .about("dev-pulse — GitHub reporting and insights across multiple orgs.")
        .arg_required_else_help(true);

    let _matches = app.get_matches();

    tracing::info!("dev-pulse scaffold: no subcommands wired yet (stage 1)");
    Ok(())
}
