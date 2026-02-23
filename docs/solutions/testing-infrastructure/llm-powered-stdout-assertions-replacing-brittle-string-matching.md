---
title: "Replace brittle stdout assertions with LLM-powered review system"
category: testing-infrastructure
component: test-harness/assertion-engine
severity: medium
tags: ["assertions", "llm-grading", "test-infrastructure", "api-evolution", "string-matching", "semantic-evaluation", "brittleness-fix", "claude-integration", "fluent-api", "yaml-configuration"]
date: 2026-02-22
status: solved
author: "Claude Opus 4.6"
cross_references:
  - "performance-issues/async-subprocess-execution"
  - "api-design/tiered-fluent-interfaces"
related_commits: ["f411d03"]
related_issues: []
performance_impact: "high"
---

# LLM-Powered Stdout Assertions: Replacing Brittle String Matching

## Problem Summary

The agent execution harness used brittle stdout assertion mechanisms that relied on exact string matching and regex patterns. This caused critical testing issues including test fragility, maintenance burden, limited evaluation scope, and unpredictable output requirements that broke when agents produced semantically correct but differently worded responses.

### Technical Symptoms

**Test Fragility**: Tests broke on minor wording changes in agent output, even when semantic meaning remained correct
**Maintenance Burden**: Regex patterns were difficult to maintain and often produced false positives/negatives
**Limited Evaluation Scope**: The system couldn't evaluate semantic qualities like tone, helpfulness, or intent - only exact text matches
**Prediction Requirements**: Test authors had to predict exact output phrasing, which is impossible with variable LLM responses

**Technical Impact**: Tests failed due to trivial output variations rather than actual functional problems, test maintenance overhead increased dramatically, and developer productivity suffered from false test failures.

## Root Cause Analysis

The fundamental problem was that the original stdout assertion system used brittle string matching (substring, regex patterns) that broke on minor wording changes in LLM output. Since LLM output is inherently variable, assertions should evaluate semantic meaning and quality, not exact strings.

**Key pain points:**
- Tests failed when agents produced semantically correct but differently worded responses
- Regex patterns were hard to maintain and often over/under-matched
- Test authors had to predict exact output phrasing
- No way to evaluate subjective criteria like tone, conciseness, or helpfulness

## Working Solution

### LLM-as-Judge Architecture

The solution replaces brittle string matching with **LLM-as-Judge** evaluation, where test authors write natural language criteria and a separate Claude instance grades the response on a 1-10 scale.

```
YAML Test / Fluent API
    ↓
review.rs (prompt construction + JSON parsing)
    ↓
Agent::grade() (agent-specific CLI invocation)
    ↓
claude --print <grading_prompt> [--model <model>]
    ↓
JSON response: {"score": 8, "reasoning": "..."}
    ↓
Pass/Fail based on threshold (default: 7)
```

### Implementation Details

#### 1. Core Review Module (`src/review.rs`)

```rust
pub struct ReviewConfig {
    pub criteria: String,      // Natural language criteria
    pub threshold: u32,        // 1-10, default 7
    pub model: Option<String>, // --model flag passed to agent
}

pub struct ReviewResult {
    pub score: u32,       // 1-10 (clamped)
    pub reasoning: String, // LLM's explanation
    pub passed: bool,     // score >= threshold
}

pub fn grade_stdout<F>(
    stdout: &Option<String>,
    config: &ReviewConfig,
    grader: F,  // Function that wraps Agent::grade()
) -> Result<ReviewResult>
where
    F: FnOnce(&str, Option<&str>) -> Result<String>,
{
    let prompt = build_grading_prompt(stdout, &config.criteria);
    let response = grader(&prompt, config.model.as_deref())?;

    let json_str = extract_json(&response);
    let parsed: GradingResponse = serde_json::from_str(json_str)?;

    let score = parsed.score.clamp(1, 10);
    Ok(ReviewResult {
        score,
        reasoning: parsed.reasoning,
        passed: score >= config.threshold,
    })
}
```

