use anyhow::{bail, Context, Result};
use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::{CommitStatus, CommitStatusState, ForgeBackend, ForgeEvent, ForgeRepo};
use crate::config::GitHubConfig;
use crate::models::Repo;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize)]
struct JwtClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

#[derive(Clone)]
pub struct GitHubBackend {
    config: GitHubConfig,
    private_key: Vec<u8>,
    token_cache: Arc<Mutex<std::collections::HashMap<i64, CachedToken>>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl GitHubBackend {
    pub fn new(config: GitHubConfig) -> Result<Self> {
        let private_key =
            std::fs::read(&config.private_key_path).context("Failed to read GitHub private key")?;

        Ok(Self {
            config,
            private_key,
            token_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    fn generate_jwt(&self) -> Result<String> {
        let now = chrono::Utc::now();
        let claims = JwtClaims {
            iat: now.timestamp() - 60,
            exp: (now + chrono::Duration::minutes(10)).timestamp(),
            iss: self.config.app_id.to_string(),
        };

        let key = EncodingKey::from_rsa_pem(&self.private_key)?;
        let token = encode(&Header::new(Algorithm::RS256), &claims, &key)?;
        Ok(token)
    }

    async fn get_or_refresh_token(&self, installation_id: i64) -> Result<String> {
        let mut cache = self.token_cache.lock().await;
        if let Some(cached) = cache.get(&installation_id) {
            if cached.expires_at > chrono::Utc::now() + chrono::Duration::minutes(5) {
                return Ok(cached.token.clone());
            }
        }

        let jwt = self.generate_jwt()?;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "https://api.github.com/app/installations/{installation_id}/access_tokens"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "nixci")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Failed to get installation token: {status} {body}");
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            token: String,
            expires_at: String,
        }

        let token_resp: TokenResponse = resp.json().await?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&token_resp.expires_at)?
            .with_timezone(&chrono::Utc);

        cache.insert(
            installation_id,
            CachedToken {
                token: token_resp.token.clone(),
                expires_at,
            },
        );

        Ok(token_resp.token)
    }
}

#[async_trait::async_trait]
impl ForgeBackend for GitHubBackend {
    fn verify_webhook(&self, headers: &HeaderMap, body: &[u8]) -> Result<()> {
        let sig = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .context("Missing x-hub-signature-256 header")?;

        let sig = sig
            .strip_prefix("sha256=")
            .context("Invalid signature format")?;

        let sig_bytes = hex::decode(sig).context("Invalid hex in signature")?;

        let mut mac =
            HmacSha256::new_from_slice(self.config.webhook_secret.as_bytes()).expect("valid key");
        mac.update(body);
        mac.verify_slice(&sig_bytes)
            .map_err(|_| anyhow::anyhow!("Invalid webhook signature"))?;

        Ok(())
    }

    fn parse_event(&self, headers: &HeaderMap, body: &[u8]) -> Result<Option<ForgeEvent>> {
        let event_type = headers
            .get("x-github-event")
            .and_then(|v| v.to_str().ok())
            .context("Missing x-github-event header")?;

        let payload: serde_json::Value = serde_json::from_slice(body)?;

        match event_type {
            "push" => {
                let git_ref = payload["ref"]
                    .as_str()
                    .context("Missing ref")?
                    .to_string();
                let branch = git_ref
                    .strip_prefix("refs/heads/")
                    .context("Not a branch push")?
                    .to_string();
                let sha = payload["after"]
                    .as_str()
                    .context("Missing after")?
                    .to_string();
                let repo = payload["repository"]["full_name"]
                    .as_str()
                    .context("Missing repo full_name")?
                    .to_string();
                let installation_id = payload["installation"]["id"]
                    .as_i64()
                    .context("Missing installation id")?;

                Ok(Some(ForgeEvent::Push {
                    repo,
                    branch,
                    sha,
                    installation_id,
                }))
            }
            "pull_request" => {
                let action = payload["action"]
                    .as_str()
                    .context("Missing action")?;
                if action != "opened" && action != "synchronize" && action != "reopened" {
                    return Ok(None);
                }

                let repo = payload["repository"]["full_name"]
                    .as_str()
                    .context("Missing repo full_name")?
                    .to_string();
                let pr_number = payload["pull_request"]["number"]
                    .as_u64()
                    .context("Missing PR number")?;
                let head_sha = payload["pull_request"]["head"]["sha"]
                    .as_str()
                    .context("Missing head sha")?
                    .to_string();
                let base_branch = payload["pull_request"]["base"]["ref"]
                    .as_str()
                    .context("Missing base ref")?
                    .to_string();
                let installation_id = payload["installation"]["id"]
                    .as_i64()
                    .context("Missing installation id")?;

                Ok(Some(ForgeEvent::PullRequest {
                    repo,
                    pr_number,
                    head_sha,
                    base_branch,
                    installation_id,
                }))
            }
            _ => Ok(None),
        }
    }

