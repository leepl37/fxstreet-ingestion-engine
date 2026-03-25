mod handler;

use crate::handler::{function_handler, AppState};
use lambda_http::{run, service_fn, Error};
use orderx_core::fxstreet::FxstreetClient;
use orderx_core::questdb::QuestDbWriter;
use std::env;
use std::sync::Arc;
use tracing::{error, info};

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
    let fxstreet_client =
        FxstreetClient::from_env().expect("Failed to initialize FxstreetClient from environment");
    info!("FXStreet client initialized (real API mode by default)");

    let state = Arc::new(AppState {
        db_writer: writer,
        expected_token,
        fxstreet_client,
    });

    run(service_fn(move |req| {
        let state_clone = Arc::clone(&state);
        async move { function_handler(req, state_clone).await }
    }))
    .await
}
