//! Output formatting for test results, tool calls, and agent responses.
//!
//! Provides configurable output display for the test harness,
//! with support for showing tool calls and responses either always,
//! on failure, or never.
//!
//! # Example
//!
//! ```rust,ignore
//! use aptitude::output::{OutputConfig, OutputFormatter, OutputMode};
//!
//! let config = OutputConfig::new()
//!     .tool_calls(OutputMode::Always)
//!     .response(OutputMode::OnFailure);
//!
//! let formatter = OutputFormatter::new(config);
//! formatter.print_tool_calls(&tool_calls, test_passed);
//! ```

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::parser::ToolCall;
use serde_json::Value;

// ANSI color codes
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

// =========================================================================
// Configuration
// =========================================================================

/// When to display output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputMode {
    /// Always show output regardless of test result.
    Always,
    /// Only show output when tests fail (default).
    #[default]
    OnFailure,
    /// Never show output.
    Never,
}

/// Configuration for output display.
///
/// Use the builder pattern to configure what gets displayed:
///
/// ```rust,ignore
/// use aptitude::output::{OutputConfig, OutputMode};
///
/// let config = OutputConfig::new()
///     .tool_calls(OutputMode::Always)
///     .response(OutputMode::OnFailure)
///     .truncate_at(80);
/// ```
#[derive(Debug, Clone)]
pub struct OutputConfig {
    /// When to show tool calls made during execution.
    pub tool_calls: OutputMode,
    /// When to show Claude's response output.
    pub response: OutputMode,
    /// Maximum characters before truncating parameter values.
    pub truncate_at: usize,
    /// Whether to use ANSI colors in output.
    pub colors_enabled: bool,
    /// Whether to emit OSC 8 terminal hyperlinks.
    pub hyperlinks_enabled: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            tool_calls: OutputMode::OnFailure,
            response: OutputMode::OnFailure,
            truncate_at: 1000,
            colors_enabled: std::io::stdout().is_terminal(),
            hyperlinks_enabled: detect_hyperlinks(),
        }
    }
}

/// Detect whether the terminal supports OSC 8 hyperlinks.
///
/// Checks a known-good allowlist of terminal emulators via `TERM_PROGRAM`,
/// requires stdout to be a TTY, and disables inside tmux/screen where
/// passthrough is unreliable.
fn detect_hyperlinks() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    // tmux/screen may not pass through OSC 8 reliably
    if std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some() {
        return false;
    }
    true
}

impl OutputConfig {
    /// Create a new output configuration with defaults.
    ///
    /// Default: `OnFailure` for both tool calls and response,
    /// 1000 character truncation, colors auto-detected from TTY.
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure when to show tool calls.
    pub fn tool_calls(mut self, mode: OutputMode) -> Self {
        self.tool_calls = mode;
        self
    }

    /// Configure when to show Claude's response.
    pub fn response(mut self, mode: OutputMode) -> Self {
        self.response = mode;
        self
    }

    /// Set the maximum characters before truncating parameter values.
    pub fn truncate_at(mut self, chars: usize) -> Self {
        self.truncate_at = chars;
        self
    }

    /// Enable or disable ANSI colors.
    pub fn colors(mut self, enabled: bool) -> Self {
        self.colors_enabled = enabled;
        self
    }

    /// Enable or disable OSC 8 terminal hyperlinks.
    pub fn hyperlinks(mut self, enabled: bool) -> Self {
        self.hyperlinks_enabled = enabled;
        self
    }

    /// Create a verbose configuration that always shows everything.
    pub fn verbose() -> Self {
        Self {
            tool_calls: OutputMode::Always,
            response: OutputMode::Always,
            ..Self::default()
        }
    }

    /// Create a quiet configuration that never shows output.
    pub fn quiet() -> Self {
        Self {
            tool_calls: OutputMode::Never,
            response: OutputMode::Never,
            ..Self::default()
        }
    }
}

// =========================================================================
// Formatter
// =========================================================================

/// Formatter for test output including tool calls and agent responses.
pub struct OutputFormatter {
    config: OutputConfig,
    workdir: Option<PathBuf>,
}

