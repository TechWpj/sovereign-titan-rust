//! External AI Tool — query external AI APIs (OpenAI, Anthropic, Gemini).
//!
//! Actions: query (provider, prompt, api_key), list_providers.
//! Builds the appropriate HTTP request structure for each provider and
//! sends it via `reqwest`. API keys are never logged.

use anyhow::Result;
use serde_json::{json, Value};
use tracing::info;

/// Supported AI provider identifiers.
const PROVIDERS: &[(&str, &str)] = &[
    ("openai", "OpenAI (GPT-4o, GPT-4, GPT-3.5)"),
    ("anthropic", "Anthropic (Claude 3.5/4 Sonnet, Claude 3.5/4 Opus)"),
    ("gemini", "Google Gemini (Gemini 2.0 Flash, Gemini 2.5 Pro)"),
];

pub struct ExternalAiTool;

impl ExternalAiTool {
    fn list_providers() -> String {
        let mut lines = vec!["Available AI providers:".to_string()];
        for (id, desc) in PROVIDERS {
            lines.push(format!("  - {id}: {desc}"));
        }
        lines.push(String::new());
        lines.push("Usage: {\"action\": \"query\", \"provider\": \"openai\", \"prompt\": \"...\", \"api_key\": \"sk-...\"}".to_string());
        lines.join("\n")
    }

    async fn query(provider: &str, prompt: &str, api_key: &str, model: Option<&str>) -> String {
        match provider {
            "openai" => Self::query_openai(prompt, api_key, model).await,
            "anthropic" => Self::query_anthropic(prompt, api_key, model).await,
            "gemini" => Self::query_gemini(prompt, api_key, model).await,
            other => format!(
                "Unknown provider: '{other}'. Available: {}",
                PROVIDERS.iter().map(|(id, _)| *id).collect::<Vec<_>>().join(", ")
            ),
        }
    }

    async fn query_openai(prompt: &str, api_key: &str, model: Option<&str>) -> String {
        let model = model.unwrap_or("gpt-4o");
        let url = "https://api.openai.com/v1/chat/completions";

        let body = json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "max_tokens": 2048,
            "temperature": 0.7
        });

        Self::send_request(url, api_key, &body, "Bearer", |resp| {
            resp.get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("(empty response)")
                .to_string()
        })
        .await
    }

    async fn query_anthropic(prompt: &str, api_key: &str, model: Option<&str>) -> String {
        let model = model.unwrap_or("claude-sonnet-4-20250514");
        let url = "https://api.anthropic.com/v1/messages";

        let body = json!({
            "model": model,
            "max_tokens": 2048,
            "messages": [
                {"role": "user", "content": prompt}
            ]
        });

        let client = reqwest::Client::new();
        let result = client
            .post(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        match result {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return format!("Anthropic API error ({status}): {text}");
                }
                match resp.json::<Value>().await {
                    Ok(json) => json
                        .get("content")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("(empty response)")
                        .to_string(),
                    Err(e) => format!("Failed to parse Anthropic response: {e}"),
                }
            }
            Err(e) => format!("Anthropic request failed: {e}"),
        }
    }

    async fn query_gemini(prompt: &str, api_key: &str, model: Option<&str>) -> String {
        let model = model.unwrap_or("gemini-2.0-flash");
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}"
        );

        let body = json!({
            "contents": [
                {
                    "parts": [
                        {"text": prompt}
                    ]
                }
            ],
            "generationConfig": {
                "maxOutputTokens": 2048,
                "temperature": 0.7
            }
        });

        let client = reqwest::Client::new();
        let result = client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        match result {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return format!("Gemini API error ({status}): {text}");
                }
                match resp.json::<Value>().await {
                    Ok(json) => json
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("content"))
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.get(0))
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("(empty response)")
                        .to_string(),
                    Err(e) => format!("Failed to parse Gemini response: {e}"),
                }
            }
            Err(e) => format!("Gemini request failed: {e}"),
        }
    }

    /// Generic request sender for APIs using Bearer token auth.
    async fn send_request<F>(url: &str, api_key: &str, body: &Value, auth_scheme: &str, extract: F) -> String
    where
        F: FnOnce(&Value) -> String,
    {
        let client = reqwest::Client::new();
        let result = client
            .post(url)
            .header("Authorization", format!("{auth_scheme} {api_key}"))
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await;

        match result {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return format!("API error ({status}): {text}");
                }
                match resp.json::<Value>().await {
                    Ok(json) => extract(&json),
                    Err(e) => format!("Failed to parse API response: {e}"),
                }
            }
            Err(e) => format!("API request failed: {e}"),
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for ExternalAiTool {
    fn name(&self) -> &'static str {
        "external_ai"
    }

    fn description(&self) -> &'static str {
        "Query external AI APIs. Input: {\"action\": \"<action>\", ...}. \
         Actions: query (provider, prompt, api_key, model?), \
         list_providers (show available providers). \
         Providers: openai, anthropic, gemini."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list_providers");

        info!("external_ai: action={action}");

        match action {
            "query" => {
                let provider = input.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                let api_key = input.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                let model = input.get("model").and_then(|v| v.as_str());

                if provider.is_empty() {
                    return Ok("query requires a \"provider\" field. Use list_providers to see options.".to_string());
                }
                if prompt.is_empty() {
                    return Ok("query requires a \"prompt\" field.".to_string());
                }
                if api_key.is_empty() {
                    return Ok("query requires an \"api_key\" field. Your API key is never logged.".to_string());
                }

                // Check provider validity before making the call
                if !PROVIDERS.iter().any(|(id, _)| *id == provider) {
                    return Ok(format!(
                        "Unknown provider: '{provider}'. Available: {}",
                        PROVIDERS.iter().map(|(id, _)| *id).collect::<Vec<_>>().join(", ")
                    ));
                }

                Ok(Self::query(provider, prompt, api_key, model).await)
            }
            "list_providers" | "providers" | "list" => Ok(Self::list_providers()),
            other => Ok(format!(
                "Unknown action: '{other}'. Use: query, list_providers."
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
    async fn test_list_providers() {
        let tool = ExternalAiTool;
        let result = tool
            .execute(json!({"action": "list_providers"}))
            .await
            .unwrap();
        assert!(result.contains("openai"));
        assert!(result.contains("anthropic"));
        assert!(result.contains("gemini"));
    }

    #[tokio::test]
    async fn test_missing_api_key() {
        let tool = ExternalAiTool;
        let result = tool
            .execute(json!({"action": "query", "provider": "openai", "prompt": "hello"}))
            .await
            .unwrap();
        assert!(result.contains("api_key"));
    }

    #[tokio::test]
    async fn test_missing_prompt() {
        let tool = ExternalAiTool;
        let result = tool
            .execute(json!({"action": "query", "provider": "openai", "api_key": "sk-test"}))
            .await
            .unwrap();
        assert!(result.contains("prompt"));
    }

    #[tokio::test]
    async fn test_missing_provider() {
        let tool = ExternalAiTool;
        let result = tool
            .execute(json!({"action": "query", "prompt": "hello", "api_key": "test"}))
            .await
            .unwrap();
        assert!(result.contains("provider"));
    }

    #[tokio::test]
    async fn test_unknown_provider() {
        let tool = ExternalAiTool;
        let result = tool
            .execute(json!({
                "action": "query",
                "provider": "skynet",
                "prompt": "hello",
                "api_key": "test"
            }))
            .await
            .unwrap();
        assert!(result.contains("Unknown provider"));
        assert!(result.contains("skynet"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = ExternalAiTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_default_action_is_list() {
        let tool = ExternalAiTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("openai"));
    }
}
