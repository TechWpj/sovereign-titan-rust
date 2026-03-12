//! Window Control Tool — manage application windows via Win32 API.
//!
//! Actions: list, focus, minimize, maximize, restore, close, snap_left, snap_right.
//! Uses PowerShell + Win32 API calls for window management.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct WindowControlTool;

impl WindowControlTool {
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

    fn escape_ps(s: &str) -> String {
        s.replace('`', "``")
            .replace('$', "`$")
            .replace('"', "`\"")
            .replace(';', "`;")
    }

    fn list_windows() -> String {
        let cmd = r#"
Get-Process | Where-Object { $_.MainWindowTitle -ne '' } |
    Select-Object Id, ProcessName, MainWindowTitle,
        @{N='MemMB';E={[math]::Round($_.WorkingSet64/1MB,1)}} |
    Sort-Object MainWindowTitle |
    Format-Table -AutoSize |
    Out-String
"#;
        let (_, stdout, stderr) = Self::run_ps(cmd);
        if stdout.trim().is_empty() {
            if !stderr.trim().is_empty() {
                format!("Error: {}", &stderr.trim()[..stderr.trim().len().min(200)])
            } else {
                "No windows found.".to_string()
            }
        } else {
            stdout.trim().to_string()
        }
    }

    fn focus_window(title: &str) -> String {
        let escaped = Self::escape_ps(title);
        let cmd = format!(
            r#"
$p = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{escaped}*' }} | Select-Object -First 1
if ($p) {{
    Add-Type @"
    using System;
    using System.Runtime.InteropServices;
    public class Win32 {{
        [DllImport("user32.dll")]
        public static extern bool SetForegroundWindow(IntPtr hWnd);
        [DllImport("user32.dll")]
        public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    }}
"@
    [Win32]::ShowWindow($p.MainWindowHandle, 9) | Out-Null
    [Win32]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
    "Focused: $($p.MainWindowTitle)"
}} else {{
    "No window found matching '{escaped}'"
}}
"#
        );
        let (_, stdout, stderr) = Self::run_ps(&cmd);
        if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else if !stderr.trim().is_empty() {
            format!("Error: {}", &stderr.trim()[..stderr.trim().len().min(200)])
        } else {
            format!("No window found matching '{title}'")
        }
    }

    fn window_action(title: &str, action: &str) -> String {
        let escaped = Self::escape_ps(title);
        let sw_cmd = match action {
            "minimize" => 6,  // SW_MINIMIZE
            "maximize" => 3,  // SW_MAXIMIZE
            "restore" => 9,   // SW_RESTORE
            _ => return format!("Unknown window action: {action}"),
        };

        let cmd = format!(
            r#"
$p = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{escaped}*' }} | Select-Object -First 1
if ($p) {{
    Add-Type @"
    using System;
    using System.Runtime.InteropServices;
    public class Win32Action {{
        [DllImport("user32.dll")]
        public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    }}
"@
    [Win32Action]::ShowWindow($p.MainWindowHandle, {sw_cmd}) | Out-Null
    "{action}: $($p.MainWindowTitle)"
}} else {{
    "No window found matching '{escaped}'"
}}
"#
        );
        let (_, stdout, stderr) = Self::run_ps(&cmd);
        if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Error: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn close_window(title: &str) -> String {
        let escaped = Self::escape_ps(title);
        let cmd = format!(
            r#"
$p = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{escaped}*' }} | Select-Object -First 1
if ($p) {{
    $p.CloseMainWindow() | Out-Null
    "Closed: $($p.MainWindowTitle)"
}} else {{
    "No window found matching '{escaped}'"
}}
"#
        );
        let (_, stdout, stderr) = Self::run_ps(&cmd);
        if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Error: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn snap_window(title: &str, direction: &str) -> String {
        let escaped = Self::escape_ps(title);
        let (x, y, w_expr, h_expr) = match direction {
            "left" => ("0", "0", "[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea.Width / 2", "[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea.Height"),
            "right" => ("[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea.Width / 2", "0", "[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea.Width / 2", "[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea.Height"),
            _ => return format!("Unknown snap direction: {direction}"),
        };

        let cmd = format!(
            r#"
Add-Type -AssemblyName System.Windows.Forms
$p = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{escaped}*' }} | Select-Object -First 1
if ($p) {{
    Add-Type @"
    using System;
    using System.Runtime.InteropServices;
    public class Win32Snap {{
        [DllImport("user32.dll")]
        public static extern bool MoveWindow(IntPtr hWnd, int X, int Y, int nWidth, int nHeight, bool bRepaint);
        [DllImport("user32.dll")]
        public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    }}
"@
    [Win32Snap]::ShowWindow($p.MainWindowHandle, 9) | Out-Null
    $x = [int]({x})
    $y = [int]({y})
    $w = [int]({w_expr})
    $h = [int]({h_expr})
    [Win32Snap]::MoveWindow($p.MainWindowHandle, $x, $y, $w, $h, $true) | Out-Null
    "Snapped {direction}: $($p.MainWindowTitle)"
}} else {{
    "No window found matching '{escaped}'"
}}
"#
        );
        let (_, stdout, stderr) = Self::run_ps(&cmd);
        if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Error: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for WindowControlTool {
    fn name(&self) -> &'static str {
        "window_control"
    }

    fn description(&self) -> &'static str {
        "Manage application windows. Input: {\"action\": \"<action>\", ...}. \
         Actions: list (shows all windows), focus (title), minimize (title), \
         maximize (title), restore (title), close (title), \
         snap_left (title), snap_right (title)."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        info!("window_control: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            let title = input_clone
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match action_owned.as_str() {
                "list" => Self::list_windows(),
                "focus" => {
                    if title.is_empty() { return "focus requires a \"title\" field.".to_string(); }
                    Self::focus_window(title)
                }
                "minimize" | "maximize" | "restore" => {
                    if title.is_empty() { return format!("{action_owned} requires a \"title\" field."); }
                    Self::window_action(title, &action_owned)
                }
                "close" => {
                    if title.is_empty() { return "close requires a \"title\" field.".to_string(); }
                    Self::close_window(title)
                }
                "snap_left" => {
                    if title.is_empty() { return "snap_left requires a \"title\" field.".to_string(); }
                    Self::snap_window(title, "left")
                }
                "snap_right" => {
                    if title.is_empty() { return "snap_right requires a \"title\" field.".to_string(); }
                    Self::snap_window(title, "right")
                }
                other => format!(
                    "Unknown action: '{other}'. Use: list, focus, minimize, maximize, restore, close, snap_left, snap_right."
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
        let tool = WindowControlTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_focus_missing_title() {
        let tool = WindowControlTool;
        let result = tool.execute(json!({"action": "focus"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_minimize_missing_title() {
        let tool = WindowControlTool;
        let result = tool.execute(json!({"action": "minimize"})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_list_default() {
        let tool = WindowControlTool;
        let result = tool.execute(json!({})).await.unwrap();
        // Should return some output (even if empty set)
        assert!(!result.is_empty());
    }
}
