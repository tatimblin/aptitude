//! Tool name mapping between agent-specific and canonical names.

use std::collections::HashMap;

/// Canonical tool names used across all agents.
///
/// These match the actual tool names emitted by Claude Code in JSONL output.
/// The fluent API's Tool enum uses these same names for type safety.
pub mod canonical {
    pub const READ: &str = "Read";
    pub const WRITE: &str = "Write";
    pub const EDIT: &str = "Edit";
    pub const BASH: &str = "Bash";
    pub const GREP: &str = "Grep";
    pub const GLOB: &str = "Glob";
    pub const LIST_DIRECTORY: &str = "LS";
    pub const ASK_USER: &str = "AskUserQuestion";
    pub const TASK: &str = "Task";
    pub const WEB_FETCH: &str = "WebFetch";
    pub const WEB_SEARCH: &str = "WebSearch";
    pub const NOTEBOOK_EDIT: &str = "NotebookEdit";
    pub const TODO_WRITE: &str = "TodoWrite";
    pub const KILL_SHELL: &str = "KillShell";
    pub const TASK_OUTPUT: &str = "TaskOutput";
    pub const SKILL: &str = "Skill";
}

/// Mapping from agent-specific to canonical tool names.
#[derive(Debug, Clone, Default)]
pub struct ToolNameMapping {
    /// Agent tool name -> Canonical name
    to_canonical: HashMap<String, String>,
}

impl ToolNameMapping {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a mapping: agent_name -> canonical_name
    pub fn add(&mut self, agent_name: &str, canonical_name: &str) -> &mut Self {
        self.to_canonical
            .insert(agent_name.to_string(), canonical_name.to_string());
        self
    }

    /// Convert agent-specific tool name to canonical.
    ///
    /// If no mapping exists, returns the original name unchanged.
    pub fn to_canonical(&self, agent_name: &str) -> String {
        self.to_canonical
            .get(agent_name)
            .cloned()
            .unwrap_or_else(|| agent_name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_mapping() {
        let mut mapping = ToolNameMapping::new();
        mapping.add("Read", canonical::READ);
        mapping.add("Write", canonical::WRITE);

        assert_eq!(mapping.to_canonical("Read"), "Read");
        assert_eq!(mapping.to_canonical("Write"), "Write");
        assert_eq!(mapping.to_canonical("Unknown"), "Unknown");
    }
}
