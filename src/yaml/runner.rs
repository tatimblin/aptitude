//! YAML test execution using the fluent API.
//!
//! This module translates YAML assertion definitions into fluent API calls
//! and collects the results. It acts as a thin adapter layer, delegating
//! all assertion logic to the fluent API.

use std::sync::Arc;

use crate::agents::Agent;
use crate::fluent::{expect_tools, AssertionResult, Tool};
use crate::parser::ToolCall;
use crate::review::{self, ReviewConfig};

use super::parser::{parse_tool_name, Assertion, StdoutConstraints, Test};

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

impl From<AssertionResult> for TestResult {
    fn from(result: AssertionResult) -> Self {
        if result.passed {
            TestResult::Pass
        } else {
            TestResult::Fail {
                reason: result.reason.unwrap_or_else(|| "unknown error".to_string()),
            }
        }
    }
}

/// Run a YAML test against tool calls and optional stdout.
///
/// This function evaluates all assertions in the test and returns the results.
/// Unlike the fluent API's immediate evaluation, this collects all results
/// without panicking.
///
/// # Example
///
/// ```rust,ignore
/// let test = load_test("test.yaml")?;
/// let results = run_yaml_test(&test, &tool_calls, &stdout);
///
/// for (description, result) in &results {
///     match result {
///         TestResult::Pass => println!("✓ {}", description),
///         TestResult::Fail { reason } => println!("✗ {} - {}", description, reason),
///     }
/// }
/// ```
pub fn run_yaml_test(
    test: &Test,
    tool_calls: &[ToolCall],
    stdout: &Option<String>,
    grader: Option<&Arc<dyn Agent>>,
) -> Vec<(String, TestResult)> {
    let mut results = Vec::new();

    for assertion in &test.assertions {
        // Check if this is a stdout assertion
        if let Some(stdout_constraints) = &assertion.stdout {
            let description = format_stdout_description(stdout_constraints);
            let result = evaluate_stdout_assertion(stdout_constraints, stdout, grader);
            results.push((description, result));
            continue;
        }

        // Tool assertion - tool name is required
        let tool_name = match &assertion.tool {
            Some(name) => name,
            None => {
                results.push((
                    "invalid assertion".to_string(),
                    TestResult::Fail {
                        reason: "Assertion must have either 'tool' or 'stdout'".to_string(),
                    },
                ));
                continue;
            }
        };

        // Validate assertion configuration
        if let Err(err) = validate_assertion(assertion) {
            results.push((
                format!("{} (invalid)", tool_name),
                TestResult::Fail { reason: err },
            ));
            continue;
        }

        // Parse tool name
        let tool = match parse_tool_name(tool_name) {
            Ok(t) => t,
            Err(e) => {
                results.push((
                    format!("{} (invalid)", tool_name),
                    TestResult::Fail {
                        reason: e.to_string(),
                    },
                ));
                continue;
            }
        };

        // Main assertion (called/not called with all constraints)
        let description = format_assertion_description(assertion);
        let result = evaluate_assertion(assertion, &tool, tool_calls);
        results.push((description, result));

        // Additional parameter assertions (nth_call_params, first_call_params, last_call_params)
        if let Some(nth_params) = &assertion.nth_call_params {
            for (n, params) in nth_params {
                let description = format!("{} call #{} params", tool, n);
                let result = evaluate_nth_params(&tool, tool_calls, *n, params);
                results.push((description, result));
            }
        }

        if let Some(first_params) = &assertion.first_call_params {
            let description = format!("{} first call params", tool);
            let result = evaluate_nth_params(&tool, tool_calls, 1, first_params);
            results.push((description, result));
        }

        if let Some(last_params) = &assertion.last_call_params {
            let description = format!("{} last call params", tool);
            let result = evaluate_last_params(&tool, tool_calls, last_params);
            results.push((description, result));
        }
    }

    results
}

// =========================================================================
// Internal: Delegation to fluent API
// =========================================================================

