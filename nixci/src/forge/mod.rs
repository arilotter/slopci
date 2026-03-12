pub mod github;

use anyhow::Result;
use axum::http::HeaderMap;
use std::path::Path;

use crate::models::Repo;

#[derive(Debug, Clone)]
pub enum ForgeEvent {
    Push {
        repo: String,
        branch: String,
        sha: String,
        installation_id: i64,
    },
    PullRequest {
        repo: String,
        pr_number: u64,
        head_sha: String,
        base_branch: String,
        installation_id: i64,
    },
}

#[derive(Debug, Clone)]
pub struct CommitStatus {
    pub state: CommitStatusState,
    pub context: String,
    pub description: String,
    pub target_url: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum CommitStatusState {
    Pending,
    Success,
    Failure,
    Error,
}

#[derive(Debug, Clone)]
pub struct ForgeRepo {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
}

#[async_trait::async_trait]
pub trait ForgeBackend: Send + Sync {
    /// Verify webhook signature.
    fn verify_webhook(&self, headers: &HeaderMap, body: &[u8]) -> Result<()>;

    /// Parse webhook payload into a normalized event.
    fn parse_event(&self, headers: &HeaderMap, body: &[u8]) -> Result<Option<ForgeEvent>>;

    /// Set commit status (pending/success/failure).
    async fn set_commit_status(
        &self,
        repo: &Repo,
        sha: &str,
        status: &CommitStatus,
    ) -> Result<()>;

    /// List repos accessible to the authenticated app/token.
    async fn list_repos(&self, installation_id: i64) -> Result<Vec<ForgeRepo>>;

    /// Clone/fetch a repo to a local path.
    async fn fetch_repo(&self, repo: &Repo, sha: &str, dest: &Path) -> Result<()>;

    /// Get an installation access token (cached if possible).
    async fn get_installation_token(&self, installation_id: i64) -> Result<String>;
}
