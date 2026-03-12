mod keyderive;
mod protocol;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let master_key_path = std::env::var("NIXCI_SECRETS_MASTER_KEY")
        .context("NIXCI_SECRETS_MASTER_KEY env var required")?;
    let socket_path = std::env::var("NIXCI_SECRETS_SOCKET")
        .unwrap_or_else(|_| "/run/nixci-secrets/nixci-secrets.sock".to_string());

    let master_key = std::fs::read(&master_key_path).context("Failed to read master key file")?;
    tracing::info!("Loaded master key from {master_key_path}");

    // Remove stale socket
    let _ = std::fs::remove_file(&socket_path);
    if let Some(parent) = PathBuf::from(&socket_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!("Listening on {socket_path}");

    loop {
        let (stream, _) = listener.accept().await?;
        let master_key = master_key.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &master_key).await {
                tracing::error!("Connection error: {e}");
            }
        });
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    master_key: &[u8],
) -> Result<()> {
    // Peek at first byte to determine request type
    let mut type_buf = [0u8; 1];
    stream.read_exact(&mut type_buf).await?;

    match type_buf[0] {
        b'P' => handle_pubkey_request(&mut stream, master_key).await,
        _ => {
            // Decrypt request — first byte is part of the length prefix
            // Re-read the remaining 3 bytes of the length
            let mut len_rest = [0u8; 3];
            stream.read_exact(&mut len_rest).await?;
            let len = u32::from_le_bytes([type_buf[0], len_rest[0], len_rest[1], len_rest[2]])
                as usize;

            let mut buf = vec![0u8; len];
            stream.read_exact(&mut buf).await?;

            let request: protocol::DecryptRequest = serde_json::from_slice(&buf)?;
            handle_decrypt_request(&mut stream, master_key, request).await
        }
    }
}

async fn handle_pubkey_request(
    stream: &mut tokio::net::UnixStream,
    master_key: &[u8],
) -> Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    let request: protocol::PubkeyRequest = serde_json::from_slice(&buf)?;

    let pubkey =
        keyderive::derive_recipient(master_key, &request.owner, &request.repo, &request.action)?;

    let response = protocol::PubkeyResponse { pubkey };
    let resp_bytes = serde_json::to_vec(&response)?;

    let len = resp_bytes.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&resp_bytes).await?;

    Ok(())
}

async fn handle_decrypt_request(
    stream: &mut tokio::net::UnixStream,
    master_key: &[u8],
    request: protocol::DecryptRequest,
) -> Result<()> {
    let identity = keyderive::derive_identity(
        master_key,
        &request.owner,
        &request.repo,
        &request.action,
    )?;

    let mut decrypted = HashMap::new();
    for entry in &request.secrets {
        match keyderive::decrypt(&identity, &entry.ciphertext) {
            Ok(plaintext) => {
                let value = String::from_utf8(plaintext)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).to_string());
                decrypted.insert(entry.name.clone(), value);
            }
            Err(e) => {
                tracing::error!("Failed to decrypt secret {}: {e}", entry.name);
            }
        }
    }

    let response = protocol::DecryptResponse {
        secrets: decrypted,
    };
    let resp_bytes = serde_json::to_vec(&response)?;

    let len = resp_bytes.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&resp_bytes).await?;

    Ok(())
}
