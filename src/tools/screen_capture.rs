//! Screen Capture Tool — take screenshots via PowerShell .NET.
//!
//! Actions: capture (full screen), capture_region (x, y, w, h).
//! Saves PNG to workspace/screenshots/.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const SCREENSHOT_DIR: &str = "workspace/screenshots";

pub struct ScreenCaptureTool;

impl ScreenCaptureTool {
    fn run_ps(cmd: &str) -> (i32, String, String) {
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .spawn();

        match result {
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

    fn capture_full() -> String {
        std::fs::create_dir_all(SCREENSHOT_DIR).ok();
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{SCREENSHOT_DIR}/screenshot_{timestamp}.png");

        let cmd = format!(
            r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$screens = [System.Windows.Forms.Screen]::AllScreens
$minX = ($screens | ForEach-Object {{ $_.Bounds.X }} | Measure-Object -Minimum).Minimum
$minY = ($screens | ForEach-Object {{ $_.Bounds.Y }} | Measure-Object -Minimum).Minimum
$maxX = ($screens | ForEach-Object {{ $_.Bounds.X + $_.Bounds.Width }} | Measure-Object -Maximum).Maximum
$maxY = ($screens | ForEach-Object {{ $_.Bounds.Y + $_.Bounds.Height }} | Measure-Object -Maximum).Maximum
$width = $maxX - $minX
$height = $maxY - $minY
$bitmap = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($minX, $minY, 0, 0, $bitmap.Size)
$bitmap.Save('{filename}', [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
"${{width}}x${{height}} screenshot saved"
"#
        );

        let (code, stdout, stderr) = Self::run_ps(&cmd);
        if code == 0 && !stdout.trim().is_empty() {
            format!("Screenshot saved: {filename} ({})", stdout.trim())
        } else {
            format!(
                "Failed to capture screenshot: {}",
                stderr.trim().chars().take(200).collect::<String>()
            )
        }
    }

    fn capture_region(x: i64, y: i64, w: i64, h: i64) -> String {
        if w <= 0 || h <= 0 {
            return "Region width and height must be positive.".to_string();
        }

        std::fs::create_dir_all(SCREENSHOT_DIR).ok();
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{SCREENSHOT_DIR}/region_{timestamp}.png");

        let cmd = format!(
            r#"
Add-Type -AssemblyName System.Drawing
$bitmap = New-Object System.Drawing.Bitmap({w}, {h})
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen({x}, {y}, 0, 0, $bitmap.Size)
$bitmap.Save('{filename}', [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
"Region {w}x{h} at ({x},{y}) captured"
"#
        );

        let (code, stdout, stderr) = Self::run_ps(&cmd);
        if code == 0 && !stdout.trim().is_empty() {
            format!("Screenshot saved: {filename} ({})", stdout.trim())
        } else {
            format!(
                "Failed to capture region: {}",
                stderr.trim().chars().take(200).collect::<String>()
            )
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for ScreenCaptureTool {
    fn name(&self) -> &'static str {
        "screen_capture"
    }

    fn description(&self) -> &'static str {
        "Take screenshots. Input: {\"action\": \"capture\"} for full screen, \
         {\"action\": \"capture_region\", \"x\": 0, \"y\": 0, \"width\": 800, \"height\": 600} \
         for a specific region. Saves PNG to workspace/screenshots/."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("capture");

        info!("screen_capture: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "capture" | "full" | "screenshot" => Self::capture_full(),
                "capture_region" | "region" => {
                    let x = input_clone.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
                    let y = input_clone.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
                    let w = input_clone
                        .get("width")
                        .or_else(|| input_clone.get("w"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(800);
                    let h = input_clone
                        .get("height")
                        .or_else(|| input_clone.get("h"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(600);
                    Self::capture_region(x, y, w, h)
                }
                other => format!(
                    "Unknown action: '{other}'. Use: capture, capture_region."
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
    async fn test_unknown_action() {
        let tool = ScreenCaptureTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[test]
    fn test_negative_region_rejected() {
        let result = ScreenCaptureTool::capture_region(0, 0, -1, 100);
        assert!(result.contains("must be positive"));
    }
}
