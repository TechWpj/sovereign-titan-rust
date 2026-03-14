//! Container Tools — Docker/container management via the `docker` CLI.
//!
//! Actions: list_containers, start, stop, logs, images, pull, run.
//! Shells out to the `docker` command. Handles missing Docker gracefully
//! by returning a descriptive error rather than panicking.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct ContainerToolsTool;

impl ContainerToolsTool {
    /// Run a docker command and return stdout, or an error message.
    fn run_docker(args: &[&str]) -> String {
        let result = std::process::Command::new("docker")
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .spawn();

        match result {
            Ok(child) => match child.wait_with_output() {
                Ok(output) => {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if stdout.is_empty() {
                            "(no output)".to_string()
                        } else {
                            stdout
                        }
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        if stderr.is_empty() {
                            format!("Docker command failed (exit code {:?}).", output.status.code())
                        } else {
                            format!("Docker error: {stderr}")
                        }
                    }
                }
                Err(e) => format!("Failed to read docker output: {e}"),
            },
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Docker is not installed or not in PATH. Install Docker Desktop to use container tools.".to_string()
                } else {
                    format!("Failed to launch docker: {e}")
                }
            }
        }
    }

    fn list_containers(all: bool) -> String {
        let mut args = vec!["ps", "--format", "table {{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}"];
        if all {
            args.push("-a");
        }
        Self::run_docker(&args)
    }

    fn start_container(name: &str) -> String {
        Self::run_docker(&["start", name])
    }

    fn stop_container(name: &str) -> String {
        Self::run_docker(&["stop", name])
    }

    fn container_logs(name: &str, lines: u32) -> String {
        let tail = format!("{}", lines);
        Self::run_docker(&["logs", "--tail", &tail, name])
    }

    fn list_images() -> String {
        Self::run_docker(&["images", "--format", "table {{.Repository}}\t{{.Tag}}\t{{.Size}}\t{{.CreatedSince}}"])
    }

    fn pull_image(image: &str) -> String {
        Self::run_docker(&["pull", image])
    }

    fn run_container(image: &str, args: &[String]) -> String {
        let mut cmd_args: Vec<&str> = vec!["run", "--rm"];
        for arg in args {
            cmd_args.push(arg.as_str());
        }
        cmd_args.push(image);
        Self::run_docker(&cmd_args)
    }
}

#[async_trait::async_trait]
impl super::Tool for ContainerToolsTool {
    fn name(&self) -> &'static str {
        "container_tools"
    }

    fn description(&self) -> &'static str {
        "Docker/container management. Input: {\"action\": \"<action>\", ...}. \
         Actions: list_containers (all?: false), start (name), stop (name), \
         logs (name, lines?: 50), images (list local images), \
         pull (image), run (image, args?: [])."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list_containers");

        info!("container_tools: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "list_containers" | "list" | "ps" => {
                    let all = input_clone.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
                    Self::list_containers(all)
                }
                "start" => {
                    let name = input_clone.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() {
                        "start requires a \"name\" field.".to_string()
                    } else {
                        Self::start_container(name)
                    }
                }
                "stop" => {
                    let name = input_clone.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() {
                        "stop requires a \"name\" field.".to_string()
                    } else {
                        Self::stop_container(name)
                    }
                }
                "logs" => {
                    let name = input_clone.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() {
                        "logs requires a \"name\" field.".to_string()
                    } else {
                        let lines = input_clone.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
                        Self::container_logs(name, lines)
                    }
                }
                "images" => Self::list_images(),
                "pull" => {
                    let image = input_clone.get("image").and_then(|v| v.as_str()).unwrap_or("");
                    if image.is_empty() {
                        "pull requires an \"image\" field.".to_string()
                    } else {
                        Self::pull_image(image)
                    }
                }
                "run" => {
                    let image = input_clone.get("image").and_then(|v| v.as_str()).unwrap_or("");
                    if image.is_empty() {
                        "run requires an \"image\" field.".to_string()
                    } else {
                        let args: Vec<String> = input_clone
                            .get("args")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        Self::run_container(image, &args)
                    }
                }
                other => format!(
                    "Unknown action: '{other}'. Use: list_containers, start, stop, logs, images, pull, run."
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
    async fn test_list_containers_no_docker() {
        // This test should succeed whether Docker is installed or not.
        // If Docker is missing, it returns a helpful message instead of panicking.
        let tool = ContainerToolsTool;
        let result = tool
            .execute(json!({"action": "list_containers"}))
            .await
            .unwrap();
        // Either shows container list or a "not installed" message — both are valid
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_start_missing_name() {
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "start"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_stop_missing_name() {
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "stop"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_pull_missing_image() {
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "pull"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_run_missing_image() {
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "run"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_logs_missing_name() {
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "logs"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_images_no_docker() {
        // Gracefully handles missing Docker
        let tool = ContainerToolsTool;
        let result = tool.execute(json!({"action": "images"})).await.unwrap();
        assert!(!result.is_empty());
    }
}
