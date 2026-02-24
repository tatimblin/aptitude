# Brainstorm: Terminal Hyperlinks for Session Path

**Date:** 2026-02-23
**Status:** Draft

## What We're Building

Replace the verbose, full-path session display in CLI output with a clean, clickable terminal hyperlink showing just the session UUID. Terminals that support OSC 8 will render it as a clickable link to the `.jsonl` file; others will see a plain UUID.

**Current output:**
```
[session: "/Users/tristantimblin/.claude/projects/-Users-tristantimblin-repos-agent-execution-harness-examples-increment-number/fdbb606e-98de-49f3-896c-0aa1b4e57af1.jsonl"]
```

**Proposed output (OSC 8 terminal):**
```
[session: fdbb606e-98de-49f3-896c-0aa1b4e57af1]
          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
          clickable — opens the .jsonl file
```

**Proposed output (fallback):**
```
[session: fdbb606e-98de-49f3-896c-0aa1b4e57af1]
```

## Why This Approach

- OSC 8 hyperlinks are widely supported (iTerm2, Kitty, GNOME Terminal, Windows Terminal, VS Code terminal, WezTerm, Alacritty 0.14+)
- The full path adds noise and provides no value at a glance — the UUID is the meaningful identifier
- Auto-detection means zero config for most users while remaining safe for unsupported terminals
- `file://` URIs are the standard scheme for local file links in OSC 8

## Key Decisions

1. **Display text:** Session UUID only (extracted from the filename, without `.jsonl` extension)
2. **Link target:** `file:///full/path/to/session.jsonl` — enables click-to-open in supported terminals
3. **Detection strategy:** Auto-detect via `TERM_PROGRAM` env var (known-good list: iTerm2, WezTerm, vscode, etc.), with a `--hyperlinks` CLI flag to override (`always`/`never`/`auto`)
4. **Fallback:** Plain text showing just the UUID — same display, no OSC 8 escape sequences
5. **Change location:** `src/main.rs:265` — the `SessionDetected` handler in `drain_stream_events()`

## Open Questions

None — requirements are clear.
