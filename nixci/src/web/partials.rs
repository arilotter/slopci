use axum::extract::{Path, State};
use axum::http::StatusCode;
use std::sync::Arc;

use super::AppState;
use crate::models::Build;
use crate::templates;

pub async fn build_row_partial(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<maud::Markup, StatusCode> {
    let build: Build = sqlx::query_as("SELECT * FROM builds WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(templates::build_row(&build))
}
