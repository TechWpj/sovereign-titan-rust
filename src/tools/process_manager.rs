//! Process Manager Tool — rich process listing, killing, and info.
//!
//! Actions: list, kill, info, tree.
//! Uses `sysinfo` crate + PowerShell for richer process data.

use anyhow::Result;
use serde_json::Value;
use sysinfo::System;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Processes that must never be killed.
const CRITICAL_PROCESSES: &[&str] = &[
    "csrss", "lsass", "services", "svchost", "wininit", "smss", "dwm",
    "system", "winlogon", "fontdrvhost", "memory compression",
];

pub struct ProcessManagerTool;

impl ProcessManagerTool {
    fn run_ps(cmd: &str) -> String {
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .spawn();

        match result {
            Ok(child) => match child.wait_with_output() {
                Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        }
    }

    fn list_processes(filter: &str, sort_by: &str, limit: usize) -> String {
        let mut sys = System::new();
        sys.refresh_processes();

        let mut procs: Vec<_> = sys
            .processes()
            .values()
            .filter(|p| {
                if filter.is_empty() {
                    true
                } else {
                    p.name().to_lowercase().contains(&filter.to_lowercase())
                }
            })
            .collect();

        match sort_by {
            "cpu" => procs.sort_by(|a, b| b.cpu_usage().partial_cmp(&a.cpu_usage()).unwrap_or(std::cmp::Ordering::Equal)),
            "name" => procs.sort_by(|a, b| a.name().cmp(b.name())),
            _ => procs.sort_by(|a, b| b.memory().cmp(&a.memory())), // default: memory
        }

        procs.truncate(limit);

        if procs.is_empty() {
            if filter.is_empty() {
                return "No processes found.".to_string();
            }
            return format!("No processes matching '{filter}'.");
        }

        let mut lines = vec![format!(
            "{:<8} {:<30} {:>10} {:>8}",
            "PID", "Name", "Mem (MB)", "CPU %"
        )];
        lines.push("-".repeat(60));

        for p in &procs {
            let mem_mb = p.memory() as f64 / (1024.0 * 1024.0);
            let name = p.name();
            let display_name = &name[..name.len().min(30)];
            lines.push(format!(
                "{:<8} {:<30} {:>10.1} {:>8.1}",
                p.pid(),
                display_name,
                mem_mb,
                p.cpu_usage()
            ));
        }

        lines.join("\n")
    }

    fn kill_process(target: &str) -> String {
        let lower = target.trim().to_lowercase();

        if CRITICAL_PROCESSES.iter().any(|&p| lower.contains(p)) {
            return format!("BLOCKED: Cannot kill critical system process '{target}'.");
        }

        // Try as PID first
        if let Ok(pid) = target.parse::<u32>() {
            let mut sys = System::new();
            sys.refresh_processes();
            if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid)) {
                let name = process.name().to_string();
                if process.kill() {
                    return format!("Killed process {pid} ({name}).");
                }
                return format!("Failed to kill process {pid} ({name}).");
            }
            return format!("No process with PID {pid}.");
        }

        // Kill by name
        let mut sys = System::new();
        sys.refresh_processes();
        let mut killed = 0;
        for (_, process) in sys.processes() {
            if process.name().to_lowercase().contains(&lower) {
                if process.kill() {
                    killed += 1;
                }
            }
        }

