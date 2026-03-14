//! Claude Code Tool — integration with the Claude Code CLI.
//!
//! Provides a tool that invokes the Claude Code CLI (`claude --print`)
//! to delegate code generation, editing, and analysis tasks.
//! Ports the Python `tools/claude_code.py` to Rust.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

/// Tool struct for invoking the Claude Code CLI.
pub struct ClaudeCodeTool;

/// Configuration for Claude Code CLI invocations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaudeCodeConfig {
    /// Path to the `claude` CLI binary.
    pub cli_path: String,
    /// Working directory for the CLI process.
    pub working_dir: Option<String>,
    /// Maximum execution time in seconds.
    pub timeout_secs: u64,
    /// Maximum characters to keep in output before truncation.
    pub max_output_chars: usize,
    /// Explicitly allowed tools for the CLI session.
    pub allowed_tools: Vec<String>,
    /// Tools to deny for the CLI session.
    pub deny_tools: Vec<String>,
}

impl Default for ClaudeCodeConfig {
    fn default() -> Self {
        Self {
            cli_path: "claude".to_string(),
            working_dir: None,
            timeout_secs: 300,
            max_output_chars: 50_000,
            allowed_tools: Vec::new(),
            deny_tools: Vec::new(),
        }
    }
}

impl ClaudeCodeConfig {
    /// Create a config with a custom CLI path.
    pub fn with_cli_path(mut self, path: &str) -> Self {
        self.cli_path = path.to_string();
        self
    }

    /// Set the working directory.
    pub fn with_working_dir(mut self, dir: &str) -> Self {
        self.working_dir = Some(dir.to_string());
        self
    }

    /// Set the timeout in seconds.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set the maximum output characters.
    pub fn with_max_output(mut self, chars: usize) -> Self {
        self.max_output_chars = chars;
        self
    }

    /// Add an allowed tool.
    pub fn allow_tool(mut self, tool: &str) -> Self {
        self.allowed_tools.push(tool.to_string());
        self
    }

    /// Add a denied tool.
    pub fn deny_tool(mut self, tool: &str) -> Self {
        self.deny_tools.push(tool.to_string());
        self
    }
}

/// Result from a Claude Code CLI invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaudeCodeResult {
    /// The CLI output (possibly truncated).
    pub output: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Execution duration in milliseconds.
    pub duration_ms: f64,
    /// Whether the output was truncated.
    pub truncated: bool,
    /// Files that were modified or created.
    pub files_modified: Vec<String>,
}

impl ClaudeCodeTool {
    /// Create a new `ClaudeCodeTool`.
    pub fn new() -> Self {
        Self
    }

    /// Build the CLI command arguments from config and prompt.
    pub fn build_command(config: &ClaudeCodeConfig, prompt: &str) -> Vec<String> {
        let mut args = vec![config.cli_path.clone(), "--print".to_string()];

        if let Some(ref dir) = config.working_dir {
            args.push("--cwd".to_string());
            args.push(dir.clone());
        }

        for tool in &config.allowed_tools {
            args.push("--allowedTools".to_string());
            args.push(tool.clone());
        }

        for tool in &config.deny_tools {
            args.push("--denyTools".to_string());
            args.push(tool.clone());
        }

        args.push(prompt.to_string());
        args
    }

    /// Parse raw CLI output into a structured result.
    pub fn parse_output(raw: &str, max_chars: usize) -> ClaudeCodeResult {
        let truncated = raw.len() > max_chars;
        let output = if truncated {
            format!(
                "{}...[truncated {} chars]",
                &raw[..max_chars],
                raw.len() - max_chars
            )
        } else {
            raw.to_string()
        };

        // Extract modified files from output (lines starting with file-action prefixes).
        let files_modified: Vec<String> = raw
            .lines()
            .filter(|l| {
                l.starts_with("Modified:")
                    || l.starts_with("Created:")
                    || l.starts_with("Wrote:")
            })
            .map(|l| l.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        ClaudeCodeResult {
            output,
            exit_code: 0,
            duration_ms: 0.0,
            truncated,
            files_modified,
        }
    }

    /// Extract file paths from arbitrary output text.
    ///
    /// Looks for common patterns: "Modified: path", "Created: path", "Wrote: path".
    pub fn extract_files(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                for prefix in &["Modified:", "Created:", "Wrote:", "Edited:", "Updated:"] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        let path = rest.trim().to_string();
                        if !path.is_empty() {
                            return Some(path);
                        }
                    }
                }
                None
            })
            .collect()
    }

    /// Check if the Claude CLI is available on the system.
    pub fn is_available() -> bool {
        std::process::Command::new("claude")
            .arg("--version")
            .output()
            .is_ok()
    }

    /// Validate a prompt before execution.
    pub fn validate_prompt(prompt: &str) -> std::result::Result<(), String> {
        if prompt.is_empty() {
            return Err("Prompt cannot be empty".to_string());
        }
        if prompt.len() > 100_000 {
            return Err(format!(
                "Prompt too long: {} chars (max 100000)",
                prompt.len()
            ));
        }
        Ok(())
    }
}

