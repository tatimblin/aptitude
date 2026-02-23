---
title: "Refactor: Codebase Simplification"
type: refactor
status: active
date: 2026-02-22
---

# Refactor: Codebase Simplification

## Overview

Comprehensive pass to trim dead weight, eliminate duplication, flatten unnecessary abstractions, and reduce module count — while preserving the existing separation of concerns between parsing, assertion, execution, and output layers.

Current state: ~5,600 lines across 24 source files, 12 dependencies.

## Proposed Changes

### 1. Remove Dead Dependencies

**Remove `notify` crate** — imported in `Cargo.toml:28` but never used anywhere in the codebase. The streaming module uses its own polling-based file tailing instead.

**Remove `chrono` crate** — used only for `DateTime<Utc>` on `ToolCall.timestamp`. The actual timestamps come as RFC3339 strings from JSONL, get parsed into `DateTime<Utc>`, then get formatted back to `"%H:%M:%S"` for display. Replace with a plain `String` field and extract `HH:MM:SS` by slicing the RFC3339 string directly. This also removes the `chrono` serde feature (which isn't used — ToolCall doesn't derive Serialize).

Files touched: `Cargo.toml`, `parser.rs`, `streaming.rs`, `fluent/builder.rs`, `fluent/tests.rs`, `output/formatter.rs`, `main.rs`, `yaml/runner.rs`

### 2. Eliminate Identity Mapping

`ClaudeAdapter::new()` (`agents/claude.rs:19-43`) builds a `ToolNameMapping` that maps every tool name to itself: `"Read" -> "Read"`, `"Write" -> "Write"`, etc. Since `ToolNameMapping::to_canonical()` already falls back to the original name when no mapping exists, an empty mapping produces identical behavior.

- **Remove** the 16-line identity mapping in `ClaudeAdapter::new()`
- **Remove** the `canonical` constants module in `agents/mapping.rs:9-26` (only consumer was the identity mapping)
- Keep `ToolNameMapping` struct and trait method — the abstraction is correct for future multi-agent support, just the current data is pointless

Files touched: `agents/claude.rs`, `agents/mapping.rs`

### 3. Remove Backward-Compatibility Cruft

- **Remove** `ToolCallExpectation` type alias (`fluent/builder.rs:137-138`) — marked `#[doc(hidden)]`, serves no purpose
- **Remove** its re-export from `fluent/mod.rs:39` and `lib.rs:75`

Files touched: `fluent/builder.rs`, `fluent/mod.rs`, `lib.rs`

### 4. Extract Duplicated `format_tool_calls`

`ToolAssertion::format_tool_calls()` (`fluent/builder.rs:625-654`) and `NthCallAssertion::format_tool_calls()` (`fluent/builder.rs:734-763`) are identical — 30 lines copy-pasted. Extract into a free function `fn format_tool_calls(calls: &[ToolCall]) -> String` and call it from both.

Files touched: `fluent/builder.rs`

### 5. Extract Duplicated Test Result Display

`run_single_test()` (`main.rs:317-346`) and `analyze_session()` (`main.rs:481-509`) have nearly identical pass/fail counting and result-printing logic. Extract into a shared function:

```rust
fn print_results(results: &[(String, TestResult)]) -> bool
```

Returns whether all tests passed. Handles formatting, colors, and the summary line.

Files touched: `main.rs`

### 6. Flatten `output/` Into Single File

`output/mod.rs` (24 lines), `output/config.rs` (143 lines), and `output/formatter.rs` (261 lines) total ~428 lines. The config and formatter are tightly coupled (formatter takes config). Merge into a single `output.rs` file.

Files touched: delete `output/mod.rs`, `output/config.rs`, `output/formatter.rs`; create `output.rs`

### 7. Flatten `agents/` Small Files

`agents/traits.rs` (63 lines) and `agents/mapping.rs` (72 lines after removing canonical constants) are tiny. Inline them into `agents/mod.rs` (currently 38 lines), bringing it to ~140 lines total. Keep `claude.rs` and `harness.rs` as separate files since they have distinct responsibilities.

Files touched: delete `agents/traits.rs`, `agents/mapping.rs`; expand `agents/mod.rs`

## Summary Table

| Change | Files Removed | Lines Saved (est.) | Impact |
|--------|:---:|:---:|--------|
| Remove `notify` dep | 0 | ~5 (Cargo) | Smaller binary, fewer deps |
| Remove `chrono` dep | 0 | ~20 | Simpler ToolCall, fewer deps |
| Eliminate identity mapping | 0 | ~35 | Less noise in claude.rs |
| Remove compat alias | 0 | ~5 | Cleaner public API |
| Extract `format_tool_calls` | 0 | ~25 | DRY |
| Extract result display | 0 | ~30 | DRY |
| Flatten `output/` | 2 | ~15 (boilerplate) | Fewer files |
| Flatten agents small files | 2 | ~15 (boilerplate) | Fewer files |
| **Total** | **4 files** | **~150 lines** | 10 deps (from 12) |

## Design Constraint: Agent-Agnostic Architecture

The harness is designed to work with any coding agent, not just Claude. The `Agent` trait, `AgentType` enum, `ToolNameMapping`, `AgentHarness`, and `ExecutionConfig` form an abstraction layer that lets new agents (Aider, Cursor, or others) be plugged in by implementing the `Agent` trait — no changes needed to the assertion, YAML, or output layers.

Every simplification in this plan preserves that boundary. We remove dead data (identity mappings, unused constants) but keep the traits, the harness facade, and the tool-name normalization pipeline intact. A future agent with different tool names would add real entries to `ToolNameMapping` and everything downstream would work unchanged.

## What This Preserves

- **Agent-agnostic design**: `Agent` trait, `AgentHarness` facade, `ToolNameMapping` normalization pipeline all retained — new agents plug in without touching assertion or output code
- **Separation of concerns**: parsing, fluent assertions, YAML support, agent abstraction, output formatting remain distinct layers
- **Fluent API surface**: all public assertion methods unchanged
- **YAML test format**: no changes to test file schema
- **CLI interface**: no changes to commands or flags

## Acceptance Criteria

- [ ] `cargo build` succeeds
- [ ] `cargo test` passes (all existing tests)
- [ ] `notify` and `chrono` no longer in dependency tree
- [ ] `canonical` constants module removed
- [ ] No duplicated `format_tool_calls` implementations
- [ ] `output/` is a single file, not a directory
- [ ] `agents/traits.rs` and `agents/mapping.rs` inlined into `agents/mod.rs`
- [ ] Public API: `ToolCallExpectation` alias removed
- [ ] `ToolCall.timestamp` is `String`, not `DateTime<Utc>`

## Sources

- Codebase analysis performed by reading all 24 source files
- Previous refactoring (commit 3dafc25) resolved 9 code review findings
- No external research needed — all changes are internal simplification
