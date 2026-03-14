//! Advanced Research Tool — multi-source research aggregation.
//!
//! Provides deep search across multiple sources, summarization of
//! findings, and basic fact-checking. Currently a structured stub
//! that returns formatted data; intended to be backed by real search
//! APIs and LLM summarization in future iterations.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

/// Advanced multi-source research tool.
pub struct AdvancedResearchTool;

impl AdvancedResearchTool {
    /// Perform a deep search across multiple sources.
    ///
    /// Currently returns a structured stub result. In production, this
    /// would aggregate results from web search, academic search, and
    /// other knowledge sources.
    fn deep_search(query: &str, sources: &[String]) -> String {
        if query.is_empty() {
            return "deep_search requires a non-empty \"query\" field.".to_string();
        }

        let source_list = if sources.is_empty() {
            vec![
                "web".to_string(),
                "academic".to_string(),
                "knowledge_base".to_string(),
            ]
        } else {
            sources.to_vec()
        };

        let mut output = format!(
            "Deep Search: \"{query}\"\n\
             Sources: {}\n\
             Status: Structured search initiated\n\n",
            source_list.join(", ")
        );

        for source in &source_list {
            output.push_str(&format!("--- Source: {source} ---\n"));
            match source.as_str() {
                "web" => {
                    output.push_str(&format!(
                        "  [Web Search] Query: \"{query}\"\n\
                         Recommendation: Use the 'api_search' or 'web' tool for live web results.\n\
                         Example: {{\"action\": \"search\", \"query\": \"{query}\"}}\n\n"
                    ));
                }
                "academic" => {
                    output.push_str(&format!(
                        "  [Academic] Query: \"{query}\"\n\
                         Recommendation: Use the 'academic_search' tool for paper results.\n\
                         Example: {{\"action\": \"search\", \"query\": \"{query}\"}}\n\n"
                    ));
                }
                "knowledge_base" | "kb" => {
                    output.push_str(&format!(
                        "  [Knowledge Base] Query: \"{query}\"\n\
                         Recommendation: Use the 'rag' tool to search ingested documents.\n\
                         Example: {{\"action\": \"search\", \"query\": \"{query}\"}}\n\n"
                    ));
                }
                "news" => {
                    output.push_str(&format!(
                        "  [News] Query: \"{query}\"\n\
                         Recommendation: Use 'api_search' with news-focused query.\n\
                         Example: {{\"query\": \"{query} latest news\"}}\n\n"
                    ));
                }
                other => {
                    output.push_str(&format!(
                        "  [Unknown Source: {other}] Not yet integrated.\n\n"
                    ));
                }
            }
        }

        output.push_str(
            "Note: This tool provides research orchestration. For live results, \
             invoke the recommended tools directly."
        );

        output
    }

    /// Summarize a collection of research findings.
    ///
    /// Currently provides a structured summary format. In production,
    /// this would use the LLM to generate a coherent summary.
    fn summarize_findings(findings: &[String]) -> String {
        if findings.is_empty() {
            return "summarize_findings requires a non-empty \"findings\" array.".to_string();
        }

        let mut output = format!(
            "Research Summary ({} findings)\n\
             ================================\n\n",
            findings.len()
        );

        // Group and list findings.
        for (i, finding) in findings.iter().enumerate() {
            let trimmed = finding.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Truncate long findings for the summary.
            let preview = if trimmed.len() > 200 {
                format!("{}...", &trimmed[..200])
            } else {
                trimmed.to_string()
            };

            output.push_str(&format!("{}. {}\n\n", i + 1, preview));
        }

        output.push_str(&format!(
            "---\n\
             Total findings: {}\n\
             Note: For LLM-powered summarization, pass these findings to the \
             model's reasoning loop.",
            findings.len()
        ));

        output
    }

