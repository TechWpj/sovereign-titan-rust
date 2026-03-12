//! Native Browser Tool — opens URLs in the user's default signed-in browser.
//!
//! Unlike `web_search` (headless Chrome), this launches the actual default
//! browser so the user can interact with the page in their signed-in context.
//! Defaults to Chrome with the htrey5985@gmail.com profile.

use anyhow::Result;
use serde_json::Value;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Chrome executable path.
const CHROME_PATH: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

/// Default Chrome profile directory (htrey5985@gmail.com).
const CHROME_PROFILE: &str = "Default";

/// Tool that opens URLs in Chrome (signed-in profile) or OS default browser.
pub struct NativeBrowserTool;

impl NativeBrowserTool {
    /// Try to launch Chrome with the user's profile. Falls back to `open::that()`.
    fn open_url(url: &str) -> Result<String, String> {
        // Try Chrome with profile first.
        let chrome_result = {
            let mut cmd = std::process::Command::new(CHROME_PATH);
            cmd.arg(format!("--profile-directory={CHROME_PROFILE}"))
                .arg(url);

            #[cfg(windows)]
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

            cmd.spawn()
        };

        match chrome_result {
            Ok(_) => Ok(format!("Opened {url} in Chrome (signed-in profile).")),
            Err(_) => {
                // Fallback to OS default browser.
                match open::that(url) {
                    Ok(()) => Ok(format!("Opened {url} in default browser.")),
                    Err(e) => Err(format!("Failed to open {url}: {e}")),
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for NativeBrowserTool {
    fn name(&self) -> &'static str {
        "open_browser"
    }

    fn description(&self) -> &'static str {
        "Open a URL in Chrome (signed-in profile) or the default browser. \
         Input: {\"url\": \"https://youtube.com\"} — if the URL has no scheme, \
         https:// is auto-prepended. Use this when the user wants to browse a \
         site interactively rather than just fetching text."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let raw = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if raw.is_empty() {
            return Ok(
                "Error: missing 'url' field. Provide {\"url\": \"https://example.com\"}".into(),
            );
        }

        // Auto-prepend https:// if no scheme is present.
        let url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_string()
        } else {
            format!("https://{raw}")
        };

        let result = tokio::task::spawn_blocking(move || Self::open_url(&url))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?;

        match result {
            Ok(msg) => Ok(msg),
            Err(msg) => Ok(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_missing_url_returns_error() {
        let tool = NativeBrowserTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("missing 'url'"));
    }

    #[tokio::test]
    async fn test_empty_url_returns_error() {
        let tool = NativeBrowserTool;
        let result = tool.execute(json!({"url": ""})).await.unwrap();
        assert!(result.contains("missing 'url'"));
    }
}
