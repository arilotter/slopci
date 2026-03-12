#![allow(dead_code)]

mod actions;
mod builder;
mod config;
mod db;
mod forge;
mod models;
mod templates;
mod web;

use anyhow::Result;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::Config::from_env()?;
    tracing::info!("Starting nixci on {}", config.listen_addr);

    // Ensure work directory exists
    tokio::fs::create_dir_all(&config.work_dir).await?;

    // Set up database
    let pool = db::setup(&config.database_url).await?;
    tracing::info!("Database ready");

    // Set up GitHub backend
    let forge: Arc<dyn forge::ForgeBackend> =
        Arc::new(forge::github::GitHubBackend::new(config.github.clone())?);

    // Set up build orchestrator
    let orchestrator = Arc::new(builder::BuildOrchestrator::new(
        pool.clone(),
        forge.clone(),
        config.max_concurrent_builds,
        config.work_dir.clone(),
    ));

    // Set up web app
    let state = Arc::new(web::AppState {
        pool,
        orchestrator,
        forge,
    });

    let app = web::router(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!("Listening on {}", config.listen_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    tracing::info!("Shutting down...");
}
