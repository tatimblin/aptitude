---
title: "feat: Replace brittle stdout assertions with LLM-powered review"
type: feat
status: active
date: 2026-02-22
---

# Replace Brittle Stdout Assertions with LLM-Powered Review

## Overview

Replace the existing stdout assertion system (substring matching, regex) with an LLM-as-judge approach. Test authors write natural language criteria like `"should say task was successful and be no more than 10 words"`, and a separate Claude invocation grades the response on a 1-10 scale against that criteria.

## Problem Statement

The current stdout assertions (`contains`, `not_contains`, `matches`, `not_matches`) are brittle:

- They break on minor wording changes in agent output
- They can't evaluate semantic meaning ("was the tone helpful?")
- Regex patterns are hard to maintain and often over- or under-match
- They force test authors to predict exact output phrasing

LLM output is inherently variable. Assertions on it should evaluate intent and quality, not exact strings.

## Proposed Solution

Replace the `StdoutConstraints` struct and `StdoutAssertion` builder with a review-based system that:

1. Accepts a natural language `review` criteria string
2. Invokes a configurable Claude instance to grade stdout against the criteria
3. Returns a 1-10 score
4. Passes/fails based on a configurable threshold (default: 7)

### YAML Syntax (New)

```yaml
assertions:
  - stdout:
      review: "should say the task was completed successfully and be concise"
      threshold: 7                    # optional, default: 7 (1-10 scale)
      model: "claude-sonnet-4-20250514"     # optional, passed as --model to grading agent
      agent: "claude"                 # optional, defaults to "claude"
```

### YAML Syntax (Removed)

```yaml
# These fields are removed entirely:
assertions:
  - stdout:
      exists: true           # REMOVED
      contains: "success"    # REMOVED
      not_contains: "error"  # REMOVED
      matches: "pattern"     # REMOVED
      not_matches: "pattern" # REMOVED
```

### Fluent API (New)

```rust
// Basic review
expect(&output)
    .stdout()
    .review("should confirm the file was created")
    .to_pass();

// With configuration
expect(&output)
    .stdout()
    .review("should list exactly 3 items")
    .with_threshold(8)
    .with_model("claude-sonnet-4-20250514")
    .to_pass();

// Non-panicking
let result = expect(&output)
    .stdout()
    .review("should be a valid JSON object")
    .evaluate();
```

### Fluent API (Removed)

```rust
// These methods are removed entirely:
.contains("text")     // REMOVED
.not_contains("text") // REMOVED
.matches("pattern")   // REMOVED
.not_matches("pattern") // REMOVED
.to_exist()           // REMOVED
.to_be_empty()        // REMOVED
.evaluate_empty()     // REMOVED
```

## Technical Approach

### Architecture: Grading Through the Agent Adapter

The grading invocation must go through the `Agent` trait so that different agents (Claude, Aider, Cursor) can implement their own CLI command for grading. The flow is:

```
StdoutAssertion / YAML runner
    → review.rs (prompt construction + response parsing)
        → Agent::grade() (agent-specific CLI invocation)
```

### Modified: `src/agents/mod.rs` — New `grade` method on Agent trait

Add a lightweight grading method to the `Agent` trait. Unlike `execute()`, this skips session log management and only returns the text response.

```rust
pub trait Agent: Send + Sync {
    fn name(&self) -> &'static str;
    fn execute(&self, prompt: &str, config: &ExecutionConfig) -> Result<RawExecutionResult>;
    fn parse_session(&self, result: &RawExecutionResult) -> Result<Vec<ToolCall>>;
    fn tool_mapping(&self) -> &ToolNameMapping;
    fn is_available(&self) -> bool;

    /// Run a prompt and return only the text response (no session log tracking).
    ///
    /// Used for grading/review where we only need the LLM's text output.
    /// Accepts an optional model override.
    fn grade(&self, prompt: &str, model: Option<&str>) -> Result<String>;
}
```

### Modified: `src/agents/claude.rs` — Implement `grade` for ClaudeAdapter

A lightweight invocation that skips session log discovery:

```rust
impl Agent for ClaudeAdapter {
    // ... existing methods unchanged ...

    fn grade(&self, prompt: &str, model: Option<&str>) -> Result<String> {
        let mut cmd = Command::new("claude");
        cmd.arg("--print").arg(prompt).stdin(Stdio::null());

        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        let output = cmd.output().context("Failed to execute claude command for grading")?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if stdout.trim().is_empty() {
            anyhow::bail!("Grading agent returned empty response");
        }

        Ok(stdout)
    }
}
```

Future agents (Aider, Cursor) would implement `grade()` with their own CLI invocation.

### Modified: `src/agents/harness.rs` — Expose grading through facade

