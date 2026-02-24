//! Kiro agent adapter.
//!
//! This adapter integrates with Kiro CLI (`kiro chat --no-interactive`).
//! Unlike Claude which uses JSONL files, Kiro stores session data in a SQLite database.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Agent, ExecutionConfig, RawExecutionResult, ToolNameMapping};
use crate::parser::ToolCall;

// =========================================================================
// Kiro JSON data structures for parsing session logs
// =========================================================================

/// Tool use entry from Kiro's JSON format.
#[derive(Debug, Deserialize)]
struct KiroToolUse {
    name: String,
    args: Value,
}

/// Wrapper for the tool_uses array in Kiro's ToolUse content.
#[derive(Debug, Deserialize)]
struct KiroToolUseWrapper {
    tool_uses: Vec<KiroToolUse>,
}

/// Assistant response - can be ToolUse or other types.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KiroAssistantResponse {
    ToolUse {
        #[serde(rename = "ToolUse")]
        tool_use: KiroToolUseWrapper,
    },
    #[allow(dead_code)]
    Other(Value),
}

/// User message structure to extract timestamp.
#[derive(Debug, Deserialize)]
struct KiroUserMessage {
    timestamp: Option<String>,
}

/// A history entry containing user and assistant messages.
#[derive(Debug, Deserialize)]
struct KiroHistoryEntry {
    user: KiroUserMessage,
    assistant: Option<KiroAssistantResponse>,
}

/// Top-level conversation structure from Kiro's database.
#[derive(Debug, Deserialize)]
struct KiroConversation {
    history: Vec<KiroHistoryEntry>,
}

// =========================================================================
// Helper functions for Kiro database access
// =========================================================================

/// Get the path to Kiro's SQLite database.
///
/// Resolution order:
/// 1. `KIRO_DB_PATH` environment variable (for testing and custom installs)
/// 2. Platform data directory via `dirs::data_dir()`:
///    - macOS: `~/Library/Application Support/kiro-cli/data.sqlite3`
///    - Linux: `$XDG_DATA_HOME/kiro-cli/data.sqlite3` (or `~/.local/share/...`)
///    - Windows: `{FOLDERID_RoamingAppData}/kiro-cli/data.sqlite3`
fn get_kiro_db_path() -> Result<PathBuf> {
    // Allow override via environment variable
    if let Ok(path) = std::env::var("KIRO_DB_PATH") {
        let db_path = PathBuf::from(path);
        if !db_path.exists() {
            return Err(anyhow!(
                "Kiro database not found at {} (from KIRO_DB_PATH).",
                db_path.display()
            ));
        }
        return Ok(db_path);
    }

    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow!("Could not determine platform data directory"))?;

    let db_path = data_dir.join("kiro-cli").join("data.sqlite3");

    if !db_path.exists() {
        return Err(anyhow!(
            "Kiro database not found at {}. Kiro may not be installed or has not been used yet.",
            db_path.display()
        ));
    }

    Ok(db_path)
}

/// Parse tool uses from Kiro's JSON content format.
/// Extracts tool calls from assistant messages containing ToolUse content.
pub(crate) fn parse_kiro_tool_uses(content: &str) -> Result<Vec<ToolCall>> {
    let conversation: KiroConversation =
        serde_json::from_str(content).context("Failed to parse Kiro conversation JSON")?;

    let mut tool_calls = Vec::new();

    for entry in conversation.history {
        // Get timestamp from user message for this exchange (keep full ISO format)
        let timestamp = entry
            .user
            .timestamp
            .clone()
            .unwrap_or_default();

        if let Some(KiroAssistantResponse::ToolUse { tool_use }) = entry.assistant {
            for tu in tool_use.tool_uses {
                tool_calls.push(ToolCall {
                    name: tu.name,
                    params: tu.args,
                    timestamp: timestamp.clone(),
                });
            }
        }
    }

    Ok(tool_calls)
}

