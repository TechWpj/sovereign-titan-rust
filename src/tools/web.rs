//! Web Browsing Tool — headless Chrome for URL fetching and text extraction.
//!
//! Ported from `sovereign_titan/tools/web.py`.
//! Launches a headless Chrome/Chromium instance, navigates to a URL,
//! extracts the visible body text, and returns it for LLM consumption.

use anyhow::Result;
use headless_chrome::Browser;
use serde_json::Value;
use std::time::Duration;

/// Maximum characters to return from a page.
const MAX_BODY_CHARS: usize = 15_000;

/// Timeout for page navigation.
const NAV_TIMEOUT: Duration = Duration::from_secs(30);

/// Tool for fetching and reading web pages via headless Chrome.
pub struct WebSearchTool;

#[async_trait::async_trait]
impl super::Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn description(&self) -> &'static str {
        "Fetch a web page and extract its text content. Input: {url} — navigates to the URL \
         in a headless browser and returns the visible text. Useful for reading articles, \
         documentation, search results, etc."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if url.is_empty() {
            return Ok("Error: missing 'url' field. Provide a full URL like {\"url\": \"https://example.com\"}".into());
        }

        // Basic URL validation.
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(format!("Error: URL must start with http:// or https:// (got '{url}')"));
        }

        let url_owned = url.to_string();

        // Run browser operations on a blocking thread.
        let result = tokio::task::spawn_blocking(move || fetch_page(&url_owned))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))??;

        Ok(result)
    }
}

/// Launch headless Chrome, navigate to URL, and extract body text.
fn fetch_page(url: &str) -> Result<String> {
    let browser = Browser::default()
        .map_err(|e| anyhow::anyhow!("Failed to launch browser: {e}"))?;

    let tab = browser
        .new_tab()
        .map_err(|e| anyhow::anyhow!("Failed to open tab: {e}"))?;

    // Navigate to the URL.
    tab.navigate_to(url)
        .map_err(|e| anyhow::anyhow!("Navigation failed: {e}"))?;

    tab.wait_until_navigated()
        .map_err(|e| anyhow::anyhow!("Wait for navigation failed: {e}"))?;

    // Wait for body to be present.
    let body = tab
        .wait_for_element_with_custom_timeout("body", NAV_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("Could not find <body>: {e}"))?;

    // Extract innerText (visible text only, excludes script/style).
    let remote_obj = body
        .call_js_fn(
            "function() { return this.innerText; }",
            vec![],
            false,
        )
        .map_err(|e| anyhow::anyhow!("JS extraction failed: {e}"))?;

    let text = remote_obj
        .value
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();

    // Clean up: collapse whitespace runs, trim.
    let cleaned = clean_text(&text);

    // Truncate if too long.
    let output = if cleaned.len() > MAX_BODY_CHARS {
        format!(
            "{}\n\n[... truncated at {} chars, total {} chars]",
            &cleaned[..MAX_BODY_CHARS],
            MAX_BODY_CHARS,
            cleaned.len()
        )
    } else {
        cleaned
    };

    if output.is_empty() {
        Ok(format!("Page loaded ({url}) but no visible text was extracted."))
    } else {
        Ok(format!("[Fetched: {url}]\n\n{output}"))
    }
}

/// Collapse excessive whitespace and blank lines.
fn clean_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut blank_count = 0u32;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(trimmed);
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_text_collapses_blanks() {
        let input = "Hello\n\n\n\n\nWorld\n\nFoo";
        let cleaned = clean_text(input);
        // Should have at most 2 blank lines between sections.
        assert!(!cleaned.contains("\n\n\n\n"));
        assert!(cleaned.contains("Hello"));
        assert!(cleaned.contains("World"));
    }

    #[test]
    fn test_clean_text_trims() {
        let input = "   Hello World   \n   Goodbye   ";
        let cleaned = clean_text(input);
        assert_eq!(cleaned, "Hello World\nGoodbye");
    }
}
