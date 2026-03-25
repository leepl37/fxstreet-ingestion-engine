mod handler;

use crate::handler::{function_handler, AppState};
use aws_sdk_ssm::Client as SsmClient;
use lambda_http::{run, service_fn, Error};
use orderx_core::fxstreet::FxstreetClient;
use orderx_core::questdb::QuestDbWriter;
use std::env;
use std::sync::Arc;
use tracing::{error, info};

async fn load_secret_from_ssm(ssm: &SsmClient, parameter_name: &str) -> Result<String, Error> {
    let res = ssm
        .get_parameter()
        .name(parameter_name)
        .with_decryption(true)
        .send()
        .await?;

    let value = res
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| format!("SSM parameter '{}' has no value", parameter_name))?;

    Ok(value.to_string())
}

async fn load_secret_with_fallback(
    ssm: &SsmClient,
    direct_env_var: &str,
    parameter_name_env_var: &str,
) -> Result<String, Error> {
    if let Ok(value) = env::var(direct_env_var) {
        if !value.trim().is_empty() {
            info!(
                env_var = direct_env_var,
                "Using direct environment secret (local/dev fallback)"
            );
            return Ok(value);
        }
    }

    match env::var(parameter_name_env_var) {
        Ok(parameter_name) if !parameter_name.trim().is_empty() => {
            info!(
                env_var = parameter_name_env_var,
                parameter = %parameter_name,
                "Loading secret value from AWS SSM Parameter Store"
            );
            load_secret_from_ssm(ssm, &parameter_name).await
        }
        _ => Ok(String::new()),
    }
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

    let aws_config = aws_config::load_from_env().await;
    let ssm_client = SsmClient::new(&aws_config);

    let expected_token = load_secret_with_fallback(
        &ssm_client,
        "WEBHOOK_SECRET_TOKEN",
        "WEBHOOK_SECRET_TOKEN_PARAM",
    )
    .await?;
    let fxstreet_bearer_token = load_secret_with_fallback(
        &ssm_client,
        "FXSTREET_BEARER_TOKEN",
        "FXSTREET_BEARER_TOKEN_PARAM",
    )
    .await?;
    if !fxstreet_bearer_token.is_empty() {
        env::set_var("FXSTREET_BEARER_TOKEN", fxstreet_bearer_token);
    }

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
