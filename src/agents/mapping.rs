//! Tool name mapping between agent-specific and canonical names.

use std::collections::HashMap;

/// Canonical tool names used across all agents.
///
/// These are the standard names that test assertions should use.
pub mod canonical {
    pub const READ_FILE: &str = "read_file";
    pub const WRITE_FILE: &str = "write_file";
    pub const EDIT_FILE: &str = "edit_file";
    pub const EXECUTE_COMMAND: &str = "execute_command";
    pub const SEARCH_FILES: &str = "search_files";
    pub const GLOB_FILES: &str = "glob_files";
    pub const LIST_DIRECTORY: &str = "list_directory";
    pub const ASK_USER: &str = "ask_user";
    pub const TASK: &str = "task";
    pub const WEB_FETCH: &str = "web_fetch";
    pub const WEB_SEARCH: &str = "web_search";
    pub const NOTEBOOK_EDIT: &str = "notebook_edit";
}

/// Bidirectional mapping between agent-specific and canonical tool names.
#[derive(Debug, Clone, Default)]
pub struct ToolNameMapping {
    /// Agent tool name -> Canonical name
    to_canonical: HashMap<String, String>,
    /// Canonical name -> Agent tool name
    from_canonical: HashMap<String, String>,
}

impl ToolNameMapping {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a mapping: agent_name <-> canonical_name
    pub fn add(&mut self, agent_name: &str, canonical_name: &str) -> &mut Self {
        self.to_canonical
            .insert(agent_name.to_string(), canonical_name.to_string());
        self.from_canonical
            .insert(canonical_name.to_string(), agent_name.to_string());
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

    /// Convert canonical tool name to agent-specific.
    ///
    /// If no mapping exists, returns the original name unchanged.
    pub fn from_canonical(&self, canonical_name: &str) -> String {
        self.from_canonical
            .get(canonical_name)
            .cloned()
            .unwrap_or_else(|| canonical_name.to_string())
    }

    /// Check if a mapping exists for an agent tool name.
    pub fn has_agent_name(&self, agent_name: &str) -> bool {
        self.to_canonical.contains_key(agent_name)
    }

    /// Check if a mapping exists for a canonical tool name.
    pub fn has_canonical_name(&self, canonical_name: &str) -> bool {
        self.from_canonical.contains_key(canonical_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_mapping() {
        let mut mapping = ToolNameMapping::new();
        mapping.add("Read", canonical::READ_FILE);
        mapping.add("Write", canonical::WRITE_FILE);

        assert_eq!(mapping.to_canonical("Read"), "read_file");
        assert_eq!(mapping.to_canonical("Write"), "write_file");
        assert_eq!(mapping.to_canonical("Unknown"), "Unknown");

        assert_eq!(mapping.from_canonical("read_file"), "Read");
        assert_eq!(mapping.from_canonical("write_file"), "Write");
        assert_eq!(mapping.from_canonical("unknown"), "unknown");
    }
}