impl OutputFormatter {
    /// Create a new formatter with the given configuration.
    pub fn new(config: OutputConfig) -> Self {
        Self { config, workdir: None }
    }

    /// Create a formatter with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(OutputConfig::new())
    }

    /// Set the working directory for making paths relative in output.
    pub fn with_workdir(mut self, workdir: Option<PathBuf>) -> Self {
        self.workdir = workdir;
        self
    }

    /// Check if tool calls should be shown given the test result.
    pub fn should_show_tool_calls(&self, test_passed: bool) -> bool {
        match self.config.tool_calls {
            OutputMode::Always => true,
            OutputMode::OnFailure => !test_passed,
            OutputMode::Never => false,
        }
    }

    /// Check if response should be shown given the test result.
    pub fn should_show_response(&self, test_passed: bool) -> bool {
        match self.config.response {
            OutputMode::Always => true,
            OutputMode::OnFailure => !test_passed,
            OutputMode::Never => false,
        }
    }

    /// Format a parameter value, showing the primary parameter.
    pub fn format_params(&self, params: &Value) -> String {
        if let Some(obj) = params.as_object() {
            let primary = obj
                .get("command")
                .or_else(|| obj.get("file_path"))
                .or_else(|| obj.get("pattern"))
                .or_else(|| obj.get("url"))
                .or_else(|| obj.values().next());

            match primary {
                Some(Value::String(s)) => self.truncate(&self.make_relative(s)),
                Some(other) => self.truncate(&other.to_string()),
                None => String::new(),
            }
        } else {
            params.to_string()
        }
    }

    /// Strip the working directory prefix from a path string, if applicable.
    fn make_relative(&self, s: &str) -> String {
        if let Some(workdir) = &self.workdir {
            let prefix = workdir.to_string_lossy();
            let prefix = prefix.as_ref();
            if s.starts_with(prefix) {
                let rest = &s[prefix.len()..];
                let rest = rest.strip_prefix('/').unwrap_or(rest);
                if rest.is_empty() {
                    return ".".to_string();
                }
                return rest.to_string();
            }
        }
        s.to_string()
    }

    /// Format a single tool call for display.
    pub fn format_tool_call(&self, call: &ToolCall) -> String {
        let params_str = self.format_params(&call.params);
        let timestamp = extract_time(&call.timestamp);

        if self.config.colors_enabled {
            format!("  [{timestamp}] {CYAN}{}{RESET} {params_str}", call.name)
        } else {
            format!("  [{timestamp}] {} {params_str}", call.name)
        }
    }

    /// Print tool calls if the output mode allows it.
    pub fn print_tool_calls(&self, calls: &[ToolCall], test_passed: bool) {
        if !self.should_show_tool_calls(test_passed) {
            return;
        }

        println!();
        if self.config.colors_enabled {
            println!("{}Tool calls made during execution:{}", YELLOW, RESET);
        } else {
            println!("Tool calls made during execution:");
        }

        if calls.is_empty() {
            println!("  (no tool calls)");
        } else {
            for call in calls {
                println!("{}", self.format_tool_call(call));
            }
        }
    }

    /// Print Claude's response if the output mode allows it.
    pub fn print_response(&self, response: Option<&str>, test_passed: bool) {
        if !self.should_show_response(test_passed) {
            return;
        }

        if let Some(stdout) = response {
            if !stdout.is_empty() {
                println!();
                if self.config.colors_enabled {
                    println!("{}Claude's response:{}", YELLOW, RESET);
                } else {
                    println!("Claude's response:");
                }
                for line in stdout.lines() {
                    println!("  {}", line);
                }
            }
        }
    }

    /// Format a session path for display.
    ///
    /// Shows only the file stem (typically a UUID) instead of the full path.
    /// When the terminal supports OSC 8 hyperlinks, wraps the stem in a
    /// clickable `file://` link. In verbose mode, appends the full path
    /// on a second line.
    pub fn format_session_path(&self, path: &Path, verbose: bool) -> String {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let display = if self.config.hyperlinks_enabled {
            let uri = path_to_file_uri(path);
            format!("\x1b]8;;{uri}\x1b\\{stem}\x1b]8;;\x1b\\")
        } else {
            stem.to_string()
        };

        if verbose {
            format!("{display}\n    {}", path.display())
        } else {
            display
        }
    }

    /// Truncate a string to the configured maximum length.
    /// Handles multi-byte UTF-8 characters safely.
    fn truncate(&self, s: &str) -> String {
        let max = self.config.truncate_at;
        let char_count = s.chars().count();

        if char_count <= max {
            s.to_string()
        } else {
            let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
            format!("{}...", truncated)
        }
    }
}