/// Query tool calls from the Kiro database for a specific working directory.
/// If start_time_ms is provided, only returns conversations updated after that time.
///
/// **Known limitation (TOCTOU):** The start timestamp is recorded before spawning
/// the Kiro process. If another Kiro session writes to the same working directory
/// between the timestamp snapshot and execution, its data may be included.
/// A session-ID–based approach would eliminate this race but requires upstream
/// Kiro support.
fn query_tool_calls(db_path: &Path, working_dir: &Path, start_time_ms: Option<u64>) -> Result<Vec<ToolCall>> {
    let conn = Connection::open(db_path).context("Failed to open Kiro database")?;

    let working_dir_str = working_dir.to_string_lossy();

    let mut tool_calls = Vec::new();

    // Use 0 as the default start time to match all conversations
    let start_ms = start_time_ms.unwrap_or(0) as i64;

    let mut stmt = conn
        .prepare(
            "SELECT value FROM conversations_v2 WHERE key = ?1 AND updated_at >= ?2 ORDER BY updated_at DESC LIMIT 1",
        )
        .context("Failed to prepare SQL query")?;

    let rows: Vec<String> = stmt
        .query_map(rusqlite::params![&working_dir_str, start_ms], |row| {
            row.get::<_, String>(0)
        })
        .context("Failed to execute SQL query")?
        .filter_map(|r| match r {
            Ok(val) => Some(val),
            Err(e) => {
                eprintln!("Warning: Failed to read database row: {}", e);
                None
            }
        })
        .collect();

    for content in rows {
        match parse_kiro_tool_uses(&content) {
            Ok(calls) => tool_calls.extend(calls),
            Err(e) => {
                // Log parsing errors but continue with other conversations
                eprintln!("Warning: Failed to parse conversation content: {}", e);
            }
        }
    }

    Ok(tool_calls)
}

/// Agent-specific context for Kiro session recovery.
///
/// Stored in `RawExecutionResult::agent_context` by `execute()` and
/// downcast in `parse_session()` to query the correct conversation.
struct KiroSessionContext {
    working_dir: PathBuf,
    start_time_ms: u64,
}

/// Kiro agent adapter.
pub struct KiroAdapter {
    mapping: ToolNameMapping,
}

impl KiroAdapter {
    pub fn new() -> Self {
        // Kiro tool name mappings to canonical names
        let mut mapping = ToolNameMapping::new();
        mapping.add("fs_read", "Read");
        mapping.add("execute_bash", "Bash");
        mapping.add("fs_write", "Write");
        mapping.add("fs_edit", "Edit");
        mapping.add("glob", "Glob");
        mapping.add("grep", "Grep");
        Self { mapping }
    }
}

impl Default for KiroAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current time in milliseconds since Unix epoch.
fn current_time_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[async_trait]
impl Agent for KiroAdapter {
    fn name(&self) -> &'static str {
        "kiro"
    }

    fn execute(&self, prompt: &str, config: &ExecutionConfig) -> Result<RawExecutionResult> {
        // Record start time for filtering database queries
        let start_time_ms = current_time_ms();

        let mut cmd = Command::new("kiro-cli");
        cmd.arg("chat").arg("--no-interactive");

        // Pass extra args from config
        for arg in &config.extra_args {
            cmd.arg(arg);
        }

        // Set working directory if provided, canonicalize for database matching
        let working_dir = if let Some(dir) = &config.working_dir {
            cmd.current_dir(dir);
            // Canonicalize to match how Kiro stores paths in the database
            dir.canonicalize().ok().or_else(|| Some(dir.clone()))
        } else {
            // Use current directory as working_dir for database queries
            std::env::current_dir().ok().and_then(|d| d.canonicalize().ok())
        };

        // Set up stdin pipe for prompt
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn kiro command")?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write prompt to kiro stdin")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait for kiro command")?;

        // Capture stdout
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stdout = if stdout.is_empty() { None } else { Some(stdout) };

        let agent_context: Option<Box<dyn std::any::Any + Send>> = working_dir.map(|dir| {
            Box::new(KiroSessionContext {
                working_dir: dir,
                start_time_ms,
            }) as Box<dyn std::any::Any + Send>
        });

        Ok(RawExecutionResult {
            session_log_path: None, // Kiro uses SQLite database, not log files
            stdout,
            agent_context,
        })
    }

    fn parse_session(&self, result: &RawExecutionResult) -> Result<Vec<ToolCall>> {
        let db_path = get_kiro_db_path()?;

        let ctx = result
            .agent_context
            .as_ref()
            .and_then(|c| c.downcast_ref::<KiroSessionContext>())
            .ok_or_else(|| {
                anyhow!("No Kiro session context in execution result - cannot query database")
            })?;

        query_tool_calls(&db_path, &ctx.working_dir, Some(ctx.start_time_ms))
    }

    fn tool_mapping(&self) -> &ToolNameMapping {
        &self.mapping
    }

    fn is_available(&self) -> bool {
        Command::new("kiro-cli")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn grade(&self, prompt: &str, model: Option<&str>) -> Result<String> {
        let mut cmd = Command::new("kiro-cli");
        cmd.arg("chat").arg("--no-interactive");

        // Pass model override if provided
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        // Set up stdin pipe for prompt
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .context("Failed to spawn kiro command for grading")?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write prompt to kiro stdin")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait for kiro command")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if stdout.trim().is_empty() {
            anyhow::bail!("Grading agent returned empty response");
        }

        Ok(stdout)
    }

}


