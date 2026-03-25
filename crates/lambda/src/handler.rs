use lambda_http::{Body, Error, Request, Response};
use orderx_core::fxstreet::FxstreetClient;
use orderx_core::models::{EconomicEvent, EventSource, FxEventRaw};
use orderx_core::questdb::QuestDbWriter;
use std::env;
use std::sync::Arc;
use tokio::time::Instant;
use tracing::{error, info, instrument, warn};

#[derive(serde::Deserialize, Debug)]
pub struct WebhookPayload {
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    #[serde(rename = "eventDateId")]
    pub event_date_id: Option<String>,
    #[serde(rename = "eventId")]
    pub event_id: Option<String>,
}

pub struct AppState {
    pub db_writer: QuestDbWriter,
    pub expected_token: String,
    pub fxstreet_client: FxstreetClient,
}

fn failed_event_log_payload(event: &EconomicEvent) -> String {
    let max_bytes = env::var("FAILED_EVENT_LOG_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(2048);

    let serialized = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    if serialized.len() <= max_bytes {
        return serialized;
    }

    let mut truncated = serialized.chars().take(max_bytes).collect::<String>();
    truncated.push_str("...(truncated)");
    truncated
}

enum PayloadParseError {
    EmptyBody,
    InvalidJson(serde_json::Error),
}

fn webhook_token(req: &Request) -> &str {
    req.headers()
        .get("X-Webhook-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
}

fn is_test_mode(req: &Request) -> bool {
    req.headers()
        .get("X-Test-Mode")
        .and_then(|h| h.to_str().ok())
        .map(|v| v == "true")
        .unwrap_or(false)
}

fn parse_webhook_payload(req: &Request) -> Result<WebhookPayload, PayloadParseError> {
    match req.body() {
        Body::Text(text) => {
            serde_json::from_str::<WebhookPayload>(text).map_err(PayloadParseError::InvalidJson)
        }
        Body::Binary(bytes) => {
            serde_json::from_slice::<WebhookPayload>(bytes).map_err(PayloadParseError::InvalidJson)
        }
        Body::Empty => Err(PayloadParseError::EmptyBody),
    }
}

async fn fetch_event_date_with_retry(
    fxstreet_client: &FxstreetClient,
    event_date_id: &str,
) -> Result<FxEventRaw, orderx_core::error::CoreError> {
    let max_retries = 2;
    for attempt in 0..=max_retries {
        match fxstreet_client.fetch_event_date_by_id(event_date_id).await {
            Ok(event) => return Ok(event),
            Err(e) if e.is_retryable() && attempt < max_retries => {
                let wait_ms = 500 * (attempt as u64 + 1);
                warn!(
                    category = "api",
                    attempt,
                    event_id = %event_date_id,
                    "Transient API error, retrying in {}ms: {}",
                    wait_ms, e
                );
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("retry loop should return success or error")
}

#[instrument(skip(state, req))]
pub async fn function_handler(req: Request, state: Arc<AppState>) -> Result<Response<Body>, Error> {
    let start = Instant::now();
    let mut status_code = 200;

    if req.method() != lambda_http::http::Method::POST {
        status_code = 405;
        error!(
            status = status_code,
            category = "input",
            latency_ms = start.elapsed().as_millis(),
            "Method not allowed"
        );
        return Ok(Response::builder()
            .status(status_code)
            .body(Body::Text("Method Not Allowed".into()))?);
    }

    let token = webhook_token(&req);
    if !state.expected_token.is_empty() && token != state.expected_token {
        status_code = 403;
        error!(
            status = status_code,
            category = "input",
            latency_ms = start.elapsed().as_millis(),
            "Forbidden webhook token"
        );
        return Ok(Response::builder()
            .status(status_code)
            .body(Body::Text("Forbidden".into()))?);
    }

    if is_test_mode(&req) {
        let dummy_client = FxstreetClient::new_mock();
        let raw = dummy_client
            .fetch_event_date_by_id("lambda-test-dummy")
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        let event = EconomicEvent::from((raw, EventSource::Webhook));
        if let Err(e) = state.db_writer.write_event(&event).await {
            let failed_event = failed_event_log_payload(&event);
            error!(
                category = "db",
                error = %e,
                failed_event = %failed_event,
                "[TEST] QuestDB write failed; event persisted to structured logs for replay"
            );
            return Ok(Response::builder()
                .status(500)
                .body(Body::Text("Internal Server Error".into()))?);
        }
        info!(
            latency_ms = start.elapsed().as_millis(),
            "[TEST] Inserted 1 dummy event into QuestDB"
        );
        return Ok(Response::builder()
            .status(200)
            .body(Body::Text("OK (test mode)".into()))?);
    }

    let payload = match parse_webhook_payload(&req) {
        Ok(p) => p,
        Err(PayloadParseError::EmptyBody) => {
            return Ok(Response::builder()
                .status(400)
                .body(Body::Text("Empty Body (Bad Request)".into()))?)
        }
        Err(PayloadParseError::InvalidJson(e)) => {
            status_code = 400;
            error!(status = status_code, category = "input", error = ?e, latency_ms = start.elapsed().as_millis(), "Invalid JSON payload");
            return Ok(Response::builder()
                .status(status_code)
                .body(Body::Text("Bad Request".into()))?);
        }
    };

    let event_date_id = match payload.event_date_id {
        Some(id) => id,
        None => {
            info!(
                status = 202,
                category = "input",
                source = "webhook",
                event_type = ?payload.event_type,
                event_id = ?payload.event_id,
                latency_ms = start.elapsed().as_millis(),
                "Ignoring webhook event without eventDateId"
            );
            return Ok(Response::builder().status(202).body(Body::Text(
                "Accepted: event type not handled by eventDate ingestion".into(),
            ))?);
        }
    };

    let raw_event = match fetch_event_date_with_retry(&state.fxstreet_client, &event_date_id).await
    {
        Ok(event) => event,
        Err(e) => {
            status_code = 500;
            error!(status = status_code, category = "api", event_id = %event_date_id, error = ?e, latency_ms = start.elapsed().as_millis(), "External FXStreet API HTTP request failure");
            return Ok(Response::builder()
                .status(status_code)
                .body(Body::Text("Internal Server Error".into()))?);
        }
    };

    let economic_event = EconomicEvent::from((raw_event, EventSource::Webhook));
    if let Err(e) = state.db_writer.write_event(&economic_event).await {
        status_code = 500;
        let failed_event = failed_event_log_payload(&economic_event);
        error!(
            status = status_code,
            category = "db",
            event_id = %event_date_id,
            error = %e,
            failed_event = %failed_event,
            latency_ms = start.elapsed().as_millis(),
            "QuestDB write_event failure; event persisted to structured logs for replay"
        );
        return Ok(Response::builder()
            .status(status_code)
            .body(Body::Text("Internal Server Error".into()))?);
    }

    info!(status = status_code, source = "webhook", event_id = %event_date_id, latency_ms = start.elapsed().as_millis(), "Successfully processed and persisted Webhook event");
    Ok(Response::builder()
        .status(200)
        .body(Body::Text("OK".into()))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_payload_parsing() {
        let sample_json = r#"{
            "type": "eventDateUpdated",
            "eventDateId": "e93de8e7-fc33-4f11-925f-2ec8284fcdcf",
            "eventId": "a1b2c3d4",
            "lastUpdated": 1709218290
        }"#;

        let payload: WebhookPayload =
            serde_json::from_str(sample_json).expect("Sample payload should parse perfectly");

        assert_eq!(
            payload.event_date_id.as_deref(),
            Some("e93de8e7-fc33-4f11-925f-2ec8284fcdcf")
        );
    }
}
