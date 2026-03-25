use anyhow::{Context, Result};
use orderx_core::fxstreet::FxstreetClient;
use orderx_core::models::{EconomicEvent, EventSource, FxEventRaw};
use orderx_core::questdb::QuestDbWriter;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::args::Args;

const MAX_RETRIES: usize = 3;

#[derive(Default, Debug)]
pub(crate) struct Stats {
    fetched: usize,
    inserted: usize,
    failed: usize,
    retried: usize,
}

pub(crate) async fn run_test_mode(db_writer: &QuestDbWriter, dry_run: bool) -> Result<()> {
    let dummy_client = FxstreetClient::new_mock();
    let raw = dummy_client
        .fetch_event_date_by_id("cli-test-dummy")
        .await
        .context("Failed to build dummy event")?;
    let event = EconomicEvent::from((raw, EventSource::Backfill));

    if dry_run {
        info!("[TEST][DRY-RUN] Built 1 dummy event; skipped QuestDB write");
        println!("\n=== Test Mode (Dry Run) ===");
        println!("dummy_events: 1");
        println!("inserted: 0");
        println!("===========================\n");
        return Ok(());
    }

    db_writer
        .write_event(&event)
        .await
        .context("Failed to write dummy event to QuestDB")?;
    info!("[TEST] Inserted 1 dummy event into QuestDB successfully");
    println!("\n=== Test Mode ===");
    println!("inserted: 1 dummy event");
    println!("==================");
    Ok(())
}

pub(crate) async fn run_backfill(
    args: &Args,
    fxstreet_client: &FxstreetClient,
    db_writer: &QuestDbWriter,
) -> Result<Stats> {
    let mut stats = Stats::default();
    let mut skip = 0;

    loop {
        let maybe_raw_events =
            fetch_page_with_retry(fxstreet_client, args, skip, MAX_RETRIES, &mut stats).await?;

        let raw_events = match maybe_raw_events {
            Some(events) => events,
            None => {
                // If one page fails after retries, skip it and continue salvage.
                stats.failed += args.page_size;
                skip += args.page_size;
                continue;
            }
        };

        if raw_events.is_empty() {
            info!("No events fetched on this page. Finished pagination.");
            break;
        }

        let count = process_page(raw_events, db_writer, args.dry_run, &mut stats).await;

        // If returned count is less than requested page size, it means this was the last page.
        if count < args.page_size {
            break;
        }

        skip += args.page_size;
    }

    Ok(stats)
}

async fn fetch_page_with_retry(
    fxstreet_client: &FxstreetClient,
    args: &Args,
    skip: usize,
    max_retries: usize,
    stats: &mut Stats,
) -> Result<Option<Vec<FxEventRaw>>> {
    for attempt in 0..=max_retries {
        info!(
            "Fetching skip={}, attempt {}/{}",
            skip,
            attempt + 1,
            max_retries + 1
        );

        match fxstreet_client
            .fetch_event_dates_range(args.from, args.to, skip, args.page_size)
            .await
        {
            Ok(events) => return Ok(Some(events)),
            Err(e) if e.is_retryable() && attempt < max_retries => {
                stats.retried += 1;
                let backoff_secs = 2u64.pow(attempt as u32);
                warn!(
                    "Retryable API error: {}. Retrying in {} seconds (Attempt {}/{})",
                    e,
                    backoff_secs,
                    attempt + 1,
                    max_retries + 1
                );
                sleep(Duration::from_secs(backoff_secs)).await;
            }
            Err(e) if e.is_retryable() => {
                error!(
                    "Max retries reached for block starting at skip={}. Skipping this page. Error: {}",
                    skip, e
                );
                return Ok(None);
            }
            Err(e) => return Err(anyhow::anyhow!("Non-retryable API error: {}", e)),
        }
    }

    Ok(None)
}

async fn process_page(
    raw_events: Vec<FxEventRaw>,
    db_writer: &QuestDbWriter,
    dry_run: bool,
    stats: &mut Stats,
) -> usize {
    let count = raw_events.len();
    stats.fetched += count;

    if dry_run {
        info!(
            "[DRY RUN] Would have transformed and inserted {} events",
            count
        );
        return count;
    }

    let economic_events: Vec<EconomicEvent> = raw_events
        .into_iter()
        .map(|raw| EconomicEvent::from((raw, EventSource::Backfill)))
        .collect();

    if let Err(e) = db_writer.write_batch(&economic_events).await {
        error!("Failed to write batch to QuestDB: {}", e);
        stats.failed += count;
    } else {
        stats.inserted += count;
        info!("Successfully inserted {} events into QuestDB", count);
    }

    count
}

pub(crate) fn print_summary(stats: &Stats, elapsed_ms: u128) {
    println!("\n=== Backfill Summary ===");
    println!("fetched: {}", stats.fetched);
    println!("inserted: {}", stats.inserted);
    println!("failed: {}", stats.failed);
    println!("retried: {}", stats.retried);
    println!("elapsed_ms: {}", elapsed_ms);
    println!("========================\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use orderx_core::questdb::QuestDbWriter;

    fn sample_args(page_size: usize, dry_run: bool) -> Args {
        Args {
            from: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
            to: Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap(),
            page_size,
            dry_run,
            test: false,
        }
    }

    #[tokio::test]
    async fn fetch_page_with_retry_returns_mock_first_page() {
        let args = sample_args(10, true);
        let client = FxstreetClient::new_mock();
        let mut stats = Stats::default();

        let result = fetch_page_with_retry(&client, &args, 0, 0, &mut stats)
            .await
            .expect("mock fetch should succeed");

        let events = result.expect("first mock page should return events");
        assert_eq!(events.len(), 2);
        assert_eq!(stats.retried, 0);
    }

    #[tokio::test]
    async fn fetch_page_with_retry_returns_empty_for_later_mock_page() {
        let args = sample_args(10, true);
        let client = FxstreetClient::new_mock();
        let mut stats = Stats::default();

        let result = fetch_page_with_retry(&client, &args, 10, 0, &mut stats)
            .await
            .expect("mock fetch should succeed");

        let events = result.expect("request itself should be successful");
        assert!(events.is_empty());
        assert_eq!(stats.retried, 0);
    }

    #[tokio::test]
    async fn process_page_dry_run_updates_fetched_only() {
        let client = FxstreetClient::new_mock();
        let raw_events = client
            .fetch_event_dates_range(
                Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap(),
                0,
                10,
            )
            .await
            .expect("mock range fetch should succeed");
        let db_writer = QuestDbWriter::from_env().expect("db writer should be creatable");
        let mut stats = Stats::default();

        let count = process_page(raw_events, &db_writer, true, &mut stats).await;

        assert_eq!(count, 2);
        assert_eq!(stats.fetched, 2);
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.failed, 0);
    }

    #[tokio::test]
    async fn process_page_write_failure_increments_failed() {
        std::env::set_var("QUESTDB_WRITE_MAX_RETRIES", "0");

        let client = FxstreetClient::new_mock();
        let raw_events = client
            .fetch_event_dates_range(
                Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap(),
                0,
                10,
            )
            .await
            .expect("mock range fetch should succeed");
        let db_writer = QuestDbWriter::from_env().expect("db writer should be creatable");
        let mut stats = Stats::default();

        let count = process_page(raw_events, &db_writer, false, &mut stats).await;

        assert_eq!(count, 2);
        assert_eq!(stats.fetched, 2);
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.failed, 2);
    }
}
