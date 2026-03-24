//! Domain models for FXStreet economic calendar events.
//!
//! - `FxEventRaw`: payload shape from **REST API** EventDate (e.g. GET /eventDates/{from}/{to} or
//!   GET /eventDates/{eventDateId}). Field names/aliases match the OpenAPI EventDate schema.
//!   Note: Webhook POST body only sends `{ type, eventDateId, eventId, lastUpdated }`; the Lambda
//!   should fetch the full event via GET /eventDates/{eventDateId} and then deserialize into this.
//! - `EconomicEvent`: internal canonical model for storage (QuestDB).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Raw event as received from FXStreet API or webhook.
/// Field names and aliases match the [Calendar API EventDate schema](https://calendar-api.fxstreet.com/swagger/v1/openapi.json).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FxEventRaw {
    /// ID of the event occurrence (EventDate.id)
    pub id: String,
    /// Date and time of the occurrence (UTC). API field: `dateUtc`.
    #[serde(alias = "dateUtc")]
    pub date: DateTime<Utc>,
    /// Country code (ISO 3166-1). API field: `countryCode`.
    #[serde(default, alias = "countryCode")]
    pub country: Option<String>,
    /// Currency code (ISO 4217). API field: `currencyCode`.
    #[serde(default, alias = "currencyCode")]
    pub currency: Option<String>,
    /// Name of the recurrent event. API field: `name`.
    #[serde(alias = "name")]
    pub title: String,
    #[serde(default)]
    pub actual: Option<f64>,
    /// Consensus/forecast value. API field: `consensus`.
    #[serde(default, alias = "consensus")]
    pub forecast: Option<f64>,
    #[serde(default)]
    pub previous: Option<f64>,
}

/// Source of the event (webhook vs backfill).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSource {
    Webhook,
    Backfill,
}

impl EventSource {
    /// Stable tag string for QuestDB ILP `source` column (matches `serde` lowercase names).
    #[must_use]
    pub const fn as_ilp_tag(self) -> &'static str {
        match self {
            EventSource::Webhook => "webhook",
            EventSource::Backfill => "backfill",
        }
    }
}

/// Internal standard model for storage (QuestDB).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EconomicEvent {
    pub event_id: String,
    pub event_time: DateTime<Utc>,
    pub country: Option<String>,
    pub currency: Option<String>,
    pub title: String,
    pub actual: Option<f64>,
    pub forecast: Option<f64>,
    pub previous: Option<f64>,
    pub source: EventSource,
    pub ingested_at: DateTime<Utc>,
}

impl EconomicEvent {
    /// Builds an `EconomicEvent` from a raw FXStreet payload and the given source.
    pub fn from_raw(raw: FxEventRaw, source: EventSource) -> Self {
        Self {
            event_id: raw.id,
            event_time: raw.date,
            country: raw.country,
            currency: raw.currency,
            title: raw.title,
            actual: raw.actual,
            forecast: raw.forecast,
            previous: raw.previous,
            source,
            ingested_at: Utc::now(),
        }
    }
}

impl From<(FxEventRaw, EventSource)> for EconomicEvent {
    fn from((raw, source): (FxEventRaw, EventSource)) -> Self {
        EconomicEvent::from_raw(raw, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_raw() -> FxEventRaw {
        FxEventRaw {
            id: "evt-001".to_string(),
            date: Utc.with_ymd_and_hms(2026, 3, 19, 14, 30, 0).unwrap(),
            country: Some("US".to_string()),
            currency: Some("USD".to_string()),
            title: "Nonfarm Payrolls".to_string(),
            actual: Some(200_000.0),
            forecast: Some(195_000.0),
            previous: Some(210_000.0),
        }
    }

    #[test]
    fn from_raw_webhook() {
        let raw = sample_raw();
        let event = EconomicEvent::from_raw(raw.clone(), EventSource::Webhook);
        assert_eq!(event.event_id, "evt-001");
        assert_eq!(event.event_time, raw.date);
        assert_eq!(event.country.as_deref(), Some("US"));
        assert_eq!(event.currency.as_deref(), Some("USD"));
        assert_eq!(event.title, "Nonfarm Payrolls");
        assert_eq!(event.actual, Some(200_000.0));
        assert_eq!(event.source, EventSource::Webhook);
        assert!(event.ingested_at.timestamp() <= Utc::now().timestamp() + 1);
    }

    #[test]
    fn from_raw_backfill() {
        let raw = sample_raw();
        let event = EconomicEvent::from_raw(raw, EventSource::Backfill);
        assert_eq!(event.source, EventSource::Backfill);
    }

    #[test]
    fn event_source_ilp_tags_match_serde_lowercase() {
        assert_eq!(EventSource::Webhook.as_ilp_tag(), "webhook");
        assert_eq!(EventSource::Backfill.as_ilp_tag(), "backfill");
    }

    #[test]
    fn parse_raw_json_our_shape() {
        let json = r#"{"id":"e1","date":"2026-03-19T12:00:00Z","title":"CPI","country":"US","currency":"USD"}"#;
        let raw: FxEventRaw = serde_json::from_str(json).unwrap();
        assert_eq!(raw.id, "e1");
        assert_eq!(raw.title, "CPI");
        assert_eq!(raw.country.as_deref(), Some("US"));
        assert!(raw.actual.is_none());
    }

    /// Parses JSON using actual FXStreet API field names (EventDate schema).
    #[test]
    fn parse_raw_json_api_shape() {
        let json = r#"{"id":"4fe1bd69-acce-4b24-9d54-f45c81708d29","dateUtc":"2019-03-21T10:30:00Z","name":"Retail Sales (MoM)","countryCode":"US","currencyCode":"USD","actual":0.2,"consensus":0.2,"previous":0.4}"#;
        let raw: FxEventRaw = serde_json::from_str(json).unwrap();
        assert_eq!(raw.id, "4fe1bd69-acce-4b24-9d54-f45c81708d29");
        assert_eq!(raw.title, "Retail Sales (MoM)");
        assert_eq!(raw.country.as_deref(), Some("US"));
        assert_eq!(raw.currency.as_deref(), Some("USD"));
        assert_eq!(raw.actual, Some(0.2));
        assert_eq!(raw.forecast, Some(0.2));
        assert_eq!(raw.previous, Some(0.4));
    }
}
