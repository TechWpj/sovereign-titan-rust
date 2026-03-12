//! Shell Tool — execute system commands with safety guardrails.
//!
//! Ported from `sovereign_titan/tools/shell.py`. Runs commands via PowerShell
//! on Windows with a hardcoded blocklist of destructive commands, automatic
//! `Start-Process` wrapping for GUI programs, and output truncation.

use std::os::windows::process::CommandExt;
use std::time::Duration;

use serde_json::Value;
use tracing::{info, warn};

use super::Tool;

/// Maximum output length before truncation.
const MAX_OUTPUT: usize = 10_000;

/// Default execution timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Commands that are unconditionally blocked.
const HARD_BLOCKED: &[&str] = &[
    "format c:",
    "format d:",
    "format e:",
    "rm -rf /",
    "rm -rf ~",
    "del /s /q c:\\",
    "rd /s /q c:\\",
    "diskpart",
    "bcdedit",
    "cipher /w",
    "sfc /scannow",
    "dism",
    "bcdboot",
    "bootrec",
    "reagentc",
    "wbadmin",
    "vssadmin delete",
    "reg delete",
    "reg add",
    "sc delete",
    "sc stop windefend",
    "sc stop mpssvc",
    "netsh advfirewall set allprofiles state off",
    "netsh firewall set opmode disable",
    "takeown /f c:\\windows",
    "icacls c:\\windows /grant",
    "powershell -encodedcommand",
    "powershell -enc",
    "iex(new-object",
    "invoke-webrequest",
    "invoke-expression",
    "downloadstring",
    "downloadfile",
    "start-bitstransfer",
    "certutil -urlcache",
    "bitsadmin /transfer",
    "mshta",
    "cscript",
    "wscript",
    "msiexec /i http",
    "schtasks /create",
    "at /every",
    "net user /add",
    "net localgroup administrators",
    "runas /user:administrator",
    "wmic os get",
    "wmic process call create",
    "enable-psremoting",
    "set-executionpolicy unrestricted",
    "set-executionpolicy bypass",
    "shutdown /s",
    "shutdown /r",
    "shutdown /p",
    "stop-computer",
    "restart-computer",
    "logoff",
    "taskkill /f /im csrss",
    "taskkill /f /im lsass",
    "taskkill /f /im services",
    "taskkill /f /im svchost",
    "taskkill /f /im wininit",
    "taskkill /f /im smss",
];

/// Interactive commands that hang if run bare (no arguments).
const INTERACTIVE_BARE: &[&str] = &[
    "python",
    "python3",
    "node",
    "powershell",
    "pwsh",
    "cmd",
    "bash",
    "wsl",
    "irb",
    "ghci",
    "lua",
    "R",
    "julia",
];

/// GUI programs that should be launched with `Start-Process` to avoid blocking.
const GUI_PROGRAMS: &[&str] = &[
    "notepad",
    "notepad++",
    "code",
    "devenv",
    "chrome",
    "msedge",
    "firefox",
    "brave",
    "explorer",
    "mspaint",
    "calc",
    "winword",
    "excel",
    "powerpnt",
    "outlook",
    "teams",
    "discord",
    "spotify",
    "vlc",
    "steam",
];

/// Native shell command executor with safety guardrails.
pub struct ShellTool;

impl ShellTool {
    /// Strip obfuscation characters (carets, backticks) for blocklist checking.
    fn deobfuscate(cmd: &str) -> String {
        cmd.replace('^', "").replace('`', "").to_lowercase()
    }

    /// Check if the command is hard-blocked.
    fn is_blocked(cmd: &str) -> bool {
        let clean = Self::deobfuscate(cmd);
        HARD_BLOCKED.iter().any(|b| clean.contains(b))
    }

    /// Check if command is an interactive bare command (would hang).
    fn is_interactive_bare(cmd: &str) -> bool {
        let trimmed = cmd.trim();
        INTERACTIVE_BARE
            .iter()
            .any(|&prog| trimmed.eq_ignore_ascii_case(prog))
    }