#### 2. Agent Trait Extension

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    // ... existing methods ...

    /// Run a prompt and return only the text response (no session log tracking).
    fn grade(&self, prompt: &str, model: Option<&str>) -> Result<String>;

    /// Async version for parallel processing.
    async fn grade_async(&self, prompt: &str, model: Option<&str>) -> Result<String>;
}
```

#### 3. Tiered Fluent API Design

**Simple Constructor (90% of use cases):**
```rust
let assertion = StdoutAssertion::with_review(
    Some("Task completed successfully".to_string()),
    "should confirm task completion"
).with_grader(agent);
```

**Advanced Builder (complex configurations):**
```rust
let assertion = StdoutAssertion::builder()
    .stdout(Some("output".to_string()))
    .review("should meet all criteria")
    .with_threshold(9)
    .with_model("claude-opus-4")
    .with_grader(agent)
    .build();
```

#### 4. YAML API Transformation

**Before (Brittle String Matching):**
```yaml
assertions:
  - stdout:
      contains: "success"    # REMOVED
      not_contains: "error"  # REMOVED
      matches: "pattern"     # REMOVED
```

**After (LLM-Powered Review):**
```yaml
assertions:
  - stdout:
      review: "should say the task was completed successfully and be concise"
      threshold: 7                    # optional, default: 7 (1-10 scale)
      model: "claude-sonnet-4-20250514"     # optional, passed as --model
      agent: "claude"                 # optional, defaults to "claude"
```

### Performance Optimizations

Added async grading pipeline with parallel execution to address sequential grading bottlenecks:

```rust
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

    futures::future::try_join_all(futures).await
}
```

## Key Files Modified

- `src/review.rs` - **NEW**: Core LLM grading module (308 lines)
- `src/agents/mod.rs` - Added `grade()` and `grade_async()` methods to Agent trait
- `src/agents/claude.rs` - Implemented grading methods using `claude --print`
- `src/agents/harness.rs` - Added grading facade methods
- `src/yaml/parser.rs` - Replaced StdoutConstraints fields with review-based fields
- `src/fluent/stdout.rs` - **Complete rewrite**: Tiered API with async support (382 lines)
- `src/fluent/builder.rs` - Added grader field to ExecutionExpectation
- `src/yaml/runner.rs` - Updated to accept grading agent and use LLM evaluation
- `examples/increment-number/` - **NEW**: Working example demonstrating LLM review

## Working Example

File: `/examples/increment-number/test.aptitude.yaml`
```yaml
name: "Increment number and report"
prompt: "Read counter.txt, run increment.sh to increment the number, and tell me the new value."

assertions:
  - tool: Read
    called: true
    params:
      file_path: "counter.txt"

  - tool: Bash
    called: true
    params:
      command: ".*increment\\.sh.*"

  - tool: Bash
    called_after: Read

  - stdout:
      review: "should state the number was incremented and report the new value (43), in no more than two sentences"
      threshold: 6
```

This replaces brittle assertions like:
```yaml
# OLD (brittle): Multiple narrow assertions that break easily
- stdout:
    contains: "incremented"
    contains: "43"
    not_contains: "error"

# NEW (semantic): Single assertion focusing on intent
- stdout:
    review: "should state the number was incremented and report the new value (43), in no more than two sentences"
```

## Prevention Strategies

### Best Practices for Semantic Assertions

1. **Intent-Driven Criteria**: Focus on what the output should achieve, not how it should look
```yaml
# GOOD: Focus on intent and meaning
review: "should confirm the task completed successfully and provide actionable next steps"

# BAD: Brittle pattern matching
contains: "Task completed successfully at"
```

2. **Tolerance-Based Scoring**: Use appropriate thresholds for different requirements
- Threshold 6-7: Lenient semantic matching
- Threshold 8-9: Precise requirements (JSON format, specific data)
- Threshold 10: Exact structured output (use sparingly)

3. **Multi-Level Assertion Strategy**:
```yaml
# Layer 1: Core Intent (threshold 6-7)
- stdout:
    review: "should confirm the operation completed successfully"
    threshold: 7

# Layer 2: Specific Requirements (threshold 7-8)
- stdout:
    review: "should include the file path and indicate no errors occurred"
    threshold: 8
