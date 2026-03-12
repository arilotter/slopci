use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct DecryptRequest {
    pub owner: String,
    pub repo: String,
    pub action: String,
    pub secrets: Vec<SecretEntry>,
}

#[derive(Debug, Deserialize)]
pub struct SecretEntry {
    pub name: String,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct DecryptResponse {
    pub secrets: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct PubkeyRequest {
    pub owner: String,
    pub repo: String,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct PubkeyResponse {
    pub pubkey: String,
}
