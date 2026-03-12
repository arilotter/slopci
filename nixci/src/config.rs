use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub database_url: String,
    pub work_dir: PathBuf,
    pub github: GitHubConfig,
    pub max_concurrent_builds: usize,
    pub secrets_socket: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub app_id: u64,
    pub private_key_path: PathBuf,
    pub webhook_secret: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let listen_addr =
            std::env::var("NIXCI_LISTEN").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let database_url =
            std::env::var("NIXCI_DATABASE_URL").unwrap_or_else(|_| "sqlite:nixci.db".to_string());
        let work_dir = std::env::var("NIXCI_WORK_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/nixci"));
        let max_concurrent_builds: usize = std::env::var("NIXCI_MAX_CONCURRENT_BUILDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);
        let secrets_socket = std::env::var("NIXCI_SECRETS_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/run/nixci-secrets/nixci-secrets.sock"));

        let github = GitHubConfig {
            app_id: std::env::var("NIXCI_GITHUB_APP_ID")
                .context("NIXCI_GITHUB_APP_ID required")?
                .parse()
                .context("NIXCI_GITHUB_APP_ID must be a number")?,
            private_key_path: std::env::var("NIXCI_GITHUB_PRIVATE_KEY")
                .context("NIXCI_GITHUB_PRIVATE_KEY required")?
                .into(),
            webhook_secret: std::env::var("NIXCI_GITHUB_WEBHOOK_SECRET")
                .context("NIXCI_GITHUB_WEBHOOK_SECRET required")?,
        };

        Ok(Config {
            listen_addr,
            database_url,
            work_dir,
            github,
            max_concurrent_builds,
            secrets_socket,
        })
    }
}
