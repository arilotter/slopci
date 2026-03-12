use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use std::convert::Infallible;
use std::sync::Arc;

use super::AppState;
use crate::templates;

pub async fn build_logs_stream(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Response {
    let rx = state.orchestrator.subscribe_logs(id).await;

    let stream = async_stream::stream! {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(log_line) => {
                    let html = templates::log_line_html(&log_line.stream, &log_line.line).into_string();
                    yield Ok::<_, Infallible>(Event::default().event("log").data(html));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE client lagged by {n} messages");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).into_response()
}

pub async fn action_logs_stream(
    State(_state): State<Arc<AppState>>,
    Path(_id): Path<i64>,
) -> Response {
    let stream = async_stream::stream! {
        yield Ok::<_, Infallible>(Event::default().event("log").data("Action log streaming not yet implemented\n"));
    };

    Sse::new(stream).into_response()
}
