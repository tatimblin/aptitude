//! Streaming tool call support for real-time observation during agent execution.
//!
//! This module provides a polling-based file tail that emits tool calls through
//! an `mpsc::channel` as Claude writes them to the session log, enabling
//! real-time observation during execution.
//!
//! # Example
//!
//! ```rust,ignore
//! use aptitude::streaming::execute_streaming;
//! use aptitude::agents::ExecutionConfig;
//!
//! let config = ExecutionConfig::new();
//! let handle = execute_streaming("List files", &config)?;
//!
//! for event in &handle.receiver {
//!     match event {
//!         StreamEvent::ToolCall(tc) => println!("Tool: {}", tc.name),
//!         StreamEvent::SessionDetected(path) => println!("Session: {:?}", path),
//!         StreamEvent::Error(msg) => eprintln!("Error: {}", msg),
//!     }
//! }
//!
//! let raw_result = handle.wait()?;
//! ```

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::agents::{
    build_claude_command, ExecutionConfig, RawExecutionResult, SessionLocator, SESSION_FILE_GRACE,
};
use crate::parser::{parse_line_internal, ToolCall};

/// Events emitted during streaming execution.
#[derive(Debug)]
pub enum StreamEvent {
    /// A parsed tool call from the session log.
    ToolCall(ToolCall),
    /// The session log file was detected at this path.
    SessionDetected(PathBuf),
    /// A non-fatal streaming error.
    Error(String),
}

/// Handle to a streaming execution.
///
/// Provides access to the event receiver and methods to wait for completion.
pub struct StreamHandle {
    /// Receiver for streaming events. Iterate over this to get live tool calls.
    pub receiver: mpsc::Receiver<StreamEvent>,
    join_handle: JoinHandle<Result<RawExecutionResult>>,
}

impl StreamHandle {
    /// Block until the agent process completes and return the raw result.
    pub fn wait(self) -> Result<RawExecutionResult> {
        // Drop receiver so that if the orchestrator is blocked on send, it unblocks
        drop(self.receiver);
        self.join_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Orchestrator thread panicked"))?
    }

    /// Drain all remaining events from the receiver and wait for completion.
    ///
    /// Returns all tool calls received during streaming along with the raw result.
    pub fn collect_all(self) -> Result<(Vec<ToolCall>, RawExecutionResult)> {
        let mut tool_calls = Vec::new();
        for event in &self.receiver {
            if let StreamEvent::ToolCall(tc) = event {
                tool_calls.push(tc);
            }
        }
        let result = self
            .join_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Orchestrator thread panicked"))??;
        Ok((tool_calls, result))
    }
}

/// Start streaming execution of Claude with the given prompt.
///
/// Spawns Claude as a child process pinned to a fresh session ID and tails
/// that session's log file, emitting `StreamEvent`s through the returned
/// handle's receiver.
pub fn execute_streaming(
    prompt: &str,
    config: &ExecutionConfig,
) -> Result<StreamHandle> {
    // Build the command up front so argument errors (e.g. --resume) surface
    // immediately; the orchestrator thread spawns it.
    let (cmd, locator) = build_claude_command(prompt, config)?;

    let (sender, receiver) = mpsc::channel::<StreamEvent>();

    let join_handle = thread::spawn(move || -> Result<RawExecutionResult> {
        orchestrate(cmd, locator, sender)
    });

    Ok(StreamHandle {
        receiver,
        join_handle,
    })
}

