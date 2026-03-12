pub mod microvm;
pub mod secrets;

use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;

use crate::builder::config::ActionEntry;
use crate::models::Repo;

pub async fn run_action(
    build_id: i64,
    repo: &Repo,
    entry: &ActionEntry,
    work_dir: &Path,
    pool: &SqlitePool,
    secrets_socket: &Path,
) -> Result<()> {
    // Create action record
    let action_id: i64 = sqlx::query_scalar(
        "INSERT INTO actions (build_id, repo_id, name, app_attr, status) VALUES (?, ?, ?, ?, 'pending') RETURNING id",
    )
    .bind(build_id)
    .bind(repo.id)
    .bind(&entry.name)
    .bind(&entry.app)
    .fetch_one(pool)
    .await?;

    // Request decrypted secrets from nixci-secrets
    let decrypted_secrets = if !entry.secrets.is_empty() {
        secrets::request_secrets(
            secrets_socket,
            &repo.owner,
            &repo.name,
            &entry.name,
            &entry.secrets,
            pool,
            repo.id,
        )
        .await?
    } else {
        std::collections::HashMap::new()
    };

    // Update status to running
    sqlx::query("UPDATE actions SET status = 'running', started_at = datetime('now') WHERE id = ?")
        .bind(action_id)
        .execute(pool)
        .await?;

    // Build the nix app
    let build_result = tokio::process::Command::new("nix")
        .args(["build", &entry.app, "--no-link", "--print-out-paths"])
        .current_dir(work_dir)
        .output()
        .await?;

    if !build_result.status.success() {
        let stderr = String::from_utf8_lossy(&build_result.stderr);
        tracing::error!("Action {action_id}: nix build failed: {stderr}");
        sqlx::query(
            "UPDATE actions SET status = 'failure', finished_at = datetime('now') WHERE id = ?",
        )
        .bind(action_id)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let app_path = String::from_utf8_lossy(&build_result.stdout)
        .trim()
        .to_string();

    // Run in microVM
    let exit_code = microvm::run_in_microvm(action_id, &app_path, &decrypted_secrets, pool).await?;

    let status = if exit_code == 0 { "success" } else { "failure" };

    sqlx::query(
        "UPDATE actions SET status = ?, exit_code = ?, finished_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(exit_code)
    .bind(action_id)
    .execute(pool)
    .await?;

    Ok(())
}
