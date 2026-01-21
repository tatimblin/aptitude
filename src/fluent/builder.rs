//! Fluent assertion builder for tool calls.
//!
//! This module provides the core builder types for making assertions about tool calls:
//! - `expect()` - Entry point for creating assertions
//! - `ToolCallExpectation` - Holds tool calls and creates tool-specific assertions
//! - `ToolAssertion` - Builder for assertions on a specific tool

use crate::parser::ToolCall;
use super::matchers::params_match;
use super::Tool;
use std::collections::HashMap;

/// Result of evaluating an assertion.
#[derive(Debug, Clone)]
pub struct AssertionResult {
    /// Whether the assertion passed.
    pub passed: bool,
    /// Description of what was asserted.
    pub description: String,
    /// Failure reason if the assertion failed.
    pub reason: Option<String>,
}

impl AssertionResult {
    fn pass(description: impl Into<String>) -> Self {
        Self {
            passed: true,
            description: description.into(),
            reason: None,
        }
    }

    fn fail(description: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            passed: false,
            description: description.into(),
            reason: Some(reason.into()),
        }
    }
}

/// Create an expectation on a set of tool calls.
///
/// This is the entry point for the fluent assertion API.
///
/// # Example
///
/// ```rust,ignore
/// use agent_harness::{expect, Tool};
///
/// let tool_calls = vec![/* ... */];
/// expect(&tool_calls).tool(Tool::Read).to_be_called();
/// ```
pub fn expect(tool_calls: &[ToolCall]) -> ToolCallExpectation {
    ToolCallExpectation::new(tool_calls)
}

/// Holds tool calls and creates tool-specific assertions.
///
/// This is the starting point for building assertions. Call `.tool()` to
/// create a `ToolAssertion` for a specific tool type.
#[derive(Debug, Clone)]
pub struct ToolCallExpectation {
    tool_calls: Vec<ToolCall>,
}

impl ToolCallExpectation {
    /// Create a new expectation from tool calls.
    pub fn new(tool_calls: &[ToolCall]) -> Self {
        Self {
            tool_calls: tool_calls.to_vec(),
        }
    }

    /// Create an assertion for a specific tool.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .to_be_called();
    /// ```
    pub fn tool(&self, tool: Tool) -> ToolAssertion {
        ToolAssertion::new(self.tool_calls.clone(), tool)
    }
}

/// Builder for assertions on a specific tool.
///
/// Methods like `to_be_called()` evaluate immediately and panic on failure.
/// Use `evaluate()` for non-panicking evaluation.
#[derive(Debug, Clone)]
pub struct ToolAssertion {
    tool_calls: Vec<ToolCall>,
    tool: Tool,
    params: Option<HashMap<String, String>>,
    expected_count: Option<usize>,
    min_count: Option<usize>,
    max_count: Option<usize>,
    after_tool: Option<Tool>,
    before_tool: Option<Tool>,
}

impl ToolAssertion {
    /// Create a new tool assertion.
    pub fn new(tool_calls: Vec<ToolCall>, tool: Tool) -> Self {
        Self {
            tool_calls,
            tool,
            params: None,
            expected_count: None,
            min_count: None,
            max_count: None,
            after_tool: None,
            before_tool: None,
        }
    }

    // =========================================================================
    // Builder methods (chainable)
    // =========================================================================

    /// Set parameter expectations for matching.
    ///
    /// Parameters can use glob patterns, regex, or exact matching.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use agent_harness::params;
    ///
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .with_params(params!{"file_path" => "*.txt"})
    ///     .to_be_called();
    /// ```
    pub fn with_params(mut self, params: HashMap<String, String>) -> Self {
        self.params = Some(params);
        self
    }

    /// Assert the tool was called exactly N times.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .times(3)
    ///     .to_be_called();
    /// ```
    pub fn times(mut self, n: usize) -> Self {
        self.expected_count = Some(n);
        self
    }

    /// Assert the tool was called at least N times.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .at_least(2)
    ///     .to_be_called();
    /// ```
    pub fn at_least(mut self, n: usize) -> Self {
        self.min_count = Some(n);
        self
    }

    /// Assert the tool was called at most N times.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .at_most(5)
    ///     .to_be_called();
    /// ```
    pub fn at_most(mut self, n: usize) -> Self {
        self.max_count = Some(n);
        self
    }

    /// Assert this tool was called after another tool.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Write)
    ///     .after(Tool::Read)
    ///     .to_be_called();
    /// ```
    pub fn after(mut self, tool: Tool) -> Self {
        self.after_tool = Some(tool);
        self
    }

