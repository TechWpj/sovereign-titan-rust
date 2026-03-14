//! Sandbox — restricted execution environment for untrusted operations.
//!
//! Ported from `sovereign_titan/cognitive/sandbox.py`.
//! Provides resource-limited command execution with timeout, output capture,
//! and blocklist enforcement.

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Result of a sandboxed execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub blocked: bool,
    pub blocked_reason: Option<String>,
}

/// Sandbox configuration.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum execution time.
    pub timeout: Duration,
    /// Maximum output size (bytes).
    pub max_output_bytes: usize,
    /// Blocked command patterns.
    pub blocked_patterns: HashSet<String>,
    /// Whether network access is allowed.
    pub allow_network: bool,
    /// Working directory for sandboxed commands.
    pub work_dir: Option<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        let mut blocked = HashSet::new();
        // Dangerous commands that should never be sandboxed
        for pattern in &[
            "rm -rf /", "format c:", "del /s /q",
            ":(){ :|:& };:", "mkfs", "dd if=/dev/zero",
            "shutdown", "reboot", "halt",
            "reg delete", "bcdedit",
        ] {
            blocked.insert(pattern.to_string());
        }

        Self {
            timeout: Duration::from_secs(30),
            max_output_bytes: 1024 * 1024, // 1 MB
            blocked_patterns: blocked,
            allow_network: false,
            work_dir: None,
        }
    }
}

/// Sandboxed execution environment.
pub struct Sandbox {
    config: SandboxConfig,
    /// Execution history.
    history: Vec<SandboxHistoryEntry>,
    /// Total executions.
    total_executions: u64,
    /// Total blocked executions.
    total_blocked: u64,
}

/// History entry for a sandboxed execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxHistoryEntry {
    pub command: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub blocked: bool,
    pub timestamp: f64,
}

impl Sandbox {
    /// Create a new sandbox with the given config.
    pub fn new(config: SandboxConfig) -> Self {
        Self {
            config,
            history: Vec::new(),
            total_executions: 0,
            total_blocked: 0,
        }
    }

    /// Check if a command is blocked.
    pub fn is_blocked(&self, command: &str) -> Option<String> {
        let lower = command.to_lowercase();
        for pattern in &self.config.blocked_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return Some(format!("Command matches blocked pattern: {}", pattern));
            }
        }

        // Check for shell injection attempts
        let dangerous_chars = ['|', '`', '$', ';', '&', '>', '<'];
        if dangerous_chars.iter().any(|&c| command.contains(c)) {
            // Allow some safe uses
            if !command.contains("&&") && !command.contains("||") {
                // Single use of pipe or redirect might be OK in some contexts,
                // but in a sandbox, be conservative
                return Some("Command contains potentially dangerous shell characters".to_string());
            }
        }

        None
    }

    /// Execute a command in the sandbox.
    pub async fn execute(&mut self, command: &str) -> SandboxResult {
        self.total_executions += 1;

        // Check blocklist
        if let Some(reason) = self.is_blocked(command) {
            self.total_blocked += 1;
            self.record_history(command, 1, false, true);
            return SandboxResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                timed_out: false,
                blocked: true,
                blocked_reason: Some(reason),
            };
        }

        // Execute with timeout
        let timeout = self.config.timeout;
        let max_output = self.config.max_output_bytes;
        let cmd = command.to_string();

        let result = tokio::time::timeout(timeout, async {
            let output = tokio::process::Command::new("cmd")
                .args(["/C", &cmd])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output()
                .await;

            match output {
                Ok(out) => {
                    let mut stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let mut stderr = String::from_utf8_lossy(&out.stderr).to_string();

                    // Truncate output if too large
                    if stdout.len() > max_output {
                        stdout.truncate(max_output);
                        stdout.push_str("\n[...output truncated...]");
                    }
                    if stderr.len() > max_output {
                        stderr.truncate(max_output);
                        stderr.push_str("\n[...output truncated...]");
                    }

                    SandboxResult {
                        stdout,
                        stderr,
                        exit_code: out.status.code().unwrap_or(-1),
                        timed_out: false,
                        blocked: false,
                        blocked_reason: None,
                    }
                }
                Err(e) => SandboxResult {
                    stdout: String::new(),
                    stderr: format!("Failed to execute command: {e}"),
                    exit_code: -1,
                    timed_out: false,
                    blocked: false,
                    blocked_reason: None,
                },
            }
        })
        .await;

        match result {
            Ok(sandbox_result) => {
                self.record_history(command, sandbox_result.exit_code, false, false);
                sandbox_result
            }
            Err(_) => {
                self.record_history(command, -1, true, false);
                SandboxResult {
                    stdout: String::new(),
                    stderr: "Command timed out".to_string(),
                    exit_code: -1,
                    timed_out: true,
                    blocked: false,
                    blocked_reason: None,
                }
            }
        }
    }

    fn record_history(&mut self, command: &str, exit_code: i32, timed_out: bool, blocked: bool) {
        use std::time::{SystemTime, UNIX_EPOCH};
        self.history.push(SandboxHistoryEntry {
            command: command.chars().take(200).collect(),
            exit_code,
            timed_out,
            blocked,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        });

        // Keep last 100 entries
        if self.history.len() > 100 {
            self.history.drain(..self.history.len() - 100);
        }
    }

    /// Get execution statistics.
    pub fn stats(&self) -> SandboxStats {
        SandboxStats {
            total_executions: self.total_executions,
            total_blocked: self.total_blocked,
            timeout_secs: self.config.timeout.as_secs(),
            blocked_patterns_count: self.config.blocked_patterns.len(),
        }
    }

    /// Get execution history.
    pub fn history(&self) -> &[SandboxHistoryEntry] {
        &self.history
    }

    /// Add a blocked pattern.
    pub fn add_blocked_pattern(&mut self, pattern: &str) {
        self.config.blocked_patterns.insert(pattern.to_string());
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new(SandboxConfig::default())
    }
}

/// Sandbox statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStats {
    pub total_executions: u64,
    pub total_blocked: u64,
    pub timeout_secs: u64,
    pub blocked_patterns_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_pattern() {
        let sandbox = Sandbox::default();
        assert!(sandbox.is_blocked("rm -rf /").is_some());
        assert!(sandbox.is_blocked("shutdown").is_some());
    }

    #[test]
    fn test_allowed_command() {
        let sandbox = Sandbox::default();
        assert!(sandbox.is_blocked("echo hello").is_none());
    }

    #[test]
    fn test_dangerous_chars() {
        let sandbox = Sandbox::default();
        assert!(sandbox.is_blocked("echo test | rm").is_some());
    }

    #[test]
    fn test_add_blocked_pattern() {
        let mut sandbox = Sandbox::default();
        sandbox.add_blocked_pattern("custom_danger");
        assert!(sandbox.is_blocked("run custom_danger now").is_some());
    }

    #[test]
    fn test_stats() {
        let sandbox = Sandbox::default();
        let stats = sandbox.stats();
        assert_eq!(stats.total_executions, 0);
        assert!(stats.blocked_patterns_count > 0);
    }

    #[tokio::test]
    async fn test_execute_blocked() {
        let mut sandbox = Sandbox::default();
        let result = sandbox.execute("rm -rf /").await;
        assert!(result.blocked);
        assert!(result.blocked_reason.is_some());
    }

    #[tokio::test]
    async fn test_execute_echo() {
        let mut sandbox = Sandbox::default();
        let result = sandbox.execute("echo hello_sandbox_test").await;
        assert!(!result.blocked);
        assert!(!result.timed_out);
        assert!(result.stdout.contains("hello_sandbox_test"));
    }
}