        if killed > 0 {
            format!("Killed {killed} process(es) matching '{target}'.")
        } else {
            format!("No running processes matching '{target}'.")
        }
    }

    fn process_info(target: &str) -> String {
        let mut sys = System::new();
        sys.refresh_processes();

        let lower = target.trim().to_lowercase();

        // Find by PID or name
        let process = if let Ok(pid) = target.parse::<u32>() {
            sys.process(sysinfo::Pid::from_u32(pid))
        } else {
            sys.processes()
                .values()
                .find(|p| p.name().to_lowercase().contains(&lower))
        };

        match process {
            Some(p) => {
                let mem_mb = p.memory() as f64 / (1024.0 * 1024.0);
                let virt_mb = p.virtual_memory() as f64 / (1024.0 * 1024.0);
                format!(
                    "Process: {}\nPID: {}\nMemory: {:.1} MB\nVirtual Memory: {:.1} MB\nCPU: {:.1}%\nStatus: {:?}\nPath: {}",
                    p.name(),
                    p.pid(),
                    mem_mb,
                    virt_mb,
                    p.cpu_usage(),
                    p.status(),
                    p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "N/A".to_string())
                )
            }
            None => format!("No process found matching '{target}'."),
        }
    }

    fn process_tree() -> String {
        let cmd = r#"
$procs = Get-CimInstance Win32_Process | Select-Object ProcessId, Name, ParentProcessId, @{N='MemMB';E={[math]::Round($_.WorkingSetSize/1MB,1)}}
$roots = $procs | Where-Object { $_.ParentProcessId -eq 0 -or -not ($procs.ProcessId -contains $_.ParentProcessId) } | Sort-Object Name | Select-Object -First 20
$output = @()
foreach ($root in $roots) {
    $output += "$($root.Name) (PID $($root.ProcessId), $($root.MemMB)MB)"
    $children = $procs | Where-Object { $_.ParentProcessId -eq $root.ProcessId } | Sort-Object Name | Select-Object -First 5
    foreach ($child in $children) {
        $output += "  └─ $($child.Name) (PID $($child.ProcessId), $($child.MemMB)MB)"
    }
}
$output -join "`n"
"#;
        let output = Self::run_ps(cmd);
        if output.is_empty() {
            "No process tree available.".to_string()
        } else {
            format!("Process Tree (top 20 roots):\n{output}")
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for ProcessManagerTool {
    fn name(&self) -> &'static str {
        "process_manager"
    }

    fn description(&self) -> &'static str {
        "Manage system processes. Input: {\"action\": \"<action>\", ...}. \
         Actions: list (filter?, sort_by?: memory|cpu|name, limit?: 30), \
         kill (target — name or PID), info (target — name or PID), \
         tree (shows process hierarchy)."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        info!("process_manager: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "list" => {
                    let filter = input_clone.get("filter").and_then(|v| v.as_str()).unwrap_or("");
                    let sort_by = input_clone.get("sort_by").and_then(|v| v.as_str()).unwrap_or("memory");
                    let limit = input_clone.get("limit").and_then(|v| v.as_u64()).unwrap_or(30) as usize;
                    Self::list_processes(filter, sort_by, limit)
                }
                "kill" => {
                    let target = input_clone.get("target").or_else(|| input_clone.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                    if target.is_empty() {
                        "kill requires a \"target\" field (name or PID).".to_string()
                    } else {
                        Self::kill_process(target)
                    }
                }
                "info" => {
                    let target = input_clone.get("target").or_else(|| input_clone.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                    if target.is_empty() {
                        "info requires a \"target\" field (name or PID).".to_string()
                    } else {
                        Self::process_info(target)
                    }
                }
                "tree" => Self::process_tree(),
                other => format!(
                    "Unknown action: '{other}'. Use: list, kill, info, tree."
                ),
            }
        })
        .await?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_list_processes() {
        let tool = ProcessManagerTool;
        let result = tool.execute(json!({"action": "list", "limit": 5})).await.unwrap();
        assert!(result.contains("PID") || result.contains("Name"));
    }

    #[tokio::test]
    async fn test_kill_critical_blocked() {
        let result = ProcessManagerTool::kill_process("csrss");
        assert!(result.contains("BLOCKED"));
    }

    #[tokio::test]
    async fn test_kill_missing_target() {
        let tool = ProcessManagerTool;
        let result = tool.execute(json!({"action": "kill"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_info_nonexistent() {
        let result = ProcessManagerTool::process_info("xyznonexistent999");
        assert!(result.contains("No process"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = ProcessManagerTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_default_is_list() {
        let tool = ProcessManagerTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("PID") || result.contains("Name") || result.contains("No processes"));
    }
}
