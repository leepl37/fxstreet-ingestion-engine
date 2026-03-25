use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Backfill historical FXStreet economic events"
)]
pub(crate) struct Args {
    /// Start date in RFC3339 format (e.g., 2026-03-01T00:00:00Z)
    #[arg(long)]
    pub(crate) from: DateTime<Utc>,

    /// End date in RFC3339 format (e.g., 2026-03-10T00:00:00Z)
    #[arg(long)]
    pub(crate) to: DateTime<Utc>,

    /// Number of events per page
    #[arg(long, default_value_t = 100)]
    pub(crate) page_size: usize,

    /// Fetch data but skip Database insertion
    #[arg(long, default_value_t = false)]
    pub(crate) dry_run: bool,

    /// Insert a single dummy event directly into QuestDB (bypasses FXStreet API)
    #[arg(long, default_value_t = false)]
    pub(crate) test: bool,
}

pub(crate) fn validate_date_range(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<()> {
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
