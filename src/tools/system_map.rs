//! System Map Tool — hardware and system information.
//!
//! Actions: cpu, gpu, ram, disk, displays, network, all.
//! Uses `sysinfo` crate + PowerShell for GPU/display info.

use anyhow::Result;
use serde_json::Value;
use sysinfo::System;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct SystemMapTool;

impl SystemMapTool {
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

    fn cpu_info() -> String {
        let mut sys = System::new();
        sys.refresh_cpu();

        let cpus = sys.cpus();
        if cpus.is_empty() {
            return "No CPU info available.".to_string();
        }

        let name = cpus[0].brand().to_string();
        let cores = cpus.len();
        let freq = cpus[0].frequency();

        // Get usage after a brief refresh
        std::thread::sleep(std::time::Duration::from_millis(200));
        sys.refresh_cpu();
        let avg_usage: f32 = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / cores as f32;

        format!(
            "CPU: {name}\nCores/Threads: {cores}\nFrequency: {freq} MHz\nUsage: {avg_usage:.1}%"
        )
    }

    fn gpu_info() -> String {
        let cmd = r#"Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, @{N='VRAM_GB';E={[math]::Round($_.AdapterRAM/1GB,1)}} | Format-List | Out-String"#;
        let output = Self::run_ps(cmd);
        if output.is_empty() {
            "No GPU info available.".to_string()
        } else {
            format!("GPU:\n{output}")
        }
    }

    fn ram_info() -> String {
        let mut sys = System::new();
        sys.refresh_memory();

        let total = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let used = sys.used_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let available = sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let pct = if total > 0.0 { used / total * 100.0 } else { 0.0 };

        format!(
            "RAM: {used:.1} GB / {total:.1} GB ({pct:.1}% used)\nAvailable: {available:.1} GB"
        )
    }

    fn disk_info() -> String {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut lines = Vec::new();

        for disk in disks.list() {
            let mount = disk.mount_point().to_string_lossy();
            let total = disk.total_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let free = disk.available_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let used = total - free;
            let pct = if total > 0.0 { used / total * 100.0 } else { 0.0 };
            let fs = disk.file_system().to_string_lossy();

            lines.push(format!(
                "{mount}: {used:.1} GB / {total:.1} GB ({pct:.1}% used) [{fs}]"
            ));
        }

        if lines.is_empty() {
            "No disk info available.".to_string()
        } else {
            format!("Disks:\n{}", lines.join("\n"))
        }
    }

    fn display_info() -> String {
        let cmd = r#"
Add-Type -AssemblyName System.Windows.Forms
$screens = [System.Windows.Forms.Screen]::AllScreens
$results = @()
foreach ($s in $screens) {
    $results += "$($s.DeviceName): $($s.Bounds.Width)x$($s.Bounds.Height) $(if($s.Primary){'(Primary)'})"
}
$results -join "`n"
"#;
        let output = Self::run_ps(cmd);
        if output.is_empty() {
            "No display info available.".to_string()
        } else {
            format!("Displays:\n{output}")
        }
    }

    fn network_info() -> String {
        let cmd = r#"
Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } |
    Select-Object Name, InterfaceDescription,
        @{N='Speed';E={"$([math]::Round($_.LinkSpeed/1e6))Mbps"}},
        MacAddress |
    Format-Table -AutoSize |
    Out-String
"#;
        let output = Self::run_ps(cmd);
        if output.is_empty() {
            "No network adapters found.".to_string()
        } else {
            format!("Network Adapters:\n{output}")
        }
    }

    fn all_info() -> String {
        let sections = [
            Self::cpu_info(),
            Self::gpu_info(),
            Self::ram_info(),
            Self::disk_info(),
            Self::display_info(),
            Self::network_info(),
        ];
        sections.join("\n\n")
    }
}

#[async_trait::async_trait]
impl super::Tool for SystemMapTool {
    fn name(&self) -> &'static str {
        "system_map"
    }

    fn description(&self) -> &'static str {
        "Get system hardware information. Input: {\"action\": \"<action>\"}. \
         Actions: cpu, gpu, ram, disk, displays, network, all."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        info!("system_map: action={action}");

        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "cpu" => Self::cpu_info(),
                "gpu" => Self::gpu_info(),
                "ram" | "memory" => Self::ram_info(),
                "disk" | "storage" => Self::disk_info(),
                "displays" | "monitors" | "screens" => Self::display_info(),
                "network" | "net" => Self::network_info(),
                "all" => Self::all_info(),
                other => format!(
                    "Unknown action: '{other}'. Use: cpu, gpu, ram, disk, displays, network, all."
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
    async fn test_cpu_info() {
        let tool = SystemMapTool;
        let result = tool.execute(json!({"action": "cpu"})).await.unwrap();
        assert!(result.contains("CPU"));
    }

    #[tokio::test]
    async fn test_ram_info() {
        let tool = SystemMapTool;
        let result = tool.execute(json!({"action": "ram"})).await.unwrap();
        assert!(result.contains("RAM") || result.contains("GB"));
    }

    #[tokio::test]
    async fn test_disk_info() {
        let tool = SystemMapTool;
        let result = tool.execute(json!({"action": "disk"})).await.unwrap();
        assert!(result.contains("Disk") || result.contains("GB"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = SystemMapTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_default_is_all() {
        let tool = SystemMapTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("CPU") || result.contains("RAM"));
    }
}
