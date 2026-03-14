//! Isolated Node — process-isolated model execution.
//!
//! Ported from `sovereign_titan/cognitive/isolated_node.py`.
//! Features:
//! - State-machine node lifecycle (Idle -> Loading -> Ready -> Busy)
//! - Request/response envelope types
//! - Metrics tracking (tokens, requests, errors)

use serde::{Deserialize, Serialize};

/// Lifecycle state of an isolated inference node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Idle,
    Loading,
    Ready,
    Busy,
    Failed,
    Shutdown,
}

impl NodeStatus {
    /// Whether the node can accept new work.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Whether the node is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Failed | Self::Shutdown)
    }

    /// Whether the node is actively processing.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Busy)
    }
}

/// Configuration for an isolated inference node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub model_path: String,
    pub cpu_threads: usize,
    pub max_tokens: usize,
    pub temperature: f64,
    pub timeout_secs: u64,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            cpu_threads: 4,
            max_tokens: 512,
            temperature: 0.7,
            timeout_secs: 30,
        }
    }
}

impl NodeConfig {
    /// Create a config with the given model path and otherwise default settings.
    pub fn with_model(model_path: &str) -> Self {
        Self {
            model_path: model_path.to_string(),
            ..Default::default()
        }
    }
}

/// Request envelope for inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub prompt: String,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f64>,
    pub stop_sequences: Vec<String>,
    pub request_id: String,
}

impl InferenceRequest {
    /// Create a basic inference request.
    pub fn new(prompt: &str, request_id: &str) -> Self {
        Self {
            prompt: prompt.to_string(),
            max_tokens: None,
            temperature: None,
            stop_sequences: Vec::new(),
            request_id: request_id.to_string(),
        }
    }
}

/// Response envelope from inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    pub text: String,
    pub tokens_generated: usize,
    pub duration_ms: f64,
    pub request_id: String,
    pub truncated: bool,
}

impl InferenceResponse {
    /// Create a simple response.
    pub fn new(text: &str, tokens: usize, duration_ms: f64, request_id: &str) -> Self {
        Self {
            text: text.to_string(),
            tokens_generated: tokens,
            duration_ms,
            request_id: request_id.to_string(),
            truncated: false,
        }
    }
}

/// An isolated inference node with lifecycle management and metrics.
pub struct IsolatedNode {
    config: NodeConfig,
    status: NodeStatus,
    requests_processed: u64,
    total_tokens: u64,
    errors: Vec<String>,
    max_errors: usize,
}

impl IsolatedNode {
    /// Create a new node in `Idle` state.
    pub fn new(config: NodeConfig) -> Self {
        Self {
            config,
            status: NodeStatus::Idle,
            requests_processed: 0,
            total_tokens: 0,
            errors: Vec::new(),
            max_errors: 100,
        }
    }

    /// Current node status.
    pub fn status(&self) -> &NodeStatus {
        &self.status
    }

    /// Update the node status.
    pub fn set_status(&mut self, s: NodeStatus) {
        self.status = s;
    }

    /// Reference to the node configuration.
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Number of completed requests.
    pub fn requests_processed(&self) -> u64 {
        self.requests_processed
    }

    /// Total tokens generated across all completions.
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    /// Record a successful completion.
    pub fn record_completion(&mut self, tokens: usize) {
        self.requests_processed += 1;
        self.total_tokens += tokens as u64;
    }

    /// Record an error message.
    pub fn record_error(&mut self, error: &str) {
        self.errors.push(error.to_string());
        if self.errors.len() > self.max_errors {
            self.errors.remove(0);
        }
    }

