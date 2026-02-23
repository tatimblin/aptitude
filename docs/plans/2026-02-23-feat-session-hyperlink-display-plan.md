---
title: "feat: Terminal hyperlinks for session path display"
type: feat
status: completed
date: 2026-02-23
origin: docs/brainstorms/2026-02-23-session-hyperlink-brainstorm.md
---

# Terminal Hyperlinks for Session Path Display

## Overview

Replace the verbose full-path session display with a clean, clickable OSC 8 terminal hyperlink showing just the session file stem. Terminals that support OSC 8 render it as a clickable `file://` link; others see plain text. Auto-detection via `TERM_PROGRAM` env var.

## Problem Statement / Motivation

The current session line is unreadable noise:

```
[session: "/Users/tristantimblin/.claude/projects/-Users-tristantimblin-repos-agent-execution-harness-examples-increment-number/fdbb606e-98de-49f3-896c-0aa1b4e57af1.jsonl"]
```

The only meaningful identifier is the UUID. The rest is a long project-encoded path that adds no value during normal use. (see brainstorm: docs/brainstorms/2026-02-23-session-hyperlink-brainstorm.md)

## Proposed Solution

Emit an OSC 8 hyperlink wrapping the file stem as display text, pointing to a `file://` URI of the full path. When the terminal doesn't support hyperlinks (or output is piped), emit plain text with just the file stem.

**OSC 8 format:**
```
\x1b]8;;file:///full/path/to/session.jsonl\x1b\\fdbb606e-98de-49f3-896c-0aa1b4e57af1\x1b]8;;\x1b\\
```

**Output examples:**

| Context | Output |
|---------|--------|
| Supported terminal | `[session: fdbb606e-98de-49f3-896c-0aa1b4e57af1]` (clickable) |
| Unsupported / piped | `[session: fdbb606e-98de-49f3-896c-0aa1b4e57af1]` (plain text) |
| `--verbose` mode | `[session: fdbb606e-98de-49f3-896c-0aa1b4e57af1]` + full path on next line |

## Technical Considerations

### Architecture: Add to `OutputFormatter` (not inline)

The session path is displayed in **4 locations** in `src/main.rs`:
- Line 265: `drain_stream_events()` — live streaming session detection
- Line 336: `run_single_test()` — post-execution session log
- Line 543: `log_command()` — post-execution session log
- Line 430: `analyze_session()` — user-provided session path

Rather than duplicating hyperlink logic in each location, add a `format_session_path(&self, path: &Path) -> String` method to `OutputFormatter` in `src/output.rs`. This follows the existing `colors_enabled` pattern and keeps detection/formatting in one place.

### Changes to `OutputConfig` (`src/output.rs`)

Add a `hyperlinks_enabled: bool` field to `OutputConfig`, populated by auto-detection: check `TERM_PROGRAM` against an allowlist AND verify stdout is a TTY.

**Auto-detection allowlist:**

| Env var | Values that enable hyperlinks |
|---------|-------------------------------|
| `TERM_PROGRAM` | `iTerm.app`, `WezTerm`, `kitty`, `vscode`, `Hyper`, `Tabby` |
| `WT_SESSION` | Any value present (Windows Terminal) |

**Disable when:**
- stdout is not a TTY (`!stdout().is_terminal()`)
- `TMUX` env var is set (tmux passthrough is unreliable)
- `STY` env var is set (GNU screen)
- `TERM_PROGRAM` is absent or not in the allowlist

### `file://` URI Construction

- Convert path to absolute via `std::fs::canonicalize()` or `std::path::Path::canonicalize()`
- Percent-encode spaces and special characters per RFC 3986 (use `percent_encoding` crate or manual encoding for the small subset needed)
- Unix: `file:///home/user/path.jsonl`
- Windows: `file:///C:/Users/user/path.jsonl` (forward slashes)

### Display Text Extraction

Extract the file stem from the path (`Path::file_stem()`). No UUID validation — display whatever the stem is. This handles:
- Standard UUID filenames: `fdbb606e-98de-49f3-896c-0aa1b4e57af1`
- Non-UUID filenames from `analyze` command: `my-session`
- Edge case of empty stem: fall back to full filename

### `--verbose` Interaction

When `--verbose` is active, show the hyperlinked stem on the primary line and the full path on a second indented line:
```
  [session: fdbb606e-98de-49f3-896c-0aa1b4e57af1]
    /Users/tristantimblin/.claude/projects/.../fdbb606e-98de-49f3-896c-0aa1b4e57af1.jsonl
```

### String Terminator

Use `\x1b\\` (ESC + backslash) as the OSC 8 string terminator — more widely supported than `\x07` (BEL). No `id=` parameter needed since the display text won't wrap lines.

## Acceptance Criteria