Add a `grade` method to `AgentHarness` that resolves the agent and delegates:

```rust
impl AgentHarness {
    // ... existing methods unchanged ...

    /// Grade content using the specified agent.
    ///
    /// Resolves the agent type and delegates to `Agent::grade()`.
    pub fn grade(
        &self,
        agent_type: Option<AgentType>,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String> {
        let agent_type = agent_type.unwrap_or(self.default_agent);
        let agent = self.agents.get(&agent_type)
            .ok_or_else(|| anyhow::anyhow!("Agent not registered: {:?}", agent_type))?;

        if !agent.is_available() {
            bail!("Agent '{}' is not available on this system", agent.name());
        }

        agent.grade(prompt, model)
    }
}
```

### New Module: `src/review.rs` — Prompt Construction + Response Parsing

This module handles the grading protocol (prompt template, JSON parsing) but delegates CLI invocation to the agent. It does NOT know how to run `claude` — it receives a grading function.

```rust
// src/review.rs

use anyhow::{Context, Result};
use serde::Deserialize;

pub struct ReviewConfig {
    pub criteria: String,
    pub threshold: u32,        // 1-10, default 7
    pub model: Option<String>, // --model flag passed to agent
}

pub struct ReviewResult {
    pub score: u32,       // 1-10
    pub reasoning: String,
    pub passed: bool,     // score >= threshold
}

#[derive(Deserialize)]
struct GradingResponse {
    score: u32,
    reasoning: String,
}

/// Build the grading prompt from criteria and stdout.
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

/// Grade stdout against criteria, using the provided grading function.
///
/// The `grader` function takes a prompt and model, and returns the raw text response.
/// This decouples the review logic from the specific agent implementation.
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

    // Parse JSON response (handle markdown code fences)
    let json_str = extract_json(&response);
    let parsed: GradingResponse = serde_json::from_str(json_str)
        .context("Failed to parse grading response as JSON")?;

    let score = parsed.score.clamp(1, 10);
    Ok(ReviewResult {
        score,
        reasoning: parsed.reasoning,
        passed: score >= config.threshold,
    })
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
```

The `grader` function parameter is the key design element — callers pass in a closure that wraps `Agent::grade()`, keeping `review.rs` decoupled from the agent system.

**Grading prompt template:**

```
You are a test grader. Evaluate the following output against the given criteria.

Criteria: {criteria}

Output to evaluate:
---
{stdout or "(empty - no output was produced)"}
---

Rate how well the output meets the criteria on a scale of 1-10:
- 1-3: Clearly fails the criteria
- 4-6: Partially meets the criteria
- 7-9: Meets the criteria well
- 10: Perfectly meets the criteria

Respond with ONLY a JSON object, no other text:
{"score": <number>, "reasoning": "<brief explanation>"}
```

### Modified: `src/yaml/parser.rs`

Replace `StdoutConstraints` fields:

```rust
/// Constraints for stdout assertions (LLM-graded review).
#[derive(Debug, Deserialize, Clone)]
pub struct StdoutConstraints {
    /// Natural language criteria for grading stdout.
    pub review: String,
    /// Minimum score to pass (1-10, default: 7).
    #[serde(default = "default_threshold")]
    pub threshold: u32,
    /// Model to use for grading (passed as --model to agent).
    pub model: Option<String>,
    /// Agent to use for grading (default: "claude").
    pub agent: Option<String>,
}

fn default_threshold() -> u32 {
    7
}
```

Remove: `exists`, `contains`, `not_contains`, `matches`, `not_matches` fields and `default_true()`.

### Modified: `src/fluent/stdout.rs`

Replace the builder to use review-based evaluation. The key change is that `StdoutAssertion` holds an `Arc<dyn Agent>` to delegate grading to the correct agent adapter.