/// Evaluate the main assertion using the fluent API.
fn evaluate_assertion(assertion: &Assertion, tool: &Tool, tool_calls: &[ToolCall]) -> TestResult {
    // Build fluent assertion with all constraints
    let mut builder = expect_tools(tool_calls).tool(*tool);

    // Add parameter constraints
    if let Some(params) = &assertion.params {
        builder = builder.with_params(params.clone());
    }

    // Add count constraints
    if let Some(count) = assertion.call_count {
        builder = builder.times(count as usize);
    }
    if let Some(min) = assertion.min_calls {
        builder = builder.at_least(min as usize);
    }
    if let Some(max) = assertion.max_calls {
        builder = builder.at_most(max as usize);
    }

    // Add ordering constraints
    if let Some(after_str) = &assertion.called_after {
        if let Ok(after_tool) = parse_tool_name(after_str) {
            builder = builder.after(after_tool);
        } else {
            return TestResult::Fail {
                reason: format!("Unknown tool in called_after: '{}'", after_str),
            };
        }
    }
    if let Some(before_str) = &assertion.called_before {
        if let Ok(before_tool) = parse_tool_name(before_str) {
            builder = builder.before(before_tool);
        } else {
            return TestResult::Fail {
                reason: format!("Unknown tool in called_before: '{}'", before_str),
            };
        }
    }

    // Evaluate based on called expectation
    let result = if assertion.called {
        builder.evaluate()
    } else {
        builder.evaluate_not_called()
    };

    result.into()
}

/// Evaluate nth call parameters using the fluent API.
fn evaluate_nth_params(
    tool: &Tool,
    tool_calls: &[ToolCall],
    n: u32,
    expected_params: &std::collections::HashMap<String, String>,
) -> TestResult {
    // Check if there are enough calls
    let call_count = tool_calls.iter().filter(|c| c.name == tool.as_str()).count();
    if n == 0 || n as usize > call_count {
        return TestResult::Fail {
            reason: format!(
                "Tool '{}' call #{} does not exist (only {} calls made)",
                tool, n, call_count
            ),
        };
    }

    // Use fluent API's nth_call
    let result = expect_tools(tool_calls)
        .tool(*tool)
        .nth_call(n as usize)
        .evaluate_params(expected_params.clone());

    result.into()
}

/// Evaluate last call parameters using the fluent API.
fn evaluate_last_params(
    tool: &Tool,
    tool_calls: &[ToolCall],
    expected_params: &std::collections::HashMap<String, String>,
) -> TestResult {
    // Check if there are any calls
    let call_count = tool_calls.iter().filter(|c| c.name == tool.as_str()).count();
    if call_count == 0 {
        return TestResult::Fail {
            reason: format!("Tool '{}' was never called", tool),
        };
    }

    // Use fluent API's last_call
    let result = expect_tools(tool_calls)
        .tool(*tool)
        .last_call()
        .evaluate_params(expected_params.clone());

    result.into()
}

/// Evaluate stdout assertion using LLM-powered review.
fn evaluate_stdout_assertion(
    constraints: &StdoutConstraints,
    stdout: &Option<String>,
    grader: Option<&Arc<dyn Agent>>,
) -> TestResult {
    let grader = match grader {
        Some(g) => g,
        None => {
            return TestResult::Fail {
                reason: "No grading agent available for stdout review".to_string(),
            }
        }
    };

    let config = ReviewConfig {
        criteria: constraints.review.clone(),
        threshold: constraints.threshold,
        model: constraints.model.clone(),
    };

    let grader_ref = grader.clone();
    let result = review::grade_stdout(stdout, &config, |prompt, model| {
        grader_ref.grade(prompt, model)
    });

    match result {
        Ok(review_result) => {
            if review_result.passed {
                TestResult::Pass
            } else {
                TestResult::Fail {
                    reason: format!(
                        "score {}/10 below threshold {} — {}",
                        review_result.score, constraints.threshold, review_result.reasoning
                    ),
                }
            }
        }
        Err(e) => TestResult::Fail {
            reason: format!("grading failed: {}", e),
        },
    }
}

// =========================================================================
// Validation and formatting helpers
// =========================================================================

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

fn format_assertion_description(assertion: &Assertion) -> String {
    let mut desc = assertion
        .tool
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(params) = &assertion.params {
        let param_str: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}='{}'", k, v))
            .collect();
        desc = format!("{} with {}", desc, param_str.join(", "));
    }

    if assertion.called {
        if let Some(after) = &assertion.called_after {
            format!("{} called after {}", desc, after)
        } else if let Some(before) = &assertion.called_before {
            format!("{} called before {}", desc, before)
        } else {
            format!("{} called", desc)
        }
    } else {
        format!("{} not called", desc)
    }
}