```

### Anti-patterns to Avoid

- Never use exact substring matches for LLM output
- Avoid regex patterns that over-specify format details
- Don't create assertions that fail on minor formatting changes
- Resist temptation to hardcode specific word choices that may vary

### Performance Monitoring

**Performance Bottleneck Detection:**
```rust
pub struct GradingMetrics {
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub timeout_rate: f64,
    pub error_rate: f64,
}

impl GradingMetrics {
    pub fn is_performance_degraded(&self) -> bool {
        self.average_latency_ms > 5000.0 ||  // 5 second average
        self.p95_latency_ms > 15000.0 ||     // 15 second p95
        self.timeout_rate > 0.05 ||          // 5% timeout rate
        self.error_rate > 0.10               // 10% error rate
    }
}
```

**Batch Processing Strategy:**
```rust
pub fn should_use_batch_processing(assertion_count: usize) -> bool {
    assertion_count >= 5  // Threshold for parallel processing
}
```

## Test Cases for Validation

### Essential Test Categories

**1. Threshold Boundary Tests**: Test score exactly at threshold vs one below
**2. Natural Language Variation Tests**: Test semantic equivalence across different phrasings
**3. Edge Case Robustness Tests**: Empty output, very long output, special characters
**4. Multi-Language Output Tests**: Cross-language semantic matching
**5. Grading Consistency Tests**: Verify consistent scoring for identical inputs
**6. Complex Criteria Interpretation Tests**: Test parsing of multi-part requirements

## Related Documentation

### Core Documentation
- `/docs/plans/2026-02-22-feat-llm-powered-stdout-review-plan.md` - Technical specification
- `/docs/fluent-api.md` - Updated fluent API documentation with StdoutAssertion examples
- `/docs/yaml-api.md` - Updated YAML format documentation for stdout review assertions

### Performance and Issues
- `/todos/001-pending-p1-performance-llm-grading-bottleneck.md` - **Critical**: 100,000x slowdown from LLM grading
- `/todos/002-pending-p1-subprocess-security-validation.md` - Security validation concerns
- `/todos/003-pending-p2-error-handling-robustness.md` - Error handling robustness improvements
- `/todos/004-pending-p3-simplify-fluent-api-complexity.md` - API complexity reduction opportunities

### Similar Patterns
- **Jest-like API Design**: Fluent API explicitly models Jest's `expect()` pattern
- **Parameter Matchers**: Similar to Jest matchers with regex/glob pattern support
- **Agent Abstraction**: Consistent with existing `Agent::execute()` pattern

## Cross-References

- **Performance Issues**: This solution introduces performance considerations that require async optimization
- **API Design**: Represents significant evolution from string-based to semantic evaluation APIs
- **Testing Methodology**: Paradigm shift from exact matching to semantic understanding
- **LLM Integration**: Pattern for integrating LLM evaluation into testing infrastructure

## Lessons Learned

1. **Functional Composition**: Decoupling review logic from agent implementation through closure injection allows future agent support
2. **Tiered API Design**: Simple constructors for common cases + advanced builders for power users maximizes usability
3. **Performance Implications**: Semantic accuracy comes with latency trade-offs that require async architecture
4. **Breaking Changes**: Complete removal of old methods (vs deprecation) forces better design decisions
5. **Test Evolution**: Moving from brittle pattern matching to semantic evaluation requires cultural shift in how teams think about assertions

## Impact Assessment

**Positive Impact:**
- Tests pass when output is semantically correct but differently worded
- Natural language criteria like "be concise", "confirm success", "list exactly 3 items"
- Async parallel grading for multiple assertions
- Extensible agent abstraction for multiple CLI tools
- Tiered API from simple constructors to advanced builders

**Known Issues:**
- Performance bottleneck: 100,000x slower than regex (tracked in todos)
- Requires external LLM service availability
- Grading consistency depends on LLM service stability
- Higher resource usage and cost per test run

**Success Metrics:**
- All 117 tests pass with no regressions
- Example test demonstrates semantic evaluation working correctly
- API provides both simplicity and power through tiered design
- Foundation for async optimization and caching strategies

This solution transforms brittle string matching into flexible semantic evaluation, providing a testing system that works **with** the inherent variability of LLM output rather than fighting against it. The result is more resilient tests that focus on intent and meaning rather than exact phrasing, while maintaining precision through configurable scoring thresholds.