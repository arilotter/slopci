use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

#[derive(Debug, Serialize)]
struct DecryptRequest {
    owner: String,
    repo: String,
    action: String,
    secrets: Vec<SecretEntry>,
}

#[derive(Debug, Serialize)]
struct SecretEntry {
    name: String,
    ciphertext: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct DecryptResponse {
    secrets: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct PubkeyRequest {
    owner: String,
    repo: String,
    action: String,
}

#[derive(Debug, Deserialize)]
struct PubkeyResponse {
    pubkey: String,
}

/// Request decrypted secrets from the nixci-secrets service over a unix socket.
pub async fn request_secrets(
    socket_path: &Path,
    owner: &str,
    repo: &str,
    action: &str,
    secret_names: &[String],
    pool: &SqlitePool,
    repo_id: i64,
) -> Result<HashMap<String, String>> {
    // Fetch ciphertexts from DB
    let mut entries = Vec::new();
    for name in secret_names {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT ciphertext FROM secrets WHERE repo_id = ? AND action_name = ? AND secret_name = ?",
        )
        .bind(repo_id)
        .bind(action)
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some((ciphertext,)) = row {
            entries.push(SecretEntry {
                name: name.clone(),
                ciphertext,
            });
        } else {
            tracing::warn!("Secret {name} not found for action {action}");
        }
    }

    if entries.is_empty() {
        return Ok(HashMap::new());
    }

    let request = DecryptRequest {
        owner: owner.to_string(),
        repo: repo.to_string(),
        action: action.to_string(),
        secrets: entries,
    };

    let request_bytes = serde_json::to_vec(&request)?;

    let mut stream = UnixStream::connect(socket_path)
        .await
        .context("Failed to connect to nixci-secrets socket")?;

    // Send length-prefixed JSON
    let len = request_bytes.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&request_bytes).await?;

    // Read response
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let resp_len = u32::from_le_bytes(len_buf) as usize;

    let mut resp_buf = vec![0u8; resp_len];
    stream.read_exact(&mut resp_buf).await?;

    let response: DecryptResponse = serde_json::from_slice(&resp_buf)?;
    Ok(response.secrets)
}

/// Request a public key from the nixci-secrets service.
pub async fn request_pubkey(
    socket_path: &Path,
    owner: &str,
    repo: &str,
    action: &str,
) -> Result<String> {
    let request = PubkeyRequest {
        owner: owner.to_string(),
        repo: repo.to_string(),
        action: action.to_string(),
    };

    let request_bytes = serde_json::to_vec(&request)?;

    let mut stream = UnixStream::connect(socket_path)
        .await
        .context("Failed to connect to nixci-secrets socket")?;

    // Prefix with "P" to indicate pubkey request (vs "D" for decrypt)
    stream.write_all(b"P").await?;
    let len = request_bytes.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&request_bytes).await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let resp_len = u32::from_le_bytes(len_buf) as usize;

    let mut resp_buf = vec![0u8; resp_len];
    stream.read_exact(&mut resp_buf).await?;

    let response: PubkeyResponse = serde_json::from_slice(&resp_buf)?;
    Ok(response.pubkey)
}
