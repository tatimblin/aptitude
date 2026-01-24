//! Configuration file support for aptitude.
//!
//! This module handles loading and discovering `.aptitude.yaml` configuration files.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Default configuration embedded at compile time.
const DEFAULT_CONFIG_STR: &str = include_str!("../default.aptitude.yaml");

/// Parsed default config, initialized once on first access.
fn default_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_yaml::from_str(DEFAULT_CONFIG_STR)
            .expect("embedded default.aptitude.yaml should be valid YAML")
    })
}

/// Configuration for test discovery.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// Glob pattern for matching test files.
    pub test_pattern: String,

    /// Root directory to start search.
    #[serde(default)]
    pub root: Option<PathBuf>,

    /// Whether to scan directories recursively.
    pub recursive: bool,

    /// Directories to exclude from scanning.
    pub exclude: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        default_config().clone()
    }
}

impl Config {
    /// Discover config by searching from start_dir upward.
    /// Returns (config, config_dir) for root path resolution.
    pub fn discover(start_dir: &Path) -> Option<(Self, PathBuf)> {
        let config_path = find_config_file(start_dir)?;
        let config_dir = config_path.parent()?.to_path_buf();
        let config = load_config(&config_path).ok()?;
        Some((config, config_dir))
    }

    /// Load config from explicit path.
    pub fn load(path: &Path) -> Result<(Self, PathBuf)> {
        let config_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let config = load_config(path)?;
        Ok((config, config_dir))
    }

    /// Merge CLI overrides into this config.
    pub fn with_overrides(mut self, pattern: Option<String>, root: Option<PathBuf>, no_recursive: bool) -> Self {
        if let Some(p) = pattern {
            self.test_pattern = p;
        }
        if let Some(r) = root {
            self.root = Some(r);
        }
        if no_recursive {
            self.recursive = false;
        }
        self
    }

    /// Get the search directory, resolving root relative to config_dir if needed.
    pub fn search_dir(&self, base_dir: &Path, config_dir: Option<&Path>) -> PathBuf {
        match (&self.root, config_dir) {
            (Some(root), Some(dir)) => dir.join(root),
            (Some(root), None) => base_dir.join(root),
            (None, _) => base_dir.to_path_buf(),
        }
    }
}

/// Search for a config file starting from start_dir and walking up to root.
fn find_config_file(start: &Path) -> Option<PathBuf> {
    let mut current = start.canonicalize().ok()?;

    loop {
        let candidate = current.join(".aptitude.yaml");
        if candidate.exists() {
            return Some(candidate);
        }

        if !current.pop() {
            return None;
        }
    }
}

/// Load and parse a config file.
fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;
    let config: Config = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {:?}", path))?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.test_pattern, "*.aptitude.{yaml,yml}");
        assert!(config.recursive);
        assert!(config.exclude.contains(&"target".to_string()));
    }

    #[test]
    fn test_with_overrides() {
        let config = Config::default()
            .with_overrides(Some("*.test.yaml".to_string()), None, true);
        assert_eq!(config.test_pattern, "*.test.yaml");
        assert!(!config.recursive);
    }

    #[test]
    fn test_search_dir_with_root() {
        let mut config = Config::default();
        config.root = Some(PathBuf::from("tests"));

        let base = Path::new("/project");
        let config_dir = Path::new("/project/subdir");

        assert_eq!(config.search_dir(base, Some(config_dir)), PathBuf::from("/project/subdir/tests"));
    }

    #[test]
    fn test_search_dir_without_root() {
        let config = Config::default();
        let base = Path::new("/project/tests");

        assert_eq!(config.search_dir(base, None), PathBuf::from("/project/tests"));
    }
}
