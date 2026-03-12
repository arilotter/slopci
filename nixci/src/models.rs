use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Installation {
    pub id: i64,
    pub account_login: String,
    pub account_type: String,
    pub access_token: Option<String>,
    pub token_expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Repo {
    pub id: i64,
    pub installation_id: i64,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
    pub webhook_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Build {
    pub id: i64,
    pub repo_id: i64,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub status: String,
    pub flake_attr: String,
    pub triggered_by: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct BuildLog {
    pub id: i64,
    pub build_id: i64,
    pub seq: i64,
    pub stream: String,
    pub line: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Action {
    pub id: i64,
    pub build_id: i64,
    pub repo_id: i64,
    pub name: String,
    pub app_attr: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ActionLog {
    pub id: i64,
    pub action_id: i64,
    pub seq: i64,
    pub stream: String,
    pub line: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Secret {
    pub id: i64,
    pub repo_id: i64,
    pub action_name: String,
    pub secret_name: String,
    pub ciphertext: Vec<u8>,
    pub pubkey: String,
    pub created_at: String,
}