/// Orchestrator: spawns claude, spawns watcher, waits for completion.
fn orchestrate(
    mut cmd: Command,
    locator: SessionLocator,
    sender: mpsc::Sender<StreamEvent>,
) -> Result<RawExecutionResult> {
    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn claude command")?;

    // Shared flag: orchestrator sets this when the process exits
    let process_exited = Arc::new(AtomicBool::new(false));

    // Spawn the watcher thread
    let watcher_sender = sender.clone();
    let watcher_exited = Arc::clone(&process_exited);
    let watcher_locator = locator.clone();

    let watcher_handle = thread::spawn(move || {
        watch_for_session(watcher_locator, watcher_sender, watcher_exited)
    });

    // Wait for the claude process to complete
    let output = child.wait_with_output().context("Failed to wait for claude process")?;

    // Signal the watcher that the process has exited
    process_exited.store(true, Ordering::Release);

    // Wait for the watcher to finish its final drain
    let session_path = watcher_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Watcher thread panicked"))?;

    let session_log_path =
        session_path.ok_or_else(|| locator.missing_error(&output.status, &output.stderr))?;

    // Capture stdout
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stdout = if stdout.is_empty() { None } else { Some(stdout) };

    Ok(RawExecutionResult {
        session_log_path: Some(session_log_path),
        stdout,
        agent_context: None,
    })
}

/// Watcher: polls for this run's exact session file, then tails it.
///
/// Returns `None` if the file has not appeared within `SESSION_FILE_GRACE`
/// after the process exits.
fn watch_for_session(
    locator: SessionLocator,
    sender: mpsc::Sender<StreamEvent>,
    process_exited: Arc<AtomicBool>,
) -> Option<PathBuf> {
    let session_path = loop {
        if let Some(path) = locator.locate() {
            break path;
        }

        if process_exited.load(Ordering::Acquire) {
            // Process exited before the file appeared — allow a short grace period.
            break locator.wait_for(SESSION_FILE_GRACE)?;
        }

        thread::sleep(Duration::from_millis(200));
    };

    // Notify that we found the session
    let _ = sender.send(StreamEvent::SessionDetected(session_path.clone()));

    // Tail the session file
    tail_session_file(&session_path, &sender, &process_exited);

    Some(session_path)
}

/// Tail a session log file, parsing each new line and sending tool call events.
fn tail_session_file(
    path: &PathBuf,
    sender: &mpsc::Sender<StreamEvent>,
    process_exited: &Arc<AtomicBool>,
) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            let _ = sender.send(StreamEvent::Error(format!(
                "Failed to open session file: {}",
                e
            )));
            return;
        }
    };

    let mut reader = BufReader::new(file);

    loop {
        let exited = process_exited.load(Ordering::Acquire);

        // Read all available lines
        read_and_send_lines(&mut reader, sender);

        if exited {
            // Final drain: read one more time to catch any remaining lines
            read_and_send_lines(&mut reader, sender);
            break;
        }

        thread::sleep(Duration::from_millis(100));
    }
}

