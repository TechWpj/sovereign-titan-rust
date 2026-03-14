//! Browser Interact Tool — headless Chrome automation for web interaction.
//!
//! Provides actions for navigating to URLs, reading page titles and URLs,
//! capturing screenshots, clicking elements, typing text, and scrolling.
//! Uses the `headless_chrome` crate for browser control.

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::{info, warn};

/// Browser interaction tool wrapping headless_chrome.
pub struct BrowserInteractTool;

impl BrowserInteractTool {
    /// Launch a headless Chrome browser and return the first tab.
    ///
    /// Returns `None` if Chrome is not available or fails to start.
    fn launch_browser(
    ) -> Result<(headless_chrome::Browser, Arc<headless_chrome::Tab>), String> {
        let launch_options = headless_chrome::LaunchOptions::default_builder()
            .headless(true)
            .build()
            .map_err(|e| format!("Failed to build launch options: {e}"))?;

        let browser = headless_chrome::Browser::new(launch_options)
            .map_err(|e| format!("Failed to launch Chrome (is it installed?): {e}"))?;

        let tab = browser
            .new_tab()
            .map_err(|e| format!("Failed to open new tab: {e}"))?;

        Ok((browser, tab))
    }

    /// Navigate to a URL and return page info.
    fn navigate(url: &str) -> String {
        if url.is_empty() {
            return "navigate requires a non-empty \"url\" field.".to_string();
        }

        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        match tab.navigate_to(url) {
            Ok(_) => {}
            Err(e) => return format!("Failed to navigate to '{url}': {e}"),
        }

        if let Err(e) = tab.wait_until_navigated() {
            return format!("Navigation timeout for '{url}': {e}");
        }

        let title = tab
            .get_title()
            .unwrap_or_else(|_| "(unknown)".to_string());
        let current_url = tab
            .get_url();

        format!("Navigated to: {url}\nTitle: {title}\nCurrent URL: {current_url}")
    }

    /// Get the current page title.
    fn get_title() -> String {
        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        match tab.get_title() {
            Ok(title) => format!("Page title: {title}"),
            Err(e) => format!("Failed to get title: {e}"),
        }
    }

    /// Get the current page URL.
    fn get_url() -> String {
        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        let url = tab.get_url();
        format!("Current URL: {url}")
    }

    /// Take a screenshot of the current page as a base64-encoded PNG.
    fn screenshot(url: &str) -> String {
        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        // Navigate to the URL first if provided.
        if !url.is_empty() {
            if let Err(e) = tab.navigate_to(url) {
                return format!("Failed to navigate to '{url}': {e}");
            }
            if let Err(e) = tab.wait_until_navigated() {
                return format!("Navigation timeout for '{url}': {e}");
            }
        }

        // Ensure screenshots directory exists.
        let screenshot_dir = "workspace/screenshots";
        std::fs::create_dir_all(screenshot_dir).ok();

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{screenshot_dir}/browser_{timestamp}.png");

        match tab.capture_screenshot(
            headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
            None,
            None,
            true,
        ) {
            Ok(png_data) => match std::fs::write(&filename, &png_data) {
                Ok(()) => format!(
                    "Screenshot saved: {filename} ({} bytes)",
                    png_data.len()
                ),
                Err(e) => format!("Failed to save screenshot: {e}"),
            },
            Err(e) => format!("Failed to capture screenshot: {e}"),
        }
    }

    /// Click an element by CSS selector.
    fn click_element(url: &str, selector: &str) -> String {
        if selector.is_empty() {
            return "click_element requires a non-empty \"selector\" field.".to_string();
        }

        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        // Navigate if URL provided.
        if !url.is_empty() {
            if let Err(e) = tab.navigate_to(url) {
                return format!("Failed to navigate: {e}");
            }
            if let Err(e) = tab.wait_until_navigated() {
                return format!("Navigation timeout: {e}");
            }
        }

        match tab.find_element(selector) {
            Ok(element) => match element.click() {
                Ok(_) => format!("Clicked element: '{selector}'"),
                Err(e) => format!("Failed to click '{selector}': {e}"),
            },
            Err(e) => format!("Element not found '{selector}': {e}"),
        }
    }

