use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use std::sync::Arc;

use super::AppState;

pub async fn github_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, StatusCode> {
    // Verify webhook signature
    state
        .forge
        .verify_webhook(&headers, &body)
        .map_err(|e| {
            tracing::warn!("Webhook verification failed: {e}");
            StatusCode::UNAUTHORIZED
        })?;

    // Parse event
    let event = state
        .forge
        .parse_event(&headers, &body)
        .map_err(|e| {
            tracing::warn!("Failed to parse webhook event: {e}");
            StatusCode::BAD_REQUEST
        })?;

    let event = match event {
        Some(e) => e,
        None => return Ok(StatusCode::OK), // Ignored event type
    };

    // Handle installation events separately
    if let Some(event_type) = headers.get("x-github-event").and_then(|v| v.to_str().ok()) {
        if event_type == "installation" {
            handle_installation_event(&state, &body).await;
            return Ok(StatusCode::OK);
        }
    }

    // Dispatch to build orchestrator
    let orchestrator = state.orchestrator.clone();
    tokio::spawn(async move {
        if let Err(e) = orchestrator.handle_event(event).await {
            tracing::error!("Failed to handle webhook event: {e}");
        }
    });

    Ok(StatusCode::OK)
}

async fn handle_installation_event(state: &AppState, body: &[u8]) {
    #[derive(serde::Deserialize)]
    struct InstallationPayload {
        action: String,
        installation: InstallationInfo,
    }
    #[derive(serde::Deserialize)]
    struct InstallationInfo {
        id: i64,
        account: AccountInfo,
    }
    #[derive(serde::Deserialize)]
    struct AccountInfo {
        login: String,
        #[serde(rename = "type")]
        account_type: String,
    }

    let Ok(payload) = serde_json::from_slice::<InstallationPayload>(body) else {
        return;
    };

    match payload.action.as_str() {
        "created" => {
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO installations (id, account_login, account_type) VALUES (?, ?, ?)",
            )
            .bind(payload.installation.id)
            .bind(&payload.installation.account.login)
            .bind(&payload.installation.account.account_type)
            .execute(&state.pool)
            .await;
        }
        "deleted" => {
            let _ = sqlx::query("DELETE FROM installations WHERE id = ?")
                .bind(payload.installation.id)
                .execute(&state.pool)
                .await;
        }
        _ => {}
    }
}