    /// Perform basic fact-checking on a claim.
    ///
    /// Currently provides a structured analysis framework. In production,
    /// this would cross-reference multiple sources.
    fn fact_check(claim: &str) -> String {
        if claim.is_empty() {
            return "fact_check requires a non-empty \"claim\" field.".to_string();
        }

        let claim_length = claim.split_whitespace().count();
        let has_numbers = claim.chars().any(|c| c.is_ascii_digit());
        let has_proper_nouns = claim
            .split_whitespace()
            .any(|w| w.chars().next().is_some_and(|c| c.is_uppercase()));

        let mut output = format!(
            "Fact Check Analysis\n\
             ====================\n\n\
             Claim: \"{claim}\"\n\n\
             Preliminary Analysis:\n"
        );

        // Structural analysis of the claim.
        output.push_str(&format!(
            "- Claim length: {} words\n\
             - Contains numbers: {}\n\
             - Contains proper nouns: {}\n\n",
            claim_length,
            if has_numbers { "yes" } else { "no" },
            if has_proper_nouns { "yes" } else { "no" },
        ));

        // Suggest verification steps.
        output.push_str("Recommended Verification Steps:\n");
        output.push_str("1. Search for the claim using 'api_search' or 'web' tool\n");
        if has_proper_nouns {
            output.push_str(
                "2. Look up named entities in the claim for authoritative sources\n",
            );
        }
        if has_numbers {
            output.push_str(
                "3. Verify numerical claims against official statistics or databases\n",
            );
        }
        output.push_str(
            "4. Check for academic papers on the topic using 'academic_search'\n",
        );
        output.push_str(
            "5. Cross-reference multiple independent sources before concluding\n\n",
        );

        output.push_str(
            "Status: Manual verification required. Use the recommended tools \
             to gather evidence for or against this claim."
        );

        output
    }
}

#[async_trait::async_trait]
impl super::Tool for AdvancedResearchTool {
    fn name(&self) -> &'static str {
        "advanced_research"
    }

    fn description(&self) -> &'static str {
        "Advanced multi-source research tool. Actions: \
         deep_search (query, sources?: [\"web\", \"academic\", \"knowledge_base\", \"news\"]), \
         summarize_findings (findings: [\"finding1\", ...]), \
         fact_check (claim). \
         Input: {\"action\": \"deep_search\", \"query\": \"quantum computing breakthroughs\"}."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("deep_search");

        info!("advanced_research: action={action}");

        match action {
            "deep_search" | "search" => {
                let query = input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let sources: Vec<String> = input
                    .get("sources")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(Self::deep_search(query, &sources))
            }
            "summarize_findings" | "summarize" => {
                let findings: Vec<String> = input
                    .get("findings")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(Self::summarize_findings(&findings))
            }
            "fact_check" | "verify" => {
                let claim = input
                    .get("claim")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(Self::fact_check(claim))
            }
            other => Ok(format!(
                "Unknown action: '{other}'. Use: deep_search, summarize_findings, fact_check."
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
        let tool = AdvancedResearchTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_empty_query() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({"action": "deep_search", "query": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_deep_search_default_sources() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({"action": "deep_search", "query": "quantum computing"}))
            .await
            .unwrap();
        assert!(result.contains("Deep Search"));
        assert!(result.contains("quantum computing"));
        assert!(result.contains("web"));
        assert!(result.contains("academic"));
    }

    #[tokio::test]
    async fn test_deep_search_custom_sources() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({
                "action": "deep_search",
                "query": "machine learning",
                "sources": ["academic", "news"]
            }))
            .await
            .unwrap();
        assert!(result.contains("academic"));
        assert!(result.contains("news"));
    }

    #[tokio::test]
    async fn test_summarize_findings() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({
                "action": "summarize_findings",
                "findings": [
                    "Finding A: important discovery",
                    "Finding B: supporting evidence"
                ]
            }))
            .await
            .unwrap();
        assert!(result.contains("Research Summary"));
        assert!(result.contains("2 findings"));
    }

    #[tokio::test]
    async fn test_summarize_findings_empty() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({"action": "summarize_findings", "findings": []}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_fact_check() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({
                "action": "fact_check",
                "claim": "The Earth orbits the Sun at 30 km/s"
            }))
            .await
            .unwrap();
        assert!(result.contains("Fact Check"));
        assert!(result.contains("numbers: yes"));
        assert!(result.contains("proper nouns: yes"));
    }

    #[tokio::test]
    async fn test_fact_check_empty() {
        let tool = AdvancedResearchTool;
        let result = tool
            .execute(json!({"action": "fact_check", "claim": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[test]
    fn test_tool_name() {
        let tool = AdvancedResearchTool;
        assert_eq!(tool.name(), "advanced_research");
    }

    #[test]
    fn test_tool_description_not_empty() {
        let tool = AdvancedResearchTool;
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_default_action_is_deep_search() {
        let tool = AdvancedResearchTool;
        // No action field, defaults to deep_search. Empty query returns error.
        let result = tool.execute(json!({"query": ""})).await.unwrap();
        assert!(result.contains("requires"));
    }
}
