//! Tool Registry — trait-based tool system for the ReAct agent.
//!
//! Each tool implements the [`Tool`] trait, providing a name, description,
//! and async execution method. Tools are registered in a [`ToolRegistry`]
//! which allows lookup by name and produces description blocks for prompt
//! injection.

pub mod academic_search;
pub mod advanced_research;
pub mod api_search;
pub mod audio_control;
pub mod browser_interact;
pub mod calculator;
pub mod claude_code;
pub mod clipboard;
pub mod clock;
pub mod code;
pub mod computer_use;
pub mod container_tools;
pub mod document_create;
pub mod external_ai;
pub mod file_ops;
pub mod file_search;
pub mod media;
pub mod media_processing;
pub mod network_tools;
pub mod os_browser;
pub mod preflight;
pub mod pro_document;
pub mod process_manager;
pub mod rag;
pub mod schemas;
pub mod screen_capture;
pub mod screen_interact;
pub mod shell;
pub mod software_control;
pub mod system_control;
pub mod system_map;
pub mod text_transform;
pub mod web;
pub mod window_control;
pub mod workspace_chunk;

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

    // ── Original 10 tools ────────────────────────────────────────────────
    registry.register(Arc::new(file_search::FileSearchTool));
    registry.register(Arc::new(shell::ShellTool));
    registry.register(Arc::new(system_control::SystemControlTool));
    registry.register(Arc::new(computer_use::ComputerControlTool));
    registry.register(Arc::new(web::WebSearchTool));
    registry.register(Arc::new(code::CodeOpsTool));
    registry.register(Arc::new(os_browser::NativeBrowserTool));
    registry.register(Arc::new(api_search::ApiSearchTool::new()));
    registry.register(Arc::new(media::MediaTool));
    registry.register(Arc::new(rag::RagTool::new()));

    // ── Wave 2: 10 new tools ─────────────────────────────────────────────
    registry.register(Arc::new(window_control::WindowControlTool));
    registry.register(Arc::new(clipboard::ClipboardTool));
    registry.register(Arc::new(audio_control::AudioControlTool));
    registry.register(Arc::new(clock::ClockTool));
    registry.register(Arc::new(calculator::CalculatorTool));
    registry.register(Arc::new(screen_capture::ScreenCaptureTool));
    registry.register(Arc::new(system_map::SystemMapTool));
    registry.register(Arc::new(process_manager::ProcessManagerTool));
    registry.register(Arc::new(text_transform::TextTransformTool));
    registry.register(Arc::new(network_tools::NetworkToolsTool));

    // ── Wave 4: 2 new tools ──────────────────────────────────────────────
    registry.register(Arc::new(software_control::SoftwareControlTool));
    registry.register(Arc::new(document_create::DocumentCreateTool));

    // ── Wave 5: 4 new tools ──────────────────────────────────────────────
    registry.register(Arc::new(file_ops::FileOpsTool));
    registry.register(Arc::new(container_tools::ContainerToolsTool));
    registry.register(Arc::new(external_ai::ExternalAiTool));
    registry.register(Arc::new(preflight::PreflightTool));

    // ── Wave 7: 7 new tools ──────────────────────────────────────────────
    registry.register(Arc::new(academic_search::AcademicSearchTool::new()));
    registry.register(Arc::new(browser_interact::BrowserInteractTool));
    registry.register(Arc::new(screen_interact::ScreenInteractTool));
    registry.register(Arc::new(workspace_chunk::WorkspaceChunkTool));
    registry.register(Arc::new(media_processing::MediaProcessingTool));
    registry.register(Arc::new(pro_document::ProDocumentTool));
    registry.register(Arc::new(advanced_research::AdvancedResearchTool));

    // ── Wave 8: Claude Code ─────────────────────────────────────────────
    registry.register(Arc::new(claude_code::ClaudeCodeTool));

    registry
}
