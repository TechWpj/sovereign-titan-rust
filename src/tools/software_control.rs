//! Software Control — list installed programs, install/uninstall via winget.
//!
//! Uses `winget` CLI for install/uninstall and PowerShell for listing.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct SoftwareControlTool;

impl SoftwareControlTool {
    fn run_cmd(program: &str, args: &[&str]) -> String {
        let result = std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .spawn();

        match result {
            Ok(child) => match child.wait_with_output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    if !stderr.is_empty() && stdout.is_empty() {
                        stderr
                    } else {
                        stdout
                    }
                }
                Err(e) => format!("Failed to wait for process: {e}"),
            },
            Err(e) => format!("Failed to spawn process: {e}"),
        }
    }

    fn list_installed() -> String {
        let output = Self::run_cmd(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-ItemProperty HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*, \
                 HKLM:\\Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\* | \
                 Where-Object { $_.DisplayName } | \
                 Select-Object DisplayName, DisplayVersion, Publisher | \
                 Sort-Object DisplayName | Format-Table -AutoSize | Out-String -Width 200",
            ],
        );

        if output.trim().is_empty() {
            "No installed programs found.".to_string()
        } else {
            format!("**Installed Programs:**\n```\n{}\n```", output.trim())
        }
    }

    fn search_packages(query: &str) -> String {
        // Validate input
        if query.contains(';') || query.contains('|') || query.contains('&') {
            return "Invalid search query.".to_string();
        }

        let output = Self::run_cmd(
            "winget",
            &["search", query, "--accept-source-agreements"],
        );

        if output.trim().is_empty() {
            format!("No packages found matching '{query}'.")
        } else {
            format!("**Search results for '{query}':**\n```\n{}\n```", output.trim())
        }
    }

    fn install_package(package_id: &str) -> String {
        // Validate input
        if package_id.contains(';') || package_id.contains('|') || package_id.contains('&') {
            return "Invalid package ID.".to_string();
        }

        let output = Self::run_cmd(
            "winget",
            &[
                "install",
                "--id",
                package_id,
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
        );

        if output.to_lowercase().contains("successfully installed")
            || output.to_lowercase().contains("found an existing package")
        {
            format!("Successfully installed **{package_id}**.")
        } else {
            format!("Install result for '{package_id}':\n{}", output.trim())
        }
    }

    fn uninstall_package(package_id: &str) -> String {
        if package_id.contains(';') || package_id.contains('|') || package_id.contains('&') {
            return "Invalid package ID.".to_string();
        }

        let output = Self::run_cmd(
            "winget",
            &["uninstall", "--id", package_id],
        );

        if output.to_lowercase().contains("successfully uninstalled") {
            format!("Successfully uninstalled **{package_id}**.")
        } else {
            format!("Uninstall result for '{package_id}':\n{}", output.trim())
        }
    }

    fn check_updates() -> String {
        let output = Self::run_cmd(
            "winget",
            &["upgrade", "--accept-source-agreements"],
        );

        if output.trim().is_empty() || output.contains("No installed package found") {
            "All packages are up to date.".to_string()
        } else {
            format!("**Available Updates:**\n```\n{}\n```", output.trim())
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for SoftwareControlTool {
    fn name(&self) -> &'static str {
        "software_control"
    }

    fn description(&self) -> &'static str {
        "Manage installed software. Input: {\"action\": \"<action>\", ...}. \
         Actions: list_installed, search (query), install (package_id), \
         uninstall (package_id), check_updates."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list_installed");

        info!("software_control: action={action}");

        match action {
            "list_installed" | "list" => {
                let result = tokio::task::spawn_blocking(Self::list_installed).await?;
                Ok(result)
            }
            "search" | "find" => {
                let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if query.is_empty() {
                    return Ok("search requires a \"query\" field.".to_string());
                }
                let q = query.to_string();
                let result = tokio::task::spawn_blocking(move || Self::search_packages(&q)).await?;
                Ok(result)
            }
            "install" => {
                let package_id = input.get("package_id").and_then(|v| v.as_str()).unwrap_or("");
                if package_id.is_empty() {
                    return Ok("install requires a \"package_id\" field.".to_string());
                }
                let pid = package_id.to_string();
                let result =
                    tokio::task::spawn_blocking(move || Self::install_package(&pid)).await?;
                Ok(result)
            }
            "uninstall" | "remove" => {
                let package_id = input.get("package_id").and_then(|v| v.as_str()).unwrap_or("");
                if package_id.is_empty() {
                    return Ok("uninstall requires a \"package_id\" field.".to_string());
                }
                let pid = package_id.to_string();
                let result =
                    tokio::task::spawn_blocking(move || Self::uninstall_package(&pid)).await?;
                Ok(result)
            }
            "check_updates" | "updates" | "upgrade" => {
                let result = tokio::task::spawn_blocking(Self::check_updates).await?;
                Ok(result)
            }
            other => Ok(format!(
                "Unknown action: '{other}'. Use: list_installed, search, install, uninstall, check_updates."
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = SoftwareControlTool;
        let result = tool.execute(json!({"action": "dance"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_search_missing_query() {
        let tool = SoftwareControlTool;
        let result = tool.execute(json!({"action": "search"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_install_missing_id() {
        let tool = SoftwareControlTool;
        let result = tool.execute(json!({"action": "install"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_uninstall_missing_id() {
        let tool = SoftwareControlTool;
        let result = tool.execute(json!({"action": "uninstall"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[test]
    fn test_search_injection_blocked() {
        let result = SoftwareControlTool::search_packages("evil;rm -rf /");
        assert_eq!(result, "Invalid search query.");
    }

    #[test]
    fn test_install_injection_blocked() {
        let result = SoftwareControlTool::install_package("evil|bad");
        assert_eq!(result, "Invalid package ID.");
    }
}
