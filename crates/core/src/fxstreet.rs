use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::Client;

use crate::error::CoreError;
use crate::models::FxEventRaw;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxstreetMode {
    Mock,
    Real,
}

impl FxstreetMode {
    fn from_env(mode: &str) -> Result<Self, CoreError> {
        match mode.to_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "real" => Ok(Self::Real),
            other => Err(CoreError::Config(format!(
                "Invalid FXSTREET_MODE '{other}', expected 'mock' or 'real'"
            ))),
        }
    }
}

pub struct FxstreetClient {
    mode: FxstreetMode,
    base_url: String,
    bearer_token: Option<String>,
    http_client: Client,
}

impl FxstreetClient {
    pub fn from_env() -> Result<Self, CoreError> {
        let mode_raw = std::env::var("FXSTREET_MODE").unwrap_or_else(|_| "mock".to_string());
        let mode = FxstreetMode::from_env(&mode_raw)?;

        let base_url = std::env::var("FXSTREET_API_BASE")
            .unwrap_or_else(|_| "https://calendar-api.fxstreet.com/en/api/v1".to_string());

        let bearer_token = std::env::var("FXSTREET_BEARER_TOKEN")
            .ok()
            .filter(|v| !v.trim().is_empty());

        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        Ok(Self {
            mode,
            base_url,
            bearer_token,
            http_client,
        })
    }

    pub fn new_mock() -> Self {
        Self {
            mode: FxstreetMode::Mock,
            base_url: "http://mock.local".to_string(),
            bearer_token: None,
            http_client: Client::new(),
        }
    }

    pub fn mode(&self) -> FxstreetMode {
        self.mode
    }

    pub async fn fetch_event_date_by_id(&self, event_date_id: &str) -> Result<FxEventRaw, CoreError> {
        match self.mode {
            FxstreetMode::Mock => Ok(mock_event(event_date_id, Utc::now())),
            FxstreetMode::Real => {
                let token = self
                    .bearer_token
                    .as_deref()
                    .ok_or_else(|| CoreError::Config("Missing FXSTREET_BEARER_TOKEN in real mode".to_string()))?;
                let url = format!("{}/eventDates/{}", self.base_url.trim_end_matches('/'), event_date_id);
                let res = self.http_client.get(url).bearer_auth(token).send().await?;
                if !res.status().is_success() {
                    let status = res.status().as_u16();
                    let body = res.text().await.unwrap_or_default();
                    let message = if body.is_empty() {
                        "empty response body".to_string()
                    } else {
                        body.chars().take(240).collect()
                    };
                    return Err(CoreError::ExternalApiStatus { status, message });
                }
                Ok(res.json::<FxEventRaw>().await?)
            }
        }
    }

    pub async fn fetch_event_dates_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        skip: usize,
        take: usize,
    ) -> Result<Vec<FxEventRaw>, CoreError> {
        match self.mode {
            FxstreetMode::Mock => {
                if skip > 0 {
                    return Ok(Vec::new());
                }
                let from_event = mock_event("mock-range-1", from);
                let to_event = mock_event("mock-range-2", to);
                let mut all = vec![from_event, to_event];
                all.truncate(take);
                Ok(all)
            }
            FxstreetMode::Real => {
                let token = self
                    .bearer_token
                    .as_deref()
                    .ok_or_else(|| CoreError::Config("Missing FXSTREET_BEARER_TOKEN in real mode".to_string()))?;
                let from_str = from.to_rfc3339_opts(SecondsFormat::Secs, true);
                let to_str = to.to_rfc3339_opts(SecondsFormat::Secs, true);
                let url = format!(
                    "{}/eventDates/{}/{}",
                    self.base_url.trim_end_matches('/'),
                    from_str,
                    to_str
                );
                let res = self
                    .http_client
                    .get(url)
                    .query(&[("skip", skip), ("take", take)])
                    .bearer_auth(token)
                    .send()
                    .await?;
                if !res.status().is_success() {
                    let status = res.status().as_u16();
                    let body = res.text().await.unwrap_or_default();
                    let message = if body.is_empty() {
                        "empty response body".to_string()
                    } else {
                        body.chars().take(240).collect()
                    };
                    return Err(CoreError::ExternalApiStatus { status, message });
                }
                Ok(res.json::<Vec<FxEventRaw>>().await?)
            }
        }
    }
}

fn mock_event(id: &str, date: DateTime<Utc>) -> FxEventRaw {
    FxEventRaw {
        id: id.to_string(),
        date,
        country: Some("US".to_string()),
        currency: Some("USD".to_string()),
        title: format!("Mock Event {id}"),
        actual: Some(1.1),
        forecast: Some(1.0),
        previous: Some(0.9),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[tokio::test]
    async fn mock_by_id_returns_event() {
        let client = FxstreetClient::new_mock();
        let event = client
            .fetch_event_date_by_id("abc-123")
            .await
            .expect("mock by id should succeed");
        assert_eq!(event.id, "abc-123");
        assert!(event.title.contains("Mock Event"));
    }

    #[tokio::test]
    async fn mock_range_returns_first_page_only() {
        let client = FxstreetClient::new_mock();
        let from = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap();

        let first_page = client
            .fetch_event_dates_range(from, to, 0, 10)
            .await
            .expect("first mock page should succeed");
        assert_eq!(first_page.len(), 2);

        let second_page = client
            .fetch_event_dates_range(from, to, 10, 10)
            .await
            .expect("second mock page should succeed");
        assert!(second_page.is_empty());
    }
}