fn format_stdout_description(constraints: &StdoutConstraints) -> String {
    format!(
        "stdout review: \"{}\" (threshold: {}/10)",
        constraints.review, constraints.threshold
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::agents::{ExecutionConfig, RawExecutionResult, ToolNameMapping};

    fn make_call(name: &str, params: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            params,
            timestamp: String::new(),
        }
    }

    fn make_assertion(tool: &str) -> Assertion {
        Assertion {
            tool: Some(tool.to_string()),
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
            stdout: None,
        }
    }

    /// Mock agent for grading tests.
    struct MockGrader {
        response: String,
    }

    impl MockGrader {
        fn passing() -> Arc<dyn Agent> {
            Arc::new(Self {
                response: r#"{"score": 9, "reasoning": "Meets criteria"}"#.to_string(),
            })
        }

        fn failing() -> Arc<dyn Agent> {
            Arc::new(Self {
                response: r#"{"score": 3, "reasoning": "Does not meet criteria"}"#.to_string(),
            })
        }
    }

    #[async_trait::async_trait]
    impl Agent for MockGrader {
        fn name(&self) -> &'static str { "mock" }
        fn execute(&self, _: &str, _: &ExecutionConfig) -> anyhow::Result<RawExecutionResult> {
            unimplemented!()
        }
        fn parse_session(&self, _: &RawExecutionResult) -> anyhow::Result<Vec<ToolCall>> {
            unimplemented!()
        }
        fn tool_mapping(&self) -> &ToolNameMapping { unimplemented!() }
        fn is_available(&self) -> bool { true }
        fn grade(&self, _: &str, _: Option<&str>) -> anyhow::Result<String> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn test_run_yaml_test_basic() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![make_assertion("Read")],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls, &None, None);

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
                called: false,
                ..make_assertion("Bash")
            }],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls, &None, None);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_case_insensitive() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![make_assertion("read")],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls, &None, None);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_alias() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![make_assertion("read_file")],
        };

        let calls = vec![make_call("Read", json!({"file_path": "/test.txt"}))];
        let results = run_yaml_test(&test, &calls, &None, None);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_with_count() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                call_count: Some(2),
                ..make_assertion("Read")
            }],
        };

        let calls = vec![
            make_call("Read", json!({"file_path": "/a.txt"})),
            make_call("Read", json!({"file_path": "/b.txt"})),
        ];
        let results = run_yaml_test(&test, &calls, &None, None);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_ordering() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                called_after: Some("Read".to_string()),
                ..make_assertion("Write")
            }],
        };

        let calls = vec![
            make_call("Read", json!({"file_path": "/input.txt"})),
            make_call("Write", json!({"file_path": "/output.txt"})),
        ];
        let results = run_yaml_test(&test, &calls, &None, None);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_nth_call_params() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                nth_call_params: Some({
                    let mut map = std::collections::HashMap::new();
                    let mut params = std::collections::HashMap::new();
                    params.insert("file_path".to_string(), "/second.txt".to_string());
                    map.insert(2, params);
                    map
                }),
                ..make_assertion("Read")
            }],
        };

        let calls = vec![
            make_call("Read", json!({"file_path": "/first.txt"})),
            make_call("Read", json!({"file_path": "/second.txt"})),
        ];
        let results = run_yaml_test(&test, &calls, &None, None);

        assert_eq!(results.len(), 2);
        assert!(results[0].1.is_pass());
        assert!(results[1].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_stdout_review_pass() {
        let grader = MockGrader::passing();
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: None,
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
                stdout: Some(StdoutConstraints {
                    review: "should confirm success".to_string(),
                    threshold: 7,
                    model: None,
                    agent: None,
                }),
            }],
        };

        let stdout = Some("Operation completed successfully".to_string());
        let results = run_yaml_test(&test, &[], &stdout, Some(&grader));

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_pass());
    }

    #[test]
    fn test_run_yaml_test_stdout_review_fail() {
        let grader = MockGrader::failing();
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: None,
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
                stdout: Some(StdoutConstraints {
                    review: "should confirm success".to_string(),
                    threshold: 7,
                    model: None,
                    agent: None,
                }),
            }],
        };

        let stdout = Some("Operation failed with error".to_string());
        let results = run_yaml_test(&test, &[], &stdout, Some(&grader));

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_fail());
    }

    #[test]
    fn test_run_yaml_test_stdout_no_grader() {
        let test = Test {
            name: "Test".to_string(),
            prompt: "Test prompt".to_string(),
            agent: None,
            assertions: vec![Assertion {
                tool: None,
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
                stdout: Some(StdoutConstraints {
                    review: "should confirm success".to_string(),
                    threshold: 7,
                    model: None,
                    agent: None,
                }),
            }],
        };

        let stdout = Some("test".to_string());
        let results = run_yaml_test(&test, &[], &stdout, None);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_fail());
        match &results[0].1 {
            TestResult::Fail { reason } => {
                assert!(reason.contains("No grading agent"));
            }
            _ => panic!("Expected failure"),
        }
    }
}