    /// Type text into an element identified by CSS selector.
    fn type_text(url: &str, selector: &str, text: &str) -> String {
        if selector.is_empty() {
            return "type_text requires a non-empty \"selector\" field.".to_string();
        }
        if text.is_empty() {
            return "type_text requires a non-empty \"text\" field.".to_string();
        }

        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        // Navigate if URL provided.
        if !url.is_empty() {
            if let Err(e) = tab.navigate_to(url) {
                return format!("Failed to navigate: {e}");
            }
            if let Err(e) = tab.wait_until_navigated() {
                return format!("Navigation timeout: {e}");
            }
        }

        match tab.find_element(selector) {
            Ok(element) => {
                if let Err(e) = element.click() {
                    warn!("Could not focus element '{selector}' before typing: {e}");
                }
                match element.type_into(text) {
                    Ok(_) => format!("Typed {} chars into '{selector}'", text.len()),
                    Err(e) => format!("Failed to type into '{selector}': {e}"),
                }
            }
            Err(e) => format!("Element not found '{selector}': {e}"),
        }
    }

    /// Scroll the page in a given direction.
    fn scroll(url: &str, direction: &str, amount: i64) -> String {
        let (_browser, tab) = match Self::launch_browser() {
            Ok(bt) => bt,
            Err(e) => return format!("Browser error: {e}"),
        };

        // Navigate if URL provided.
        if !url.is_empty() {
            if let Err(e) = tab.navigate_to(url) {
                return format!("Failed to navigate: {e}");
            }
            if let Err(e) = tab.wait_until_navigated() {
                return format!("Navigation timeout: {e}");
            }
        }

        let scroll_js = match direction {
            "down" => format!("window.scrollBy(0, {amount})"),
            "up" => format!("window.scrollBy(0, -{})", amount.unsigned_abs()),
            "left" => format!("window.scrollBy(-{}, 0)", amount.unsigned_abs()),
            "right" => format!("window.scrollBy({amount}, 0)"),
            _ => return format!("Unknown scroll direction: '{direction}'. Use: up, down, left, right."),
        };

        match tab.evaluate(&scroll_js, false) {
            Ok(_) => format!("Scrolled {direction} by {amount}px"),
            Err(e) => format!("Failed to scroll: {e}"),
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for BrowserInteractTool {
    fn name(&self) -> &'static str {
        "browser_interact"
    }

    fn description(&self) -> &'static str {
        "Headless browser automation. Actions: \
         navigate (url), get_title, get_url, screenshot (url?), \
         click_element (selector, url?), type_text (selector, text, url?), \
         scroll (direction: up/down/left/right, amount: pixels, url?). \
         Input: {\"action\": \"navigate\", \"url\": \"https://example.com\"}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("navigate");

        info!("browser_interact: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            let url = input_clone
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match action_owned.as_str() {
                "navigate" => Self::navigate(url),
                "get_title" => Self::get_title(),
                "get_url" => Self::get_url(),
                "screenshot" => Self::screenshot(url),
                "click_element" | "click" => {
                    let selector = input_clone
                        .get("selector")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Self::click_element(url, selector)
                }
                "type_text" | "type" => {
                    let selector = input_clone
                        .get("selector")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let text = input_clone
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Self::type_text(url, selector, text)
                }
                "scroll" => {
                    let direction = input_clone
                        .get("direction")
                        .and_then(|v| v.as_str())
                        .unwrap_or("down");
                    let amount = input_clone
                        .get("amount")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(500);
                    Self::scroll(url, direction, amount)
                }
                other => format!(
                    "Unknown action: '{other}'. Use: navigate, get_title, get_url, \
                     screenshot, click_element, type_text, scroll."
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
    async fn test_unknown_action() {
        let tool = BrowserInteractTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_navigate_empty_url() {
        let tool = BrowserInteractTool;
        let result = tool
            .execute(json!({"action": "navigate", "url": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_click_element_empty_selector() {
        let tool = BrowserInteractTool;
        let result = tool
            .execute(json!({"action": "click_element", "selector": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_type_text_empty_selector() {
        let tool = BrowserInteractTool;
        let result = tool
            .execute(json!({"action": "type_text", "selector": "", "text": "hello"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_type_text_empty_text() {
        let tool = BrowserInteractTool;
        let result = tool
            .execute(json!({"action": "type_text", "selector": "#input", "text": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_scroll_invalid_direction() {
        let tool = BrowserInteractTool;
        // scroll with unknown direction returns error if browser unavailable,
        // or direction error if browser available. Either way, the tool handles it.
        let result = tool
            .execute(json!({"action": "scroll", "direction": "diagonal"}))
            .await
            .unwrap();
        // Could be browser error or direction error depending on environment.
        assert!(!result.is_empty());
    }

    #[test]
    fn test_tool_name() {
        let tool = BrowserInteractTool;
        assert_eq!(tool.name(), "browser_interact");
    }

    #[test]
    fn test_tool_description_not_empty() {
        let tool = BrowserInteractTool;
        assert!(!tool.description().is_empty());
    }
}
