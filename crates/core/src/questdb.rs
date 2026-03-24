use crate::error::CoreError;
use crate::models::EconomicEvent;
use reqwest::Client;
use std::env;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};

pub struct QuestDbWriter {
    host: String,
    http_port: u16,
    ilp_port: u16,
    http_client: Client,
}

impl QuestDbWriter {
    pub fn from_env() -> Result<Self, CoreError> {
        let host = env::var("QUESTDB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let ilp_port = env::var("QUESTDB_ILP_PORT")
            .unwrap_or_else(|_| "9009".to_string())
            .parse()
            .map_err(|_| CoreError::Config("Invalid QUESTDB_ILP_PORT".into()))?;
        let http_port = env::var("QUESTDB_HTTP_PORT")
            .unwrap_or_else(|_| "9000".to_string())
            .parse()
            .map_err(|_| CoreError::Config("Invalid QUESTDB_HTTP_PORT".into()))?;

        Ok(Self {
            host,
            http_port,
            ilp_port,
            http_client: Client::new(),
        })
    }

    pub async fn ensure_table_exists(&self) -> Result<(), CoreError> {
        let query = "CREATE TABLE IF NOT EXISTS economic_events (
            event_id SYMBOL,
            country SYMBOL,
            currency SYMBOL,
            title STRING,
            actual DOUBLE,
            forecast DOUBLE,
            previous DOUBLE,
            source SYMBOL,
            ingested_at TIMESTAMP,
            event_time TIMESTAMP
        ) TIMESTAMP(event_time) PARTITION BY MONTH
        DEDUP UPSERT KEYS(event_time, event_id);";

        let url = format!("http://{}:{}/exec", self.host, self.http_port);
        let res = self
            .http_client
            .get(&url)
            .query(&[("query", query)])
            .send()
            .await
            .map_err(CoreError::Http)?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(CoreError::QuestDb(format!(
                "Failed to create table. Status: {}, body: {}",
                status, text
            )));
        }

        Ok(())
    }

    pub async fn write_batch(&self, events: &[EconomicEvent]) -> Result<(), CoreError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut payload = String::new();
        for event in events {
            payload.push_str(&to_ilp_line(event));
            payload.push('\n');
        }

        self.write_ilp_with_retry(&payload).await
    }

    pub async fn write_event(&self, event: &EconomicEvent) -> Result<(), CoreError> {
        self.write_batch(std::slice::from_ref(event)).await
    }

    async fn write_ilp_with_retry(&self, payload: &str) -> Result<(), CoreError> {
        let max_retries: u32 = env::var("QUESTDB_WRITE_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        let base_backoff_ms: u64 = env::var("QUESTDB_WRITE_RETRY_BASE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);

        let addr = format!("{}:{}", self.host, self.ilp_port);
        let mut last_error: Option<CoreError> = None;

        for attempt in 0..=max_retries {
            match TcpStream::connect(&addr).await {
                Ok(mut stream) => match stream.write_all(payload.as_bytes()).await {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        last_error = Some(CoreError::QuestDb(format!("TCP write failed: {}", e)));
                    }
                },
                Err(e) => {
                    last_error = Some(CoreError::QuestDb(format!("TCP connection failed: {}", e)));
                }
            }

            if attempt < max_retries {
                let backoff = base_backoff_ms.saturating_mul(2_u64.pow(attempt));
                sleep(Duration::from_millis(backoff)).await;
            }
        }

        Err(last_error
            .unwrap_or_else(|| CoreError::QuestDb("Unknown ILP write failure".to_string())))
    }
}

pub fn to_ilp_line(event: &EconomicEvent) -> String {
    // InfluxDB Line Protocol (ILP) formatting
    let source_str = format!("{:?}", event.source).to_lowercase();
    let mut line = format!(
        "economic_events,event_id={},source={}",
        escape_tag(&event.event_id),
        escape_tag(&source_str)
    );

    if let Some(ref c) = event.country {
        if !c.is_empty() {
            line.push_str(&format!(",country={}", escape_tag(c)));
        }
    }
    if let Some(ref c) = event.currency {
        if !c.is_empty() {
            line.push_str(&format!(",currency={}", escape_tag(c)));
        }
    }

    line.push_str(&format!(" title=\"{}\"", escape_string_field(&event.title)));

    if let Some(act) = event.actual {
        line.push_str(&format!(",actual={}", act));
    }
    if let Some(fc) = event.forecast {
        line.push_str(&format!(",forecast={}", fc));
    }
    if let Some(prev) = event.previous {
        line.push_str(&format!(",previous={}", prev));
    }

    let ingested_micros = event.ingested_at.timestamp_micros();
    line.push_str(&format!(",ingested_at={}t", ingested_micros));

    let nanos = event.event_time.timestamp_nanos_opt().unwrap_or(0);
    line.push_str(&format!(" {}", nanos));

    line
}

fn escape_tag(val: &str) -> String {
    val.replace(',', "\\,")
        .replace('=', "\\=")
        .replace(' ', "\\ ")
}

fn escape_string_field(val: &str) -> String {
    val.replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{EventSource, FxEventRaw};
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_ilp_serialization() {
        let raw = FxEventRaw {
            id: "evt-123".to_string(),
            date: Utc.with_ymd_and_hms(2026, 3, 19, 14, 30, 0).unwrap(),
            country: Some("US".to_string()),
            currency: Some("USD".to_string()),
            title: "Nonfarm Payrolls".to_string(),
            actual: Some(200_000.0),
            forecast: Some(195_000.0),
            previous: None,
        };
        let mut event = EconomicEvent::from_raw(raw, EventSource::Webhook);
        // hardcode ingested_at for stable test
        event.ingested_at = Utc.with_ymd_and_hms(2026, 3, 19, 15, 0, 0).unwrap();

        let ilp = to_ilp_line(&event);
        let expected_nanos = event.event_time.timestamp_nanos_opt().unwrap();
        let expected_micros = event.ingested_at.timestamp_micros();

        let expected = format!(
            "economic_events,event_id=evt-123,source=webhook,country=US,currency=USD title=\"Nonfarm Payrolls\",actual=200000,forecast=195000,ingested_at={}t {}",
            expected_micros, expected_nanos
        );

        assert_eq!(ilp, expected);
    }

    #[test]
    fn test_ilp_escaping() {
        let raw = FxEventRaw {
            id: "evt=123,456".to_string(), // tags with = and ,
            date: Utc.with_ymd_and_hms(2026, 3, 19, 14, 30, 0).unwrap(),
            country: None,
            currency: None,
            title: "Test \"Quote\" Event".to_string(),
            actual: None,
            forecast: None,
            previous: None,
        };
        let mut event = EconomicEvent::from_raw(raw, EventSource::Backfill);
        event.ingested_at = Utc.with_ymd_and_hms(2026, 3, 19, 15, 0, 0).unwrap();

        let ilp = to_ilp_line(&event);
        let expected_nanos = event.event_time.timestamp_nanos_opt().unwrap();
        let expected_micros = event.ingested_at.timestamp_micros();

        let expected = format!(
            "economic_events,event_id=evt\\=123\\,456,source=backfill title=\"Test \\\"Quote\\\" Event\",ingested_at={}t {}",
            expected_micros, expected_nanos
        );

        assert_eq!(ilp, expected);
    }
}
