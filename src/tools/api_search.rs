//! API Search Tool — Google Custom Search with DuckDuckGo fallback + URL fetch.
//!
//! Ported from `sovereign_titan/tools/advanced_research.py`. Provides API-based
//! web search (no headless browser needed) and simple page text extraction.

use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tracing::warn;

/// Request timeout for all HTTP calls.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum characters to return from a fetched page.
const MAX_PAGE_CHARS: usize = 15_000;

/// API-based web search and page fetch tool.
pub struct ApiSearchTool {
    client: Client,
    google_api_key: Option<String>,
    google_cse_id: Option<String>,
}

impl ApiSearchTool {
    /// Create a new `ApiSearchTool`, reading API keys from environment.
    pub fn new() -> Self {
        let google_api_key = std::env::var("TITAN_GOOGLE_API_KEY").ok().filter(|s| !s.is_empty());
        let google_cse_id = std::env::var("TITAN_GOOGLE_CSE_ID").ok().filter(|s| !s.is_empty());

        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent("SovereignTitan/1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            google_api_key,
            google_cse_id,
        }
    }

    /// Search via Google Custom Search JSON API.
    async fn google_search(&self, query: &str, num: u32) -> Result<String> {
        let api_key = self
            .google_api_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("TITAN_GOOGLE_API_KEY not set"))?;
        let cse_id = self
            .google_cse_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("TITAN_GOOGLE_CSE_ID not set"))?;

        let resp = self
            .client
            .get("https://www.googleapis.com/customsearch/v1")
            .query(&[
                ("key", api_key),
                ("cx", cse_id),
                ("q", query),
            ])
            .query(&[("num", num.min(10))])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google API returned {status}: {}", &body[..body.len().min(300)]);
        }

        let data: Value = resp.json().await?;

        let items = data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if items.is_empty() {
            return Ok(format!("No Google results found for: {query}"));
        }

        let mut output = format!("Search results for: {query}\n\n");
        for (i, item) in items.iter().enumerate() {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("(no title)");
            let link = item.get("link").and_then(|v| v.as_str()).unwrap_or("");
            let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
            output.push_str(&format!("{}. {}\n   {}\n   {}\n\n", i + 1, title, link, snippet));
        }

        Ok(output.trim_end().to_string())
    }

    /// Fallback search via DuckDuckGo Instant Answer API.
    async fn duckduckgo_search(&self, query: &str) -> Result<String> {
        let resp = self
            .client
            .get("https://api.duckduckgo.com/")
            .query(&[("q", query), ("format", "json"), ("no_html", "1")])
            .send()
            .await?;

        let data: Value = resp.json().await?;

        let mut parts: Vec<String> = Vec::new();

        // Abstract text (main answer).
        if let Some(abstract_text) = data.get("AbstractText").and_then(|v| v.as_str()) {
            if !abstract_text.is_empty() {
                let source = data
                    .get("AbstractSource")
                    .and_then(|v| v.as_str())
                    .unwrap_or("DuckDuckGo");
                parts.push(format!("[{source}] {abstract_text}"));
            }
        }

        // Related topics.
        if let Some(topics) = data.get("RelatedTopics").and_then(|v| v.as_array()) {
            for (i, topic) in topics.iter().take(5).enumerate() {
                if let Some(text) = topic.get("Text").and_then(|v| v.as_str()) {
                    let url = topic
                        .get("FirstURL")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    parts.push(format!("{}. {} ({})", i + 1, text, url));
                }
            }
        }

        if parts.is_empty() {
            Ok(format!("No instant results found for: {query}"))
        } else {
            Ok(format!("DuckDuckGo results for: {query}\n\n{}", parts.join("\n\n")))
        }
    }

    /// Fetch a URL and extract text by stripping HTML tags.
    async fn fetch_url(&self, url: &str) -> Result<String> {
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Ok(format!("Failed to fetch {url}: HTTP {}", resp.status()));
        }

        let body = resp.text().await?;
        let text = strip_html(&body);

        let cleaned = clean_whitespace(&text);

        if cleaned.len() > MAX_PAGE_CHARS {
            Ok(format!(
                "[Fetched: {url}]\n\n{}\n\n[... truncated at {} chars, total {} chars]",
                &cleaned[..MAX_PAGE_CHARS],
                MAX_PAGE_CHARS,
                cleaned.len()
            ))
        } else if cleaned.is_empty() {
            Ok(format!("Page loaded ({url}) but no text was extracted."))
        } else {
            Ok(format!("[Fetched: {url}]\n\n{cleaned}"))
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for ApiSearchTool {
    fn name(&self) -> &'static str {
        "api_search"
    }

    fn description(&self) -> &'static str {
        "Search the web using Google Custom Search (or DuckDuckGo fallback), or fetch a \
         URL's text content. Input: {\"query\": \"rust async\"} with optional \"num_results\" \
         (1-10), OR {\"url\": \"https://example.com\"} to fetch page text. Faster than \
         headless browsing for information retrieval."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        // URL fetch mode.
        if let Some(url) = input.get("url").and_then(|v| v.as_str()) {
            if !url.is_empty() {
                return self.fetch_url(url).await;
            }
        }

        // Search mode.
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if query.is_empty() {
            return Ok(
                "Error: provide {\"query\": \"search terms\"} or {\"url\": \"https://...\"}".into(),
            );
        }

        let num_results = input
            .get("num_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as u32;

        // Try Google first if configured, fall back to DuckDuckGo.
        if self.google_api_key.is_some() && self.google_cse_id.is_some() {
            match self.google_search(query, num_results).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!("Google search failed, falling back to DuckDuckGo: {e}");
                }
            }
        }

        self.duckduckgo_search(query).await
    }
}

/// Strip HTML tags from a string, preserving text content.
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;

    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        if !in_tag && chars[i] == '<' {
            in_tag = true;

            // Check for <script or <style opening tags.
            let remaining: String = lower_chars[i..].iter().take(10).collect();
            if remaining.starts_with("<script") {
                in_script = true;
            } else if remaining.starts_with("<style") {
                in_style = true;
            } else if remaining.starts_with("</script") {
                in_script = false;
            } else if remaining.starts_with("</style") {
                in_style = false;
            }

            i += 1;
            continue;
        }

        if in_tag {
            if chars[i] == '>' {
                in_tag = false;
            }
            i += 1;
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        result.push(chars[i]);
        i += 1;
    }

    // Decode common HTML entities.
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Collapse excessive whitespace and blank lines.
fn clean_whitespace(text: &str) -> String {
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
    use crate::tools::Tool;
    use serde_json::json;

    #[test]
    fn test_strip_html_basic() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<h1>"));
    }

    #[test]
    fn test_strip_html_removes_script() {
        let html = "<p>Before</p><script>var x = 1;</script><p>After</p>";
        let text = strip_html(html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(!text.contains("var x"));
    }

    #[test]
    fn test_strip_html_entities() {
        let html = "Tom &amp; Jerry &lt;3";
        let text = strip_html(html);
        assert_eq!(text, "Tom & Jerry <3");
    }

    #[test]
    fn test_clean_whitespace() {
        let input = "Hello\n\n\n\n\nWorld\n\nFoo";
        let cleaned = clean_whitespace(input);
        assert!(!cleaned.contains("\n\n\n\n"));
    }

    #[tokio::test]
    async fn test_missing_input_returns_error() {
        let tool = ApiSearchTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("Error"));
    }

    #[tokio::test]
    async fn test_empty_query_returns_error() {
        let tool = ApiSearchTool::new();
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(result.contains("Error"));
    }
}
