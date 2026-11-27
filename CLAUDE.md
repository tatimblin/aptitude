# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Run Commands

```bash
# Build the project (default features: yaml, kiro)
cargo build

# Run tests
cargo test

# Run a single test
cargo test test_name

# Run the aptitude binary
cargo run -- <subcommand>

# Run with release optimizations
cargo build --release

# Build without optional features
cargo build --no-default-features --features yaml   # yaml only, no Kiro
```

### Feature flags (Cargo.toml)

- `yaml` (default) — enables the YAML test format (`serde_yaml`). Gates the `yaml` module.
- `kiro` (default) — enables the Kiro agent adapter (`rusqlite`). Gates `agents::kiro`.

## CLI Usage

The `aptitude` binary has four subcommands:

```bash
# Run a test (executes an agent with the prompt and asserts on results)
cargo run -- run <test.yaml> [-v] [-w <workdir>] [--agent <name>]
cargo run -- run <tests_directory/> [--pattern <glob>] [--no-recursive] [--list-tests]

# Analyze an existing session log against a test (no execution)
cargo run -- analyze <test.yaml> <session.jsonl> [--agent <name>]

# List registered agents and their availability
cargo run -- agents

# Execute a prompt and print tool calls without assertions
cargo run -- log "<prompt>" [-w <workdir>] [--agent <name>] [--model <model>]
```

Agent resolution order for `run`: `--agent` flag > test file `agent:` field > default (`claude`).

## Architecture

This is a test harness for validating AI agent behavior against steering guides. It executes
an agent (Claude Code or Kiro) with a prompt and asserts on the tool calls the agent makes,
and optionally grades the agent's text output with an LLM.

The design centers on an **agent abstraction layer**: canonical tool names are defined once,
and each agent adapter maps its native tool names and session-log format onto them, so
assertions are written against canonical names regardless of which agent produced them.

### Top-level modules (`src/`)

- **main.rs** — CLI entry point (`clap`). Dispatches the four subcommands and handles
  presentation (live tool-call printing, results summary, exit codes).
- **lib.rs** — Library crate root; re-exports the public API (fluent API, agents, prompt
  builder, review, streaming, YAML).
- **parser.rs** — Parses Claude Code JSONL session logs into `ToolCall { name, params, timestamp }`.
  Handles `type: "assistant"` messages containing `tool_use` content blocks. `parse_line_internal`
  is reused by the streaming module.
- **config.rs** — Loads/discovers `.aptitude.yaml` config for test discovery (test glob pattern,
  recursion, excludes). Defaults are embedded at compile time from `default.aptitude.yaml`.
- **discovery.rs** — Walks a directory (via `walkdir`) applying the config's pattern/excludes to
  find test files.
- **streaming.rs** — Polling-based session-log tail. Spawns the agent, watches `~/.claude/projects/`
  for the new session file, and emits `StreamEvent`s (`ToolCall`, `SessionDetected`, `Error`) over
  an `mpsc` channel for live observation. Used for agents whose `supports_streaming()` is true.
- **output.rs** — `OutputFormatter` / `OutputConfig` for terminal presentation, including OSC 8
  hyperlinks for session paths and workdir-relative path shortening.
- **prompt.rs** — `prompt(...)` builder (`PromptBuilder`) for the fluent Rust API: configure
  workdir/agent, then `.run()` (→ `Vec<ToolCall>`) or `.run_full()` (→ `ExecutionOutput`).
- **review.rs** — LLM-powered stdout grading protocol: builds the grading prompt, parses the
  JSON `{score, reasoning}` response, and evaluates against a threshold. CLI invocation is
  delegated to a grader agent passed in by the caller.

### Agent abstraction (`src/agents/`)

- **mod.rs** — Defines the `Agent` trait (`execute`, `parse_session`, `tool_mapping`,
  `supports_streaming`, `is_available`, `grade`/`grade_async`), `ExecutionConfig`,
  `RawExecutionResult`, and `ToolNameMapping` (agent-native → canonical name map).
- **harness.rs** — `AgentHarness` facade + `AgentType` enum. Registers built-in agents,
  resolves them, executes (sync or streaming), normalizes tool names to canonical form, and
  exposes grading. `ExecutionOutput` / `NormalizedResult` are the normalized results.
- **claude.rs** — `ClaudeAdapter`. Runs `claude --print`, locates the resulting session log
  under `~/.claude/projects/`, parses it via `parser.rs`. Supports streaming.
- **kiro.rs** *(feature `kiro`)* — `KiroAdapter`. Runs `kiro chat --no-interactive`; Kiro stores
  session data in a SQLite database (`rusqlite`) rather than JSONL, so it has its own parsing path.

### Fluent API (`src/fluent/`)

Jest-like assertion API for writing tests in Rust with `#[test]`.

- **mod.rs** — Re-exports `expect`, `expect_tools`, `Tool`, assertion types.
- **tool.rs** — `Tool` enum (canonical tool names) and aliases.
- **builder.rs** — `ExecutionExpectation` and `ToolAssertion` (`.tool()`, `.with_params()`,
  `.times()`/`.at_least()`/`.at_most()`, `.after()`/`.before()`, `.nth_call()`/`.last_call()`,
  `.to_be_called()`/`.not_to_be_called()`, `.evaluate()`).
- **matchers.rs** — Parameter matching: regex-first with exact-match fallback; glob support.
- **stdout.rs** — `StdoutAssertion` for LLM-graded stdout review (`.review()`, `.with_threshold()`,
  `.with_model()`, `.with_grader()`, `.to_pass()`).
- **tests.rs** — Unit tests for the fluent API.

### YAML test format (`src/yaml/`, feature `yaml`)

- **mod.rs** — Re-exports `load_test`, `run_yaml_test`, `Test`, `Assertion`, `TestResult`.
- **parser.rs** — Deserializes YAML test files into `Test` / `Assertion` structs and validates
  invalid assertion combinations.
- **runner.rs** — Evaluates a parsed `Test` against captured tool calls and stdout, producing
  `(description, TestResult)` pairs. Bridges YAML assertions to the fluent matchers and the
  review module.

## Test File Format (YAML)

```yaml
name: "Test name"
prompt: "The prompt to send to the agent"
agent: claude            # optional; default "claude"
assertions:
  - tool: Read
    called: true
    params:
      file_path: "*.txt"  # glob / regex, exact-match fallback
  - tool: Bash
    called: false
  - tool: Write
    called_after: Read
  - stdout:
      review: "should confirm the task completed successfully"
      threshold: 7        # 1-10, default 7
```

See `docs/yaml-api.md` for the full assertion reference (`call_count`, `min_calls`/`max_calls`,
`called_after`/`called_before`, `nth_call_params`/`first_call_params`/`last_call_params`, `stdout`)
and `docs/fluent-api.md` for the Rust API.

## Testing Notes

- Tests that actually invoke an agent CLI are marked `#[ignore]` (they require `claude`/`kiro`
  installed and configured); `cargo test` skips them by default.
- Property tests use `proptest` (regressions live in `proptest-regressions/`).
- Example test projects live under `examples/` (e.g. `env-protection/`, `read-order/`,
  `typescript-rule/`, `claude-skill/`, `kiro-increment/`).
