//! Preflight Tool — pre-flight validation checks before tool execution.
//!
//! Actions: check_all, check_network, check_disk, check_memory, check_gpu.
//! Uses the `sysinfo` crate for memory/disk and PowerShell for GPU info.
//! Returns structured health check results with pass/warn/fail status.

use anyhow::Result;
use serde_json::Value;
use sysinfo::{Disks, System};
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Minimum free disk space (in MB) before warning.
const DISK_WARN_MB: u64 = 1024;
/// Minimum free disk space (in MB) before failure.
const DISK_FAIL_MB: u64 = 256;
/// Minimum free RAM (in MB) before warning.
const MEM_WARN_MB: u64 = 512;
/// Minimum free RAM (in MB) before failure.
const MEM_FAIL_MB: u64 = 128;

pub struct PreflightTool;

/// Result of a single preflight check.
struct CheckResult {
    name: String,
    status: CheckStatus,
    detail: String,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "PASS"),
            CheckStatus::Warn => write!(f, "WARN"),
            CheckStatus::Fail => write!(f, "FAIL"),
        }
    }
}

impl CheckResult {
    fn format(&self) -> String {
        let icon = match self.status {
            CheckStatus::Pass => "[OK]",
            CheckStatus::Warn => "[!!]",
            CheckStatus::Fail => "[XX]",
        };
        format!("{icon} {}: {} — {}", self.name, self.status, self.detail)
    }
}

impl PreflightTool {
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

    fn check_network() -> CheckResult {
        // Quick connectivity check — try DNS resolution of a well-known host
        use std::net::ToSocketAddrs;
        match "dns.google:443".to_socket_addrs() {
            Ok(mut addrs) => {
                if addrs.next().is_some() {
                    CheckResult {
                        name: "Network".to_string(),
                        status: CheckStatus::Pass,
                        detail: "DNS resolution working (dns.google reachable).".to_string(),
                    }
                } else {
                    CheckResult {
                        name: "Network".to_string(),
                        status: CheckStatus::Warn,
                        detail: "DNS resolved but returned no addresses.".to_string(),
                    }
                }
            }
            Err(e) => CheckResult {
                name: "Network".to_string(),
                status: CheckStatus::Fail,
                detail: format!("DNS resolution failed: {e}. No internet?"),
            },
        }
    }

    fn check_disk() -> CheckResult {
        let disks = Disks::new_with_refreshed_list();

        if disks.list().is_empty() {
            return CheckResult {
                name: "Disk".to_string(),
                status: CheckStatus::Warn,
                detail: "No disks detected.".to_string(),
            };
        }

        let mut worst_status = CheckStatus::Pass;
        let mut details = Vec::new();

        for disk in disks.list() {
            let mount = disk.mount_point().to_string_lossy().to_string();
            let free_mb = disk.available_space() / (1024 * 1024);
            let total_mb = disk.total_space() / (1024 * 1024);
            let used_pct = if total_mb > 0 {
                ((total_mb - free_mb) as f64 / total_mb as f64 * 100.0) as u64
            } else {
                0
            };

            let status = if free_mb < DISK_FAIL_MB {
                CheckStatus::Fail
            } else if free_mb < DISK_WARN_MB {
                CheckStatus::Warn
            } else {
                CheckStatus::Pass
            };

            if status > worst_status {
                worst_status = status;
            }

            details.push(format!(
                "{mount}: {free_mb} MB free / {total_mb} MB total ({used_pct}% used)"
            ));
        }

        CheckResult {
            name: "Disk".to_string(),
            status: worst_status,
            detail: details.join("; "),
        }
    }

    fn check_memory() -> CheckResult {
        let mut sys = System::new();
        sys.refresh_memory();

        let total_mb = sys.total_memory() / (1024 * 1024);
        let available_mb = sys.available_memory() / (1024 * 1024);
        let used_mb = total_mb.saturating_sub(available_mb);
        let used_pct = if total_mb > 0 {
            (used_mb as f64 / total_mb as f64 * 100.0) as u64
        } else {
            0
        };

        let status = if available_mb < MEM_FAIL_MB {
            CheckStatus::Fail
        } else if available_mb < MEM_WARN_MB {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        };

        CheckResult {
            name: "Memory".to_string(),
            status,
            detail: format!(
                "{available_mb} MB available / {total_mb} MB total ({used_pct}% used)"
            ),
        }
    }

    fn check_gpu() -> CheckResult {
        let cmd = r#"
$gpu = Get-CimInstance Win32_VideoController | Select-Object -First 1
if ($gpu) {
    $vram_gb = [math]::Round($gpu.AdapterRAM/1GB, 1)
    "$($gpu.Name)|$($gpu.DriverVersion)|$vram_gb"
} else {
    "NONE"
}
"#;
        let output = Self::run_ps(cmd);

        if output.is_empty() || output == "NONE" {
            return CheckResult {
                name: "GPU".to_string(),
                status: CheckStatus::Warn,
                detail: "No GPU detected or unable to query GPU info.".to_string(),
            };
        }

        let parts: Vec<&str> = output.split('|').collect();
        if parts.len() >= 3 {
            let name = parts[0];
            let driver = parts[1];
            let vram = parts[2];
            CheckResult {
                name: "GPU".to_string(),
                status: CheckStatus::Pass,
                detail: format!("{name}, Driver: {driver}, VRAM: {vram} GB"),
            }
        } else {
            CheckResult {
                name: "GPU".to_string(),
                status: CheckStatus::Pass,
                detail: format!("GPU detected: {output}"),
            }
        }
    }

