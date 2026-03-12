pub mod pages;
pub mod partials;
pub mod sse;
pub mod webhooks;

use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tower_http::services::ServeDir;

use crate::builder::BuildOrchestrator;
use crate::forge::ForgeBackend;

pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub orchestrator: Arc<BuildOrchestrator>,
    pub forge: Arc<dyn ForgeBackend>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(pages::dashboard))
        .route("/repos", get(pages::repos_list))
        .route("/repos", post(pages::repos_create))
        .route("/repos/{id}", get(pages::repo_detail))
        .route("/repos/{id}/secrets", get(pages::repo_secrets_list))
        .route("/repos/{id}/secrets", post(pages::repo_secrets_create))
        .route("/repos/{id}/secrets/{name}", axum::routing::delete(pages::repo_secrets_delete))
        .route("/builds/{id}", get(pages::build_detail))
        .route("/builds/{id}/logs", get(sse::build_logs_stream))
        .route("/builds/{id}/retry", post(pages::build_retry))
        .route("/actions/{id}", get(pages::action_detail))
        .route("/actions/{id}/logs", get(sse::action_logs_stream))
        .route("/settings", get(pages::settings))
        .route("/secrets/pubkey/{owner}/{repo}/{action}", get(pages::secrets_pubkey))
        .route("/webhooks/github", post(webhooks::github_webhook))
        .route("/partials/build/{id}", get(partials::build_row_partial))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}
