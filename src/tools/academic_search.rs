//! Academic Search Tool — search and cite research papers.
//!
//! Uses the free Semantic Scholar API to search for academic papers
//! and retrieve citation information. No API key required for basic
//! access (rate-limited to ~100 requests per 5 minutes).

use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tracing::info;

/// Request timeout for Semantic Scholar API calls.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Semantic Scholar API base URL.
const S2_API_BASE: &str = "https://api.semanticscholar.org/graph/v1";

/// Academic paper search and citation tool.
pub struct AcademicSearchTool {
    client: Client,
}

impl AcademicSearchTool {
    /// Create a new `AcademicSearchTool`.
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent("SovereignTitan/1.0")
            .build()
            .unwrap_or_default();

        Self { client }
    }

    /// Search for papers matching a query string.
    async fn search(&self, query: &str, max_results: u32) -> Result<String> {
        if query.is_empty() {
            return Ok("search requires a non-empty \"query\" field.".to_string());
        }

        let url = format!("{S2_API_BASE}/paper/search");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("query", query),
                (
                    "fields",
                    "title,authors,year,abstract,citationCount,url,externalIds",
                ),
            ])
            .query(&[("limit", max_results.min(20))])
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                return Ok(format!(
                    "Failed to reach Semantic Scholar API: {e}. \
                     Check network connectivity."
                ));
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Ok(format!(
                "Semantic Scholar API returned {status}: {}",
                &body[..body.len().min(300)]
            ));
        }

        let data: Value = resp.json().await?;

        let papers = data
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if papers.is_empty() {
            return Ok(format!("No academic papers found for: {query}"));
        }

        let total = data
            .get("total")
            .and_then(|v| v.as_u64())
            .unwrap_or(papers.len() as u64);

        let mut output = format!(
            "Academic search: \"{query}\" ({total} total results, showing {})\n\n",
            papers.len()
        );

        for (i, paper) in papers.iter().enumerate() {
            let title = paper
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            let year = paper
                .get("year")
                .and_then(|v| v.as_u64())
                .map(|y| y.to_string())
                .unwrap_or_else(|| "n/a".to_string());
            let citations = paper
                .get("citationCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let paper_id = paper
                .get("paperId")
                .and_then(|v| v.as_str())
                .unwrap_or("?");

            // Extract author names.
            let authors: Vec<&str> = paper
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            let author_str = if authors.is_empty() {
                "(unknown authors)".to_string()
            } else if authors.len() <= 3 {
                authors.join(", ")
            } else {
                format!("{} et al.", authors[0])
            };

            // Abstract (truncated).
            let abstract_text = paper
                .get("abstract")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let abstract_preview: String = if abstract_text.len() > 200 {
                format!("{}...", &abstract_text[..200])
            } else {
                abstract_text.to_string()
            };

            let url = paper
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            output.push_str(&format!(
                "{}. **{}** ({})\n   Authors: {}\n   Citations: {}\n   ID: {}\n",
                i + 1,
                title,
                year,
                author_str,
                citations,
                paper_id,
            ));
            if !url.is_empty() {
                output.push_str(&format!("   URL: {url}\n"));
            }
            if !abstract_preview.is_empty() {
                output.push_str(&format!("   Abstract: {abstract_preview}\n"));
            }
            output.push('\n');
        }

        Ok(output.trim_end().to_string())
    }

    /// Get citation details for a specific paper by its Semantic Scholar ID.
    async fn cite(&self, paper_id: &str) -> Result<String> {
        if paper_id.is_empty() {
            return Ok("cite requires a non-empty \"paper_id\" field.".to_string());
        }

        let url = format!("{S2_API_BASE}/paper/{paper_id}");
        let resp = self
            .client
            .get(&url)
            .query(&[(
                "fields",
                "title,authors,year,venue,citationCount,referenceCount,abstract,url,externalIds",
            )])
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                return Ok(format!("Failed to reach Semantic Scholar API: {e}"));
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            return Ok(format!(
                "Paper not found or API error: {status}. Check the paper ID."
            ));
        }

        let paper: Value = resp.json().await?;

        let title = paper
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("(untitled)");
        let year = paper
            .get("year")
            .and_then(|v| v.as_u64())
            .map(|y| y.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        let venue = paper
            .get("venue")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown venue)");
        let citations = paper
            .get("citationCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let references = paper
            .get("referenceCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let url = paper
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let abstract_text = paper
            .get("abstract")
            .and_then(|v| v.as_str())
            .unwrap_or("(no abstract available)");

        // Extract author names.
        let authors: Vec<&str> = paper
            .get("authors")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        let author_str = authors.join(", ");

        // Extract DOI if available.
        let doi = paper
            .get("externalIds")
            .and_then(|v| v.get("DOI"))
            .and_then(|v| v.as_str())
            .unwrap_or("n/a");

        Ok(format!(
            "**{title}** ({year})\n\
             Authors: {author_str}\n\
             Venue: {venue}\n\
             Citations: {citations} | References: {references}\n\
             DOI: {doi}\n\
             URL: {url}\n\n\
             Abstract:\n{abstract_text}"
        ))
    }
}

#[async_trait::async_trait]
impl super::Tool for AcademicSearchTool {
    fn name(&self) -> &'static str {
        "academic_search"
    }

    fn description(&self) -> &'static str {
        "Search academic/research papers via Semantic Scholar. Actions: \
         search (query, max_results?), cite (paper_id). \
         Input: {\"action\": \"search\", \"query\": \"transformer attention mechanism\"} or \
         {\"action\": \"cite\", \"paper_id\": \"649def34f8be52c8b66281af98ae884c09aef38b\"}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        info!("academic_search: action={action}");

        match action {
            "search" => {
                let query = input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let max_results = input
                    .get("max_results")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as u32;
                self.search(query, max_results).await
            }
            "cite" => {
                let paper_id = input
                    .get("paper_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.cite(paper_id).await
            }
            other => Ok(format!(
                "Unknown action: '{other}'. Use: search, cite."
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
    async fn test_empty_query() {
        let tool = AcademicSearchTool::new();
        let result = tool
            .execute(json!({"action": "search", "query": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = AcademicSearchTool::new();
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_cite_empty_id() {
        let tool = AcademicSearchTool::new();
        let result = tool
            .execute(json!({"action": "cite", "paper_id": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_default_action_is_search() {
        let tool = AcademicSearchTool::new();
        // No action specified, defaults to search. Empty query returns error.
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[test]
    fn test_tool_name() {
        let tool = AcademicSearchTool::new();
        assert_eq!(tool.name(), "academic_search");
    }

    #[test]
    fn test_tool_description_not_empty() {
        let tool = AcademicSearchTool::new();
        assert!(!tool.description().is_empty());
    }
}