    /// Check if command starts with a GUI program that needs Start-Process wrapping.
    fn needs_gui_wrap(cmd: &str) -> Option<&'static str> {
        let lower = cmd.trim().to_lowercase();
        GUI_PROGRAMS
            .iter()
            .find(|&&prog| {
                lower.starts_with(prog)
                    && lower[prog.len()..]
                        .chars()
                        .next()
                        .map_or(true, |c| c.is_whitespace() || c == '.')
            })
            .copied()
    }

    /// Unwrap JSON-wrapped commands (model sometimes emits `{"command": "..."}`)
    fn unwrap_command(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                    return cmd.to_string();
                }
            }
        }
        trimmed.to_string()
    }

    /// Truncate output to MAX_OUTPUT chars.
    fn truncate(s: &str) -> String {
        if s.len() > MAX_OUTPUT {
            format!("{}...\n[truncated at {} chars]", &s[..MAX_OUTPUT], MAX_OUTPUT)
        } else {
            s.to_string()
        }
    }
}

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn description(&self) -> &'static str {
        "Execute a system shell command. Input: {\"command\": \"<shell command>\"}. \
         Runs via PowerShell on Windows. Destructive commands are blocked for safety."
    }

    async fn execute(&self, input: Value) -> Result<String, anyhow::Error> {
        // Extract command string.
        let raw = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("shell requires a \"command\" string field"))?;

        let command = Self::unwrap_command(raw);

        // Safety checks.
        if Self::is_blocked(&command) {
            warn!("shell: BLOCKED destructive command: {command}");
            return Ok(format!("BLOCKED: This command is not allowed for safety reasons."));
        }

        if Self::is_interactive_bare(&command) {
            warn!("shell: BLOCKED bare interactive command: {command}");
            return Ok(format!(
                "BLOCKED: Running bare '{command}' would hang. Provide arguments or a script."
            ));
        }

        info!("shell: executing: {command}");

        // Build the PowerShell invocation.
        let ps_command = if let Some(_gui) = Self::needs_gui_wrap(&command) {
            // Wrap GUI programs with Start-Process to avoid blocking.
            format!("Start-Process {command}")
        } else {
            command.clone()
        };

        let timeout = Duration::from_secs(DEFAULT_TIMEOUT_SECS);

        // Run in a blocking task to avoid blocking the async runtime.
        let result = tokio::task::spawn_blocking(move || {
            let child = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn();

            match child {
                Ok(child) => {
                    // Wait with timeout using a thread.
                    let (tx, rx) = std::sync::mpsc::channel();
                    let handle = std::thread::spawn(move || {
                        let output = child.wait_with_output();
                        let _ = tx.send(output);
                    });

                    match rx.recv_timeout(timeout) {
                        Ok(Ok(output)) => {
                            let _ = handle.join();
                            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                            let code = output.status.code().unwrap_or(-1);
                            (code, stdout, stderr)
                        }
                        Ok(Err(e)) => (-1, String::new(), format!("Process error: {e}")),
                        Err(_) => {
                            // Timeout — detach the thread (it will clean up when process exits).
                            drop(handle);
                            (-1, String::new(), format!("Command timed out after {DEFAULT_TIMEOUT_SECS}s"))
                        }
                    }
                }
                Err(e) => (-1, String::new(), format!("Failed to spawn process: {e}")),
            }
        })
        .await?;

        let (code, stdout, stderr) = result;

        // Format the observation.
        let mut obs = String::new();
        if !stdout.trim().is_empty() {
            obs.push_str(&Self::truncate(stdout.trim()));
        }
        if !stderr.trim().is_empty() {
            if !obs.is_empty() {
                obs.push_str("\n\n");
            }
            obs.push_str("[stderr] ");
            obs.push_str(&stderr.trim()[..stderr.trim().len().min(200)]);
        }
        if obs.is_empty() {
            obs = if code == 0 {
                "Command completed successfully (no output).".to_string()
            } else {
                format!("Command exited with code {code} (no output).")
            };
        } else if code != 0 {
            obs.push_str(&format!("\n[exit code: {code}]"));
        }

        Ok(obs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_blocked_command() {
        assert!(ShellTool::is_blocked("format c:"));
        assert!(ShellTool::is_blocked("FORMAT C:"));
        assert!(ShellTool::is_blocked("fo^rmat c:"));
        assert!(!ShellTool::is_blocked("echo hello"));
    }

    #[test]
    fn test_interactive_bare() {
        assert!(ShellTool::is_interactive_bare("python"));
        assert!(ShellTool::is_interactive_bare("  node  "));
        assert!(!ShellTool::is_interactive_bare("python script.py"));
    }

    #[test]
    fn test_gui_wrap() {
        assert!(ShellTool::needs_gui_wrap("notepad test.txt").is_some());
        assert!(ShellTool::needs_gui_wrap("chrome").is_some());
        assert!(ShellTool::needs_gui_wrap("echo hello").is_none());
    }

    #[test]
    fn test_unwrap_json_command() {
        assert_eq!(
            ShellTool::unwrap_command(r#"{"command": "dir"}"#),
            "dir"
        );
        assert_eq!(ShellTool::unwrap_command("dir"), "dir");
    }

    #[tokio::test]
    async fn test_missing_command_returns_error() {
        let tool = ShellTool;
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blocked_command_returns_blocked() {
        let tool = ShellTool;
        let result = tool.execute(json!({"command": "format c:"})).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("BLOCKED"));
    }
}