    fn check_all() -> String {
        let checks = vec![
            Self::check_network(),
            Self::check_disk(),
            Self::check_memory(),
            Self::check_gpu(),
        ];

        let mut lines = vec!["=== Preflight Check Results ===".to_string()];
        let mut pass_count = 0;
        let mut warn_count = 0;
        let mut fail_count = 0;

        for check in &checks {
            lines.push(check.format());
            match check.status {
                CheckStatus::Pass => pass_count += 1,
                CheckStatus::Warn => warn_count += 1,
                CheckStatus::Fail => fail_count += 1,
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "Summary: {pass_count} passed, {warn_count} warnings, {fail_count} failures"
        ));

        if fail_count > 0 {
            lines.push("Status: NOT READY — critical failures detected.".to_string());
        } else if warn_count > 0 {
            lines.push("Status: READY (with warnings)".to_string());
        } else {
            lines.push("Status: ALL CLEAR — system is ready.".to_string());
        }

        lines.join("\n")
    }
}

#[async_trait::async_trait]
impl super::Tool for PreflightTool {
    fn name(&self) -> &'static str {
        "preflight"
    }

    fn description(&self) -> &'static str {
        "Pre-flight validation checks. Input: {\"action\": \"<action>\"}. \
         Actions: check_all (run all checks), check_network (DNS/connectivity), \
         check_disk (free space), check_memory (available RAM), check_gpu (GPU presence/VRAM). \
         Returns pass/warn/fail for each check."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("check_all");

        info!("preflight: action={action}");

        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "check_all" | "all" => Self::check_all(),
                "check_network" | "network" => Self::check_network().format(),
                "check_disk" | "disk" => Self::check_disk().format(),
                "check_memory" | "memory" | "ram" => Self::check_memory().format(),
                "check_gpu" | "gpu" => Self::check_gpu().format(),
                other => format!(
                    "Unknown action: '{other}'. Use: check_all, check_network, check_disk, check_memory, check_gpu."
                ),
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_check_all() {
        let tool = PreflightTool;
        let result = tool
            .execute(json!({"action": "check_all"}))
            .await
            .unwrap();
        assert!(result.contains("Preflight Check Results"));
        assert!(result.contains("Summary:"));
        assert!(result.contains("Status:"));
        // Should contain at least Network, Disk, Memory, GPU sections
        assert!(result.contains("Network"));
        assert!(result.contains("Disk"));
        assert!(result.contains("Memory"));
        assert!(result.contains("GPU"));
    }

    #[tokio::test]
    async fn test_check_disk() {
        let tool = PreflightTool;
        let result = tool
            .execute(json!({"action": "check_disk"}))
            .await
            .unwrap();
        assert!(result.contains("Disk"));
        // Should show at least one disk with MB info
        assert!(result.contains("MB") || result.contains("No disks"));
    }

    #[tokio::test]
    async fn test_check_memory() {
        let tool = PreflightTool;
        let result = tool
            .execute(json!({"action": "check_memory"}))
            .await
            .unwrap();
        assert!(result.contains("Memory"));
        assert!(result.contains("MB"));
    }

    #[tokio::test]
    async fn test_check_network() {
        let tool = PreflightTool;
        let result = tool
            .execute(json!({"action": "check_network"}))
            .await
            .unwrap();
        assert!(result.contains("Network"));
    }

    #[tokio::test]
    async fn test_check_gpu() {
        let tool = PreflightTool;
        let result = tool
            .execute(json!({"action": "check_gpu"}))
            .await
            .unwrap();
        assert!(result.contains("GPU"));
    }

    #[tokio::test]
    async fn test_default_is_check_all() {
        let tool = PreflightTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("Preflight Check Results"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = PreflightTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[test]
    fn test_check_status_ordering() {
        assert!(CheckStatus::Pass < CheckStatus::Warn);
        assert!(CheckStatus::Warn < CheckStatus::Fail);
    }

    #[test]
    fn test_check_result_format_pass() {
        let r = CheckResult {
            name: "Test".to_string(),
            status: CheckStatus::Pass,
            detail: "All good".to_string(),
        };
        let formatted = r.format();
        assert!(formatted.contains("[OK]"));
        assert!(formatted.contains("PASS"));
    }

    #[test]
    fn test_check_result_format_fail() {
        let r = CheckResult {
            name: "Test".to_string(),
            status: CheckStatus::Fail,
            detail: "Something broke".to_string(),
        };
        let formatted = r.format();
        assert!(formatted.contains("[XX]"));
        assert!(formatted.contains("FAIL"));
    }
}
