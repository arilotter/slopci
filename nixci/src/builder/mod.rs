pub mod config;
pub mod runner;

use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Semaphore};

use crate::forge::{CommitStatus, CommitStatusState, ForgeBackend, ForgeEvent};
use crate::models::Repo;

#[derive(Debug, Clone)]
pub struct LogLine {
    pub build_id: i64,
    pub seq: i64,
    pub stream: String,
    pub line: String,
}

pub struct BuildOrchestrator {
    pool: SqlitePool,
    forge: Arc<dyn ForgeBackend>,
    semaphore: Arc<Semaphore>,
    work_dir: PathBuf,
    log_senders: Arc<Mutex<HashMap<i64, broadcast::Sender<LogLine>>>>,
}

impl BuildOrchestrator {
    pub fn new(
        pool: SqlitePool,
        forge: Arc<dyn ForgeBackend>,
        max_concurrent: usize,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            pool,
            forge,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            work_dir,
            log_senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn subscribe_logs(&self, build_id: i64) -> broadcast::Receiver<LogLine> {
        let mut senders = self.log_senders.lock().await;
        let sender = senders
            .entry(build_id)
            .or_insert_with(|| broadcast::channel(1024).0);
        sender.subscribe()
    }

    pub async fn handle_event(&self, event: ForgeEvent) -> Result<()> {
        let (repo_full_name, sha, branch, pr_number, _installation_id) = match &event {
            ForgeEvent::Push {
                repo,
                sha,
                branch,
                installation_id,
            } => (repo.clone(), sha.clone(), Some(branch.clone()), None, *installation_id),
            ForgeEvent::PullRequest {
                repo,
                head_sha,
                pr_number,
                base_branch,
                installation_id,
            } => (
                repo.clone(),
                head_sha.clone(),
                Some(base_branch.clone()),
                Some(*pr_number as i64),
                *installation_id,
            ),
        };

        // Look up repo in DB
        let repo: Option<Repo> =
            sqlx::query_as("SELECT * FROM repos WHERE full_name = ? AND webhook_active = 1")
                .bind(&repo_full_name)
                .fetch_optional(&self.pool)
                .await?;

        let repo = match repo {
            Some(r) => r,
            None => {
                tracing::info!("Ignoring event for unregistered repo: {repo_full_name}");
                return Ok(());
            }
        };

        // Fetch repo to work directory
        let work_path = self.work_dir.join(format!("{}/{}", repo.full_name, &sha[..8]));
        self.forge.fetch_repo(&repo, &sha, &work_path).await?;

        // Read .nixci.toml
        let config_path = work_path.join(".nixci.toml");
        let ci_config = if config_path.exists() {
            let content = tokio::fs::read_to_string(&config_path).await?;
            config::NixCiConfig::parse(&content)?
        } else {
            config::NixCiConfig::default()
        };

        // Match build entries
        for entry in &ci_config.build {
            let should_build = match &event {
                ForgeEvent::Push { branch, .. } => entry.matches_branch(branch),
                ForgeEvent::PullRequest { base_branch, .. } => entry.matches_pr(base_branch),
            };

            if !should_build {
                continue;
            }

            // Create build record
            let build_id: i64 = sqlx::query_scalar(
                "INSERT INTO builds (repo_id, commit_sha, branch, pr_number, status, flake_attr, triggered_by) VALUES (?, ?, ?, ?, 'pending', ?, 'webhook') RETURNING id",
            )
            .bind(repo.id)
            .bind(&sha)
            .bind(branch.as_deref())
            .bind(pr_number)
            .bind(&entry.attr)
            .fetch_one(&self.pool)
            .await?;

            // Set pending status on GitHub
            let _ = self
                .forge
                .set_commit_status(
                    &repo,
                    &sha,
                    &CommitStatus {
                        state: CommitStatusState::Pending,
                        context: format!("nixci: {}", entry.attr),
                        description: "Build queued".to_string(),
                        target_url: None,
                    },
                )
                .await;

            // Spawn build task
            let pool = self.pool.clone();
            let forge = self.forge.clone();
            let semaphore = self.semaphore.clone();
            let work_path = work_path.clone();
            let attr = entry.attr.clone();
            let sha = sha.clone();
            let repo = repo.clone();
            let log_senders = self.log_senders.clone();
            let options = ci_config.options.clone();

            tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("semaphore closed");

                // Create broadcast channel for this build
                let (tx, _) = broadcast::channel(1024);
                {
                    let mut senders = log_senders.lock().await;
                    senders.insert(build_id, tx.clone());
                }

                // Update status to running
                let _ = sqlx::query(
                    "UPDATE builds SET status = 'running', started_at = datetime('now') WHERE id = ?",
                )
                .bind(build_id)
                .execute(&pool)
                .await;

                let _ = forge
                    .set_commit_status(
                        &repo,
                        &sha,
                        &CommitStatus {
                            state: CommitStatusState::Pending,
                            context: format!("nixci: {}", attr),
                            description: "Build running".to_string(),
                            target_url: None,
                        },
                    )
                    .await;

                // Run the build
                let result =
                    runner::run_build(build_id, &work_path, &attr, &options, &pool, &tx).await;

                let (status, exit_code) = match result {
                    Ok(code) => {
                        if code == 0 {
                            ("success", code)
                        } else {
                            ("failure", code)
                        }
                    }
                    Err(e) => {
                        tracing::error!("Build {build_id} error: {e}");
                        ("failure", -1)
                    }
                };

                // Update build record
                let _ = sqlx::query(
                    "UPDATE builds SET status = ?, exit_code = ?, finished_at = datetime('now') WHERE id = ?",
                )
                .bind(status)
                .bind(exit_code)
                .bind(build_id)
                .execute(&pool)
                .await;

                // Report final status to GitHub
                let commit_status = CommitStatus {
                    state: if status == "success" {
                        CommitStatusState::Success
                    } else {
                        CommitStatusState::Failure
                    },
                    context: format!("nixci: {}", attr),
                    description: format!("Build {status} (exit code {exit_code})"),
                    target_url: None,
                };
                let _ = forge.set_commit_status(&repo, &sha, &commit_status).await;

                // Clean up broadcast sender
                {
                    let mut senders = log_senders.lock().await;
                    senders.remove(&build_id);
                }
            });
        }