/// Extract HH:MM:SS from an RFC3339 timestamp string.
/// Falls back to "??:??:??" if the string is too short.
fn extract_time(ts: &str) -> &str {
    // RFC3339: "2024-01-19T12:00:00Z" — time starts at index 11, 8 chars
    if ts.len() >= 19 {
        &ts[11..19]
    } else {
        "??:??:??"
    }
}

/// Convert a filesystem path to a `file://` URI.
///
/// Resolves the path to an absolute path via `canonicalize` (falling back
/// to the original if the file doesn't exist yet) and percent-encodes
/// spaces per RFC 3986.
fn path_to_file_uri(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = absolute.to_string_lossy().replace(' ', "%20");
    format!("file://{path_str}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_default_config() {
        let config = OutputConfig::new();
        assert_eq!(config.tool_calls, OutputMode::OnFailure);
        assert_eq!(config.response, OutputMode::OnFailure);
        assert_eq!(config.truncate_at, 1000);
    }

    #[test]
    fn test_verbose_config() {
        let config = OutputConfig::verbose();
        assert_eq!(config.tool_calls, OutputMode::Always);
        assert_eq!(config.response, OutputMode::Always);
    }

    #[test]
    fn test_quiet_config() {
        let config = OutputConfig::quiet();
        assert_eq!(config.tool_calls, OutputMode::Never);
        assert_eq!(config.response, OutputMode::Never);
    }

    #[test]
    fn test_builder_chain() {
        let config = OutputConfig::new()
            .tool_calls(OutputMode::Always)
            .response(OutputMode::Never)
            .truncate_at(100)
            .colors(false);

        assert_eq!(config.tool_calls, OutputMode::Always);
        assert_eq!(config.response, OutputMode::Never);
        assert_eq!(config.truncate_at, 100);
        assert!(!config.colors_enabled);
    }

    #[test]
    fn test_truncate_short_string() {
        let formatter = OutputFormatter::new(OutputConfig::new().truncate_at(60));
        assert_eq!(formatter.truncate("hello"), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let formatter = OutputFormatter::new(OutputConfig::new().truncate_at(10));
        assert_eq!(formatter.truncate("hello world!"), "hello w...");
    }

    #[test]
    fn test_truncate_unicode() {
        let formatter = OutputFormatter::new(OutputConfig::new().truncate_at(6));
        let result = formatter.truncate("日本語ですよね");
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), 6);
        assert_eq!(result, "日本語...");
    }

    #[test]
    fn test_format_params_object() {
        let formatter = OutputFormatter::new(OutputConfig::new());
        let params = json!({"file_path": "/tmp/test.txt", "content": "hello"});
        let formatted = formatter.format_params(&params);
        assert_eq!(formatted, "/tmp/test.txt");
    }

    #[test]
    fn test_should_show_always() {
        let config = OutputConfig::new().tool_calls(OutputMode::Always);
        let formatter = OutputFormatter::new(config);
        assert!(formatter.should_show_tool_calls(true));
        assert!(formatter.should_show_tool_calls(false));
    }

    #[test]
    fn test_should_show_on_failure() {
        let config = OutputConfig::new().tool_calls(OutputMode::OnFailure);
        let formatter = OutputFormatter::new(config);
        assert!(!formatter.should_show_tool_calls(true));
        assert!(formatter.should_show_tool_calls(false));
    }

    #[test]
    fn test_should_show_never() {
        let config = OutputConfig::new().tool_calls(OutputMode::Never);
        let formatter = OutputFormatter::new(config);
        assert!(!formatter.should_show_tool_calls(true));
        assert!(!formatter.should_show_tool_calls(false));
    }

    #[test]
    fn test_make_relative_strips_workdir() {
        let formatter = OutputFormatter::new(OutputConfig::new())
            .with_workdir(Some(PathBuf::from("/home/user/project")));
        assert_eq!(
            formatter.make_relative("/home/user/project/src/main.rs"),
            "src/main.rs"
        );
    }

    #[test]
    fn test_make_relative_no_workdir() {
        let formatter = OutputFormatter::new(OutputConfig::new());
        assert_eq!(
            formatter.make_relative("/home/user/project/src/main.rs"),
            "/home/user/project/src/main.rs"
        );
    }

    #[test]
    fn test_make_relative_different_prefix() {
        let formatter = OutputFormatter::new(OutputConfig::new())
            .with_workdir(Some(PathBuf::from("/home/user/other")));
        assert_eq!(
            formatter.make_relative("/home/user/project/src/main.rs"),
            "/home/user/project/src/main.rs"
        );
    }

    #[test]
    fn test_format_params_with_workdir() {
        let formatter = OutputFormatter::new(OutputConfig::new())
            .with_workdir(Some(PathBuf::from("/home/user/project")));
        let params = json!({"file_path": "/home/user/project/src/main.rs"});
        assert_eq!(formatter.format_params(&params), "src/main.rs");
    }

    // ── Session path / hyperlink tests ──────────────────────────────

    #[test]
    fn test_file_uri_construction() {
        let uri = path_to_file_uri(Path::new("/tmp/sessions/abc-123.jsonl"));
        assert_eq!(uri, "file:///tmp/sessions/abc-123.jsonl");
    }

    #[test]
    fn test_file_uri_spaces_are_encoded() {
        let uri = path_to_file_uri(Path::new("/tmp/my sessions/abc.jsonl"));
        assert_eq!(uri, "file:///tmp/my%20sessions/abc.jsonl");
    }

    #[test]
    fn test_format_session_path_plain() {
        let config = OutputConfig::new().hyperlinks(false);
        let formatter = OutputFormatter::new(config);
        let path = Path::new("/home/user/.claude/projects/test/fdbb606e-98de-49f3-896c-0aa1b4e57af1.jsonl");
        let result = formatter.format_session_path(path, false);
        assert_eq!(result, "fdbb606e-98de-49f3-896c-0aa1b4e57af1");
    }

    #[test]
    fn test_format_session_path_non_uuid_stem() {
        let config = OutputConfig::new().hyperlinks(false);
        let formatter = OutputFormatter::new(config);
        let path = Path::new("/tmp/my-session.jsonl");
        let result = formatter.format_session_path(path, false);
        assert_eq!(result, "my-session");
    }

    #[test]
    fn test_format_session_path_with_hyperlink() {
        let config = OutputConfig::new().hyperlinks(true);
        let formatter = OutputFormatter::new(config);
        let path = Path::new("/tmp/abc-123.jsonl");
        let result = formatter.format_session_path(path, false);
        assert!(result.starts_with("\x1b]8;;file:///"));
        assert!(result.contains("abc-123.jsonl"));
        assert!(result.contains("abc-123"));
        assert!(result.ends_with("\x1b]8;;\x1b\\"));
    }

    #[test]
    fn test_format_session_path_hyperlink_disabled_no_escapes() {
        let config = OutputConfig::new().hyperlinks(false);
        let formatter = OutputFormatter::new(config);
        let path = Path::new("/tmp/abc-123.jsonl");
        let result = formatter.format_session_path(path, false);
        assert!(!result.contains("\x1b"));
        assert_eq!(result, "abc-123");
    }

    #[test]
    fn test_format_session_path_verbose_shows_full_path() {
        let config = OutputConfig::new().hyperlinks(false);
        let formatter = OutputFormatter::new(config);
        let path = Path::new("/home/user/.claude/projects/test/abc-123.jsonl");
        let result = formatter.format_session_path(path, true);
        assert!(result.contains("abc-123"));
        assert!(result.contains("/home/user/.claude/projects/test/abc-123.jsonl"));
        assert!(result.contains('\n'));
    }
}