```rust
use std::sync::Arc;
use crate::agents::Agent;
use crate::review::{self, ReviewConfig};

pub struct StdoutAssertion {
    stdout: Option<String>,
    review: Option<String>,
    threshold: u32,
    model: Option<String>,
    grader: Option<Arc<dyn Agent>>,
}

impl StdoutAssertion {
    pub fn new(stdout: Option<String>) -> Self {
        Self {
            stdout,
            review: None,
            threshold: 7,
            model: None,
            grader: None,
        }
    }

    // Builder methods (chainable)

    pub fn review(mut self, criteria: &str) -> Self {
        self.review = Some(criteria.to_string());
        self
    }

    pub fn with_threshold(mut self, threshold: u32) -> Self {
        self.threshold = threshold;
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Set the agent to use for grading.
    ///
    /// This determines which CLI command is used to invoke the grading LLM.
    /// If not set, evaluate() will fail with a descriptive error.
    pub fn with_grader(mut self, agent: Arc<dyn Agent>) -> Self {
        self.grader = Some(agent);
        self
    }

    // Assertion methods (panic on failure)
    pub fn to_pass(&self) {
        let result = self.evaluate();
        if !result.passed {
            self.panic_with_context(&result);
        }
    }

    // Non-panicking evaluation
    pub fn evaluate(&self) -> AssertionResult {
        let criteria = match &self.review {
            Some(c) => c,
            None => return AssertionResult::fail(
                "stdout review".to_string(),
                "no review criteria specified".to_string(),
            ),
        };

        let grader = match &self.grader {
            Some(g) => g.clone(),
            None => return AssertionResult::fail(
                format!("stdout review: \"{}\"", criteria),
                "no grading agent configured (call .with_grader())".to_string(),
            ),
        };

        let config = ReviewConfig {
            criteria: criteria.clone(),
            threshold: self.threshold,
            model: self.model.clone(),
        };

        // Delegate CLI invocation to the agent via closure
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
            Err(e) => {
                AssertionResult::fail(
                    format!("stdout review: \"{}\"", criteria),
                    format!("grading failed: {}", e),
                )
            }
        }
    }
}
```

Remove: `contains()`, `not_contains()`, `matches()`, `not_matches()`, `to_exist()`, `to_be_empty()`, `evaluate_empty()`, and the internal `evaluate_exists()`.

### Modified: `src/fluent/builder.rs` — Thread agent into StdoutAssertion

`ExecutionExpectation` needs access to the grading agent so it can pass it to `StdoutAssertion`:

```rust
pub struct ExecutionExpectation {
    tool_calls: Vec<ToolCall>,
    stdout: Option<String>,
    grader: Option<Arc<dyn Agent>>,  // NEW
}

impl ExecutionExpectation {
    pub fn new(output: &ExecutionOutput) -> Self {
        Self {
            tool_calls: output.result.tool_calls.clone(),
            stdout: output.stdout.clone(),
            grader: None,
        }
    }

    /// Set the grading agent for stdout review assertions.
    pub fn with_grader(mut self, agent: Arc<dyn Agent>) -> Self {
        self.grader = Some(agent);
        self
    }

    pub fn stdout(&self) -> StdoutAssertion {
        let mut assertion = StdoutAssertion::new(self.stdout.clone());
        if let Some(grader) = &self.grader {
            assertion = assertion.with_grader(grader.clone());
        }
        assertion
    }
}
```

**Fluent API usage with grader:**

```rust
let harness = AgentHarness::new();
let grader = harness.get_agent(AgentType::Claude).unwrap().clone();

expect(&output)
    .with_grader(grader)
    .stdout()
    .review("should confirm the file was created")
    .to_pass();
```

### Modified: `src/yaml/runner.rs` — Pass agent through for grading

The `run_yaml_test` function signature changes to accept an optional grading agent:

```rust
pub fn run_yaml_test(
    test: &Test,
    tool_calls: &[ToolCall],
    stdout: &Option<String>,
    grader: Option<&Arc<dyn Agent>>,  // NEW — agent used for stdout review grading
) -> Vec<(String, TestResult)> {
    // ...
    for assertion in &test.assertions {
        if let Some(stdout_constraints) = &assertion.stdout {
            let description = format_stdout_description(stdout_constraints);
            let result = evaluate_stdout_assertion(stdout_constraints, stdout, grader);
            results.push((description, result));
            continue;
        }
        // ... tool assertions unchanged ...
    }
}
```

Update `evaluate_stdout_assertion` to resolve the agent and delegate:

```rust
fn evaluate_stdout_assertion(
    constraints: &StdoutConstraints,
    stdout: &Option<String>,
    grader: Option<&Arc<dyn Agent>>,
) -> TestResult {
    let grader = match grader {
        Some(g) => g,
        None => return TestResult::Fail {
            reason: "No grading agent available for stdout review".to_string(),
        },
    };

    // Resolve model: stdout constraint's model field takes precedence
    let model = constraints.model.as_deref();

    let config = ReviewConfig {
        criteria: constraints.review.clone(),
        threshold: constraints.threshold,
        model: constraints.model.clone(),
    };

    let grader_ref = grader.clone();
    let result = review::grade_stdout(&stdout, &config, |prompt, model| {
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
```

Update `format_stdout_description`:

```rust
fn format_stdout_description(constraints: &StdoutConstraints) -> String {
    format!("stdout review: \"{}\" (threshold: {}/10)", constraints.review, constraints.threshold)
}
```

### Modified: `src/main.rs` — Pass grading agent to `run_yaml_test`

The call sites in `run_single_test` and `analyze_session` need to resolve the grading agent:

