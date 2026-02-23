//! Fluent assertion builder for LLM-powered stdout review.
//!
//! This module provides the builder type for making assertions about stdout
//! using an LLM grader:
//! - `StdoutAssertion` - Builder for review-based assertions on stdout content

use std::sync::Arc;

use super::builder::AssertionResult;
use crate::agents::Agent;
use crate::review::{self, ReviewConfig};

/// Builder for LLM-powered assertions on stdout.
///
/// Methods like `to_pass()` evaluate immediately and panic on failure.
/// Use `evaluate()` for non-panicking evaluation.
///
/// # Example
///
/// ```rust,ignore
/// expect(&output)
///     .with_grader(agent)
///     .stdout()
///     .review("should confirm the file was created")
///     .to_pass();
/// ```
#[derive(Clone)]
pub struct StdoutAssertion {
    stdout: Option<String>,
    review: Option<String>,
    threshold: u32,
    model: Option<String>,
    grader: Option<Arc<dyn Agent>>,
}

impl std::fmt::Debug for StdoutAssertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdoutAssertion")
            .field("stdout", &self.stdout)
            .field("review", &self.review)
            .field("threshold", &self.threshold)
            .field("model", &self.model)
            .field("grader", &self.grader.as_ref().map(|g| g.name()))
            .finish()
    }
}

impl StdoutAssertion {
    /// Create a new stdout assertion.
    pub fn new(stdout: Option<String>) -> Self {
        Self {
            stdout,
            review: None,
            threshold: 7,
            model: None,
            grader: None,
        }
    }

    /// Simple constructor for the common case - review with default threshold.
    ///
    /// This is the recommended approach for 90% of use cases. For advanced
    /// configuration (custom threshold, model, or agent), use the builder pattern
    /// via `.new()` followed by chained method calls.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Simple approach - uses default threshold of 7
    /// let assertion = StdoutAssertion::with_review(
    ///     Some("Task completed successfully".to_string()),
    ///     "should confirm task completion"
    /// ).with_grader(agent);
    ///
    /// // Advanced approach - full customization
    /// let assertion = StdoutAssertion::new(stdout)
    ///     .review("should be valid JSON")
    ///     .with_threshold(9)
    ///     .with_model("claude-sonnet-4")
    ///     .with_grader(agent);
    /// ```
    pub fn with_review(stdout: Option<String>, criteria: &str) -> Self {
        Self {
            stdout,
            review: Some(criteria.to_string()),
            threshold: 7, // sensible default for most cases
            model: None,
            grader: None,
        }
    }

    /// Advanced builder constructor for complex configurations.
    ///
    /// Use this when you need fine-grained control over threshold, model, or
    /// multiple configuration options. For simple cases, use [`StdoutAssertion::review`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let assertion = StdoutAssertion::builder()
    ///     .stdout(Some("output".to_string()))
    ///     .review("should meet all criteria")
    ///     .with_threshold(9)
    ///     .with_model("claude-opus-4")
    ///     .with_grader(agent)
    ///     .build();
    /// ```
    pub fn builder() -> StdoutAssertionBuilder {
        StdoutAssertionBuilder::new()
    }

    // =========================================================================
    // Builder methods (chainable)
    // =========================================================================

    /// Set the review criteria for grading stdout.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&output)
    ///     .with_grader(agent)
    ///     .stdout()
    ///     .review("should say task was successful and be no more than 10 words")
    ///     .to_pass();
    /// ```
    pub fn review(mut self, criteria: &str) -> Self {
        self.review = Some(criteria.to_string());
        self
    }

