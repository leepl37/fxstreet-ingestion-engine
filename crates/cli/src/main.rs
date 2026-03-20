use anyhow::{Result, Context};
use chrono::{DateTime, Utc};
use clap::Parser;
use orderx_core::fxstreet::FxstreetClient;
use orderx_core::models::{EconomicEvent, EventSource};
use orderx_core::questdb::QuestDbWriter;
use tokio::time::{sleep, Duration, Instant};
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(author, version, about = "Backfill historical FXStreet economic events")]
struct Args {
    /// Start date in RFC3339 format (e.g., 2026-03-01T00:00:00Z)
    #[arg(long)]
    from: DateTime<Utc>,

    /// End date in RFC3339 format (e.g., 2026-03-10T00:00:00Z)
    #[arg(long)]
    to: DateTime<Utc>,

    /// Number of events per page
    #[arg(long, default_value_t = 100)]
    page_size: usize,

    /// Fetch data but skip Database insertion
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Default, Debug)]
struct Stats {
    fetched: usize,
    inserted: usize,
    failed: usize,
    retried: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    let args = Args::parse();
    validate_date_range(args.from, args.to)?;
    let start_time = Instant::now();

    info!("Starting FXStreet Backfill CLI");
    info!("Range: {} to {}", args.from, args.to);
    info!("Page size: {}, Dry Run: {}", args.page_size, args.dry_run);

    let db_writer = QuestDbWriter::from_env().context("Failed to setup DB writer from environment variables")?;
    let fxstreet_client = FxstreetClient::from_env().context("Failed to setup FXStreet client from environment variables")?;
    let mut stats = Stats::default();

    let mut skip = 0;
    let max_retries = 3;
    
    loop {
        let mut attempt = 0;
        let mut success = false;
        let mut raw_events = Vec::new();

        while attempt <= max_retries {
            info!("Fetching skip={}, attempt {}/{}", skip, attempt, max_retries);
            match fxstreet_client
                .fetch_event_dates_range(args.from, args.to, skip, args.page_size)
                .await
            {
                Ok(events) => {
                    raw_events = events;
                    success = true;
                    break;
                }
                Err(e) => {
                    if e.is_retryable() && attempt < max_retries {
                        stats.retried += 1;
                        let backoff_secs = 2u64.pow(attempt as u32);
                        warn!(
                            "Retryable API error: {}. Retrying in {} seconds (Attempt {}/{})",
                            e, backoff_secs, attempt + 1, max_retries
                        );
                        sleep(Duration::from_secs(backoff_secs)).await;
                        attempt += 1;
                    } else if e.is_retryable() {
                        error!("Max retries reached for retryable error: {}", e);
                        stats.failed += args.page_size;
                        break;
                    } else {
                        return Err(anyhow::anyhow!("Non-retryable API error: {}", e));
                    }
                }
            }
        }

        if !success && attempt > max_retries {
            error!("Max retries reached for block starting at skip={}. Skipping this page.", skip);
            // Even if one page fails, we can continue to the next one to salvage the rest of the backfill
            stats.failed += args.page_size;
        }

        if raw_events.is_empty() {
            info!("No events fetched on this page. Finished pagination.");
            break; 
        }

        let count = raw_events.len();
        stats.fetched += count;

        // Transform and Insert
        if !args.dry_run {
            let economic_events: Vec<EconomicEvent> = raw_events
                .into_iter()
                .map(|r| EconomicEvent::from((r, EventSource::Backfill)))
                .collect();

            if let Err(e) = db_writer.write_batch(&economic_events).await {
                error!("Failed to write batch to QuestDB: {}", e);
                stats.failed += count;
            } else {
                stats.inserted += count;
                info!("Successfully inserted {} events into QuestDB", count);
            }
        } else {
            info!("[DRY RUN] Would have transformed and inserted {} events", count);
        }

        // If returned count is less than requested page size, it means this was the last page
        if count < args.page_size {
            break;
        }

        skip += args.page_size;
    }

    let elapsed = start_time.elapsed().as_millis();
    
    // Print the final required output
    println!("\n=== Backfill Summary ===");
    println!("fetched: {}", stats.fetched);
    println!("inserted: {}", stats.inserted);
    println!("failed: {}", stats.failed);
    println!("retried: {}", stats.retried);
    println!("elapsed_ms: {}", elapsed);
    println!("========================\n");

    Ok(())
}

fn validate_date_range(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<()> {
    if from > to {
        return Err(anyhow::anyhow!(
            "Invalid range: --from must be earlier than or equal to --to"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn accepts_valid_date_range() {
        let from = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap();
        let result = validate_date_range(from, to);
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_invalid_date_range() {
        let from = Utc.with_ymd_and_hms(2026, 3, 3, 0, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap();
        let result = validate_date_range(from, to);
        assert!(result.is_err());
    }
}
