//! Tool Registry — trait-based tool system for the ReAct agent.
//!
//! Each tool implements the [`Tool`] trait, providing a name, description,
//! and async execution method. Tools are registered in a [`ToolRegistry`]
//! which allows lookup by name and produces description blocks for prompt
//! injection.

pub mod file_search;
pub mod shell;
pub mod system_control;

use std::collections::HashMap;
use std::sync::Arc;

/// A tool that can be invoked by the ReAct agent.
///
/// Tools are stateless executors that receive JSON input and return a
/// string result (the observation).
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// The canonical name of this tool (e.g., `"file_search"`).
    fn name(&self) -> &'static str;

    /// A human-readable description for prompt injection.
    fn description(&self) -> &'static str;

    /// Execute the tool with the given JSON input, returning the observation.
    async fn execute(&self, input: serde_json::Value) -> Result<String, anyhow::Error>;
}

/// Registry of available tools, keyed by name.
pub struct ToolRegistry {
    tools: HashMap<&'static str, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Overwrites any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Generate a formatted description block for all registered tools.
    ///
    /// Used to inject tool descriptions into the ReAct system prompt.
    pub fn describe_all(&self) -> String {
        let mut descriptions: Vec<_> = self
            .tools
            .values()
            .map(|t| format!("- **{}**: {}", t.name(), t.description()))
            .collect();
        descriptions.sort();
        descriptions.join("\n")
    }

    /// List all registered tool names.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.tools.keys().copied().collect();
        names.sort();
        names
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a [`ToolRegistry`] pre-loaded with all available tools.
pub fn default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(file_search::FileSearchTool));
    registry.register(Arc::new(shell::ShellTool));
    registry.register(Arc::new(system_control::SystemControlTool));
    registry
}