impl Default for ClaudeCodeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::Tool for ClaudeCodeTool {
    fn name(&self) -> &'static str {
        "claude_code"
    }

    fn description(&self) -> &'static str {
        "Run tasks using the Claude Code CLI for code generation, editing, and analysis. \
         Input: {\"prompt\": \"<task description>\"}. \
         Optional: {\"working_dir\": \"<path>\", \"timeout\": <secs>}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if prompt.is_empty() {
            return Ok("claude_code requires a \"prompt\" field.".to_string());
        }

        if let Err(e) = Self::validate_prompt(prompt) {
            return Ok(format!("Invalid prompt: {e}"));
        }

        info!("claude_code: executing prompt ({} chars)", prompt.len());

        let mut config = ClaudeCodeConfig::default();

        // Override working directory if provided.
        if let Some(dir) = input.get("working_dir").and_then(|v| v.as_str()) {
            config.working_dir = Some(dir.to_string());
        }

        // Override timeout if provided.
        if let Some(timeout) = input.get("timeout").and_then(|v| v.as_u64()) {
            config.timeout_secs = timeout;
        }

        let args = Self::build_command(&config, prompt);
        let max_chars = config.max_output_chars;

        // Execute claude CLI in a blocking thread.
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new(&args[0])
                .args(&args[1..])
                .output()
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
        .map_err(|e| anyhow::anyhow!("Failed to execute claude CLI: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let result = Self::parse_output(&stdout, max_chars);
        let exit_code = output.status.code().unwrap_or(-1);

        if exit_code != 0 && !stderr.is_empty() {
            Ok(format!(
                "Claude Code exited with code {exit_code}.\nStderr: {stderr}\nOutput: {}",
                result.output
            ))
        } else {
            Ok(result.output)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config tests ──────────────────────────────────────────────────────

    #[test]
    fn test_config_defaults() {
        let config = ClaudeCodeConfig::default();
        assert_eq!(config.cli_path, "claude");
        assert!(config.working_dir.is_none());
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.max_output_chars, 50_000);
        assert!(config.allowed_tools.is_empty());
        assert!(config.deny_tools.is_empty());
    }

    #[test]
    fn test_config_builder_methods() {
        let config = ClaudeCodeConfig::default()
            .with_cli_path("/usr/local/bin/claude")
            .with_working_dir("/tmp/project")
            .with_timeout(60)
            .with_max_output(10_000)
            .allow_tool("Read")
            .allow_tool("Write")
            .deny_tool("Bash");

        assert_eq!(config.cli_path, "/usr/local/bin/claude");
        assert_eq!(config.working_dir.as_deref(), Some("/tmp/project"));
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_output_chars, 10_000);
        assert_eq!(config.allowed_tools, vec!["Read", "Write"]);
        assert_eq!(config.deny_tools, vec!["Bash"]);
    }

    #[test]
    fn test_config_serialization() {
        let config = ClaudeCodeConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ClaudeCodeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cli_path, config.cli_path);
        assert_eq!(parsed.timeout_secs, config.timeout_secs);
    }

    // ── build_command tests ───────────────────────────────────────────────

    #[test]
    fn test_build_command_basic() {
        let config = ClaudeCodeConfig::default();
        let args = ClaudeCodeTool::build_command(&config, "hello world");
        assert_eq!(args, vec!["claude", "--print", "hello world"]);
    }

    #[test]
    fn test_build_command_with_working_dir() {
        let config = ClaudeCodeConfig::default().with_working_dir("/my/project");
        let args = ClaudeCodeTool::build_command(&config, "fix bug");
        assert_eq!(
            args,
            vec!["claude", "--print", "--cwd", "/my/project", "fix bug"]
        );
    }

    #[test]
    fn test_build_command_with_allowed_tools() {
        let config = ClaudeCodeConfig::default()
            .allow_tool("Read")
            .allow_tool("Grep");
        let args = ClaudeCodeTool::build_command(&config, "search");
        assert_eq!(
            args,
            vec![
                "claude",
                "--print",
                "--allowedTools",
                "Read",
                "--allowedTools",
                "Grep",
                "search"
            ]
        );
    }

    #[test]
    fn test_build_command_with_deny_tools() {
        let config = ClaudeCodeConfig::default().deny_tool("Bash");
        let args = ClaudeCodeTool::build_command(&config, "analyze");
        assert_eq!(
            args,
            vec!["claude", "--print", "--denyTools", "Bash", "analyze"]
        );
    }

    #[test]
    fn test_build_command_with_custom_cli_path() {
        let config = ClaudeCodeConfig::default().with_cli_path("/opt/claude/bin/claude");
        let args = ClaudeCodeTool::build_command(&config, "test");
        assert_eq!(args[0], "/opt/claude/bin/claude");
        assert_eq!(args[1], "--print");
    }

    #[test]
    fn test_build_command_all_options() {
        let config = ClaudeCodeConfig::default()
            .with_working_dir("/src")
            .allow_tool("Read")
            .deny_tool("Bash");
        let args = ClaudeCodeTool::build_command(&config, "prompt");
        assert_eq!(
            args,
            vec![
                "claude",
                "--print",
                "--cwd",
                "/src",
                "--allowedTools",
                "Read",
                "--denyTools",
                "Bash",
                "prompt"
            ]
        );
    }

    // ── parse_output tests ────────────────────────────────────────────────

    #[test]
    fn test_parse_output_no_truncation() {
        let result = ClaudeCodeTool::parse_output("hello world", 100);
        assert_eq!(result.output, "hello world");
        assert!(!result.truncated);
        assert_eq!(result.exit_code, 0);
        assert!(result.files_modified.is_empty());
    }

    #[test]
    fn test_parse_output_with_truncation() {
        let long_text = "a".repeat(200);
        let result = ClaudeCodeTool::parse_output(&long_text, 50);
        assert!(result.truncated);
        assert!(result.output.contains("...[truncated 150 chars]"));
        assert_eq!(result.output.len(), 50 + "...[truncated 150 chars]".len());
    }

    #[test]
    fn test_parse_output_extracts_modified_files() {
        let output = "Some text\nModified: src/main.rs\nMore text\nCreated: src/new.rs\nWrote: docs/readme.md\n";
        let result = ClaudeCodeTool::parse_output(output, 10_000);
        assert_eq!(
            result.files_modified,
            vec!["src/main.rs", "src/new.rs", "docs/readme.md"]
        );
    }

    #[test]
    fn test_parse_output_no_files() {
        let output = "Just some text output\nNo file modifications here\n";
        let result = ClaudeCodeTool::parse_output(output, 10_000);
        assert!(result.files_modified.is_empty());
    }

    #[test]
    fn test_parse_output_empty_string() {
        let result = ClaudeCodeTool::parse_output("", 10_000);
        assert_eq!(result.output, "");
        assert!(!result.truncated);
        assert!(result.files_modified.is_empty());
    }

    // ── extract_files tests ───────────────────────────────────────────────

    #[test]
    fn test_extract_files_all_prefixes() {
        let text = "Modified: a.rs\nCreated: b.rs\nWrote: c.rs\nEdited: d.rs\nUpdated: e.rs\n";
        let files = ClaudeCodeTool::extract_files(text);
        assert_eq!(files, vec!["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"]);
    }

    #[test]
    fn test_extract_files_skips_empty() {
        let text = "Modified:\nCreated: valid.rs\n";
        let files = ClaudeCodeTool::extract_files(text);
        assert_eq!(files, vec!["valid.rs"]);
    }

    // ── validate_prompt tests ─────────────────────────────────────────────

    #[test]
    fn test_validate_prompt_empty() {
        assert!(ClaudeCodeTool::validate_prompt("").is_err());
    }

    #[test]
    fn test_validate_prompt_valid() {
        assert!(ClaudeCodeTool::validate_prompt("fix the bug in main.rs").is_ok());
    }

    #[test]
    fn test_validate_prompt_too_long() {
        let long = "x".repeat(100_001);
        assert!(ClaudeCodeTool::validate_prompt(&long).is_err());
    }

    // ── Tool trait tests ──────────────────────────────────────────────────

    #[test]
    fn test_tool_name_and_description() {
        use crate::tools::Tool;
        let tool = ClaudeCodeTool::new();
        assert_eq!(tool.name(), "claude_code");
        assert!(tool.description().contains("Claude Code CLI"));
    }

    #[tokio::test]
    async fn test_tool_execute_missing_prompt() {
        use crate::tools::Tool;
        let tool = ClaudeCodeTool;
        let result = tool
            .execute(serde_json::json!({}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    // ── Result serialization ──────────────────────────────────────────────

    #[test]
    fn test_result_serialization() {
        let result = ClaudeCodeResult {
            output: "done".to_string(),
            exit_code: 0,
            duration_ms: 123.4,
            truncated: false,
            files_modified: vec!["a.rs".to_string()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ClaudeCodeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.output, "done");
        assert_eq!(parsed.exit_code, 0);
        assert_eq!(parsed.files_modified, vec!["a.rs"]);
    }
}
