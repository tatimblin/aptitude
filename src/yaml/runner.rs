//! YAML test execution using the fluent API.
//!
//! This module translates YAML assertion definitions into fluent API calls
//! and collects the results.

use crate::fluent::{expect, params_match, Tool};
use crate::parser::ToolCall;

use super::parser::{parse_tool_name, Assertion, Test};

/// Result of evaluating a single assertion.
#[derive(Debug, Clone)]
pub enum TestResult {
    /// Assertion passed.
    Pass,
    /// Assertion failed with reason.
    Fail { reason: String },
}

impl TestResult {
    /// Check if this result is a pass.
    pub fn is_pass(&self) -> bool {
        matches!(self, TestResult::Pass)
    }

    /// Check if this result is a failure.
    pub fn is_fail(&self) -> bool {
        matches!(self, TestResult::Fail { .. })
    }
}

/// Run a YAML test against a set of tool calls.
///
/// This function evaluates all assertions in the test and returns the results.
/// Unlike the fluent API's immediate evaluation, this collects all results
/// without panicking.
///
/// # Example
///
/// ```rust,ignore
/// let test = load_test("test.yaml")?;
/// let results = run_yaml_test(&test, &tool_calls);
///
/// for (description, result) in &results {
///     match result {
///         TestResult::Pass => println!("✓ {}", description),
///         TestResult::Fail { reason } => println!("✗ {} - {}", description, reason),
///     }
/// }
/// ```
pub fn run_yaml_test(test: &Test, tool_calls: &[ToolCall]) -> Vec<(String, TestResult)> {
    let mut results = Vec::new();

    for assertion in &test.assertions {
        // Validate assertion configuration
        if let Err(err) = validate_assertion(assertion) {
            results.push((
                format!("{} (invalid)", assertion.tool),
                TestResult::Fail { reason: err },
            ));
            continue;
        }

        // Parse tool name
        let tool = match parse_tool_name(&assertion.tool) {
            Ok(t) => t,
            Err(e) => {
                results.push((
                    format!("{} (invalid)", assertion.tool),
                    TestResult::Fail {
                        reason: e.to_string(),
                    },
                ));
                continue;
            }
        };

        // Evaluate presence (called: true/false) - only if not using ordering assertions
        if assertion.called_after.is_none() && assertion.called_before.is_none() {
            let description = format_assertion_description(assertion, None);
            let result = evaluate_called(assertion, &tool, tool_calls);
            results.push((description, result));
        }

        // Evaluate ordering: called_after
        if let Some(after_tool) = &assertion.called_after {
            let description = format_assertion_description(assertion, None);
            let result = evaluate_called_after(assertion, &tool, after_tool, tool_calls);
            results.push((description, result));
        }

        // Evaluate ordering: called_before
        if let Some(before_tool) = &assertion.called_before {
            let description = format_assertion_description(assertion, None);
            let result = evaluate_called_before(assertion, &tool, before_tool, tool_calls);
            results.push((description, result));
        }

        // Evaluate count constraints
        if let Some(count) = assertion.call_count {
            let description = format_count_description(&tool, "call_count ==", count);
            let result = evaluate_call_count(assertion, &tool, tool_calls, count);
            results.push((description, result));
        }

        if let Some(max) = assertion.max_calls {
            let description = format_count_description(&tool, "max_calls <=", max);
            let result = evaluate_max_calls(assertion, &tool, tool_calls, max);
            results.push((description, result));
        }

        if let Some(min) = assertion.min_calls {
            let description = format_count_description(&tool, "min_calls >=", min);
            let result = evaluate_min_calls(assertion, &tool, tool_calls, min);
            results.push((description, result));
        }

        // Evaluate parameter assertions
        if let Some(nth_params) = &assertion.nth_call_params {
            for (n, params) in nth_params {
                let description = format!("{} nth_call_params[{}] matches", tool, n);
                let result = evaluate_nth_call_params(&tool, tool_calls, *n, params);
                results.push((description, result));
            }
        }

        if let Some(first_params) = &assertion.first_call_params {
            let description = format!("{} first_call_params", tool);
            let result = evaluate_first_call_params(&tool, tool_calls, first_params);
            results.push((description, result));
        }

        if let Some(last_params) = &assertion.last_call_params {
            let description = format!("{} last_call_params", tool);
            let result = evaluate_last_call_params(&tool, tool_calls, last_params);
            results.push((description, result));
        }
    }

    results
}

// =========================================================================
// Helper functions
// =========================================================================