```rust
// In run_single_test:
let grading_agent = harness.get_agent(agent_type.unwrap_or(AgentType::Claude));
let results = run_yaml_test(&test, &tool_calls, &raw_result.stdout, grading_agent.as_ref());

// In analyze_session (stdout not available, but grader still needed for completeness):
let grading_agent = harness.get_agent(agent_type);
let results = run_yaml_test(&test, &tool_calls, &None, grading_agent.as_ref());
```

The `agent` field on `StdoutConstraints` allows overriding which agent does the grading (e.g., use claude for grading even when the test runs aider). When present, the runner resolves it from the harness instead of using the test's default agent. This is a straightforward lookup in `run_yaml_test` using the existing `AgentHarness::get_agent`.

### Modified: `src/lib.rs`

Export the new review module:

```rust
pub mod review;
pub use review::{ReviewConfig, ReviewResult, grade_stdout};
```

### Test Result Display

Pass:
```
  ✓ stdout review: "should confirm success" (score: 9/10, threshold: 7)
```

Fail:
```
  ✗ stdout review: "should be under 10 words" (score: 3/10, threshold: 7)
    └─ The output contains 28 words, exceeding the 10-word limit
```

Grading error:
```
  ✗ stdout review: "should confirm success"
    └─ grading failed: Failed to execute claude command
```

## Acceptance Criteria

- [ ] `Agent` trait has a `grade(&self, prompt: &str, model: Option<&str>) -> Result<String>` method
- [ ] `ClaudeAdapter` implements `grade` using `claude --print` (no session log tracking)
- [ ] `AgentHarness` exposes `grade()` method that resolves agent and delegates
- [ ] `StdoutConstraints` has `review` (required), `threshold` (default 7), `model` (optional), `agent` (optional) fields
- [ ] Old fields (`exists`, `contains`, `not_contains`, `matches`, `not_matches`) are fully removed from parser, fluent API, and runner
- [ ] `src/review.rs` handles prompt construction and JSON response parsing, delegates CLI invocation via a grader function parameter
- [ ] `--model` flag is passed through to the grading agent's `grade()` when specified
- [ ] `agent` field on `StdoutConstraints` allows overriding which agent does the grading
- [ ] Default threshold of 7 is applied when not specified
- [ ] Score is clamped to 1-10 range
- [ ] Grading failures (CLI error, parse error) produce `TestResult::Fail` with descriptive message
- [ ] `StdoutAssertion` fluent builder has `.review()`, `.with_threshold()`, `.with_model()`, `.with_grader()`, `.to_pass()`, `.evaluate()`
- [ ] `ExecutionExpectation` has `.with_grader()` that threads the agent into `StdoutAssertion`
- [ ] Fluent API old methods (`contains`, `not_contains`, `matches`, `not_matches`, `to_exist`, `to_be_empty`) are removed
- [ ] `run_yaml_test` accepts `grader: Option<&Arc<dyn Agent>>` parameter
- [ ] `main.rs` call sites pass the grading agent to `run_yaml_test`
- [ ] Unit tests cover: grading prompt construction, JSON response parsing, score clamping, threshold comparison, error handling
- [ ] Existing stdout-related tests are updated or replaced
- [ ] `docs/yaml-api.md` and `docs/fluent-api.md` are updated with new syntax

## Files to Modify

| File | Change |
|------|--------|
| `src/review.rs` | **NEW** — Prompt construction, JSON response parsing, `grade_stdout` with grader function parameter |
| `src/agents/mod.rs` | Add `grade()` method to `Agent` trait |
| `src/agents/claude.rs` | Implement `grade()` — lightweight `claude --print` invocation |
| `src/agents/harness.rs` | Add `grade()` facade method |
| `src/yaml/parser.rs` | Replace `StdoutConstraints` fields, remove `default_true()`, update tests |
| `src/fluent/stdout.rs` | Replace builder with review-based system using `Arc<dyn Agent>`, update all tests |
| `src/fluent/builder.rs` | Add `grader` field to `ExecutionExpectation`, thread into `StdoutAssertion` |
| `src/yaml/runner.rs` | Update `run_yaml_test` signature, `evaluate_stdout_assertion`, `format_stdout_description`, update tests |
| `src/main.rs` | Pass grading agent to `run_yaml_test` call sites |
| `src/lib.rs` | Add `pub mod review` and exports |
| `docs/yaml-api.md` | Update stdout assertion docs |
| `docs/fluent-api.md` | Update fluent stdout API docs |

## Dependencies

- No new crate dependencies. `serde_json` is already in `Cargo.toml` for parsing the grading response. `std::process::Command` is used for CLI invocation (same as `ClaudeAdapter`).
- Requires `claude` CLI (or the configured grading agent) to be available at grading time.