        Ok(())
    }

    pub async fn retry_build(&self, build_id: i64) -> Result<i64> {
        let build: crate::models::Build =
            sqlx::query_as("SELECT * FROM builds WHERE id = ?")
                .bind(build_id)
                .fetch_one(&self.pool)
                .await?;

        let repo: Repo = sqlx::query_as("SELECT * FROM repos WHERE id = ?")
            .bind(build.repo_id)
            .fetch_one(&self.pool)
            .await?;

        // Create a new build record
        let new_build_id: i64 = sqlx::query_scalar(
            "INSERT INTO builds (repo_id, commit_sha, branch, pr_number, status, flake_attr, triggered_by) VALUES (?, ?, ?, ?, 'pending', ?, 'manual') RETURNING id",
        )
        .bind(build.repo_id)
        .bind(&build.commit_sha)
        .bind(&build.branch)
        .bind(build.pr_number)
        .bind(&build.flake_attr)
        .fetch_one(&self.pool)
        .await?;

        // Fetch and run
        let work_path = self
            .work_dir
            .join(format!("{}/{}", repo.full_name, &build.commit_sha[..8]));
        self.forge
            .fetch_repo(&repo, &build.commit_sha, &work_path)
            .await?;

        // Read config
        let config_path = work_path.join(".nixci.toml");
        let ci_config = if config_path.exists() {
            let content = tokio::fs::read_to_string(&config_path).await?;
            config::NixCiConfig::parse(&content)?
        } else {
            config::NixCiConfig::default()
        };

        let pool = self.pool.clone();
        let forge = self.forge.clone();
        let semaphore = self.semaphore.clone();
        let log_senders = self.log_senders.clone();
        let attr = build.flake_attr.clone();
        let sha = build.commit_sha.clone();
        let options = ci_config.options.clone();

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.expect("semaphore closed");

            let (tx, _) = broadcast::channel(1024);
            {
                let mut senders = log_senders.lock().await;
                senders.insert(new_build_id, tx.clone());
            }

            let _ = sqlx::query(
                "UPDATE builds SET status = 'running', started_at = datetime('now') WHERE id = ?",
            )
            .bind(new_build_id)
            .execute(&pool)
            .await;

            let result =
                runner::run_build(new_build_id, &work_path, &attr, &options, &pool, &tx).await;

            let (status, exit_code) = match result {
                Ok(code) => {
                    if code == 0 {
                        ("success", code)
                    } else {
                        ("failure", code)
                    }
                }
                Err(e) => {
                    tracing::error!("Build {new_build_id} error: {e}");
                    ("failure", -1)
                }
            };

            let _ = sqlx::query(
                "UPDATE builds SET status = ?, exit_code = ?, finished_at = datetime('now') WHERE id = ?",
            )
            .bind(status)
            .bind(exit_code)
            .bind(new_build_id)
            .execute(&pool)
            .await;

            let commit_status = CommitStatus {
                state: if status == "success" {
                    CommitStatusState::Success
                } else {
                    CommitStatusState::Failure
                },
                context: format!("nixci: {}", attr),
                description: format!("Build {status} (exit code {exit_code})"),
                target_url: None,
            };
            let _ = forge.set_commit_status(&repo, &sha, &commit_status).await;

            {
                let mut senders = log_senders.lock().await;
                senders.remove(&new_build_id);
            }
        });

        Ok(new_build_id)
    }
}
