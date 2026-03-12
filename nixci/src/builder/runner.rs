use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;

use super::config::BuildOptions;
use super::LogLine;

pub async fn run_build(
    build_id: i64,
    work_dir: &Path,
    flake_attr: &str,
    options: &BuildOptions,
    pool: &SqlitePool,
    tx: &broadcast::Sender<LogLine>,
) -> Result<i32> {
    let flake_ref = format!("path:{work_dir}#{attr}", work_dir = work_dir.display(), attr = flake_attr.trim_start_matches(".#"));

    let mut cmd = tokio::process::Command::new("nix-fast-build");
    cmd.arg("--flake").arg(&flake_ref);
    cmd.arg("--no-nom");
    cmd.arg("--result-file")
        .arg(format!("/tmp/nixci-result-{build_id}.json"));
    cmd.arg("--result-format").arg("json");

    if let Some(max_jobs) = options.max_jobs {
        cmd.arg("--max-jobs").arg(max_jobs.to_string());
    }

    if options.skip_cached {
        cmd.arg("--skip-cached");
    }

    if let Some(systems) = &options.systems {
        for system in systems.split_whitespace() {
            cmd.arg("--systems").arg(system);
        }
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.current_dir(work_dir);

    let mut child = cmd.spawn().context("Failed to spawn nix-fast-build")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let pool_stdout = pool.clone();
    let tx_stdout = tx.clone();
    let stdout_handle = tokio::spawn(async move {
        capture_stream(build_id, "stdout", stdout, &pool_stdout, &tx_stdout).await
    });

    let pool_stderr = pool.clone();
    let tx_stderr = tx.clone();
    let stderr_handle = tokio::spawn(async move {
        capture_stream(build_id, "stderr", stderr, &pool_stderr, &tx_stderr).await
    });

    let status = child.wait().await?;

    // Wait for stream capture to finish
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    Ok(status.code().unwrap_or(-1))
}

async fn capture_stream<R: tokio::io::AsyncRead + Unpin>(
    build_id: i64,
    stream_name: &str,
    reader: R,
    pool: &SqlitePool,
    tx: &broadcast::Sender<LogLine>,
) {
    let buf = BufReader::new(reader);
    let mut lines = buf.lines();
    let mut seq: i64 = 0;
    let mut batch: Vec<(i64, i64, String, String)> = Vec::new();
    let mut last_flush = tokio::time::Instant::now();

    while let Ok(Some(line)) = lines.next_line().await {
        seq += 1;

        let log_line = LogLine {
            build_id,
            seq,
            stream: stream_name.to_string(),
            line: line.clone(),
        };

        // Send to broadcast (ignore errors — no receivers is fine)
        let _ = tx.send(log_line);

        batch.push((build_id, seq, stream_name.to_string(), line));

        // Flush every 100 lines or 500ms
        if batch.len() >= 100 || last_flush.elapsed() >= std::time::Duration::from_millis(500) {
            flush_batch(pool, &batch).await;
            batch.clear();
            last_flush = tokio::time::Instant::now();
        }
    }

    // Flush remaining
    if !batch.is_empty() {
        flush_batch(pool, &batch).await;
    }
}

async fn flush_batch(pool: &SqlitePool, batch: &[(i64, i64, String, String)]) {
    for (build_id, seq, stream, line) in batch {
        let _ = sqlx::query(
            "INSERT INTO build_logs (build_id, seq, stream, line) VALUES (?, ?, ?, ?)",
        )
        .bind(build_id)
        .bind(seq)
        .bind(stream)
        .bind(line)
        .execute(pool)
        .await;
    }
}
