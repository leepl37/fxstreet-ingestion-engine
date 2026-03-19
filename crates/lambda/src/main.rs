use lambda_http::{run, service_fn, Body, Error, Request, Response};
use tracing::{info, error, instrument};
use orderx_core::models::{EconomicEvent, EventSource, FxEventRaw};
use orderx_core::questdb::QuestDbWriter;
use std::env;
use std::sync::Arc;
use tokio::time::Instant;

#[derive(serde::Deserialize, Debug)]
pub struct WebhookPayload {
    #[serde(rename = "eventDateId")]
    pub event_date_id: String,
}

struct AppState {
    db_writer: QuestDbWriter,
    expected_token: String,
    fxstreet_api_url: String,
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

    let event_date_id = payload.event_date_id;

    // 4. Fetch full event via FXStreet API (500 if DB/API error)
    let url = format!("{}/{}", state.fxstreet_api_url, event_date_id);
    let api_res = match reqwest::get(&url).await {
        Ok(r) => r,
        Err(e) => {
            status_code = 500;
            error!(status = status_code, category = "api", event_id = %event_date_id, error = ?e, latency_ms = start.elapsed().as_millis(), "External FXStreet API HTTP request failure");
            return Ok(Response::builder().status(status_code).body(Body::Text("Internal Server Error".into()))?);
        }
    };

    if !api_res.status().is_success() {
        status_code = 500;
        let api_status = api_res.status().as_u16();
        error!(status = status_code, api_status = api_status, category = "api", event_id = %event_date_id, latency_ms = start.elapsed().as_millis(), "FXStreet API returned non-200");
        return Ok(Response::builder().status(status_code).body(Body::Text("Internal Server Error".into()))?);
    }

    let raw_event = match api_res.json::<FxEventRaw>().await {
        Ok(e) => e,
        Err(e) => {
            status_code = 500;
            error!(status = status_code, category = "api", event_id = %event_date_id, error = ?e, latency_ms = start.elapsed().as_millis(), "Failed to parse API response into FxEventRaw");
            return Ok(Response::builder().status(status_code).body(Body::Text("Internal Server Error".into()))?);
        }
    };

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
    let fxstreet_api_url = env::var("FXSTREET_API_URL")
        .unwrap_or_else(|_| "https://calendar-api.fxstreet.com/eventDates".to_string());

    let state = Arc::new(AppState {
        db_writer: writer,
        expected_token,
        fxstreet_api_url,
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
        
        assert_eq!(payload.event_date_id, "e93de8e7-fc33-4f11-925f-2ec8284fcdcf");
    }
}
