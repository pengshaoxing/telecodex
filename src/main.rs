mod app;
mod codex;
mod codex_history;
mod commands;
mod config;
mod limits;
mod models;
mod render;
mod store;
mod telegram;
mod transcribe;

use std::fs::OpenOptions;
use std::sync::Mutex;

use anyhow::{Context, Result};
use tokio::time::{Duration, sleep};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    if let Some(delay_ms) = restart_delay_ms_from_env() {
        sleep(Duration::from_millis(delay_ms)).await;
    }

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "telecodex.toml".to_string());
    let config = config::Config::load(config_path.into())?;

    init_tracing(config.log_file.as_deref())?;

    let app = app::App::bootstrap(config).await?;
    app.run().await
}

fn init_tracing(log_file: Option<&std::path::Path>) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("telecodex=info,reqwest=warn"));

    let stderr_layer = fmt::layer()
        .with_target(false)
        .compact()
        .with_writer(std::io::stderr);

    match log_file {
        Some(path) => {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("failed to open log file: {}", path.display()))?;
            let file_layer = fmt::layer()
                .with_target(false)
                .compact()
                .with_ansi(false)
                .with_writer(Mutex::new(file));

            tracing_subscriber::registry()
                .with(env_filter)
                .with(stderr_layer)
                .with(file_layer)
                .init();

            tracing::info!("logging to file: {}", path.display());
        }
        None => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(stderr_layer)
                .init();
        }
    }

    Ok(())
}

fn restart_delay_ms_from_env() -> Option<u64> {
    std::env::var("TELECODEX_RESTART_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}