    /// Set the minimum score threshold (1-10, default: 7).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&output)
    ///     .with_grader(agent)
    ///     .stdout()
    ///     .review("should be valid JSON")
    ///     .with_threshold(9)
    ///     .to_pass();
    /// ```
    pub fn with_threshold(mut self, threshold: u32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set the model override for the grading agent.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// expect(&output)
    ///     .with_grader(agent)
    ///     .stdout()
    ///     .review("should list 3 items")
    ///     .with_model("claude-sonnet-4-20250514")
    ///     .to_pass();
    /// ```
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Set the agent to use for grading.
    ///
    /// This determines which CLI command is used to invoke the grading LLM.
    /// If not set, `evaluate()` will fail with a descriptive error.
    pub fn with_grader(mut self, agent: Arc<dyn Agent>) -> Self {
        self.grader = Some(agent);
        self
    }

    // =========================================================================
    // Assertion methods (panic on failure)
    // =========================================================================

    /// Assert stdout passes the review criteria.
    ///
    /// Panics with a detailed error message if the assertion fails.
    ///
    /// # Panics
    ///
    /// Panics if the review score is below the threshold or grading fails.
    pub fn to_pass(&self) {
        let result = self.evaluate();
        if !result.passed {
            self.panic_with_context(&result);
        }
    }

    // =========================================================================
    // Non-panicking evaluation
    // =========================================================================

    /// Evaluate the review assertion without panicking.
    ///
    /// Returns an `AssertionResult` that can be inspected.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = expect(&output)
    ///     .with_grader(agent)
    ///     .stdout()
    ///     .review("should confirm success")
    ///     .evaluate();
    ///
    /// if !result.passed {
    ///     println!("Failed: {}", result.reason.unwrap());
    /// }
    /// ```
    pub fn evaluate(&self) -> AssertionResult {
        let criteria = match &self.review {
            Some(c) => c,
            None => {
                return AssertionResult::fail(
                    "stdout review",
                    "no review criteria specified",
                )
            }
        };

        let grader = match &self.grader {
            Some(g) => g.clone(),
            None => {
                return AssertionResult::fail(
                    format!("stdout review: \"{}\"", criteria),
                    "no grading agent configured (call .with_grader())",
                )
            }
        };

        let config = ReviewConfig {
            criteria: criteria.clone(),
            threshold: self.threshold,
            model: self.model.clone(),
        };

        let result = review::grade_stdout(&self.stdout, &config, |prompt, model| {
            grader.grade(prompt, model)
        });

        match result {
            Ok(review_result) => {
                let description = format!(
                    "stdout review: \"{}\" (score: {}/10, threshold: {})",
                    criteria, review_result.score, self.threshold,
                );
                if review_result.passed {
                    AssertionResult::pass(description)
                } else {
                    AssertionResult::fail(description, review_result.reasoning)
                }
            }
            Err(e) => AssertionResult::fail(
                format!("stdout review: \"{}\"", criteria),
                format!("grading failed: {}", e),
            ),
        }
    }

    /// Async version of evaluate for parallel processing.
    ///
    /// This method uses the async grading pipeline for better performance
    /// when multiple assertions are evaluated concurrently.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = assertion.evaluate_async().await;
    /// ```
    pub async fn evaluate_async(&self) -> AssertionResult {
        let criteria = match &self.review {
            Some(c) => c,
            None => {
                return AssertionResult::fail(
                    "stdout review",
                    "no review criteria specified",
                )
            }
        };

        let grader = match &self.grader {
            Some(g) => g.clone(),
            None => {
                return AssertionResult::fail(
                    format!("stdout review: \"{}\"", criteria),
                    "no grading agent configured (call .with_grader())",
                )
            }
        };

        let config = ReviewConfig {
            criteria: criteria.clone(),
            threshold: self.threshold,
            model: self.model.clone(),
        };

        let result = review::grade_stdout_async(&self.stdout, &config, |prompt, model| {
            let grader_clone = grader.clone();
            async move {
                grader_clone.grade_async(&prompt, model.as_deref()).await
            }
        }).await;

        match result {
            Ok(review_result) => {
                let description = format!(
                    "stdout review: \"{}\" (score: {}/10, threshold: {})",
                    criteria, review_result.score, self.threshold,
                );
                if review_result.passed {
                    AssertionResult::pass(description)
                } else {
                    AssertionResult::fail(description, review_result.reasoning)
                }
            }
            Err(e) => AssertionResult::fail(
                format!("stdout review: \"{}\"", criteria),
                format!("grading failed: {}", e),
            ),
        }
    }

    /// Async version of to_pass for parallel processing.
    ///
    /// Panics with a detailed error message if the assertion fails.
    /// Uses async grading for better performance.
    ///
    /// # Panics
    ///
    /// Panics if the review score is below the threshold or grading fails.
    pub async fn to_pass_async(&self) {
        let result = self.evaluate_async().await;
        if !result.passed {
            self.panic_with_context(&result);
        }
    }

    // =========================================================================
    // Internal helpers
    // =========================================================================

    fn format_stdout_preview(&self) -> String {
        match &self.stdout {
            Some(s) if !s.is_empty() => {
                if s.len() > 100 {
                    format!("\"{}...\"", &s[..97])
                } else {
                    format!("\"{}\"", s)
                }
            }
            _ => "(empty)".to_string(),
        }
    }

    fn panic_with_context(&self, result: &AssertionResult) -> ! {
        let reason = result.reason.as_deref().unwrap_or("unknown reason");
        let preview = self.format_stdout_preview();
        panic!(
            "assertion failed: {}\n\n  reason: {}\n  stdout: {}\n",
            result.description, reason, preview
        );
    }
}

/// Advanced builder for stdout assertions with full configuration control.
///
/// This builder provides the full builder pattern for cases where you need
/// fine-grained control over all configuration options. For simple cases,
/// use [`StdoutAssertion::review`] instead.
#[derive(Clone)]
pub struct StdoutAssertionBuilder {
    stdout: Option<String>,
    review: Option<String>,
    threshold: u32,
    model: Option<String>,
    grader: Option<Arc<dyn Agent>>,
}

impl std::fmt::Debug for StdoutAssertionBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdoutAssertionBuilder")
            .field("stdout", &self.stdout)
            .field("review", &self.review)
            .field("threshold", &self.threshold)
            .field("model", &self.model)
            .field("grader", &self.grader.as_ref().map(|g| g.name()))
            .finish()
    }
}

