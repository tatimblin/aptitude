---
status: pending
priority: p3
issue_id: 004
tags: [code-review, simplicity, api-design, refactor]
dependencies: []
---

# Code Quality: Simplify Fluent API Builder Complexity

## Problem Statement

The fluent API builder pattern in the stdout assertions has accumulated significant complexity with multiple configuration methods, Arc<dyn Agent> threading, and overlapping concerns. The API surface has grown beyond what's necessary for most use cases, making it harder to understand and maintain.

## Findings

**Code Complexity Analysis:**
- `StdoutAssertion` struct has 8+ configuration fields with complex dependencies
- Builder methods overlap in functionality (.with_model() vs agent selection)
- Arc<dyn Agent> threading adds unnecessary complexity for simple use cases
- Multiple ways to achieve the same configuration (confusing API surface)
- Heavy use of Option<T> wrapping creates verbose None handling

**Evidence from Code:**
- `src/fluent/stdout.rs` - 382 lines with complex builder pattern
- Multiple redundant configuration paths
- Excessive Option unwrapping throughout evaluation logic
- Arc cloning and trait object indirection for simple scenarios

**Simplification Opportunities:**
- Most tests only need basic review text and threshold
- Advanced agent/model selection used in <10% of cases
- Builder complexity exceeds actual usage patterns
- Could provide simple constructor for common case

## Proposed Solutions

### Option 1: Tiered API Design (Recommended)
**Effort:** Medium | **Risk:** Low | **Impact:** Medium

**Implementation:**
- Simple constructor for 90% use case: `StdoutAssertion::review("criteria", threshold)`
- Advanced builder for complex cases: `StdoutAssertion::builder().review().with_agent().build()`
- Reduce configuration fields to essential subset
- Default behaviors for agent/model selection

**Pros:**
- Simple API for common cases, power for advanced users
- Reduces cognitive overhead for new users
- Maintains backward compatibility with builder pattern

**Cons:**
- Still maintains two API surfaces (simple + builder)
- May not eliminate all complexity

### Option 2: Configuration-Based Approach
**Effort:** Small | **Risk:** Low | **Impact:** Small

**Implementation:**
- Single configuration struct with defaults
- Remove builder pattern entirely
- Use `StdoutConfig` with sensible defaults
- Simple method: `assertion.evaluate(config)`

**Pros:**
- Single, clear configuration approach
- Easy to document and understand
- Minimal API surface area

**Cons:**
- Less discoverable than fluent methods
- May feel less idiomatic for Rust users
- Harder to enforce required vs optional parameters

### Option 3: Smart Defaults with Minimal Configuration
**Effort:** Small | **Risk:** Medium | **Impact:** Medium

**Implementation:**
- Eliminate most configuration options
- Use environment-based defaults (CLAUDE_MODEL, etc.)
- Simple API: `stdout.should("review criteria")`
- Auto-detect agent and model from context

**Pros:**
- Extremely simple API surface
- Relies on convention over configuration
- Minimal cognitive overhead

**Cons:**
- Less explicit control for advanced users
- Environment dependency may be surprising
- Harder to test different configurations

## Recommended Action

*[To be filled during triage]*

## Technical Details

**Affected Files:**
- `src/fluent/stdout.rs` - Primary builder implementation
- `src/fluent/builder.rs` - Integration with execution expectations
- Documentation and examples showing API usage

**Simplification Areas:**
- Reduce configuration options from 8+ to 3-4 essential ones
- Eliminate redundant configuration paths
- Simplify Arc<dyn Agent> to concrete types where possible
- Reduce Option wrapping with sensible defaults

**API Design Principles:**
- Make simple things simple, complex things possible
- Prefer explicit over implicit configuration
- Minimize required parameters for common cases
- Provide clear upgrade path from simple to advanced usage

## Acceptance Criteria

- [ ] Simple constructor handles 90% of use cases in 1 line
- [ ] Builder pattern available but not required for advanced usage
- [ ] Configuration options reduced to essential subset
- [ ] Clear documentation showing simple vs advanced patterns
- [ ] No breaking changes to existing test files
- [ ] Performance improvement from reduced allocations
- [ ] Code coverage maintained across simplified API

## Work Log

**2026-02-22:** Issue identified during code simplicity review - fluent API builder has accumulated unnecessary complexity that exceeds actual usage patterns.

## Resources

- **API Design:** Rust builder pattern best practices
- **Simplification Examples:** Look at other fluent APIs in Rust ecosystem
- **Usage Analysis:** Review existing test files to understand common patterns
- **Builder Alternatives:** Configuration structs vs method chaining