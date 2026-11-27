//! Parameter matching utilities for tool call assertions.
//!
//! This module provides utilities for matching expected parameters against
//! actual tool call parameters using regex, glob, or exact-match patterns.

use regex::Regex;
use std::collections::HashMap;

/// Match expected parameters against actual tool call parameters.
///
/// Each pattern is tried in this order:
///
/// 1. **Regex** (unanchored): if the pattern compiles as a regex, it matches when
///    the regex is found anywhere in the value. Add `^`/`$` for strict matching.
/// 2. **Glob** (anchored): if the pattern is not a valid regex (e.g. `*.env`,
///    `*amazon.com/*`), it is tried as a glob against the whole value. `*` matches
///    any characters *including* `/`, `?` matches one character, `[...]` is a
///    character class.
/// 3. **Exact**: if the pattern is neither a valid regex nor a valid glob, the value
///    must equal the pattern exactly.
///
/// Note: a pattern such as `src/*.rs` is a *valid* regex and is therefore treated as
/// a regex, not a glob. Globs only take effect when the pattern fails to compile as a
/// regex (most commonly because it starts with `*` or `?`).
///
/// # Arguments
///
/// * `expected` - Map of parameter names to expected patterns
/// * `actual` - The actual JSON value containing the tool call parameters
///
/// # Returns
///
/// `true` if all expected parameters match their actual values.
///
/// # Example
///
/// ```rust
/// use aptitude::params_match;
/// use std::collections::HashMap;
/// use serde_json::json;
///
/// let mut expected = HashMap::new();
/// expected.insert("file_path".to_string(), r".*\.txt".to_string());
///
/// assert!(params_match(&expected, &json!({"file_path": "test.txt"})));
/// assert!(!params_match(&expected, &json!({"file_path": "test.rs"})));
///
/// // Globs work too (invalid as regex, so they are matched as anchored globs)
/// let mut glob = HashMap::new();
/// glob.insert("file_path".to_string(), "*.env".to_string());
/// assert!(params_match(&glob, &json!({"file_path": "/Users/x/proj/.env"})));
/// ```
pub fn params_match(expected: &HashMap<String, String>, actual: &serde_json::Value) -> bool {
    for (key, pattern) in expected {
        let actual_value = actual.get(key);

        let actual_str = match actual_value {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => return false,
        };

        if !value_matches(pattern, &actual_str) {
            return false;
        }
    }

    true
}

/// Match a single value: regex (unanchored) first, then glob (anchored), then exact.
fn value_matches(pattern: &str, actual: &str) -> bool {
    if let Ok(re) = Regex::new(pattern) {
        return re.is_match(actual);
    }
    if let Ok(glob) = glob::Pattern::new(pattern) {
        // `*` crosses `/` and leading dots are not special, so `*.env`
        // matches absolute paths like `/Users/x/proj/.env`.
        let opts = glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };
        return glob.matches_with(actual, opts);
    }
    actual == pattern
}