    async fn set_commit_status(
        &self,
        repo: &Repo,
        sha: &str,
        status: &CommitStatus,
    ) -> Result<()> {
        let token = self
            .get_or_refresh_token(repo.installation_id)
            .await?;

        let state_str = match status.state {
            CommitStatusState::Pending => "pending",
            CommitStatusState::Success => "success",
            CommitStatusState::Failure => "failure",
            CommitStatusState::Error => "error",
        };

        let mut body = serde_json::json!({
            "state": state_str,
            "context": status.context,
            "description": status.description,
        });

        if let Some(url) = &status.target_url {
            body["target_url"] = serde_json::Value::String(url.clone());
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "https://api.github.com/repos/{}/{}/statuses/{sha}",
                repo.owner, repo.name
            ))
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "nixci")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Failed to set commit status: {status} {body}");
        }

        Ok(())
    }

    async fn list_repos(&self, installation_id: i64) -> Result<Vec<ForgeRepo>> {
        let token = self.get_or_refresh_token(installation_id).await?;

        let client = reqwest::Client::new();
        let resp = client
            .get("https://api.github.com/installation/repositories")
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "nixci")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Failed to list repos: {status} {body}");
        }

        #[derive(Deserialize)]
        struct RepoList {
            repositories: Vec<GhRepo>,
        }
        #[derive(Deserialize)]
        struct GhRepo {
            full_name: String,
            default_branch: Option<String>,
            owner: GhOwner,
            name: String,
        }
        #[derive(Deserialize)]
        struct GhOwner {
            login: String,
        }

        let repo_list: RepoList = resp.json().await?;
        Ok(repo_list
            .repositories
            .into_iter()
            .map(|r| ForgeRepo {
                owner: r.owner.login,
                name: r.name,
                full_name: r.full_name,
                default_branch: r.default_branch.unwrap_or_else(|| "main".to_string()),
            })
            .collect())
    }

    async fn fetch_repo(&self, repo: &Repo, sha: &str, dest: &Path) -> Result<()> {
        let token = self
            .get_or_refresh_token(repo.installation_id)
            .await?;

        let url = format!(
            "https://x-access-token:{token}@github.com/{}/{}.git",
            repo.owner, repo.name
        );

        if dest.exists() {
            // Fetch and checkout
            let output = tokio::process::Command::new("git")
                .args(["fetch", "origin", sha])
                .current_dir(dest)
                .output()
                .await?;
            if !output.status.success() {
                bail!(
                    "git fetch failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            let output = tokio::process::Command::new("git")
                .args(["checkout", sha])
                .current_dir(dest)
                .output()
                .await?;
            if !output.status.success() {
                bail!(
                    "git checkout failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        } else {
            tokio::fs::create_dir_all(dest).await?;
            let output = tokio::process::Command::new("git")
                .args(["clone", "--depth=1", &url, dest.to_str().unwrap()])
                .output()
                .await?;
            if !output.status.success() {
                bail!(
                    "git clone failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            let output = tokio::process::Command::new("git")
                .args(["checkout", sha])
                .current_dir(dest)
                .output()
                .await?;
            // It's okay if checkout fails for depth=1 clones where HEAD is already the right commit
            if !output.status.success() {
                tracing::warn!(
                    "git checkout {sha} failed (may already be at correct commit): {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }

    async fn get_installation_token(&self, installation_id: i64) -> Result<String> {
        self.get_or_refresh_token(installation_id).await
    }
}
