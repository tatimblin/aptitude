---
status: pending
priority: p1
issue_id: 002
tags: [code-review, security, validation]
dependencies: []
---

# Security: Subprocess Command Injection in LLM Grading

## Problem Statement

The LLM grading system uses subprocess calls to execute `claude --print` with user-controlled prompts that are not properly sanitized. This creates potential command injection vulnerabilities where malicious test configurations could execute arbitrary commands or manipulate the claude CLI invocation.

## Findings

**Security Analysis:**
- `src/review.rs:grade_stdout()` constructs subprocess commands with unsanitized user input
- Test YAML files can contain arbitrary prompt text that gets passed to shell
- No input validation on prompt content or model parameters
- No escaping of shell metacharacters in subprocess construction
- Agent resolution allows arbitrary agent names without validation

**Evidence from Code:**
- `src/review.rs` - Direct subprocess call construction around line 89
- `src/agents/claude.rs:grade()` - Command building with user input
- No validation in YAML parser for prompt content
- Model parameter passed directly to CLI without sanitization

**Attack Vectors:**
- Malicious YAML test files with embedded shell commands in prompts
- Model parameter injection (e.g., `--model "opus; rm -rf /"`)
- Agent name manipulation to execute unintended commands
- Prompt injection to manipulate subprocess behavior

## Proposed Solutions

### Option 1: Strict Input Validation and Sanitization (Recommended)
**Effort:** Small | **Risk:** Low | **Impact:** High

**Implementation:**
- Add comprehensive input validation for all user-controlled parameters
- Whitelist allowed model names and agent types
- Sanitize prompt content by removing/escaping shell metacharacters
- Use proper subprocess APIs that avoid shell interpretation
- Add validation schemas for YAML test configurations

**Pros:**
- Directly addresses security vulnerabilities
- Minimal architectural changes required
- Maintains existing functionality

**Cons:**
- May restrict some legitimate test cases with special characters
- Requires careful balance between security and functionality

### Option 2: Sandboxed Subprocess Execution
**Effort:** Medium | **Risk:** Medium | **Impact:** High

**Implementation:**
- Run subprocess calls in isolated containers or chroot environments
- Use restricted user accounts for subprocess execution
- Implement resource limits (time, memory, file access)
- Add comprehensive logging and monitoring

**Pros:**
- Strong isolation prevents system-level damage
- More robust against unknown attack vectors
- Better observability of subprocess behavior

**Cons:**
- Complex setup and deployment requirements
- May impact performance and user experience
- Platform-specific implementation challenges

### Option 3: API-Based LLM Integration
**Effort:** Large | **Risk:** Medium | **Impact:** Medium

**Implementation:**
- Replace subprocess calls with direct API integration
- Use Claude API, OpenAI API, or other HTTP-based services
- Eliminate shell/subprocess layer entirely
- Implement proper API authentication and rate limiting

**Pros:**
- Eliminates subprocess security risks entirely
- Better performance and reliability
- More sophisticated error handling and retry logic

**Cons:**
- Requires API credentials and network connectivity
- Major architectural changes needed
- Different authentication/authorization model

## Recommended Action

*[To be filled during triage]*

## Technical Details

**Affected Files:**
- `src/review.rs` - Subprocess command construction
- `src/agents/claude.rs` - CLI invocation logic
- `src/yaml/parser.rs` - Input validation for test configurations
- All agent implementations that use subprocess calls

**Security Controls Needed:**
- Input sanitization functions
- Whitelist validation for model/agent parameters
- Subprocess security wrappers
- Audit logging for security events

**Validation Requirements:**
- Prompt content: Remove shell metacharacters, limit length
- Model names: Whitelist allowed values (opus, sonnet, haiku, etc.)
- Agent names: Validate against known agent types
- File paths: Prevent path traversal in working directories

## Acceptance Criteria

- [ ] All user input validated and sanitized before subprocess calls
- [ ] Model and agent parameters restricted to whitelisted values
- [ ] Shell metacharacters properly escaped or removed from prompts
- [ ] Subprocess calls use secure APIs that avoid shell interpretation
- [ ] Security audit logs capture subprocess invocations
- [ ] Automated security tests verify injection protection
- [ ] Documentation covers secure usage patterns

## Work Log

**2026-02-22:** Issue identified during security review - subprocess calls with unsanitized user input create command injection vulnerabilities.

## Resources

- **OWASP Command Injection:** https://owasp.org/www-community/attacks/Command_Injection
- **Rust Subprocess Security:** std::process::Command vs shell execution
- **Input Validation Patterns:** Sanitization vs validation approaches
- **Similar Vulnerabilities:** Search codebase for other subprocess usage