- [x] Session display in `drain_stream_events()` shows file stem instead of full path (`src/main.rs:265`)
- [x] Session display in `run_single_test()` shows file stem (`src/main.rs:336`)
- [x] Session display in `log_command()` shows file stem (`src/main.rs:543`)
- [x] Session display in `analyze_session()` shows file stem (`src/main.rs:430`)
- [x] OSC 8 hyperlink wraps the file stem when `hyperlinks_enabled` is true
- [x] `file://` URI correctly constructed with percent-encoded special characters
- [x] Auto-detection checks `TERM_PROGRAM` allowlist + TTY + no tmux/screen
- [x] `--verbose` shows full path on a second line below the session display
- [x] `format_session_path()` method added to `OutputFormatter` in `src/output.rs`
- [x] `hyperlinks_enabled` field added to `OutputConfig`
- [x] Unit tests for: file stem extraction, `file://` URI construction, OSC 8 formatting, terminal detection logic

## Dependencies & Risks

- **`percent-encoding` crate** — may be needed for URI encoding. Alternatively, handle the small subset (spaces, `#`, `?`, `%`) manually to avoid a new dependency. Check if the existing `url` crate is already in `Cargo.toml`.
- **Terminal compatibility** — the allowlist approach is conservative. The allowlist can be expanded over time as terminals are verified.
- **No breaking change** — the display text (file stem) is always shown regardless of hyperlink support. Only the escape sequence wrapping differs.

## MVP

### `src/output.rs` — Add hyperlink support to OutputConfig and OutputFormatter

```rust
// Add to OutputConfig
pub struct OutputConfig {
    pub colors_enabled: bool,
    pub hyperlinks_enabled: bool, // NEW
    // ... existing fields
}

// Add detection in OutputConfig::default() or a new constructor
fn detect_hyperlinks() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    if std::env::var("TMUX").is_ok() || std::env::var("STY").is_ok() {
        return false;
    }
    let dominated_terminals = ["iTerm.app", "WezTerm", "kitty", "vscode", "Hyper", "Tabby"];
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        return dominated_terminals.iter().any(|t| term_program == *t);
    }
    // Windows Terminal
    std::env::var("WT_SESSION").is_ok()
}

// Add to OutputFormatter
impl OutputFormatter {
    pub fn format_session_path(&self, path: &Path, verbose: bool) -> String {
        let stem = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let display = if self.config.hyperlinks_enabled {
            let uri = path_to_file_uri(path);
            format!("\x1b]8;;{uri}\x1b\\{stem}\x1b]8;;\x1b\\")
        } else {
            stem.to_string()
        };

        let mut output = format!("[session: {display}]");
        if verbose {
            output.push_str(&format!("\n    {}", path.display()));
        }
        output
    }
}

fn path_to_file_uri(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = absolute.to_string_lossy().replace(' ', "%20");
    format!("file://{path_str}")
}
```

### `src/main.rs` — Update all 4 display locations

```rust
// Update drain_stream_events (line 265):
StreamEvent::SessionDetected(path) => {
    let formatted = formatter.format_session_path(&path, verbose);
    println!("  \x1b[2m{formatted}\x1b[0m");
}

// Update run_single_test (line 336), log_command (line 543), analyze_session (line 430):
// Replace `println!("Session log: {:?}", log_path)` with:
println!("Session log: {}", formatter.format_session_path(&log_path, verbose));
```

### `src/output.rs` — Unit tests

```rust
#[cfg(test)]
mod hyperlink_tests {
    #[test]
    fn test_file_stem_extraction_uuid() { /* UUID filename -> UUID stem */ }
    #[test]
    fn test_file_stem_extraction_non_uuid() { /* arbitrary filename -> full stem */ }
    #[test]
    fn test_file_uri_construction() { /* /foo/bar.jsonl -> file:///foo/bar.jsonl */ }
    #[test]
    fn test_file_uri_spaces() { /* /foo/bar baz.jsonl -> file:///foo/bar%20baz.jsonl */ }
    #[test]
    fn test_osc8_format_enabled() { /* contains \x1b]8;; sequences */ }
    #[test]
    fn test_osc8_format_disabled() { /* plain stem, no escape sequences */ }
    #[test]
    fn test_verbose_shows_full_path() { /* output contains newline + full path */ }
}
```

## Sources

- **Origin brainstorm:** [docs/brainstorms/2026-02-23-session-hyperlink-brainstorm.md](docs/brainstorms/2026-02-23-session-hyperlink-brainstorm.md) — Key decisions: UUID-only display text, `file://` link target, `TERM_PROGRAM` auto-detect, plain fallback
- OSC 8 spec: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
- Existing formatting: `src/output.rs:77` (`colors_enabled` via `IsTerminal`)
- Session detection: `src/streaming.rs:216` (`SessionDetected` event)
- CLI entry points: `src/main.rs` (4 session display locations)