/// Read all available complete lines from the reader and send tool call events.
fn read_and_send_lines(
    reader: &mut BufReader<std::fs::File>,
    sender: &mpsc::Sender<StreamEvent>,
) {
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // No more data available
            Ok(_) => {
                let line = line.trim_end();
                if line.is_empty() {
                    continue;
                }
                match parse_line_internal(line) {
                    Ok(Some(calls)) => {
                        for call in calls {
                            if sender.send(StreamEvent::ToolCall(call)).is_err() {
                                return; // Receiver dropped
                            }
                        }
                    }
                    Ok(None) => {} // Not an assistant message with tool calls
                    Err(e) => {
                        let _ = sender.send(StreamEvent::Error(format!(
                            "Parse error: {}",
                            e
                        )));
                    }
                }
            }
            Err(e) => {
                let _ = sender.send(StreamEvent::Error(format!("Read error: {}", e)));
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Helper: create a JSONL line with a tool_use content block.
    fn make_tool_use_line(name: &str, params: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"2024-01-19T12:00:00Z","message":{{"content":[{{"type":"tool_use","id":"123","name":"{}","input":{}}}]}}}}"#,
            name, params
        )
    }

    #[test]
    fn test_tail_parse_single_line() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        // Write a tool_use line
        let line = make_tool_use_line("Read", r#"{"file_path":"/tmp/test.txt"}"#);
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();

        // Set up reader
        let file = std::fs::File::open(&file_path).unwrap();
        let mut reader = BufReader::new(file);

        let (sender, receiver) = mpsc::channel();
        read_and_send_lines(&mut reader, &sender);
        drop(sender);

        let events: Vec<_> = receiver.iter().collect();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCall(tc) => {
                assert_eq!(tc.name, "Read");
                assert_eq!(tc.params["file_path"], "/tmp/test.txt");
            }
            other => panic!("Expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn test_final_drain_captures_all() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");
        std::fs::write(&file_path, "").unwrap();

        let process_exited = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();

        let tail_path = file_path.clone();
        let tail_exited = Arc::clone(&process_exited);

        let tail_handle = thread::spawn(move || {
            tail_session_file(&tail_path, &sender, &tail_exited);
        });

        // Give tail thread time to start and do initial read
        thread::sleep(Duration::from_millis(150));

        // Write a line, signal exit, then verify the drain catches it
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .unwrap();
            writeln!(
                f,
                "{}",
                make_tool_use_line("Glob", r#"{"pattern":"*.rs"}"#)
            )
            .unwrap();
        }

        process_exited.store(true, Ordering::Release);
        tail_handle.join().unwrap();

        let events: Vec<_> = receiver.iter().collect();
        assert!(
            events.iter().any(|e| matches!(e, StreamEvent::ToolCall(tc) if tc.name == "Glob")),
            "Expected Glob tool call in events, got {:?}",
            events
        );
    }

    #[test]
    fn test_watch_detects_new_file() {
        let dir = TempDir::new().unwrap();
        let projects_dir = dir.path().to_path_buf();
        let workdir = PathBuf::from("/w/fresh_project.v1");
        let session_id = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b".to_string();

        // A pre-existing log from a previous run must not be picked up.
        let project_dir = projects_dir.join("-w-fresh-project-v1");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("previous-run.jsonl"),
            format!("{}\n", make_tool_use_line("Bash", r#"{"command":"ls"}"#)),
        )
        .unwrap();

        let locator = SessionLocator {
            projects_dir: projects_dir.clone(),
            workdir,
            session_id: session_id.clone(),
        };

        let process_exited = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let watch_exited = Arc::clone(&process_exited);

        let watch_handle =
            thread::spawn(move || watch_for_session(locator, sender, watch_exited));

        // Give watcher time to start polling
        thread::sleep(Duration::from_millis(300));

        // Create this run's session file
        let session_file = project_dir.join(format!("{}.jsonl", session_id));
        let line = make_tool_use_line("Read", r#"{"file_path":"test.txt"}"#);
        std::fs::write(&session_file, format!("{}\n", line)).unwrap();

        // Wait for detection + tailing
        thread::sleep(Duration::from_millis(500));

        // Signal exit
        process_exited.store(true, Ordering::Release);

        let result = watch_handle.join().unwrap();
        assert_eq!(result, Some(session_file));

        let events: Vec<_> = receiver.iter().collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolCall(tc) if tc.name == "Read")),
            "Expected ToolCall event"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolCall(tc) if tc.name == "Bash")),
            "Previous run's tool calls must not appear"
        );
    }

    #[test]
    fn test_watch_returns_none_when_file_never_appears() {
        let dir = TempDir::new().unwrap();
        let project_dir = dir.path().join("-w-x");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("decoy.jsonl"), "{}\n").unwrap();

        let locator = SessionLocator {
            projects_dir: dir.path().to_path_buf(),
            workdir: PathBuf::from("/w/x"),
            session_id: "11111111-2222-4333-8444-555555555555".to_string(),
        };
        let (sender, _receiver) = mpsc::channel();
        let exited = Arc::new(AtomicBool::new(true));
        assert!(watch_for_session(locator, sender, exited).is_none());
    }
}
