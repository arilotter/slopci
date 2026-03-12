use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use maud::html;
use serde::Deserialize;
use std::sync::Arc;

use super::AppState;
use crate::models::{Build, Repo, Secret};
use crate::templates;

type HtmlResponse = Result<maud::Markup, StatusCode>;

pub async fn dashboard(State(state): State<Arc<AppState>>) -> HtmlResponse {
    let builds: Vec<Build> =
        sqlx::query_as("SELECT * FROM builds ORDER BY id DESC LIMIT 50")
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let repos: Vec<Repo> = sqlx::query_as("SELECT * FROM repos ORDER BY full_name")
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(templates::layout(
        "Dashboard",
        html! {
            h1 { "nixci" }

            h2 { "Repos" }
            table {
                thead {
                    tr {
                        th { "Repo" }
                        th { "Default Branch" }
                        th { "Status" }
                    }
                }
                tbody {
                    @for repo in &repos {
                        (templates::repo_row(repo))
                    }
                    @if repos.is_empty() {
                        tr { td colspan="3" { "No repos connected. " a href="/repos" { "Add one" } } }
                    }
                }
            }

            h2 { "Recent Builds" }
            (templates::build_table(&builds))
        },
    ))
}

pub async fn repos_list(State(state): State<Arc<AppState>>) -> HtmlResponse {
    let repos: Vec<Repo> = sqlx::query_as("SELECT * FROM repos ORDER BY full_name")
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(templates::layout(
        "Repos",
        html! {
            h1 { "Repos" }
            table {
                thead {
                    tr {
                        th { "Repo" }
                        th { "Default Branch" }
                        th { "Status" }
                    }
                }
                tbody {
                    @for repo in &repos {
                        (templates::repo_row(repo))
                    }
                }
            }

            h2 { "Connect a Repo" }
            p { "Install the nixci GitHub App on your organization/account, then repos will appear here automatically." }
        },
    ))
}

#[derive(Deserialize)]
pub struct CreateRepo {
    pub installation_id: i64,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
}

pub async fn repos_create(
    State(state): State<Arc<AppState>>,
    axum::extract::Form(form): axum::extract::Form<CreateRepo>,
) -> Result<Response, StatusCode> {
    sqlx::query(
        "INSERT OR IGNORE INTO repos (installation_id, owner, name, full_name, default_branch) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(form.installation_id)
    .bind(&form.owner)
    .bind(&form.name)
    .bind(&form.full_name)
    .bind(&form.default_branch)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to("/repos").into_response())
}