    /// Assert this tool was called before another tool.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .before(Tool::Write)
    ///     .to_be_called();
    /// ```
    pub fn before(mut self, tool: Tool) -> Self {
        self.before_tool = Some(tool);
        self
    }

    // =========================================================================
    // Assertion methods (panic on failure)
    // =========================================================================

    /// Assert the tool was called.
    ///
    /// Panics with a detailed error message if the assertion fails.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .to_be_called();
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the tool was not called (or doesn't match params/ordering).
    pub fn to_be_called(&self) {
        let result = self.evaluate_called(true);
        if !result.passed {
            self.panic_with_context(&result);
        }
    }

    /// Assert the tool was NOT called.
    ///
    /// Panics with a detailed error message if the tool was called.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&tool_calls)
    ///     .tool(Tool::Bash)
    ///     .not_to_be_called();
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the tool was called (matching any specified params).
    pub fn not_to_be_called(&self) {
        let result = self.evaluate_called(false);
        if !result.passed {
            self.panic_with_context(&result);
        }
    }

    // =========================================================================
    // nth_call pattern
    // =========================================================================

    /// Assert on the nth call (1-indexed) of this tool.
    ///
    /// The closure receives a `NthCallAssertion` for making assertions
    /// about that specific call.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use agent_harness::params;
    ///
    /// expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .nth_call(1, |call| {
    ///         call.with_params(params!{"file_path" => "/first.txt"});
    ///     })
    ///     .nth_call(2, |call| {
    ///         call.with_params(params!{"file_path" => "/second.txt"});
    ///     });
    /// ```
    pub fn nth_call<F>(&self, n: usize, f: F) -> &Self
    where
        F: FnOnce(NthCallAssertion),
    {
        let matching_calls: Vec<&ToolCall> = self
            .tool_calls
            .iter()
            .filter(|c| c.name == self.tool.as_str())
            .collect();

        if n == 0 || n > matching_calls.len() {
            panic!(
                "assertion failed: expected {} call #{} to exist\n\n  actual: {} calls made\n{}",
                self.tool,
                n,
                matching_calls.len(),
                self.format_tool_calls()
            );
        }

        let call = matching_calls[n - 1];
        let assertion = NthCallAssertion::new(call.clone(), self.tool, n);
        f(assertion);
        self
    }

    // =========================================================================
    // Non-panicking evaluation
    // =========================================================================

    /// Evaluate the assertion without panicking.
    ///
    /// Returns an `AssertionResult` that can be inspected.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = expect(&tool_calls)
    ///     .tool(Tool::Read)
    ///     .evaluate();
    ///
    /// if !result.passed {
    ///     println!("Failed: {}", result.reason.unwrap());
    /// }
    /// ```
    pub fn evaluate(&self) -> AssertionResult {
        self.evaluate_called(true)
    }

    // =========================================================================
    // Internal helpers
    // =========================================================================

    fn get_matching_calls(&self) -> Vec<&ToolCall> {
        self.tool_calls
            .iter()
            .filter(|c| c.name == self.tool.as_str())
            .filter(|c| {
                if let Some(params) = &self.params {
                    params_match(params, &c.params)
                } else {
                    true
                }
            })
            .collect()
    }

    fn evaluate_called(&self, should_be_called: bool) -> AssertionResult {
        let matching_calls = self.get_matching_calls();
        let count = matching_calls.len();
        let was_called = count > 0;

        // Check ordering first
        if let Some(after) = &self.after_tool {
            return self.evaluate_after(after);
        }
        if let Some(before) = &self.before_tool {
            return self.evaluate_before(before);
        }

        // Check count constraints
        if let Some(expected) = self.expected_count {
            if count != expected {
                return AssertionResult::fail(
                    format!("{} called {} times", self.tool, expected),
                    format!("called {} times", count),
                );
            }
        }
        if let Some(min) = self.min_count {
            if count < min {
                return AssertionResult::fail(
                    format!("{} called at least {} times", self.tool, min),
                    format!("called {} times", count),
                );
            }
        }
        if let Some(max) = self.max_count {
            if count > max {
                return AssertionResult::fail(
                    format!("{} called at most {} times", self.tool, max),
                    format!("called {} times", count),
                );
            }
        }

        // Check called/not called
        if should_be_called && !was_called {
            let param_desc = self
                .params
                .as_ref()
                .map(|p| format!(" with params {:?}", p))
                .unwrap_or_default();
            return AssertionResult::fail(
                format!("{} called", self.tool),
                format!("tool '{}'{} was never called", self.tool, param_desc),
            );
        }
        if !should_be_called && was_called {
            let found = matching_calls.first().unwrap();
            return AssertionResult::fail(
                format!("{} not called", self.tool),
                format!(
                    "tool '{}' was called but should not have been. Found: {:?}",
                    self.tool, found.params
                ),
            );
        }

        AssertionResult::pass(format!(
            "{} {}",
            self.tool,
            if should_be_called {
                "called"
            } else {
                "not called"
            }
        ))
    }

    fn evaluate_after(&self, after_tool: &Tool) -> AssertionResult {
        let mut seen_after = false;

        for call in &self.tool_calls {
            if call.name == after_tool.as_str() {
                seen_after = true;
            }
            if call.name == self.tool.as_str() && seen_after {
                if let Some(params) = &self.params {
                    if params_match(params, &call.params) {
                        return AssertionResult::pass(format!(
                            "{} called after {}",
                            self.tool, after_tool
                        ));
                    }
                } else {
                    return AssertionResult::pass(format!(
                        "{} called after {}",
                        self.tool, after_tool
                    ));
                }
            }
        }

        if !seen_after {
            return AssertionResult::fail(
                format!("{} called after {}", self.tool, after_tool),
                format!("tool '{}' was never called", after_tool),
            );
        }

        AssertionResult::fail(
            format!("{} called after {}", self.tool, after_tool),
            format!("'{}' was not called after '{}'", self.tool, after_tool),
        )
    }

    fn evaluate_before(&self, before_tool: &Tool) -> AssertionResult {
        let mut seen_this = false;

        for call in &self.tool_calls {
            if call.name == self.tool.as_str() {
                if let Some(params) = &self.params {
                    if params_match(params, &call.params) {
                        seen_this = true;
                    }
                } else {
                    seen_this = true;
                }
            }
            if call.name == before_tool.as_str() && seen_this {
                return AssertionResult::pass(format!(
                    "{} called before {}",
                    self.tool, before_tool
                ));
            }
        }

        let this_called = self.tool_calls.iter().any(|c| c.name == self.tool.as_str());
        let before_called = self
            .tool_calls
            .iter()
            .any(|c| c.name == before_tool.as_str());

        if !this_called {
            return AssertionResult::fail(
                format!("{} called before {}", self.tool, before_tool),
                format!("tool '{}' was never called", self.tool),
            );
        }
        if !before_called {
            return AssertionResult::fail(
                format!("{} called before {}", self.tool, before_tool),
                format!("tool '{}' was never called", before_tool),
            );
        }

        AssertionResult::fail(
            format!("{} called before {}", self.tool, before_tool),
            format!("'{}' was not called before '{}'", self.tool, before_tool),
        )
    }

    fn panic_with_context(&self, result: &AssertionResult) -> ! {
        let reason = result.reason.as_deref().unwrap_or("unknown reason");
        panic!(
            "assertion failed: expected {}\n\n  reason: {}\n{}",
            result.description,
            reason,
            self.format_tool_calls()
        );
    }

    fn format_tool_calls(&self) -> String {
        if self.tool_calls.is_empty() {
            return "  tool calls made: (none)\n".to_string();
        }

        let mut output = format!("  tool calls made ({}):\n", self.tool_calls.len());
        for (i, call) in self.tool_calls.iter().enumerate() {
            let params_preview = call
                .params
                .get("file_path")
                .or_else(|| call.params.get("command"))
                .or_else(|| call.params.get("pattern"))
                .and_then(|v| v.as_str())
                .map(|s| {
                    if s.len() > 50 {
                        format!("{}...", &s[..47])
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_else(|| "...".to_string());
            output.push_str(&format!(
                "    {}. {} {{ {} }}\n",
                i + 1,
                call.name,
                params_preview
            ));
        }
        output
    }
}

/// Assertion helper for a specific call (used in nth_call).
#[derive(Debug, Clone)]
pub struct NthCallAssertion {
    call: ToolCall,
    tool: Tool,
    n: usize,
}

impl NthCallAssertion {
    fn new(call: ToolCall, tool: Tool, n: usize) -> Self {
        Self { call, tool, n }
    }

    /// Assert this specific call has the given parameters.
    ///
    /// # Panics
    ///
    /// Panics if the parameters don't match.
    pub fn with_params(self, params: HashMap<String, String>) {
        if !params_match(&params, &self.call.params) {
            panic!(
                "assertion failed: {} call #{} params did not match\n\n  expected: {:?}\n  actual: {:?}",
                self.tool, self.n, params, self.call.params
            );
        }
    }
}
