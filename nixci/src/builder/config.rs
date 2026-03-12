use anyhow::Result;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct NixCiConfig {
    #[serde(default)]
    pub build: Vec<BuildEntry>,
    #[serde(default)]
    pub action: Vec<ActionEntry>,
    #[serde(default)]
    pub options: BuildOptions,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildEntry {
    pub attr: String,
    pub branches: Option<String>,
    #[serde(default)]
    pub prs: PrMatch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PrMatch {
    Bool(bool),
    Pattern(String),
}

impl Default for PrMatch {
    fn default() -> Self {
        PrMatch::Bool(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionEntry {
    pub name: String,
    pub app: String,
    #[serde(default = "default_on")]
    pub on: String,
    pub branches: Option<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
}

fn default_on() -> String {
    "success".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BuildOptions {
    pub max_jobs: Option<u32>,
    #[serde(default)]
    pub skip_cached: bool,
    pub systems: Option<String>,
}

impl Default for NixCiConfig {
    fn default() -> Self {
        NixCiConfig {
            build: vec![BuildEntry {
                attr: ".#checks".to_string(),
                branches: None,
                prs: PrMatch::Bool(true),
            }],
            action: vec![],
            options: BuildOptions::default(),
        }
    }
}

impl NixCiConfig {
    pub fn parse(content: &str) -> Result<Self> {
        let config: NixCiConfig = toml::from_str(content)?;
        Ok(config)
    }
}

impl BuildEntry {
    /// Check if this build entry matches a branch push event.
    pub fn matches_branch(&self, branch: &str) -> bool {
        match &self.branches {
            None => true,
            Some(pattern) => Regex::new(pattern)
                .map(|re| re.is_match(branch))
                .unwrap_or(false),
        }
    }

    /// Check if this build entry matches a PR event.
    pub fn matches_pr(&self, head_branch: &str) -> bool {
        match &self.prs {
            PrMatch::Bool(b) => *b,
            PrMatch::Pattern(pattern) => Regex::new(pattern)
                .map(|re| re.is_match(head_branch))
                .unwrap_or(false),
        }
    }
}

impl ActionEntry {
    pub fn matches_branch(&self, branch: &str) -> bool {
        match &self.branches {
            None => true,
            Some(pattern) => Regex::new(pattern)
                .map(|re| re.is_match(branch))
                .unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NixCiConfig::default();
        assert_eq!(config.build.len(), 1);
        assert_eq!(config.build[0].attr, ".#checks");
        assert!(config.build[0].matches_branch("main"));
        assert!(config.build[0].matches_pr("feature/foo"));
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[[build]]
attr = ".#checks"
branches = "main|release/.*"

[[build]]
attr = ".#packages.x86_64-linux.default"
branches = "main"
prs = true

[[action]]
name = "push-docker"
app = ".#apps.x86_64-linux.push-docker"
on = "success"
branches = "main"
secrets = ["DOCKER_USER", "DOCKER_PASS"]

[options]
max_jobs = 4
skip_cached = true
"#;
        let config = NixCiConfig::parse(toml).unwrap();
        assert_eq!(config.build.len(), 2);
        assert!(config.build[0].matches_branch("main"));
        assert!(config.build[0].matches_branch("release/1.0"));
        assert!(!config.build[0].matches_branch("feature/foo"));
        assert_eq!(config.action.len(), 1);
        assert_eq!(config.action[0].secrets.len(), 2);
        assert_eq!(config.options.max_jobs, Some(4));
        assert!(config.options.skip_cached);
    }

    #[test]
    fn test_pr_matching() {
        let toml = r#"
[[build]]
attr = ".#checks"
prs = "feat/.*"
"#;
        let config = NixCiConfig::parse(toml).unwrap();
        assert!(config.build[0].matches_pr("feat/new-thing"));
        assert!(!config.build[0].matches_pr("fix/bug"));
    }
}