impl StdoutAssertionBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            stdout: None,
            review: None,
            threshold: 7,
            model: None,
            grader: None,
        }
    }

    /// Set the stdout content to be graded.
    pub fn stdout(mut self, stdout: Option<String>) -> Self {
        self.stdout = stdout;
        self
    }

    /// Set the review criteria.
    pub fn review(mut self, criteria: &str) -> Self {
        self.review = Some(criteria.to_string());
        self
    }

    /// Set the minimum score threshold (1-10).
    pub fn with_threshold(mut self, threshold: u32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set the model override for the grading agent.
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Set the agent to use for grading.
    pub fn with_grader(mut self, agent: Arc<dyn Agent>) -> Self {
        self.grader = Some(agent);
        self
    }

    /// Build the final stdout assertion.
    pub fn build(self) -> StdoutAssertion {
        StdoutAssertion {
            stdout: self.stdout,
            review: self.review,
            threshold: self.threshold,
            model: self.model,
            grader: self.grader,
        }
    }
}

impl Default for StdoutAssertionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use crate::agents::{ExecutionConfig, RawExecutionResult, ToolNameMapping};
    use crate::parser::ToolCall;

    /// A mock agent that returns a predetermined grading response.
    struct MockAgent {
        response: String,
    }

    impl MockAgent {
        fn passing() -> Arc<dyn Agent> {
            Arc::new(Self {
                response: r#"{"score": 9, "reasoning": "Meets criteria well"}"#.to_string(),
            })
        }

        fn failing() -> Arc<dyn Agent> {
            Arc::new(Self {
                response: r#"{"score": 3, "reasoning": "Does not meet criteria"}"#.to_string(),
            })
        }

        fn with_score(score: u32) -> Arc<dyn Agent> {
            Arc::new(Self {
                response: format!(r#"{{"score": {}, "reasoning": "score {}"}}"#, score, score),
            })
        }
    }

    #[async_trait::async_trait]
    impl Agent for MockAgent {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn execute(&self, _prompt: &str, _config: &ExecutionConfig) -> Result<RawExecutionResult> {
            unimplemented!("not needed for grading tests")
        }

        fn parse_session(&self, _result: &RawExecutionResult) -> Result<Vec<ToolCall>> {
            unimplemented!("not needed for grading tests")
        }

        fn tool_mapping(&self) -> &ToolNameMapping {
            unimplemented!("not needed for grading tests")
        }

        fn is_available(&self) -> bool {
            true
        }

        fn grade(&self, _prompt: &str, _model: Option<&str>) -> Result<String> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn test_review_passing() {
        let assertion = StdoutAssertion::new(Some("Task completed".to_string()))
            .review("should confirm completion")
            .with_grader(MockAgent::passing());

        let result = assertion.evaluate();
        assert!(result.passed);
        assert!(result.description.contains("score: 9/10"));
    }

    #[test]
    fn test_review_failing() {
        let assertion = StdoutAssertion::new(Some("Error occurred".to_string()))
            .review("should confirm success")
            .with_grader(MockAgent::failing());

        let result = assertion.evaluate();
        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("Does not meet criteria"));
    }

    #[test]
    fn test_review_threshold() {
        let assertion = StdoutAssertion::new(Some("test".to_string()))
            .review("test criteria")
            .with_threshold(8)
            .with_grader(MockAgent::with_score(7));

        let result = assertion.evaluate();
        assert!(!result.passed); // 7 < threshold 8
    }

    #[test]
    fn test_review_no_criteria() {
        let assertion = StdoutAssertion::new(Some("test".to_string()))
            .with_grader(MockAgent::passing());

        let result = assertion.evaluate();
        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("no review criteria"));
    }

    #[test]
    fn test_review_no_grader() {
        let assertion = StdoutAssertion::new(Some("test".to_string()))
            .review("should work");

        let result = assertion.evaluate();
        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("no grading agent"));
    }

    #[test]
    fn test_review_empty_stdout() {
        let assertion = StdoutAssertion::new(None)
            .review("should have no output")
            .with_grader(MockAgent::passing());

        let result = assertion.evaluate();
        assert!(result.passed);
    }

    #[test]
    fn test_review_with_model() {
        // Verify model is stored (actual model passing tested in review module)
        let assertion = StdoutAssertion::new(Some("test".to_string()))
            .review("criteria")
            .with_model("claude-sonnet-4-20250514")
            .with_grader(MockAgent::passing());

        assert_eq!(assertion.model, Some("claude-sonnet-4-20250514".to_string()));
        let result = assertion.evaluate();
        assert!(result.passed);
    }

    #[test]
    #[should_panic(expected = "assertion failed")]
    fn test_to_pass_panics_on_failure() {
        StdoutAssertion::new(Some("bad output".to_string()))
            .review("should be good")
            .with_grader(MockAgent::failing())
            .to_pass();
    }
}
