//! Test file discovery using glob patterns and walkdir.

use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::config::Config;

/// Discover test files in a directory according to config.
pub fn discover_tests(dir: &Path, config: &Config) -> Result<Vec<PathBuf>> {
    let mut tests = Vec::new();

    let walker = if config.recursive {
        WalkDir::new(dir)
    } else {
        WalkDir::new(dir).max_depth(1)
    };

    for entry in walker
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path(), &config.exclude))
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && matches_pattern(path, &config.test_pattern) {
            tests.push(path.to_path_buf());
        }
    }

    tests.sort();
    Ok(tests)
}

/// Check if a file name matches the glob pattern (with brace expansion).
fn matches_pattern(path: &Path, pattern: &str) -> bool {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    // Use glob::Pattern::matches, expanding braces manually since glob::Pattern doesn't support them
    for expanded in expand_braces(pattern) {
        if let Ok(pat) = glob::Pattern::new(&expanded) {
            if pat.matches(file_name) {
                return true;
            }
        }
    }
    false
}

/// Expand brace expressions: "*.{yaml,yml}" -> ["*.yaml", "*.yml"]
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(end) = pattern[start..].find('}') else {
        return vec![pattern.to_string()];
    };

    let prefix = &pattern[..start];
    let suffix = &pattern[start + end + 1..];
    let alternatives = &pattern[start + 1..start + end];

    alternatives
        .split(',')
        .flat_map(|alt| expand_braces(&format!("{prefix}{alt}{suffix}")))
        .collect()
}

/// Check if a path contains an excluded directory.
fn is_excluded(path: &Path, excludes: &[String]) -> bool {
    path.components().any(|c| {
        matches!(c, std::path::Component::Normal(name)
            if name.to_str().map_or(false, |s| excludes.iter().any(|e| e == s)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_braces() {
        assert_eq!(expand_braces("*.{yaml,yml}"), vec!["*.yaml", "*.yml"]);
        assert_eq!(expand_braces("*.yaml"), vec!["*.yaml"]);
        assert_eq!(expand_braces("*.{a,b,c}"), vec!["*.a", "*.b", "*.c"]);
    }

    #[test]
    fn test_matches_pattern() {
        assert!(matches_pattern(Path::new("/foo/test.yaml"), "*.{yaml,yml}"));
        assert!(matches_pattern(Path::new("/foo/test.yml"), "*.{yaml,yml}"));
        assert!(!matches_pattern(Path::new("/foo/test.json"), "*.{yaml,yml}"));
        assert!(matches_pattern(Path::new("/foo/my.test.yaml"), "*.test.yaml"));
        assert!(!matches_pattern(Path::new("/foo/test.yaml"), "*.test.yaml"));
    }

    #[test]
    fn test_is_excluded() {
        let excludes = vec!["target".to_string(), "node_modules".to_string()];
        assert!(is_excluded(Path::new("/project/target/debug"), &excludes));
        assert!(is_excluded(Path::new("/project/node_modules/foo"), &excludes));
        assert!(!is_excluded(Path::new("/project/src/main.rs"), &excludes));
    }
}
