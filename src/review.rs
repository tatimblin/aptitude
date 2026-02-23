//! LLM-powered stdout review and grading.
//!
//! This module handles the grading protocol for stdout assertions:
//! prompt construction, JSON response parsing, and score evaluation.
//! CLI invocation is delegated to the caller via a grader function,
//! keeping this module decoupled from the agent system.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::future::Future;

/// Configuration for an LLM-powered stdout review.
#[derive(Debug, Clone)]
pub struct ReviewConfig {
    /// Natural language criteria for grading stdout.
    pub criteria: String,
    /// Minimum score to pass (1-10, default: 7).
    pub threshold: u32,
    /// Model override passed to the grading agent (e.g., `--model`).
    pub model: Option<String>,
}

/// Result of grading stdout against review criteria.
#[derive(Debug, Clone)]
pub struct ReviewResult {
    /// Score from 1-10.
    pub score: u32,
    /// Brief explanation from the grading LLM.
    pub reasoning: String,
    /// Whether the score meets the threshold.
    pub passed: bool,
}

#[derive(Deserialize)]
struct GradingResponse {
    score: u32,
    reasoning: String,
}

/// Build the grading prompt from criteria and stdout content.
pub fn build_grading_prompt(stdout: &Option<String>, criteria: &str) -> String {
    let output_text = match stdout {
        Some(s) if !s.is_empty() => s.as_str(),
        _ => "(empty - no output was produced)",
    };

    format!(
        r#"You are a test grader. Evaluate the following output against the given criteria.

Criteria: {criteria}

Output to evaluate:
---
{output_text}
---

Rate how well the output meets the criteria on a scale of 1-10:
- 1-3: Clearly fails the criteria
- 4-6: Partially meets the criteria
- 7-9: Meets the criteria well
- 10: Perfectly meets the criteria

Respond with ONLY a JSON object, no other text:
{{"score": <number>, "reasoning": "<brief explanation>"}}"#
    )
}

/// Grade stdout against criteria using the provided grading function.
///
/// The `grader` function takes a prompt and optional model, returning the raw
/// text response. This decouples the review logic from the specific agent
/// implementation — callers pass a closure wrapping `Agent::grade()`.
///
/// # Example
///
/// ```rust,ignore
/// let result = grade_stdout(&stdout, &config, |prompt, model| {
///     agent.grade(prompt, model)
/// })?;
/// ```
pub fn grade_stdout<F>(
    stdout: &Option<String>,
    config: &ReviewConfig,
    grader: F,
) -> Result<ReviewResult>
where
    F: FnOnce(&str, Option<&str>) -> Result<String>,
{
    let prompt = build_grading_prompt(stdout, &config.criteria);
    let response = grader(&prompt, config.model.as_deref())?;

    let json_str = extract_json(&response);
    let parsed: GradingResponse =
        serde_json::from_str(json_str).context("Failed to parse grading response as JSON")?;

    let score = parsed.score.clamp(1, 10);
    Ok(ReviewResult {
        score,
        reasoning: parsed.reasoning,
        passed: score >= config.threshold,
    })
}

/// Async version of grade_stdout for parallel processing.
///
/// The `grader` function returns a future for the grading operation.
///
/// # Example
///
/// ```rust,ignore
/// let result = grade_stdout_async(&stdout, &config, |prompt, model| {
///     Box::pin(agent.grade_async(prompt, model))
/// }).await?;
/// ```
pub async fn grade_stdout_async<F, Fut>(
    stdout: &Option<String>,
    config: &ReviewConfig,
    grader: F,
) -> Result<ReviewResult>
where
    F: FnOnce(String, Option<String>) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let prompt = build_grading_prompt(stdout, &config.criteria);
    let model = config.model.clone();
    let response = grader(prompt, model).await?;

    let json_str = extract_json(&response);
    let parsed: GradingResponse =
        serde_json::from_str(json_str).context("Failed to parse grading response as JSON")?;

    let score = parsed.score.clamp(1, 10);
    Ok(ReviewResult {
        score,
        reasoning: parsed.reasoning,
        passed: score >= config.threshold,
    })
}

/// Grade multiple stdout outputs in parallel for improved performance.
///
/// This function runs all grading requests concurrently and returns results
/// in the same order as the inputs. This provides significant speedup when
/// multiple stdout assertions need to be evaluated.
///
/// # Example
///
/// ```rust,ignore
/// let requests = vec![
///     (Some("output1".to_string()), config1),
///     (Some("output2".to_string()), config2),
/// ];
/// let results = grade_stdout_batch_async(&requests, |prompt, model| {
///     Box::pin(agent.grade_async(prompt, model))
/// }).await?;
/// ```
pub async fn grade_stdout_batch_async<F, Fut>(
    requests: &[(Option<String>, ReviewConfig)],
    grader: F,
) -> Result<Vec<ReviewResult>>
where
    F: Fn(String, Option<String>) -> Fut + Clone,
    Fut: Future<Output = Result<String>>,
{
    let futures: Vec<_> = requests
        .iter()
        .map(|(stdout, config)| {
            let grader = grader.clone();
            async move {
                grade_stdout_async(stdout, config, grader).await
            }
        })
        .collect();

    // Use try_join_all to execute all grading requests in parallel
    // and return the first error if any occurs
    futures::future::try_join_all(futures).await
}

