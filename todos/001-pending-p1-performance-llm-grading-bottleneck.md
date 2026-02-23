---
status: pending
priority: p1
issue_id: 001
tags: [code-review, performance, architecture]
dependencies: []
---

# Performance: LLM Grading Creates 100,000x Slowdown

## Problem Statement

The new LLM-powered stdout assertions introduce a critical performance bottleneck. Each assertion now requires a subprocess call to `claude --print` which takes 1-3 seconds compared to microsecond-level regex matching in the original implementation. This represents a 100,000x performance degradation that will significantly impact test suite execution times and user adoption.

## Findings

**Performance Analysis:**
- Original regex/substring matching: ~1-10 microseconds
- New LLM grading via subprocess: 1-3 seconds per assertion
- Multiplicative impact: Tests with multiple stdout assertions will compound the delay
- No async processing or caching implemented
- Each test run creates fresh subprocess calls with no optimization

**Evidence from Code:**
- `src/review.rs:grade_stdout()` - Synchronous subprocess call on line ~89
- `src/fluent/stdout.rs:evaluate()` - Blocking evaluation on line ~341
- No caching layer in review module
- No parallel processing for multiple assertions

## Proposed Solutions

### Option 1: Async Parallel Processing with Caching (Recommended)
**Effort:** Large | **Risk:** Medium | **Impact:** High

**Implementation:**
- Add async support to Agent trait with `async fn grade_async()`
- Implement tokio-based parallel execution for multiple assertions
- Add Redis/file-based caching layer keyed by (prompt_hash, model, agent)
- Batch multiple grading requests when possible

**Pros:**
- Maintains LLM semantic accuracy while drastically improving performance
- Caching provides near-instant results for repeated test runs
- Parallel execution minimizes total test time

**Cons:**
- Requires major architectural changes to support async
- Introduces caching complexity and potential cache invalidation issues
- External Redis dependency for optimal performance

### Option 2: Hybrid Fast/Semantic Mode
**Effort:** Medium | **Risk:** Low | **Impact:** Medium

**Implementation:**
- Add `fast_mode` option to stdout assertions
- Fast mode: Run simple regex/substring checks first
- Only invoke LLM for complex semantic requirements or when fast mode fails
- Intelligent fallback system

**Pros:**
- Backwards compatibility with existing performance
- Progressive enhancement model
- Reduced infrastructure complexity

**Cons:**
- Doesn't solve performance for semantic assertions
- Adds complexity to assertion logic
- Still requires LLM calls for advanced use cases

### Option 3: Background Processing with Eventual Consistency
**Effort:** Large | **Risk:** High | **Impact:** Medium

**Implementation:**
- Queue LLM grading requests to background workers
- Return immediate "pending" status for assertions
- Provide polling/webhook mechanism for results
- Async test result aggregation

**Pros:**
- Non-blocking test execution
- Scalable across multiple test runs

**Cons:**
- Complex eventual consistency model
- Harder to integrate with existing test flows
- Requires significant infrastructure changes

## Recommended Action

*[To be filled during triage]*

## Technical Details

**Affected Files:**
- `src/review.rs` - Core grading function needs async support
- `src/agents/mod.rs` - Agent trait needs async methods
- `src/fluent/stdout.rs` - Evaluation logic needs parallel execution
- `src/yaml/runner.rs` - Test runner needs async orchestration

**Components:**
- Review module subprocess management
- Agent trait method signatures
- Fluent API builder pattern
- YAML test execution engine

**Performance Metrics to Track:**
- Average assertion evaluation time
- Test suite total execution time
- Cache hit rates (if implemented)
- Parallel execution efficiency

## Acceptance Criteria

- [ ] LLM assertions complete in <100ms average (with caching)
- [ ] Test suites with 10+ stdout assertions complete in <5 seconds total
- [ ] Cache hit rate >80% for repeated test runs
- [ ] No regression in assertion accuracy
- [ ] Graceful degradation when LLM services unavailable
- [ ] Performance benchmarks documented

## Work Log

**2026-02-22:** Issue identified during code review - LLM subprocess calls creating 100,000x performance degradation compared to regex matching.

## Resources

- **PR:** [Link to stdout assertion PR]
- **Performance Comparison:** Original regex vs LLM timing analysis
- **Similar Patterns:** Look for async agent patterns in compound-engineering
- **Caching Strategies:** Redis vs file-based vs in-memory options