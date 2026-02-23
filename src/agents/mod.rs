//! Agent abstraction layer for multi-agent support.
//!
//! This module provides a unified interface for different coding agents
//! (Claude Code, Aider, Cursor, etc.) using the Adapter and Facade patterns.
//!
//! # Architecture
//!
//! - [`Agent`] trait: Defines the interface all agent adapters must implement
//! - [`ToolNameMapping`]: Handles conversion between agent-specific and canonical tool names
//! - [`AgentHarness`]: Facade providing unified access to all agents
//!
//! # Example
//!
//! ```ignore
//! use aptitude::{AgentHarness, AgentType, ExecutionConfig};
//!
//! let harness = AgentHarness::new();
//! let config = ExecutionConfig::new();
//! let output = harness.execute(Some(AgentType::Claude), "Hello", config)?;
//!
//! for call in &output.result.tool_calls {
//!     println!("Tool: {}", call.name);
//! }
//! ```

mod claude;
mod harness;

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::parser::ToolCall;

pub use harness::{AgentHarness, AgentType, ExecutionOutput, NormalizedResult};

// Re-export Claude session helpers for streaming module
pub(crate) use claude::{
    find_new_session, get_claude_projects_dir, get_project_dir_for_workdir, list_session_files,
};

// =========================================================================
// Agent trait and execution types
// =========================================================================

/// Configuration for agent execution.
#[derive(Debug, Clone, Default)]
pub struct ExecutionConfig {
    /// Working directory for agent execution.
    pub working_dir: Option<PathBuf>,
    /// Extra arguments to pass to the agent CLI.
    pub extra_args: Vec<String>,
}

impl ExecutionConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }
}

/// Raw result from agent execution before normalization.
#[derive(Debug)]
pub struct RawExecutionResult {
    /// Path to the session log file, if the agent produces one.
    pub session_log_path: Option<PathBuf>,
    /// Stdout from the agent command.
    pub stdout: Option<String>,
}

/// The core trait that all agent adapters must implement.
///
/// Each agent (Claude Code, Aider, Cursor, etc.) implements this trait
/// to provide a unified interface for execution and parsing.
pub trait Agent: Send + Sync {
    /// Unique identifier for this agent (e.g., "claude", "aider", "cursor").
    fn name(&self) -> &'static str;

    /// Execute the agent with a prompt.
    ///
    /// Returns a raw execution result that can be parsed for tool calls.
    fn execute(&self, prompt: &str, config: &ExecutionConfig) -> Result<RawExecutionResult>;

    /// Parse the agent's output/log format and extract tool calls.
    ///
    /// Returns tool calls in the agent's native naming convention.
    fn parse_session(&self, result: &RawExecutionResult) -> Result<Vec<ToolCall>>;

    /// Return the tool name mapping for this agent.
    ///
    /// Maps agent-specific tool names to canonical names.
    fn tool_mapping(&self) -> &ToolNameMapping;

    /// Check if this agent is available on the system.
    fn is_available(&self) -> bool;
}

// =========================================================================
// Tool name mapping
// =========================================================================

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
        mapping.add("read_file", "Read");
        mapping.add("execute", "Bash");

        assert_eq!(mapping.to_canonical("read_file"), "Read");
        assert_eq!(mapping.to_canonical("execute"), "Bash");
        // Unmapped names pass through unchanged
        assert_eq!(mapping.to_canonical("Unknown"), "Unknown");
    }
}