/// Create a parameter map from key-value pairs.
///
/// This is a convenience macro for creating parameter expectations.
///
/// # Example
///
/// ```rust,ignore
/// use aptitude::params;
///
/// let params = params! {
///     "file_path" => "*.txt",
///     "content" => "hello"
/// };
/// ```
#[macro_export]
macro_rules! params {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut map = std::collections::HashMap::new();
        $(
            map.insert($key.to_string(), $value.to_string());
        )*
        map
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_regex_file_extension() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), r".*\.env$".to_string());

        assert!(params_match(&params, &json!({"file_path": ".env"})));
        assert!(params_match(&params, &json!({"file_path": "test.env"})));
        assert!(!params_match(&params, &json!({"file_path": "test.txt"})));
    }

    #[test]
    fn test_regex_path_matching() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), r"(.*/)config\.json$".to_string());

        assert!(params_match(
            &params,
            &json!({"file_path": "src/config.json"})
        ));
        assert!(params_match(&params, &json!({"file_path": "/config.json"})));
        assert!(params_match(
            &params,
            &json!({"file_path": "/a/b/c/config.json"})
        ));
    }

    #[test]
    fn test_regex_alternation() {
        let mut params = HashMap::new();
        params.insert("command".to_string(), r"^npm (install|i)$".to_string());

        assert!(params_match(&params, &json!({"command": "npm install"})));
        assert!(params_match(&params, &json!({"command": "npm i"})));
        assert!(!params_match(&params, &json!({"command": "npm run"})));
    }

    #[test]
    fn test_regex_partial_match() {
        let mut params = HashMap::new();
        params.insert("command".to_string(), r"node run\.js".to_string());

        assert!(params_match(&params, &json!({"command": "node run.js"})));
        assert!(params_match(
            &params,
            &json!({"command": "cd /some/path && node run.js --flag"})
        ));
        assert!(!params_match(&params, &json!({"command": "node other.js"})));
    }

    #[test]
    fn test_exact_matching() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), "/tmp/test.txt".to_string());

        assert!(params_match(
            &params,
            &json!({"file_path": "/tmp/test.txt"})
        ));
        // Note: without anchors, regex matches substrings
        assert!(params_match(
            &params,
            &json!({"file_path": "/tmp/test.txt.bak"})
        ));
    }

    #[test]
    fn test_exact_matching_with_anchors() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), r"^/tmp/test\.txt$".to_string());

        assert!(params_match(
            &params,
            &json!({"file_path": "/tmp/test.txt"})
        ));
        assert!(!params_match(
            &params,
            &json!({"file_path": "/tmp/test.txt.bak"})
        ));
    }

    #[test]
    fn test_missing_key() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), "test.txt".to_string());

        assert!(!params_match(&params, &json!({"other_key": "test.txt"})));
    }

    #[test]
    fn test_multiple_params() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), r".*\.txt".to_string());
        params.insert("content".to_string(), "hello.*".to_string());

        assert!(params_match(
            &params,
            &json!({"file_path": "test.txt", "content": "hello world"})
        ));
        assert!(!params_match(
            &params,
            &json!({"file_path": "test.txt", "content": "goodbye"})
        ));
    }

    #[test]
    fn test_non_string_values() {
        let mut params = HashMap::new();
        params.insert("count".to_string(), "42".to_string());

        assert!(params_match(&params, &json!({"count": 42})));
    }

    #[test]
    fn test_glob_star_crosses_slash() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), "*.env".to_string());

        assert!(params_match(&params, &json!({"file_path": "/Users/x/proj/.env"})));
        assert!(params_match(&params, &json!({"file_path": ".env"})));
        assert!(params_match(&params, &json!({"file_path": "/a/prod.env"})));
        // Glob is anchored: `.env.example` does not end in `.env`
        assert!(!params_match(&params, &json!({"file_path": "/a/.env.example"})));
        assert!(!params_match(&params, &json!({"file_path": "/a/main.rs"})));
    }

    #[test]
    fn test_glob_url() {
        let mut params = HashMap::new();
        params.insert("url".to_string(), "*amazon.com/*".to_string());

        assert!(params_match(&params, &json!({"url": "https://aws.amazon.com/health"})));
        assert!(!params_match(&params, &json!({"url": "https://example.com/health"})));
    }

    #[test]
    fn test_glob_question_mark() {
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), "?.txt".to_string());

        assert!(params_match(&params, &json!({"file_path": "a.txt"})));
        assert!(!params_match(&params, &json!({"file_path": "ab.txt"})));
    }

    #[test]
    fn test_valid_regex_is_not_treated_as_glob() {
        // `src/*.rs` compiles as a regex (`/*` = zero or more slashes, `.` = any char),
        // so it is matched as an unanchored regex, not a glob.
        let mut params = HashMap::new();
        params.insert("file_path".to_string(), "src/*.rs".to_string());

        assert!(params_match(&params, &json!({"file_path": "srcxrs"})));
        // Surprising but intended: as a regex it does NOT match `src/main.rs`.
        assert!(!params_match(&params, &json!({"file_path": "src/main.rs"})));
    }

    #[test]
    fn test_exact_fallback_when_neither_regex_nor_glob() {
        // Invalid regex (leading `*`) and invalid glob (`**` not a whole path component)
        let mut params = HashMap::new();
        params.insert("command".to_string(), "**foo".to_string());

        assert!(params_match(&params, &json!({"command": "**foo"})));
        assert!(!params_match(&params, &json!({"command": "xfoo"})));
    }

    #[test]
    fn test_params_macro() {
        let params = params! {
            "file_path" => "test.txt",
            "content" => "hello"
        };

        assert_eq!(params.get("file_path"), Some(&"test.txt".to_string()));
        assert_eq!(params.get("content"), Some(&"hello".to_string()));
    }
}
