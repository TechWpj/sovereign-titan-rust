//! System Control Tool — OS-level control for processes, programs, services, and power.
//!
//! Ported from `sovereign_titan/tools/system_control.py`. Provides native
//! Windows system control via PowerShell subprocess calls with comprehensive
//! safety guardrails (critical process/service protection, blocked programs).

use std::os::windows::process::CommandExt;

use serde_json::Value;
use tracing::{info, warn};

use super::Tool;

/// Processes that must NEVER be killed.
const CRITICAL_PROCESSES: &[&str] = &[
    "csrss", "lsass", "services", "svchost", "wininit", "smss", "dwm",
    "system", "winlogon", "fontdrvhost", "memory compression",
];

/// Processes that are protected but can be killed with a warning.
const PROTECTED_PROCESSES: &[&str] = &[
    "explorer", "taskmgr", "searchui", "startmenuexperiencehost",
    "shellexperiencehost", "runtimebroker",
];

/// Programs that are never allowed to be launched.
const BLOCKED_PROGRAMS: &[&str] = &[
    "diskpart", "format", "bcdedit", "sfc", "dism", "reagentc",
    "bcdboot", "bootrec", "wbadmin", "cipher",
];

/// Services that must not be stopped.
const CRITICAL_SERVICES: &[&str] = &[
    "rpcss", "dcomlaunch", "lsm", "samss", "eventlog", "plugplay",
    "power", "profiling", "windefend", "mpssvc", "bfe", "dnscache",
    "nsi", "dhcp", "lanmanworkstation",
];

