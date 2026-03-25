mod args;
mod backfill;

use anyhow::{Context, Result};
use args::{validate_date_range, Args};
use backfill::{print_summary, run_backfill, run_test_mode};
use clap::Parser;
use orderx_core::fxstreet::FxstreetClient;
use orderx_core::questdb::QuestDbWriter;
use tokio::time::Instant;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    validate_date_range(args.from, args.to)?;
    let start_time = Instant::now();

    info!("Starting FXStreet Backfill CLI");
    info!("Range: {} to {}", args.from, args.to);
    info!("Page size: {}, Dry Run: {}", args.page_size, args.dry_run);

    let db_writer = QuestDbWriter::from_env()
        .context("Failed to setup DB writer from environment variables")?;
    db_writer
        .ensure_table_exists()
        .await
        .context("Failed to ensure QuestDB table exists before backfill")?;

    // --test: use a single dummy event and exit immediately.
    // If --dry-run is also set, do not write to DB.
    if args.test {
        return run_test_mode(&db_writer, args.dry_run).await;
    }

    let fxstreet_client = FxstreetClient::from_env()
        .context("Failed to setup FXStreet client from environment variables")?;
    let stats = run_backfill(&args, &fxstreet_client, &db_writer).await?;
    let elapsed = start_time.elapsed().as_millis();
    print_summary(&stats, elapsed);

    Ok(())
}
