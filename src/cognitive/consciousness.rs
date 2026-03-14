//! Consciousness Engine — Global workspace theory implementation.
//!
//! Ported from `sovereign_titan/cognitive/consciousness.py`.
//! Features:
//! - Limited capacity workspace (7±2 items)
//! - Attention-based broadcasting
//! - Integration of multiple cognitive streams

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// A single item in the global workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceItem {
    pub content: serde_json::Value,
    pub source: String,
    pub salience: f64,
    pub timestamp: f64,
    pub attention: f64,
}

/// A broadcast event from the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastEvent {
    pub content: serde_json::Value,
    pub source: String,
    pub attention: f64,
    pub timestamp: f64,
}

/// Integration result from multiple cognitive streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationResult {
    pub timestamp: f64,
    pub streams: serde_json::Value,
    pub workspace_state: Vec<WorkspaceSnapshot>,
    pub dominant_source: Option<String>,
}

/// Snapshot of a workspace item (source + attention only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub source: String,
    pub attention: f64,
}

/// Statistics about the consciousness engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsciousnessStats {
    pub workspace_size: usize,
    pub max_workspace: usize,
    pub total_broadcasts: usize,
    pub integrations: usize,
    pub sources: Vec<String>,
}

/// Global workspace theory consciousness engine.
pub struct ConsciousnessEngine {
    workspace_size: usize,
    broadcast_threshold: f64,
    attention_decay: f64,
    workspace: Mutex<Vec<WorkspaceItem>>,
    broadcasts: Mutex<VecDeque<BroadcastEvent>>,
    integration_buffer: Mutex<VecDeque<IntegrationResult>>,
}

impl ConsciousnessEngine {
    /// Create a new consciousness engine with configurable parameters.
    pub fn new(workspace_size: usize, broadcast_threshold: f64, attention_decay: f64) -> Self {
        Self {
            workspace_size,
            broadcast_threshold,
            attention_decay,
            workspace: Mutex::new(Vec::new()),
            broadcasts: Mutex::new(VecDeque::with_capacity(100)),
            integration_buffer: Mutex::new(VecDeque::with_capacity(50)),
        }
    }

    /// Add item to global workspace if salient enough.
    /// Returns true if the item was added.
    pub fn add_to_workspace(
        &self,
        content: serde_json::Value,
        source: &str,
        salience: f64,
    ) -> bool {
        let mut ws = self.workspace.lock().unwrap();
        let item = WorkspaceItem {
            content,
            source: source.to_string(),
            salience,
            timestamp: now_secs(),
            attention: salience,
        };

        if ws.len() < self.workspace_size {
            ws.push(item);
            ws.sort_by(|a, b| b.attention.partial_cmp(&a.attention).unwrap_or(std::cmp::Ordering::Equal));
            true
        } else if salience > ws.last().map(|i| i.salience).unwrap_or(0.0) {
            // Replace least salient item
            let last_idx = ws.len() - 1;
            ws[last_idx] = item;
            ws.sort_by(|a, b| b.attention.partial_cmp(&a.attention).unwrap_or(std::cmp::Ordering::Equal));
            true
        } else {
            false
        }
    }

    /// Apply attention decay to workspace items. Remove items below threshold.
    pub fn decay_attention(&self) {
        let mut ws = self.workspace.lock().unwrap();
        for item in ws.iter_mut() {
            item.attention *= 1.0 - self.attention_decay;
        }
        ws.retain(|item| item.attention > 0.1);
    }

    /// Broadcast most salient workspace content if above threshold.
    pub fn broadcast(&self) -> Option<BroadcastEvent> {
        let mut ws = self.workspace.lock().unwrap();
        if ws.is_empty() {
            return None;
        }

        let top = &mut ws[0];
        if top.attention >= self.broadcast_threshold {
            let event = BroadcastEvent {
                content: top.content.clone(),
                source: top.source.clone(),
                attention: top.attention,
                timestamp: now_secs(),
            };
            // Boost attention from broadcast
            top.attention = (top.attention + 0.1).min(1.0);

            let mut broadcasts = self.broadcasts.lock().unwrap();
            if broadcasts.len() >= 100 {
                broadcasts.pop_front();
            }
            broadcasts.push_back(event.clone());
            Some(event)
        } else {
            None
        }
    }

    /// Integrate multiple cognitive streams into unified representation.
    pub fn integrate(&self, streams: serde_json::Value) -> IntegrationResult {
        let ws = self.workspace.lock().unwrap();
        let workspace_state: Vec<WorkspaceSnapshot> = ws
            .iter()
            .map(|item| WorkspaceSnapshot {
                source: item.source.clone(),
                attention: item.attention,
            })
            .collect();

        let dominant_source = ws.first().map(|item| item.source.clone());

        let result = IntegrationResult {
            timestamp: now_secs(),
            streams,
            workspace_state,
            dominant_source,
        };

        let mut buf = self.integration_buffer.lock().unwrap();
        if buf.len() >= 50 {
            buf.pop_front();
        }
        buf.push_back(result.clone());
        result
    }

