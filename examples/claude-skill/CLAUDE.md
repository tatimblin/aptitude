## Cloud Status Skill

This is an example of a repo initialized with claude-code. It's goal is to simulate a repo with claude-code skills installed at the project level.

## Skill Invocation Instructions

**IMPORTANT**: When you see a `<command-name>/skill-name</command-name>` tag in the conversation:

1. **ALWAYS use the Skill tool** to invoke the skill - do NOT manually implement it
2. The skill name is what follows the `/` (e.g., `/cloud-status` → `skill: "cloud-status"`)
3. Pass any arguments after the skill name as the `args` parameter to the Skill tool
4. **DO NOT** read the skill files and run bash commands directly

### Examples:

- **User prompt**: `/cloud-status`
  - **Correct**: Use Skill tool with `{"skill": "cloud-status"}`
  - **Wrong**: Running `python3 .claude/skills/cloud-status/scripts/check_status.py` directly

- **User prompt**: `/cloud-status aws gcp`
  - **Correct**: Use Skill tool with `{"skill": "cloud-status", "args": "aws gcp"}`
  - **Wrong**: Reading files and manually executing scripts

The skill system is designed to be invoked via the Skill tool, which handles execution properly.