#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Arbitrary generator for valid tool names (non-empty alphanumeric with underscores)
    fn arb_tool_name() -> impl Strategy<Value = String> {
        "[a-zA-Z][a-zA-Z0-9_]{0,30}".prop_map(|s| s)
    }

    /// Arbitrary generator for JSON args (simple object with string values)
    fn arb_args() -> impl Strategy<Value = Value> {
        prop::collection::hash_map("[a-z_]{1,10}", "[a-zA-Z0-9_./]{0,50}", 0..5)
            .prop_map(|map| {
                let obj: serde_json::Map<String, Value> = map
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                Value::Object(obj)
            })
    }

    /// Arbitrary generator for a KiroToolUse-like structure
    fn arb_kiro_tool_use() -> impl Strategy<Value = (String, String, Value)> {
        (any::<u64>(), arb_tool_name(), arb_args())
            .prop_map(|(id, name, args)| (format!("tool_{}", id), name, args))
    }

    /// Serialize tool uses to Kiro's JSON format (history-based structure)
    fn serialize_to_kiro_format(tool_uses: &[(String, String, Value)]) -> String {
        let tool_uses_json: Vec<Value> = tool_uses
            .iter()
            .map(|(id, name, args)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "orig_name": name,
                    "args": args,
                    "orig_args": args
                })
            })
            .collect();

        serde_json::json!({
            "history": [
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-msg-id",
                            "content": "test content",
                            "tool_uses": tool_uses_json
                        }
                    }
                }
            ]
        })
        .to_string()
    }

    // Feature: kiro-agent-support, Property 1: Tool call parsing produces valid structs
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property 1: Tool Call Parsing Produces Valid Structs
        /// For any valid Kiro database record containing tool uses, parsing should produce
        /// ToolCall structs where each struct contains a non-empty name, valid JSON params,
        /// and a timestamp string.
        /// Validates: Requirements 4.2
        #[test]
        fn parsed_tool_calls_have_required_fields(
            tool_uses in prop::collection::vec(arb_kiro_tool_use(), 1..10)
        ) {
            let json = serialize_to_kiro_format(&tool_uses);
            let parsed = parse_kiro_tool_uses(&json).expect("Parsing should succeed for valid JSON");

            // Verify we got the expected number of tool calls
            prop_assert_eq!(parsed.len(), tool_uses.len());

            // Verify each parsed tool call has valid fields
            for (i, call) in parsed.iter().enumerate() {
                // Name must be non-empty
                prop_assert!(
                    !call.name.is_empty(),
                    "Tool call {} has empty name", i
                );

                // Name must match the input
                prop_assert_eq!(
                    &call.name, &tool_uses[i].1,
                    "Tool call {} name mismatch", i
                );

                // Params must be valid JSON (object or null)
                prop_assert!(
                    call.params.is_object() || call.params.is_null(),
                    "Tool call {} params is not an object or null: {:?}", i, call.params
                );

                // Params must match the input args
                prop_assert_eq!(
                    &call.params, &tool_uses[i].2,
                    "Tool call {} params mismatch", i
                );

                // Timestamp is a string (can be empty for Kiro)
                // This just verifies the field exists and is accessible
                let _ = &call.timestamp;
            }
        }

        /// Property 1 variant: Empty tool_uses array produces empty Vec
        #[test]
        fn empty_tool_uses_produces_empty_vec(_seed in any::<u64>()) {
            let json = serialize_to_kiro_format(&[]);
            let parsed = parse_kiro_tool_uses(&json).expect("Parsing should succeed");
            prop_assert!(parsed.is_empty(), "Expected empty Vec for empty tool_uses");
        }
    }

    #[test]
    fn test_parse_kiro_tool_uses_basic() {
        let json = r#"{
            "history": [
                {
                    "user": {"timestamp": "2026-02-23T21:20:31.146289-08:00"},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id",
                            "content": "test",
                            "tool_uses": [
                                {
                                    "id": "tool_123",
                                    "name": "readFile",
                                    "orig_name": "readFile",
                                    "args": {"path": "src/main.rs"},
                                    "orig_args": {"path": "src/main.rs"}
                                }
                            ]
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "readFile");
        assert_eq!(calls[0].params["path"], "src/main.rs");
        assert_eq!(calls[0].timestamp, "2026-02-23T21:20:31.146289-08:00");
    }

    #[test]
    fn test_parse_kiro_tool_uses_extracts_from_history() {
        let json = r#"{
            "history": [
                {
                    "user": {"content": {"Prompt": {"prompt": "Hello"}}},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id",
                            "content": "test",
                            "tool_uses": [
                                {
                                    "id": "tool_456",
                                    "name": "writeFile",
                                    "args": {"path": "test.txt", "content": "hello"}
                                }
                            ]
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "writeFile");
    }

    #[test]
    fn test_parse_kiro_tool_uses_malformed_json() {
        let json = "not valid json";
        let result = parse_kiro_tool_uses(json);
        assert!(result.is_err());
    }

    // =========================================================================
    // Task 9.2: KiroAdapter basics tests
    // Requirements: 2.2, 5.1
    // =========================================================================

    #[test]
    fn test_kiro_adapter_name() {
        let adapter = KiroAdapter::new();
        assert_eq!(adapter.name(), "kiro");
    }

    #[test]
    fn test_kiro_adapter_tool_mapping_maps_known_tools() {
        let adapter = KiroAdapter::new();
        let mapping = adapter.tool_mapping();

        // Verify configured mappings produce canonical names
        assert_eq!(mapping.to_canonical("fs_read"), "Read");
        assert_eq!(mapping.to_canonical("execute_bash"), "Bash");
        assert_eq!(mapping.to_canonical("fs_write"), "Write");
        assert_eq!(mapping.to_canonical("fs_edit"), "Edit");
        assert_eq!(mapping.to_canonical("glob"), "Glob");
        assert_eq!(mapping.to_canonical("grep"), "Grep");

        // Unmapped names pass through unchanged
        assert_eq!(mapping.to_canonical("unknown_tool"), "unknown_tool");
    }

    #[test]
    fn test_kiro_adapter_default_impl() {
        let adapter = KiroAdapter::default();
        assert_eq!(adapter.name(), "kiro");
    }

    // =========================================================================
    // Task 9.3: JSON parsing edge cases tests
    // Requirements: 4.4
    // =========================================================================

    #[test]
    fn test_parse_empty_tool_uses_array() {
        let json = r#"{
            "history": [
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id",
                            "content": "test",
                            "tool_uses": []
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_missing_optional_fields() {
        // Test with missing orig_name and orig_args (optional fields)
        let json = r#"{
            "history": [
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id",
                            "content": "test",
                            "tool_uses": [
                                {
                                    "id": "tool_789",
                                    "name": "listFiles",
                                    "args": {"directory": "/tmp"}
                                }
                            ]
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "listFiles");
        assert_eq!(calls[0].params["directory"], "/tmp");
    }

    #[test]
    fn test_parse_malformed_json_returns_error() {
        let json = "{ invalid json }";
        let result = parse_kiro_tool_uses(json);
        assert!(result.is_err());
        
        // Verify error message contains context
        let err = result.unwrap_err();
        assert!(err.to_string().contains("parse") || err.to_string().contains("JSON"));
    }

    #[test]
    fn test_parse_wrong_structure_returns_error() {
        // Valid JSON but wrong structure
        let json = r#"{"foo": "bar"}"#;
        let result = parse_kiro_tool_uses(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_history_array() {
        let json = r#"{"history": []}"#;
        let calls = parse_kiro_tool_uses(json).unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_multiple_tool_uses_in_single_entry() {
        let json = r#"{
            "history": [
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id",
                            "content": "test",
                            "tool_uses": [
                                {
                                    "id": "tool_1",
                                    "name": "readFile",
                                    "args": {"path": "a.txt"}
                                },
                                {
                                    "id": "tool_2",
                                    "name": "writeFile",
                                    "args": {"path": "b.txt", "content": "hello"}
                                }
                            ]
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "readFile");
        assert_eq!(calls[1].name, "writeFile");
    }

    #[test]
    fn test_parse_multiple_history_entries() {
        let json = r#"{
            "history": [
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id-1",
                            "content": "test",
                            "tool_uses": [
                                {"id": "t1", "name": "tool1", "args": {}}
                            ]
                        }
                    }
                },
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id-2",
                            "content": "test",
                            "tool_uses": [
                                {"id": "t2", "name": "tool2", "args": {}}
                            ]
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "tool1");
        assert_eq!(calls[1].name, "tool2");
    }

    #[test]
    fn test_parse_null_args() {
        let json = r#"{
            "history": [
                {
                    "user": {},
                    "assistant": {
                        "ToolUse": {
                            "message_id": "test-id",
                            "content": "test",
                            "tool_uses": [
                                {
                                    "id": "tool_null",
                                    "name": "noArgsCommand",
                                    "args": null
                                }
                            ]
                        }
                    }
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "noArgsCommand");
        assert!(calls[0].params.is_null());
    }

    #[test]
    fn test_parse_entry_without_assistant() {
        // Entry with null assistant should be skipped
        let json = r#"{
            "history": [
                {
                    "user": {},
                    "assistant": null
                }
            ]
        }"#;

        let calls = parse_kiro_tool_uses(json).unwrap();
        assert!(calls.is_empty());
    }
}