    /// Number of recorded errors.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Most recent error, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.errors.last().map(|s| s.as_str())
    }

    /// Average tokens per request.
    pub fn avg_tokens_per_request(&self) -> f64 {
        if self.requests_processed == 0 {
            0.0
        } else {
            self.total_tokens as f64 / self.requests_processed as f64
        }
    }

    /// Transition to the `Shutdown` state.
    pub fn shutdown(&mut self) {
        self.status = NodeStatus::Shutdown;
    }

    /// Attempt to transition to `Ready` from `Idle` or `Loading`.
    pub fn mark_ready(&mut self) -> bool {
        match self.status {
            NodeStatus::Idle | NodeStatus::Loading => {
                self.status = NodeStatus::Ready;
                true
            }
            _ => false,
        }
    }

    /// Attempt to transition to `Busy` from `Ready`.
    pub fn mark_busy(&mut self) -> bool {
        if self.status == NodeStatus::Ready {
            self.status = NodeStatus::Busy;
            true
        } else {
            false
        }
    }

    /// Transition back to `Ready` from `Busy`.
    pub fn mark_idle_after_completion(&mut self) -> bool {
        if self.status == NodeStatus::Busy {
            self.status = NodeStatus::Ready;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node() -> IsolatedNode {
        IsolatedNode::new(NodeConfig::default())
    }

    #[test]
    fn test_initial_status_idle() {
        let node = make_node();
        assert_eq!(*node.status(), NodeStatus::Idle);
    }

    #[test]
    fn test_status_is_available() {
        assert!(!NodeStatus::Idle.is_available());
        assert!(NodeStatus::Ready.is_available());
        assert!(!NodeStatus::Busy.is_available());
        assert!(!NodeStatus::Shutdown.is_available());
    }

    #[test]
    fn test_status_is_terminal() {
        assert!(!NodeStatus::Ready.is_terminal());
        assert!(NodeStatus::Failed.is_terminal());
        assert!(NodeStatus::Shutdown.is_terminal());
    }

    #[test]
    fn test_status_is_busy() {
        assert!(NodeStatus::Busy.is_busy());
        assert!(!NodeStatus::Ready.is_busy());
    }

    #[test]
    fn test_record_completion() {
        let mut node = make_node();
        node.record_completion(100);
        node.record_completion(50);
        assert_eq!(node.requests_processed(), 2);
        assert_eq!(node.total_tokens(), 150);
    }

    #[test]
    fn test_avg_tokens_per_request() {
        let mut node = make_node();
        assert!((node.avg_tokens_per_request() - 0.0).abs() < f64::EPSILON);
        node.record_completion(100);
        node.record_completion(200);
        assert!((node.avg_tokens_per_request() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_record_error() {
        let mut node = make_node();
        node.record_error("timeout");
        node.record_error("oom");
        assert_eq!(node.error_count(), 2);
        assert_eq!(node.last_error(), Some("oom"));
    }

    #[test]
    fn test_error_ring_buffer() {
        let mut node = make_node();
        node.max_errors = 3;
        for i in 0..5 {
            node.record_error(&format!("err_{i}"));
        }
        assert_eq!(node.error_count(), 3);
        assert_eq!(node.last_error(), Some("err_4"));
    }

    #[test]
    fn test_shutdown() {
        let mut node = make_node();
        node.shutdown();
        assert_eq!(*node.status(), NodeStatus::Shutdown);
        assert!(node.status().is_terminal());
    }

    #[test]
    fn test_mark_ready_from_idle() {
        let mut node = make_node();
        assert!(node.mark_ready());
        assert_eq!(*node.status(), NodeStatus::Ready);
    }

    #[test]
    fn test_mark_ready_from_busy_fails() {
        let mut node = make_node();
        node.set_status(NodeStatus::Busy);
        assert!(!node.mark_ready());
    }

    #[test]
    fn test_mark_busy_from_ready() {
        let mut node = make_node();
        node.mark_ready();
        assert!(node.mark_busy());
        assert_eq!(*node.status(), NodeStatus::Busy);
    }

    #[test]
    fn test_lifecycle_round_trip() {
        let mut node = make_node();
        assert!(node.mark_ready());
        assert!(node.mark_busy());
        node.record_completion(42);
        assert!(node.mark_idle_after_completion());
        assert_eq!(*node.status(), NodeStatus::Ready);
        assert_eq!(node.requests_processed(), 1);
    }

    #[test]
    fn test_node_config_with_model() {
        let cfg = NodeConfig::with_model("/models/test.gguf");
        assert_eq!(cfg.model_path, "/models/test.gguf");
        assert_eq!(cfg.cpu_threads, 4);
    }

    #[test]
    fn test_inference_request_new() {
        let req = InferenceRequest::new("Hello", "req_1");
        assert_eq!(req.prompt, "Hello");
        assert_eq!(req.request_id, "req_1");
        assert!(req.max_tokens.is_none());
    }

    #[test]
    fn test_inference_response_new() {
        let resp = InferenceResponse::new("world", 5, 12.3, "req_1");
        assert_eq!(resp.text, "world");
        assert_eq!(resp.tokens_generated, 5);
        assert!(!resp.truncated);
    }
}