/// Extract JSON from a response that might be wrapped in markdown code fences.
fn extract_json(response: &str) -> &str {
    let trimmed = response.trim();
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_grading_prompt_with_content() {
        let stdout = Some("Task completed successfully".to_string());
        let prompt = build_grading_prompt(&stdout, "should confirm success");
        assert!(prompt.contains("should confirm success"));
        assert!(prompt.contains("Task completed successfully"));
        assert!(!prompt.contains("(empty"));
    }

    #[test]
    fn test_build_grading_prompt_empty_stdout() {
        let stdout: Option<String> = None;
        let prompt = build_grading_prompt(&stdout, "should have output");
        assert!(prompt.contains("(empty - no output was produced)"));
    }

    #[test]
    fn test_build_grading_prompt_empty_string_stdout() {
        let stdout = Some("".to_string());
        let prompt = build_grading_prompt(&stdout, "should have output");
        assert!(prompt.contains("(empty - no output was produced)"));
    }

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"score": 8, "reasoning": "good"}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_with_code_fence() {
        let input = "```json\n{\"score\": 8, \"reasoning\": \"good\"}\n```";
        assert_eq!(extract_json(input), r#"{"score": 8, "reasoning": "good"}"#);
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let input = "Here is the result:\n{\"score\": 5, \"reasoning\": \"partial\"}\nDone.";
        assert_eq!(
            extract_json(input),
            r#"{"score": 5, "reasoning": "partial"}"#
        );
    }

    #[test]
    fn test_grade_stdout_passing() {
        let stdout = Some("Task done".to_string());
        let config = ReviewConfig {
            criteria: "should confirm completion".to_string(),
            threshold: 7,
            model: None,
        };

        let result = grade_stdout(&stdout, &config, |_prompt, _model| {
            Ok(r#"{"score": 9, "reasoning": "Output confirms completion"}"#.to_string())
        })
        .unwrap();

        assert_eq!(result.score, 9);
        assert!(result.passed);
        assert_eq!(result.reasoning, "Output confirms completion");
    }

    #[test]
    fn test_grade_stdout_failing() {
        let stdout = Some("Error occurred".to_string());
        let config = ReviewConfig {
            criteria: "should confirm success".to_string(),
            threshold: 7,
            model: None,
        };

        let result = grade_stdout(&stdout, &config, |_prompt, _model| {
            Ok(r#"{"score": 2, "reasoning": "Output indicates error, not success"}"#.to_string())
        })
        .unwrap();

        assert_eq!(result.score, 2);
        assert!(!result.passed);
    }

    #[test]
    fn test_grade_stdout_score_clamped() {
        let stdout = Some("test".to_string());
        let config = ReviewConfig {
            criteria: "test".to_string(),
            threshold: 7,
            model: None,
        };

        // Score above 10 gets clamped
        let result = grade_stdout(&stdout, &config, |_prompt, _model| {
            Ok(r#"{"score": 15, "reasoning": "over"}"#.to_string())
        })
        .unwrap();
        assert_eq!(result.score, 10);

        // Score of 0 gets clamped to 1
        let result = grade_stdout(&stdout, &config, |_prompt, _model| {
            Ok(r#"{"score": 0, "reasoning": "under"}"#.to_string())
        })
        .unwrap();
        assert_eq!(result.score, 1);
    }

    #[test]
    fn test_grade_stdout_threshold_boundary() {
        let stdout = Some("test".to_string());

        // Exactly at threshold should pass
        let config = ReviewConfig {
            criteria: "test".to_string(),
            threshold: 7,
            model: None,
        };
        let result = grade_stdout(&stdout, &config, |_, _| {
            Ok(r#"{"score": 7, "reasoning": "borderline"}"#.to_string())
        })
        .unwrap();
        assert!(result.passed);

        // One below threshold should fail
        let result = grade_stdout(&stdout, &config, |_, _| {
            Ok(r#"{"score": 6, "reasoning": "not enough"}"#.to_string())
        })
        .unwrap();
        assert!(!result.passed);
    }

    #[test]
    fn test_grade_stdout_passes_model_to_grader() {
        let stdout = Some("test".to_string());
        let config = ReviewConfig {
            criteria: "test".to_string(),
            threshold: 7,
            model: Some("claude-sonnet-4-20250514".to_string()),
        };

        let result = grade_stdout(&stdout, &config, |_prompt, model| {
            assert_eq!(model, Some("claude-sonnet-4-20250514"));
            Ok(r#"{"score": 8, "reasoning": "ok"}"#.to_string())
        })
        .unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_grade_stdout_grader_error() {
        let stdout = Some("test".to_string());
        let config = ReviewConfig {
            criteria: "test".to_string(),
            threshold: 7,
            model: None,
        };

        let result = grade_stdout(&stdout, &config, |_, _| {
            anyhow::bail!("CLI not found")
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_grade_stdout_invalid_json() {
        let stdout = Some("test".to_string());
        let config = ReviewConfig {
            criteria: "test".to_string(),
            threshold: 7,
            model: None,
        };

        let result = grade_stdout(&stdout, &config, |_, _| Ok("not json".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_grade_stdout_json_in_code_fence() {
        let stdout = Some("test".to_string());
        let config = ReviewConfig {
            criteria: "test".to_string(),
            threshold: 7,
            model: None,
        };

        let result = grade_stdout(&stdout, &config, |_, _| {
            Ok("```json\n{\"score\": 8, \"reasoning\": \"wrapped\"}\n```".to_string())
        })
        .unwrap();
        assert_eq!(result.score, 8);
        assert_eq!(result.reasoning, "wrapped");
    }
}