pub async fn repo_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> HtmlResponse {
    let repo: Repo = sqlx::query_as("SELECT * FROM repos WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let builds: Vec<Build> =
        sqlx::query_as("SELECT * FROM builds WHERE repo_id = ? ORDER BY id DESC LIMIT 50")
            .bind(id)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(templates::layout(
        &repo.full_name,
        html! {
            h1 { (repo.full_name) }
            p { "Default branch: " code { (repo.default_branch) } }

            h2 { "Builds" }
            (templates::build_table(&builds))
        },
    ))
}

pub async fn build_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> HtmlResponse {
    let build: Build = sqlx::query_as("SELECT * FROM builds WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let repo: Repo = sqlx::query_as("SELECT * FROM repos WHERE id = ?")
        .bind(build.repo_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get existing logs from DB
    let logs: Vec<crate::models::BuildLog> =
        sqlx::query_as("SELECT * FROM build_logs WHERE build_id = ? ORDER BY seq")
            .bind(id)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let is_live = build.status == "running" || build.status == "pending";

    Ok(templates::layout(
        &format!("Build #{}", build.id),
        html! {
            h1 { "Build #" (build.id) }
            div class="build-meta" {
                span { "Repo: " a href=(format!("/repos/{}", repo.id)) { (repo.full_name) } }
                span { "Commit: " code { (build.commit_sha.get(..8).unwrap_or(&build.commit_sha)) } }
                span class=(templates::status_class(&build.status)) { "Status: " (build.status) }
                @if let Some(ref branch) = build.branch {
                    span { "Branch: " (branch) }
                }
                @if let Some(pr) = build.pr_number {
                    span { "PR: #" (pr) }
                }
                span { "Attr: " code { (build.flake_attr) } }
            }

            @if build.status == "failure" || build.status == "cancelled" {
                form hx-post=(format!("/builds/{}/retry", build.id)) hx-swap="outerHTML" {
                    button type="submit" { "Retry Build" }
                }
            }

            h2 { "Logs" }
            @if is_live {
                pre class="log"
                    hx-ext="sse"
                    sse-connect=(format!("/builds/{}/logs", build.id))
                    sse-swap="log"
                    hx-swap="beforeend"
                {
                    @for log in &logs {
                        (templates::log_line_html(&log.stream, &log.line))
                    }
                }
            } @else {
                pre class="log" {
                    @for log in &logs {
                        (templates::log_line_html(&log.stream, &log.line))
                    }
                    @if logs.is_empty() {
                        "No logs recorded."
                    }
                }
            }
        },
    ))
}

pub async fn build_retry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, StatusCode> {
    let new_id = state
        .orchestrator
        .retry_build(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(&format!("/builds/{new_id}")).into_response())
}

pub async fn action_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> HtmlResponse {
    let action: crate::models::Action = sqlx::query_as("SELECT * FROM actions WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let logs: Vec<crate::models::ActionLog> =
        sqlx::query_as("SELECT * FROM action_logs WHERE action_id = ? ORDER BY seq")
            .bind(id)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let is_live = action.status == "running" || action.status == "pending";

    Ok(templates::layout(
        &format!("Action #{}", action.id),
        html! {
            h1 { "Action #" (action.id) " — " (action.name) }
            div class="build-meta" {
                span { "App: " code { (action.app_attr) } }
                span class=(templates::status_class(&action.status)) { "Status: " (action.status) }
            }

            h2 { "Logs" }
            @if is_live {
                pre class="log"
                    hx-ext="sse"
                    sse-connect=(format!("/actions/{}/logs", action.id))
                    sse-swap="log"
                    hx-swap="beforeend"
                {
                    @for log in &logs {
                        (templates::log_line_html(&log.stream, &log.line))
                    }
                }
            } @else {
                pre class="log" {
                    @for log in &logs {
                        (templates::log_line_html(&log.stream, &log.line))
                    }
                }
            }
        },
    ))
}

pub async fn settings(State(state): State<Arc<AppState>>) -> HtmlResponse {
    let installations: Vec<crate::models::Installation> =
        sqlx::query_as("SELECT * FROM installations ORDER BY account_login")
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(templates::layout(
        "Settings",
        html! {
            h1 { "Settings" }

            h2 { "GitHub App Installations" }
            @if installations.is_empty() {
                p { "No installations found. Install the nixci GitHub App on your organization or account." }
            } @else {
                table {
                    thead {
                        tr {
                            th { "Account" }
                            th { "Type" }
                            th { "ID" }
                        }
                    }
                    tbody {
                        @for inst in &installations {
                            tr {
                                td { (inst.account_login) }
                                td { (inst.account_type) }
                                td { (inst.id) }
                            }
                        }
                    }
                }
            }
        },
    ))
}

pub async fn repo_secrets_list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> HtmlResponse {
    let repo: Repo = sqlx::query_as("SELECT * FROM repos WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let secrets: Vec<Secret> =
        sqlx::query_as("SELECT * FROM secrets WHERE repo_id = ? ORDER BY action_name, secret_name")
            .bind(id)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(templates::layout(
        "Secrets",
        html! {
            h1 { "Secrets — " (repo.full_name) }
            table {
                thead {
                    tr {
                        th { "Action" }
                        th { "Name" }
                        th { "Public Key" }
                        th {}
                    }
                }
                tbody {
                    @for secret in &secrets {
                        tr {
                            td { (secret.action_name) }
                            td { (secret.secret_name) }
                            td { code { (secret.pubkey.get(..20).unwrap_or(&secret.pubkey)) "..." } }
                            td {
                                form hx-delete=(format!("/repos/{}/secrets/{}", id, secret.secret_name))
                                     hx-swap="outerHTML"
                                     hx-target="closest tr"
                                {
                                    button type="submit" { "delete" }
                                }
                            }
                        }
                    }
                }
            }

            h2 { "Add Secret" }
            form method="post" action=(format!("/repos/{}/secrets", id)) {
                label { "Action name: " input type="text" name="action_name" required {} }
                " "
                label { "Secret name: " input type="text" name="secret_name" required {} }
                " "
                label { "Ciphertext (age-encrypted, base64): " input type="text" name="ciphertext" required {} }
                " "
                label { "Public key: " input type="text" name="pubkey" required {} }
                " "
                button type="submit" { "Add" }
            }
        },
    ))
}

#[derive(Deserialize)]
pub struct CreateSecret {
    pub action_name: String,
    pub secret_name: String,
    pub ciphertext: String,
    pub pubkey: String,
}

pub async fn repo_secrets_create(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    axum::extract::Form(form): axum::extract::Form<CreateSecret>,
) -> Result<Response, StatusCode> {
    let ct_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &form.ciphertext,
    )
    .map_err(|_| StatusCode::BAD_REQUEST)?;

    sqlx::query(
        "INSERT OR REPLACE INTO secrets (repo_id, action_name, secret_name, ciphertext, pubkey) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&form.action_name)
    .bind(&form.secret_name)
    .bind(&ct_bytes)
    .bind(&form.pubkey)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(&format!("/repos/{id}/secrets")).into_response())
}

pub async fn repo_secrets_delete(
    State(state): State<Arc<AppState>>,
    Path((id, name)): Path<(i64, String)>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query("DELETE FROM secrets WHERE repo_id = ? AND secret_name = ?")
        .bind(id)
        .bind(&name)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

pub async fn secrets_pubkey(
    State(state): State<Arc<AppState>>,
    Path((owner, repo, _action)): Path<(String, String, String)>,
) -> Result<String, StatusCode> {
    // Request the public key from the secrets service via unix socket
    // For now, return a placeholder — this will be wired up with the secrets service
    let full_name = format!("{owner}/{repo}");
    let repo_row: Option<Repo> =
        sqlx::query_as("SELECT * FROM repos WHERE full_name = ?")
            .bind(&full_name)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _repo = repo_row.ok_or(StatusCode::NOT_FOUND)?;

    // TODO: call nixci-secrets over unix socket to get the pubkey
    Err(StatusCode::NOT_IMPLEMENTED)
}