    /// Get current workspace contents (preview only).
    pub fn get_workspace_contents(&self) -> Vec<serde_json::Value> {
        let ws = self.workspace.lock().unwrap();
        ws.iter()
            .map(|item| {
                serde_json::json!({
                    "source": item.source,
                    "attention": item.attention,
                    "content_preview": item.content.to_string().chars().take(100).collect::<String>()
                })
            })
            .collect()
    }

    /// Get consciousness engine statistics.
    pub fn get_stats(&self) -> ConsciousnessStats {
        let ws = self.workspace.lock().unwrap();
        let broadcasts = self.broadcasts.lock().unwrap();
        let integrations = self.integration_buffer.lock().unwrap();

        let mut sources: Vec<String> = ws.iter().map(|i| i.source.clone()).collect();
        sources.sort();
        sources.dedup();

        ConsciousnessStats {
            workspace_size: ws.len(),
            max_workspace: self.workspace_size,
            total_broadcasts: broadcasts.len(),
            integrations: integrations.len(),
            sources,
        }
    }

    /// Clear the workspace entirely.
    pub fn clear(&self) {
        self.workspace.lock().unwrap().clear();
    }

    /// Number of items currently in workspace.
    pub fn len(&self) -> usize {
        self.workspace.lock().unwrap().len()
    }

    /// Whether the workspace is empty.
    pub fn is_empty(&self) -> bool {
        self.workspace.lock().unwrap().is_empty()
    }
}

impl Default for ConsciousnessEngine {
    fn default() -> Self {
        Self::new(7, 0.6, 0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_add_to_workspace() {
        let engine = ConsciousnessEngine::default();
        assert!(engine.add_to_workspace(json!("thought 1"), "test", 0.8));
        assert_eq!(engine.len(), 1);
    }

    #[test]
    fn test_workspace_capacity() {
        let engine = ConsciousnessEngine::new(3, 0.6, 0.1);
        for i in 0..3 {
            engine.add_to_workspace(json!(format!("thought {i}")), "test", 0.5);
        }
        assert_eq!(engine.len(), 3);
        // Low salience item should not be added
        assert!(!engine.add_to_workspace(json!("low"), "test", 0.1));
        // High salience replaces lowest
        assert!(engine.add_to_workspace(json!("high"), "test", 0.9));
        assert_eq!(engine.len(), 3);
    }

    #[test]
    fn test_decay_attention() {
        let engine = ConsciousnessEngine::new(7, 0.6, 0.5);
        engine.add_to_workspace(json!("ephemeral"), "test", 0.2);
        // After decay, 0.2 * 0.5 = 0.1 — at threshold
        engine.decay_attention();
        // After another decay, should drop below 0.1
        engine.decay_attention();
        assert!(engine.is_empty());
    }

    #[test]
    fn test_broadcast_above_threshold() {
        let engine = ConsciousnessEngine::new(7, 0.5, 0.1);
        engine.add_to_workspace(json!("important"), "test", 0.8);
        let event = engine.broadcast();
        assert!(event.is_some());
        assert_eq!(event.unwrap().source, "test");
    }

    #[test]
    fn test_broadcast_below_threshold() {
        let engine = ConsciousnessEngine::new(7, 0.9, 0.1);
        engine.add_to_workspace(json!("minor"), "test", 0.3);
        assert!(engine.broadcast().is_none());
    }

    #[test]
    fn test_integrate_streams() {
        let engine = ConsciousnessEngine::default();
        engine.add_to_workspace(json!("thought"), "perception", 0.7);
        let result = engine.integrate(json!({"perception": "active", "reasoning": "idle"}));
        assert_eq!(result.dominant_source, Some("perception".to_string()));
        assert_eq!(result.workspace_state.len(), 1);
    }

    #[test]
    fn test_get_stats() {
        let engine = ConsciousnessEngine::default();
        engine.add_to_workspace(json!("a"), "source_a", 0.5);
        engine.add_to_workspace(json!("b"), "source_b", 0.6);
        let stats = engine.get_stats();
        assert_eq!(stats.workspace_size, 2);
        assert_eq!(stats.max_workspace, 7);
        assert!(stats.sources.contains(&"source_a".to_string()));
    }

    #[test]
    fn test_clear() {
        let engine = ConsciousnessEngine::default();
        engine.add_to_workspace(json!("x"), "s", 0.5);
        engine.clear();
        assert!(engine.is_empty());
    }
}