fn format_assertion_description(assertion: &Assertion, suffix: Option<&str>) -> String {
    let mut desc = assertion.tool.clone();

    if let Some(params) = &assertion.params {
        let param_str: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}='{}'", k, v))
            .collect();
        desc = format!("{} with {}", desc, param_str.join(", "));
    }

    let base_desc = if assertion.called {
        if let Some(after) = &assertion.called_after {
            format!("{} called after {}", desc, after)
        } else if let Some(before) = &assertion.called_before {
            format!("{} called before {}", desc, before)
        } else {
            format!("{} called", desc)
        }
    } else {
        format!("{} not called", desc)
    };

    match suffix {
        Some(s) => format!("{} {}", base_desc, s),
        None => base_desc,
    }
}

fn format_count_description(tool: &Tool, assertion_type: &str, count: u32) -> String {
    format!("{} {} {}", tool, assertion_type, count)
}

fn validate_assertion(assertion: &Assertion) -> Result<(), String> {
    // called: false is mutually exclusive with count assertions
    if !assertion.called {
        if assertion.call_count.is_some() {
            return Err("'called: false' cannot be combined with 'call_count'".to_string());
        }
        if assertion.min_calls.is_some() {
            return Err("'called: false' cannot be combined with 'min_calls'".to_string());
        }
        if assertion.max_calls.is_some() && assertion.max_calls != Some(0) {
            return Err(
                "'called: false' cannot be combined with 'max_calls' (except max_calls: 0)"
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn get_matching_calls<'a>(
    tool: &Tool,
    tool_calls: &'a [ToolCall],
    params: &Option<std::collections::HashMap<String, String>>,
) -> Vec<&'a ToolCall> {
    tool_calls
        .iter()
        .filter(|c| c.name == tool.as_str())
        .filter(|c| {
            if let Some(p) = params {
                params_match(p, &c.params)
            } else {
                true
            }
        })
        .collect()
}

fn evaluate_called(assertion: &Assertion, tool: &Tool, tool_calls: &[ToolCall]) -> TestResult {
    let matching_calls = get_matching_calls(tool, tool_calls, &assertion.params);
    let was_called = !matching_calls.is_empty();

    if assertion.called && !was_called {
        let param_desc = assertion
            .params
            .as_ref()
            .map(|p| format!(" with params {:?}", p))
            .unwrap_or_default();
        TestResult::Fail {
            reason: format!("Tool '{}'{} was never called", tool, param_desc),
        }
    } else if !assertion.called && was_called {
        let found = matching_calls.first().unwrap();
        TestResult::Fail {
            reason: format!(
                "Tool '{}' was called but should not have been. Found: {:?}",
                tool, found.params
            ),
        }
    } else {
        TestResult::Pass
    }
}

fn evaluate_called_after(
    _assertion: &Assertion,
    tool: &Tool,
    after_tool_str: &str,
    tool_calls: &[ToolCall],
) -> TestResult {
    let after_tool = match parse_tool_name(after_tool_str) {
        Ok(t) => t,
        Err(e) => return TestResult::Fail { reason: e.to_string() },
    };

    let result = expect(tool_calls)
        .tool(*tool)
        .after(after_tool)
        .evaluate();

    if result.passed {
        TestResult::Pass
    } else {
        TestResult::Fail {
            reason: result.reason.unwrap_or_else(|| "unknown error".to_string()),
        }
    }
}

fn evaluate_called_before(
    _assertion: &Assertion,
    tool: &Tool,
    before_tool_str: &str,
    tool_calls: &[ToolCall],
) -> TestResult {
    let before_tool = match parse_tool_name(before_tool_str) {
        Ok(t) => t,
        Err(e) => return TestResult::Fail { reason: e.to_string() },
    };

    let result = expect(tool_calls)
        .tool(*tool)
        .before(before_tool)
        .evaluate();

    if result.passed {
        TestResult::Pass
    } else {
        TestResult::Fail {
            reason: result.reason.unwrap_or_else(|| "unknown error".to_string()),
        }
    }
}

fn evaluate_call_count(
    assertion: &Assertion,
    tool: &Tool,
    tool_calls: &[ToolCall],
    expected: u32,
) -> TestResult {
    let matching_calls = get_matching_calls(tool, tool_calls, &assertion.params);
    let actual = matching_calls.len() as u32;

    if actual == expected {
        TestResult::Pass
    } else {
        TestResult::Fail {
            reason: format!(
                "Tool '{}' was called {} times, expected exactly {}",
                tool, actual, expected
            ),
        }
    }
}

fn evaluate_max_calls(
    assertion: &Assertion,
    tool: &Tool,
    tool_calls: &[ToolCall],
    max: u32,
) -> TestResult {
    let matching_calls = get_matching_calls(tool, tool_calls, &assertion.params);
    let actual = matching_calls.len() as u32;

    if actual <= max {
        TestResult::Pass
    } else {
        TestResult::Fail {
            reason: format!(
                "Tool '{}' was called {} times, expected at most {}",
                tool, actual, max
            ),
        }
    }
}

fn evaluate_min_calls(
    assertion: &Assertion,
    tool: &Tool,
    tool_calls: &[ToolCall],
    min: u32,
) -> TestResult {
    let matching_calls = get_matching_calls(tool, tool_calls, &assertion.params);
    let actual = matching_calls.len() as u32;

    if actual >= min {
        TestResult::Pass
    } else {
        TestResult::Fail {
            reason: format!(
                "Tool '{}' was called {} times, expected at least {}",
                tool, actual, min
            ),
        }
    }
}

fn evaluate_nth_call_params(
    tool: &Tool,
    tool_calls: &[ToolCall],
    n: u32,
    expected_params: &std::collections::HashMap<String, String>,
) -> TestResult {
    let matching_calls: Vec<&ToolCall> = tool_calls
        .iter()
        .filter(|c| c.name == tool.as_str())
        .collect();

    let index = (n as usize).saturating_sub(1);
    if let Some(call) = matching_calls.get(index) {
        if params_match(expected_params, &call.params) {
            TestResult::Pass
        } else {
            TestResult::Fail {
                reason: format!(
                    "Tool '{}' call #{} params did not match. Expected {:?}, got {:?}",
                    tool, n, expected_params, call.params
                ),
            }
        }
    } else {
        TestResult::Fail {
            reason: format!(
                "Tool '{}' call #{} does not exist (only {} calls made)",
                tool,
                n,
                matching_calls.len()
            ),
        }
    }
}

fn evaluate_first_call_params(
    tool: &Tool,
    tool_calls: &[ToolCall],
    expected_params: &std::collections::HashMap<String, String>,
) -> TestResult {
    let first_call = tool_calls.iter().find(|c| c.name == tool.as_str());

    match first_call {
        Some(call) => {
            if params_match(expected_params, &call.params) {
                TestResult::Pass
            } else {
                TestResult::Fail {
                    reason: format!(
                        "Tool '{}' first call params did not match. Expected {:?}, got {:?}",
                        tool, expected_params, call.params
                    ),
                }
            }
        }
        None => TestResult::Fail {
            reason: format!("Tool '{}' was never called", tool),
        },
    }
}

fn evaluate_last_call_params(
    tool: &Tool,
    tool_calls: &[ToolCall],
    expected_params: &std::collections::HashMap<String, String>,
) -> TestResult {
    let last_call = tool_calls
        .iter()
        .filter(|c| c.name == tool.as_str())
        .last();

    match last_call {
        Some(call) => {
            if params_match(expected_params, &call.params) {
                TestResult::Pass
            } else {
                TestResult::Fail {
                    reason: format!(
                        "Tool '{}' last call params did not match. Expected {:?}, got {:?}",
                        tool, expected_params, call.params
                    ),
                }
            }
        }
        None => TestResult::Fail {
            reason: format!("Tool '{}' was never called", tool),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn make_call(name: &str, params: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            params,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_run_yaml_test_basic() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: "Read".to_string(),
                called: true,
                params: None,
                called_after: None,
                called_before: None,
                call_count: None,
                max_calls: None,
                min_calls: None,
                nth_call_params: None,
                first_call_params: None,
                last_call_params: None,
            }],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_not_called() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: "Bash".to_string(),
                called: false,
                params: None,
                called_after: None,
                called_before: None,
                call_count: None,
                max_calls: None,
                min_calls: None,
                nth_call_params: None,
                first_call_params: None,
                last_call_params: None,
            }],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_case_insensitive() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: "read".to_string(), // lowercase
                called: true,
                params: None,
                called_after: None,
                called_before: None,
                call_count: None,
                max_calls: None,
                min_calls: None,
                nth_call_params: None,
                first_call_params: None,
                last_call_params: None,
            }],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_alias() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: "read_file".to_string(), // alias
                called: true,
                params: None,
                called_after: None,
                called_before: None,
                call_count: None,
                max_calls: None,
                min_calls: None,
                nth_call_params: None,
                first_call_params: None,
                last_call_params: None,
            }],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }
}
