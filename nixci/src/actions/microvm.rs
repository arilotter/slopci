use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::collections::HashMap;
use tokio::io::AsyncBufReadExt;

/// Run an action's app inside a throwaway microVM.
///
/// For now, this runs the app directly as a subprocess (microVM integration
/// will be added when the microvm.nix infrastructure is in place).
/// The interface is designed so switching to real microVM execution is a
/// drop-in replacement.
pub async fn run_in_microvm(
    action_id: i64,
    app_path: &str,
    secrets: &HashMap<String, String>,
    pool: &SqlitePool,
) -> Result<i32> {
    // Find the executable — nix app output is typically a directory with a bin/ inside,
    // or the path itself might be a script/binary.
    let exe_path = if std::path::Path::new(app_path).join("bin").exists() {
        // Try to find the executable in bin/
        let mut entries = tokio::fs::read_dir(format!("{app_path}/bin")).await?;
        let entry = entries
            .next_entry()
            .await?
            .context("No executable found in bin/")?;
        entry.path().to_string_lossy().to_string()
    } else {
        app_path.to_string()
    };

    let mut cmd = tokio::process::Command::new(&exe_path);

    // Inject secrets as environment variables
    for (key, value) in secrets {
        cmd.env(key, value);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("Failed to spawn action")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Capture logs
    let pool_out = pool.clone();
    let stdout_handle = tokio::spawn(async move {
        capture_action_stream(action_id, "stdout", stdout, &pool_out).await;
    });

    let pool_err = pool.clone();
    let stderr_handle = tokio::spawn(async move {
        capture_action_stream(action_id, "stderr", stderr, &pool_err).await;
    });

    let status = child.wait().await?;
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    Ok(status.code().unwrap_or(-1))
}

async fn capture_action_stream<R: tokio::io::AsyncRead + Unpin>(
    action_id: i64,
    stream_name: &str,
    reader: R,
    pool: &SqlitePool,
) {
    let buf = tokio::io::BufReader::new(reader);
    let mut lines = buf.lines();
    let mut seq: i64 = 0;

    while let Ok(Some(line)) = lines.next_line().await {
        seq += 1;
        let _ = sqlx::query(
            "INSERT INTO action_logs (action_id, seq, stream, line) VALUES (?, ?, ?, ?)",
        )
        .bind(action_id)
        .bind(seq)
        .bind(stream_name)
        .bind(&line)
        .execute(pool)
        .await;
    }
}
