---
status: pending
priority: p2
issue_id: 003
tags: [code-review, reliability, error-handling]
dependencies: []
---

# Architecture: Improve Error Handling and Resilience in LLM Grading

## Problem Statement

The LLM grading system lacks comprehensive error handling for various failure modes including subprocess failures, JSON parsing errors, network timeouts, and LLM service unavailability. This creates brittle test execution where transient failures can cause entire test suites to fail unexpectedly.

## Findings

**Error Handling Analysis:**
- Limited error recovery in `grade_stdout()` function
- No retry logic for transient subprocess or network failures
- JSON parsing errors not gracefully handled with fallback scoring
- No timeout handling for long-running LLM requests
- Missing graceful degradation when LLM services unavailable

**Evidence from Code:**
- `src/review.rs` - Basic error propagation without recovery
- No configurable retry policies or circuit breaker patterns
- Subprocess failures immediately bubble up as test failures
- No fallback mechanisms for service degradation scenarios

**Failure Scenarios:**
- Claude CLI not available or misconfigured
- Network connectivity issues
- LLM service rate limiting or temporary unavailability
- Malformed JSON responses from LLM
- Subprocess timeout or resource exhaustion

## Proposed Solutions

### Option 1: Comprehensive Retry and Circuit Breaking (Recommended)
**Effort:** Medium | **Risk:** Low | **Impact:** High

**Implementation:**
- Add configurable retry policies with exponential backoff
- Implement circuit breaker pattern for LLM service calls
- Add timeout configuration for subprocess and network calls
- Graceful degradation to regex fallback when LLM unavailable
- Comprehensive error logging and metrics

**Pros:**
- Significantly improves test reliability and user experience
- Handles transient failures gracefully without test suite disruption
- Configurable policies allow tuning for different environments

**Cons:**
- Adds complexity to error handling logic
- May mask underlying infrastructure problems
- Requires careful tuning of retry parameters

### Option 2: Fallback Scoring Mechanisms
**Effort:** Small | **Risk:** Low | **Impact:** Medium

**Implementation:**
- Default scoring when LLM grading fails
- Simple heuristic scoring based on output length/content
- User-configurable fallback behavior per test
- Clear indication when fallback scoring is used

**Pros:**
- Simple to implement and understand
- Maintains test execution continuity
- Clear visibility into when fallbacks occur

**Cons:**
- May give false confidence in test results
- Fallback scoring less accurate than LLM grading
- Doesn't address root cause reliability issues

### Option 3: Async Resilience with Queue-Based Processing
**Effort:** Large | **Risk:** Medium | **Impact:** High

**Implementation:**
- Async job queue for LLM grading requests
- Dead letter queues for failed requests
- Worker pool with health monitoring
- Persistent retry state across test runs

**Pros:**
- Highly resilient to various failure modes
- Scalable across multiple concurrent test runs
- Sophisticated failure analysis and recovery

**Cons:**
- Significant architectural complexity
- Requires external queue infrastructure
- More difficult to debug and monitor

## Recommended Action

*[To be filled during triage]*

## Technical Details

**Affected Files:**
- `src/review.rs` - Core error handling and retry logic
- `src/agents/mod.rs` - Agent trait error handling contracts
- `src/fluent/stdout.rs` - Assertion evaluation error recovery
- Configuration files - Retry policy and timeout settings

**Error Handling Patterns:**
- Exponential backoff retry with jitter
- Circuit breaker state management
- Timeout configuration per operation type
- Structured error types with context

**Monitoring Requirements:**
- Retry attempt counts and success rates
- Circuit breaker state transitions
- Fallback activation frequency
- Performance impact of retry overhead

## Acceptance Criteria

- [ ] Transient subprocess failures automatically retried with backoff
- [ ] Circuit breaker prevents cascade failures from LLM service issues
- [ ] Configurable timeout handling for all external calls
- [ ] Graceful fallback scoring when LLM grading unavailable
- [ ] Comprehensive error logging with structured context
- [ ] Metrics tracking for retry rates and failure modes
- [ ] Documentation covers error handling configuration

## Work Log

**2026-02-22:** Issue identified during architecture review - insufficient error handling creates brittle test execution with poor resilience to transient failures.

## Resources

- **Retry Pattern:** Exponential backoff best practices
- **Circuit Breaker:** Implementation patterns in Rust
- **Timeout Handling:** Tokio timeout utilities and patterns
- **Error Types:** anyhow vs custom error types for context