/// Well-known browser executable paths for resolution.
const BROWSER_PATHS: &[(&str, &str)] = &[
    ("chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
    ("google chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
    ("msedge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
    ("edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
    ("microsoft edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
    ("firefox", r"C:\Program Files\Mozilla Firefox\firefox.exe"),
    ("brave", r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"),
];

/// Well-known application name → executable mappings.
const KNOWN_APPS: &[(&str, &str)] = &[
    ("notepad", "notepad.exe"),
    ("calculator", "calc.exe"),
    ("calc", "calc.exe"),
    ("paint", "mspaint.exe"),
    ("task manager", "taskmgr.exe"),
    ("cmd", "cmd.exe"),
    ("command prompt", "cmd.exe"),
    ("powershell", "powershell.exe"),
    ("terminal", "wt.exe"),
    ("windows terminal", "wt.exe"),
    ("explorer", "explorer.exe"),
    ("file explorer", "explorer.exe"),
    ("snipping tool", "snippingtool.exe"),
    ("control panel", "control.exe"),
    ("settings", "ms-settings:"),
    ("device manager", "devmgmt.msc"),
];

/// Native system control tool for OS operations.
pub struct SystemControlTool;

impl SystemControlTool {
    /// Escape a string for safe inclusion in a PowerShell command.
    fn escape_ps(s: &str) -> String {
        s.replace('`', "``")
            .replace('$', "`$")
            .replace('"', "`\"")
            .replace(';', "`;")
    }

    /// Check for shell injection patterns in a string.
    fn has_injection(s: &str) -> bool {
        let patterns = [";;", "&&", "||", "$(", "`(", "| ", "> ", ">> ", "< "];
        patterns.iter().any(|p| s.contains(p))
    }

    /// Run a PowerShell command and return (exit_code, stdout, stderr).
    fn run_ps(cmd: &str) -> (i32, String, String) {
        match std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()
        {
            Ok(child) => match child.wait_with_output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let code = output.status.code().unwrap_or(-1);
                    (code, stdout, stderr)
                }
                Err(e) => (-1, String::new(), format!("Process error: {e}")),
            },
            Err(e) => (-1, String::new(), format!("Failed to spawn PowerShell: {e}")),
        }
    }

    /// Resolve a program name to an executable path.
    fn resolve_program(name: &str) -> String {
        let lower = name.trim().to_lowercase();

        // 1. Check known apps.
        for &(app_name, exe) in KNOWN_APPS {
            if lower == app_name {
                return exe.to_string();
            }
        }

        // 2. Check browser paths.
        for &(browser_name, path) in BROWSER_PATHS {
            if lower == browser_name {
                return path.to_string();
            }
        }

        // 3. If it looks like a full path already, return as-is.
        if name.contains('\\') || name.contains('/') || name.ends_with(".exe") {
            return name.to_string();
        }

        // 4. Try to find via `where.exe` (PATH lookup).
        let (code, stdout, _) = Self::run_ps(&format!("(Get-Command '{}' -ErrorAction SilentlyContinue).Source", Self::escape_ps(&lower)));
        if code == 0 && !stdout.trim().is_empty() {
            return stdout.trim().to_string();
        }

        // 5. Fallback — return the name and let Start-Process handle it.
        name.to_string()
    }

    // ── Action handlers ──────────────────────────────────────────────────

    fn kill_process(input: &Value) -> String {
        let target = match input.get("name").or(input.get("target")).and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return "kill_process requires a \"name\" field.".to_string(),
        };

        let lower = target.trim().to_lowercase();

        // Block critical processes.
        if CRITICAL_PROCESSES.iter().any(|&p| lower.contains(p)) {
            warn!("system_control: BLOCKED kill of critical process: {target}");
            return format!("BLOCKED: Cannot kill critical system process '{target}'.");
        }

        if PROTECTED_PROCESSES.iter().any(|&p| lower.contains(p)) {
            warn!("system_control: killing protected process (with warning): {target}");
        }

        let escaped = Self::escape_ps(target);

        // Try by name first, then by PID if it looks numeric.
        let cmd = if target.chars().all(|c| c.is_ascii_digit()) {
            format!("Stop-Process -Id {escaped} -Force -ErrorAction Stop")
        } else {
            format!("Stop-Process -Name '{escaped}' -Force -ErrorAction Stop")
        };

        let (code, _stdout, stderr) = Self::run_ps(&cmd);
        if code == 0 {
            format!("Successfully killed process '{target}'.")
        } else {
            format!("Failed to kill '{target}': {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn list_processes(input: &Value) -> String {
        let filter = input.get("filter").and_then(|v| v.as_str()).unwrap_or("");
        let cmd = if filter.is_empty() {
            "Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First 30 Name, Id, @{N='MemMB';E={[math]::Round($_.WorkingSet64/1MB,1)}} | Format-Table -AutoSize | Out-String".to_string()
        } else {
            let escaped = Self::escape_ps(filter);
            format!("Get-Process | Where-Object {{ $_.Name -like '*{escaped}*' }} | Sort-Object WorkingSet64 -Descending | Select-Object -First 30 Name, Id, @{{N='MemMB';E={{[math]::Round($_.WorkingSet64/1MB,1)}}}} | Format-Table -AutoSize | Out-String")
        };

        let (_, stdout, stderr) = Self::run_ps(&cmd);
        if stdout.trim().is_empty() {
            if !stderr.trim().is_empty() {
                format!("Error listing processes: {}", &stderr.trim()[..stderr.trim().len().min(200)])
            } else {
                "No processes found.".to_string()
            }
        } else {
            stdout.trim().to_string()
        }
    }

    fn start_program(input: &Value) -> String {
        let target = match input.get("name").or(input.get("target")).or(input.get("program")).and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return "start_program requires a \"name\" field.".to_string(),
        };

        let lower = target.trim().to_lowercase();

        // Block dangerous programs.
        if BLOCKED_PROGRAMS.iter().any(|&b| lower.starts_with(b)) {
            warn!("system_control: BLOCKED launch of: {target}");
            return format!("BLOCKED: Cannot launch '{target}' for safety reasons.");
        }

        if Self::has_injection(target) {
            warn!("system_control: injection pattern detected in: {target}");
            return "BLOCKED: Suspicious characters detected in program name.".to_string();
        }

        // Extract URL from target if present (e.g., "chrome https://google.com").
        let (program, args) = if let Some(idx) = target.find("http://").or_else(|| target.find("https://")) {
            (target[..idx].trim(), Some(target[idx..].trim()))
        } else {
            (target.trim(), input.get("args").and_then(|v| v.as_str()))
        };

        let resolved = Self::resolve_program(program);

        // For ms-settings: and shell: URIs, use Start-Process directly.
        if resolved.starts_with("ms-settings:") || resolved.starts_with("shell:") {
            let (code, _, stderr) = Self::run_ps(&format!("Start-Process '{}'", Self::escape_ps(&resolved)));
            return if code == 0 {
                format!("Launched '{target}'.")
            } else {
                format!("Failed to launch '{target}': {}", &stderr.trim()[..stderr.trim().len().min(200)])
            };
        }

        let cmd = if let Some(args) = args {
            format!(
                "Start-Process '{}' -ArgumentList '{}'",
                Self::escape_ps(&resolved),
                Self::escape_ps(args)
            )
        } else {
            format!("Start-Process '{}'", Self::escape_ps(&resolved))
        };

        let (code, _, stderr) = Self::run_ps(&cmd);
        if code == 0 {
            format!("Launched '{target}'.")
        } else {
            format!("Failed to launch '{target}': {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn manage_service(input: &Value, action: &str) -> String {
        let name = match input.get("name").or(input.get("service")).and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return format!("{action}_service requires a \"name\" field."),
        };

        let lower = name.trim().to_lowercase();

        // Block critical services from being stopped/restarted.
        if action != "start" && CRITICAL_SERVICES.iter().any(|&s| lower == s) {
            warn!("system_control: BLOCKED {action} of critical service: {name}");
            return format!("BLOCKED: Cannot {action} critical system service '{name}'.");
        }

        let escaped = Self::escape_ps(name);
        let cmd = match action {
            "start" => format!("Start-Service -Name '{escaped}' -ErrorAction Stop"),
            "stop" => format!("Stop-Service -Name '{escaped}' -Force -ErrorAction Stop"),
            "restart" => format!("Restart-Service -Name '{escaped}' -Force -ErrorAction Stop"),
            _ => return format!("Unknown service action: {action}"),
        };

        let (code, _, stderr) = Self::run_ps(&cmd);
        if code == 0 {
            format!("Service '{name}' {action}ed successfully.")
        } else {
            format!("Failed to {action} service '{name}': {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn power_action(action: &str) -> String {
        let cmd = match action {
            "lock" => "rundll32.exe user32.dll,LockWorkStation",
            "sleep" => "rundll32.exe powrprof.dll,SetSuspendState 0,1,0",
            "shutdown" => {
                return "BLOCKED: Use the shell tool with 'shutdown /s /t 60' if you really need this.".to_string();
            }
            "restart" => {
                return "BLOCKED: Use the shell tool with 'shutdown /r /t 60' if you really need this.".to_string();
            }
            _ => return format!("Unknown power action: {action}"),
        };

        let (code, _, stderr) = Self::run_ps(cmd);
        if code == 0 {
            format!("Power action '{action}' executed.")
        } else {
            format!("Power action '{action}' failed: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    /// Normalize action aliases (e.g., "start_process" → "start_program").
    fn normalize_action(action: &str) -> &str {
        match action {
            "start_process" | "launch" | "open" | "run" => "start_program",
            "kill" | "terminate" | "stop_process" | "end_process" => "kill_process",
            "processes" | "ps" => "list_processes",
            "start_svc" => "start_service",
            "stop_svc" => "stop_service",
            "restart_svc" => "restart_service",
            "restart_computer" | "reboot" => "restart",
            other => other,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SystemControlTool {
    fn name(&self) -> &'static str {
        "system_control"
    }

    fn description(&self) -> &'static str {
        "Control the operating system: manage processes, launch programs, control services, \
         and power actions. Input: {\"action\": \"<action>\", ...params}. \
         Actions: start_program (name), kill_process (name), list_processes (filter?), \
         start_service/stop_service/restart_service (name), lock, sleep."
    }

    async fn execute(&self, input: Value) -> Result<String, anyhow::Error> {
        let action_raw = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("system_control requires an \"action\" string field"))?;

        let action = Self::normalize_action(action_raw);

        info!("system_control: action={action}");

        // Run in blocking task since all operations spawn subprocesses.
        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "kill_process" => Self::kill_process(&input_clone),
                "list_processes" => Self::list_processes(&input_clone),
                "start_program" => Self::start_program(&input_clone),
                "start_service" => Self::manage_service(&input_clone, "start"),
                "stop_service" => Self::manage_service(&input_clone, "stop"),
                "restart_service" => Self::manage_service(&input_clone, "restart"),
                "lock" | "sleep" | "shutdown" | "restart" => Self::power_action(&action_owned),
                other => format!("Unknown action: '{other}'. Available: start_program, kill_process, \
                    list_processes, start_service, stop_service, restart_service, lock, sleep."),
            }
        })
        .await?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_action() {
        assert_eq!(SystemControlTool::normalize_action("start_process"), "start_program");
        assert_eq!(SystemControlTool::normalize_action("kill"), "kill_process");
        assert_eq!(SystemControlTool::normalize_action("ps"), "list_processes");
        assert_eq!(SystemControlTool::normalize_action("lock"), "lock");
    }

    #[test]
    fn test_escape_ps() {
        assert_eq!(SystemControlTool::escape_ps("hello$world"), "hello`$world");
        assert_eq!(SystemControlTool::escape_ps("say \"hi\""), "say `\"hi`\"");
    }

    #[test]
    fn test_has_injection() {
        assert!(SystemControlTool::has_injection("foo && bar"));
        assert!(SystemControlTool::has_injection("$(whoami)"));
        assert!(!SystemControlTool::has_injection("chrome https://google.com"));
    }

    #[test]
    fn test_resolve_known_app() {
        assert_eq!(SystemControlTool::resolve_program("notepad"), "notepad.exe");
        assert_eq!(SystemControlTool::resolve_program("calculator"), "calc.exe");
    }

    #[test]
    fn test_critical_process_blocked() {
        let result = SystemControlTool::kill_process(&json!({"name": "csrss"}));
        assert!(result.contains("BLOCKED"));
    }

    #[test]
    fn test_blocked_program() {
        let result = SystemControlTool::start_program(&json!({"name": "diskpart"}));
        assert!(result.contains("BLOCKED"));
    }

    #[tokio::test]
    async fn test_missing_action_returns_error() {
        let tool = SystemControlTool;
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = SystemControlTool;
        let result = tool.execute(json!({"action": "fly_to_moon"})).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Unknown action"));
    }
}
