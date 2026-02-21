//! Integration tests for streaming execution.
//!
//! These tests require Claude Code to be installed and available.
//! Run with: cargo test --test streaming_integration -- --ignored

use aptitude::streaming::{execute_streaming, StreamEvent};
use aptitude::agents::ExecutionConfig;
use aptitude::prompt;

#[test]
#[ignore]
fn test_streaming_events_arrive() {
    let config = ExecutionConfig::new();
    let handle = execute_streaming("What is 2+2? Reply with just the number.", &config)
        .expect("Failed to start streaming execution");

    let mut got_session = false;
    let mut tool_calls = Vec::new();

    for event in &handle.receiver {
        match event {
            StreamEvent::ToolCall(tc) => {
                tool_calls.push(tc);
            }
            StreamEvent::SessionDetected(_) => {
                got_session = true;
            }
            StreamEvent::Error(msg) => {
                eprintln!("Stream error: {}", msg);
            }
        }
    }

    let result = handle.wait().expect("Failed to wait for streaming result");

    assert!(got_session, "Should have detected a session file");
    assert!(result.stdout.is_some(), "Should have captured stdout");
}

#[test]
#[ignore]
fn test_streaming_via_prompt_builder() {
    let handle = prompt("What is 2+2? Reply with just the number.")
        .run_streaming()
        .expect("Failed to start streaming execution");

    let (_tool_calls, result) = handle.collect_all().expect("Failed to collect streaming results");

    assert!(result.stdout.is_some(), "Should have captured stdout");
}
