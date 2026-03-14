//! API Server — OpenAI-compatible HTTP endpoints.
//!
//! Ported from `sovereign_titan/api/server.py`.
//! Provides /v1/chat/completions, /v1/models, /health, /stats endpoints.
//! Uses Axum for HTTP serving with rate limiting and streaming support.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

// ── Rate Limiting ────────────────────────────────────────────────────────────

/// Token bucket rate limiter.
pub struct TokenBucket {
    rate: f64,
    per: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl TokenBucket {
    /// Create a new token bucket (e.g., 10 requests per 60 seconds).
    pub fn new(rate: f64, per: f64) -> Self {
        Self {
            rate,
            per,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a request is allowed for the given key.
    pub async fn allow(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().await;
        let now = Instant::now();

        let (tokens, last) = buckets
            .entry(key.to_string())
            .or_insert((self.rate, now));

        let elapsed = now.duration_since(*last).as_secs_f64();
        *tokens = (*tokens + elapsed * (self.rate / self.per)).min(self.rate);
        *last = now;

        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Seconds until the next token is available for a key.
    pub async fn retry_after(&self, key: &str) -> f64 {
        let buckets = self.buckets.lock().await;
        if let Some((tokens, _)) = buckets.get(key) {
            if *tokens >= 1.0 {
                return 0.0;
            }
            (1.0 - tokens) * (self.per / self.rate)
        } else {
            0.0
        }
    }
}

// ── OpenAI-Compatible Types ──────────────────────────────────────────────────

/// Chat message (OpenAI format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub verbosity: Option<String>,
}

fn default_temperature() -> f64 { 0.7 }
fn default_max_tokens() -> usize { 2048 }

/// Chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: ChatUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A single choice in the completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub finish_reason: String,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Streaming chunk (SSE format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

/// A single choice in a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: usize,
    pub delta: ChatDelta,
    pub finish_reason: Option<String>,
}

/// Delta content in a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Health check response.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub model: String,
    pub uptime_secs: f64,
}

/// Model info for /v1/models.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

/// Models list response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

// ── Response Helpers ─────────────────────────────────────────────────────────

/// Generate a unique completion ID.
pub fn gen_completion_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("chatcmpl-{ts:x}")
}

/// Get current Unix timestamp.
pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Build a non-streaming completion response.
pub fn build_completion_response(
    content: &str,
    model: &str,
    prompt_tokens: usize,
    completion_tokens: usize,
    metadata: Option<serde_json::Value>,
) -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: gen_completion_id(),
        object: "chat.completion".to_string(),
        created: unix_timestamp(),
        model: model.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: content.to_string(),
            },
            finish_reason: "stop".to_string(),
        }],
        usage: ChatUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        },
        metadata,
    }
}

/// Trim repetitive output (detects 3+ repetitions of chunks).
pub fn trim_repetitive_output(text: &str) -> String {
    if text.len() < 200 {
        return text.to_string();
    }

    // Check for repeating chunks of 40-120 chars
    for chunk_size in (40..=120).rev() {
        if text.len() < chunk_size * 3 {
            continue;
        }

        // Look for the pattern repeating
        for start in 0..text.len().saturating_sub(chunk_size * 3) {
            let chunk = &text[start..start + chunk_size];
            let rest = &text[start + chunk_size..];

            // Count occurrences
            let mut count = 1;
            let mut pos = 0;
            while pos + chunk_size <= rest.len() {
                if &rest[pos..pos + chunk_size] == chunk {
                    count += 1;
                    pos += chunk_size;
                } else {
                    break;
                }
            }

            if count >= 3 {
                // Truncate at second occurrence
                let truncate_at = start + chunk_size * 2;
                let mut result = text[..truncate_at].to_string();

                // Try to end at sentence boundary
                if let Some(last_period) = result.rfind(". ") {
                    result.truncate(last_period + 1);
                }

                return result;
            }
        }
    }

    text.to_string()
}

/// Build a lightweight system HUD header.
pub fn build_server_hud() -> String {
    let now = chrono::Local::now();
    format!(
        "[Time: {} | Server: active]",
        now.format("%H:%M:%S"),
    )
}

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub model_name: String,
    pub rate_limit: f64,
    pub rate_window: f64,
}

impl ServerConfig {
    /// Build a shared rate limiter from this config, wrapped in `Arc` for multi-handler use.
    pub fn build_rate_limiter(&self) -> Arc<TokenBucket> {
        Arc::new(TokenBucket::new(self.rate_limit, self.rate_window))
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8000,
            model_name: "sovereign-titan".to_string(),
            rate_limit: 10.0,
            rate_window: 60.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket_allow() {
        let bucket = TokenBucket::new(10.0, 60.0);
        assert!(bucket.allow("test_key").await);
    }

    #[tokio::test]
    async fn test_token_bucket_exhaust() {
        let bucket = TokenBucket::new(2.0, 60.0);
        assert!(bucket.allow("key").await);
        assert!(bucket.allow("key").await);
        assert!(!bucket.allow("key").await); // Exhausted
    }

    #[tokio::test]
    async fn test_retry_after() {
        let bucket = TokenBucket::new(1.0, 60.0);
        bucket.allow("key").await;
        let retry = bucket.retry_after("key").await;
        assert!(retry > 0.0);
    }

    #[test]
    fn test_gen_completion_id() {
        let id = gen_completion_id();
        assert!(id.starts_with("chatcmpl-"));
    }

    #[test]
    fn test_build_completion_response() {
        let resp = build_completion_response("Hello!", "test-model", 10, 5, None);
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content, "Hello!");
        assert_eq!(resp.usage.total_tokens, 15);
    }

    #[test]
    fn test_trim_repetitive_no_repeat() {
        let text = "This is a normal response without repetition.";
        assert_eq!(trim_repetitive_output(text), text);
    }

    #[test]
    fn test_trim_repetitive_with_repeat() {
        let chunk = "This is a repeating chunk of text that goes on. ";
        let text = chunk.repeat(5);
        let trimmed = trim_repetitive_output(&text);
        assert!(trimmed.len() < text.len());
    }

    #[test]
    fn test_build_server_hud() {
        let hud = build_server_hud();
        assert!(hud.contains("Time:"));
        assert!(hud.contains("Server: active"));
    }

    #[test]
    fn test_chat_message_serde() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, "user");
    }

    #[test]
    fn test_request_defaults() {
        let json = r#"{"messages": [{"role": "user", "content": "hi"}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!((req.temperature - 0.7).abs() < 0.01);
        assert_eq!(req.max_tokens, 2048);
        assert!(!req.stream);
    }

    #[test]
    fn test_models_response() {
        let resp = ModelsResponse {
            object: "list".to_string(),
            data: vec![ModelInfo {
                id: "sovereign-titan".to_string(),
                object: "model".to_string(),
                created: unix_timestamp(),
                owned_by: "sovereign".to_string(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("sovereign-titan"));
    }

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8000);
        assert_eq!(config.model_name, "sovereign-titan");
    }

    #[tokio::test]
    async fn test_server_config_build_rate_limiter() {
        let config = ServerConfig::default();
        let limiter = config.build_rate_limiter();
        assert!(limiter.allow("test").await);
    }
}
