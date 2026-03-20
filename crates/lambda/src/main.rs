use lambda_http::{run, service_fn, Body, Error, Request, Response};
use tracing::{info, error, instrument};
use orderx_core::fxstreet::{FxstreetClient, FxstreetMode};
use orderx_core::models::{EconomicEvent, EventSource};
use orderx_core::questdb::QuestDbWriter;
use std::env;
use std::sync::Arc;
use tokio::time::Instant;

#[derive(serde::Deserialize, Debug)]
pub struct WebhookPayload {
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    #[serde(rename = "eventDateId")]
    pub event_date_id: Option<String>,
    #[serde(rename = "eventId")]
    pub event_id: Option<String>,
}

struct AppState {
    db_writer: QuestDbWriter,
    expected_token: String,
    fxstreet_client: FxstreetClient,
}

#[instrument(skip(state, req))]
async fn function_handler(req: Request, state: Arc<AppState>) -> Result<Response<Body>, Error> {
    let start = Instant::now();
    let mut status_code = 200;
    
    // 1. Method check: POST only (405)
    if req.method() != lambda_http::http::Method::POST {
        status_code = 405;
        error!(status = status_code, category = "input", latency_ms = start.elapsed().as_millis(), "Method not allowed");
        return Ok(Response::builder().status(status_code).body(Body::Text("Method Not Allowed".into()))?);
    }

    // 2. Security Check: X-Webhook-Token validates against secret
    let token = req.headers().get("X-Webhook-Token").and_then(|h| h.to_str().ok()).unwrap_or("");
    if !state.expected_token.is_empty() && token != state.expected_token {
        status_code = 403;
        error!(status = status_code, category = "input", latency_ms = start.elapsed().as_millis(), "Forbidden webhook token");
        return Ok(Response::builder().status(status_code).body(Body::Text("Forbidden".into()))?);
    }

    // 2.5 Test mode: X-Test-Mode header → insert dummy event directly (no FXStreet API call)
    let is_test_mode = req.headers().get("X-Test-Mode")
        .and_then(|h| h.to_str().ok())
        .map(|v| v == "true")
        .unwrap_or(false);

    if is_test_mode {
        use orderx_core::fxstreet::FxstreetClient;
        let dummy_client = FxstreetClient::new_mock();
        let raw = dummy_client.fetch_event_date_by_id("lambda-test-dummy").await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        let event = EconomicEvent::from((raw, EventSource::Webhook));
        if let Err(e) = state.db_writer.write_event(&event).await {
            error!(category = "db", error = %e, "[TEST] QuestDB write failed");
            return Ok(Response::builder().status(500).body(Body::Text("Internal Server Error".into()))?);
        }
        info!(latency_ms = start.elapsed().as_millis(), "[TEST] Inserted 1 dummy event into QuestDB");
        return Ok(Response::builder().status(200).body(Body::Text("OK (test mode)".into()))?);
    }

    // 3. Payload parsing & validation (400 if failed)
    let payload_res = match req.body() {
        Body::Text(text) => serde_json::from_str::<WebhookPayload>(text),
        Body::Binary(bytes) => serde_json::from_slice::<WebhookPayload>(bytes),
        Body::Empty => return Ok(Response::builder().status(400).body(Body::Text("Empty Body (Bad Request)".into()))?),
    };

    let payload = match payload_res {
        Ok(p) => p,
        Err(e) => {
            status_code = 400;
            error!(status = status_code, category = "input", error = ?e, latency_ms = start.elapsed().as_millis(), "Invalid JSON payload");
            return Ok(Response::builder().status(status_code).body(Body::Text("Bad Request".into()))?);
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
            return Ok(Response::builder()
                .status(202)
                .body(Body::Text("Accepted: event type not handled by eventDate ingestion".into()))?);
        }
    };

    // 4. Fetch full event via FXStreet API — up to 2 retries on transient errors
    let mut raw_event = None;
    let max_retries = 2;
    for attempt in 0..=max_retries {
        match state.fxstreet_client.fetch_event_date_by_id(&event_date_id).await {
            Ok(event) => {
                raw_event = Some(event);
                break;
            }
            Err(e) if e.is_retryable() && attempt < max_retries => {
                let wait_ms = 500 * (attempt as u64 + 1);
                tracing::warn!(
                    category = "api",
                    attempt,
                    event_id = %event_date_id,
                    "Transient API error, retrying in {}ms: {}",
                    wait_ms, e
                );
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            }
            Err(e) => {
                status_code = 500;
                error!(status = status_code, category = "api", event_id = %event_date_id, error = ?e, latency_ms = start.elapsed().as_millis(), "External FXStreet API HTTP request failure");
                return Ok(Response::builder().status(status_code).body(Body::Text("Internal Server Error".into()))?);
            }
        }
    }
    let raw_event = raw_event.unwrap();

    // 5. Transform and Write to Database
    let economic_event = EconomicEvent::from((raw_event, EventSource::Webhook));
    
    if let Err(e) = state.db_writer.write_event(&economic_event).await {
        status_code = 500;
        error!(status = status_code, category = "db", event_id = %event_date_id, error = %e, latency_ms = start.elapsed().as_millis(), "QuestDB write_event failure");
        return Ok(Response::builder().status(status_code).body(Body::Text("Internal Server Error".into()))?);
    }

    info!(status = status_code, source = "webhook", event_id = %event_date_id, latency_ms = start.elapsed().as_millis(), "Successfully processed and persisted Webhook event");

    Ok(Response::builder()
        .status(200)
        .body(Body::Text("OK".into()))?)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    info!("Initializing Webhook Lambda execution context");

    let writer = QuestDbWriter::from_env().expect("Failed to init QuestDB writer from environment");
    
    if let Err(e) = writer.ensure_table_exists().await {
        error!(category="db", error = %e, "Cold start: table bootstrap failed (ensure_table_exists)");
    } else {
        info!("Cold start: table bootstrap successfully completed");
    }

    let expected_token = env::var("WEBHOOK_SECRET_TOKEN").unwrap_or_default();
    let fxstreet_client = FxstreetClient::from_env()
        .expect("Failed to initialize FxstreetClient from environment");
    info!(mode = ?fxstreet_client.mode(), "FXStreet client initialized");
    if fxstreet_client.mode() == FxstreetMode::Mock {
        info!("Running in FXSTREET_MODE=mock (no external token required)");
    }

    let state = Arc::new(AppState {
        db_writer: writer,
        expected_token,
        fxstreet_client,
    });

    run(service_fn(move |req| {
        let state_clone = Arc::clone(&state);
        async move { function_handler(req, state_clone).await }
    })).await
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

        let payload: WebhookPayload = serde_json::from_str(sample_json)
            .expect("Sample payload should parse perfectly");
        
        assert_eq!(payload.event_date_id.as_deref(), Some("e93de8e7-fc33-4f11-925f-2ec8284fcdcf"));
    }
}
