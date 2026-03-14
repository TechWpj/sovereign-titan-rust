//! Discovery — auto-discovery of new capabilities and tools.
//!
//! Ported from `sovereign_titan/cognitive/discovery.py`.
//! Tracks discovered capabilities from tool usage patterns and system exploration.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// A discovered capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub description: String,
    pub tool: String,
    pub discovered_at: f64,
    pub usage_count: u64,
    pub success_rate: f64,
    pub confidence: f64,
}

/// Discovery engine for new capabilities.
pub struct DiscoveryEngine {
    /// Known capabilities.
    capabilities: HashMap<String, Capability>,
    /// Discovery log.
    log: Vec<DiscoveryEvent>,
    /// Max capabilities to track.
    max_capabilities: usize,
}

/// A discovery event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryEvent {
    pub capability: String,
    pub event_type: String,
    pub timestamp: f64,
    pub details: String,
}

impl DiscoveryEngine {
    /// Create a new discovery engine.
    pub fn new(max_capabilities: usize) -> Self {
        Self {
            capabilities: HashMap::new(),
            log: Vec::new(),
            max_capabilities,
        }
    }

    /// Register a new capability.
    pub fn register(&mut self, name: &str, description: &str, tool: &str) -> bool {
        if self.capabilities.len() >= self.max_capabilities && !self.capabilities.contains_key(name) {
            return false;
        }

        let entry = self.capabilities.entry(name.to_string()).or_insert_with(|| {
            self.log.push(DiscoveryEvent {
                capability: name.to_string(),
                event_type: "discovered".to_string(),
                timestamp: now_secs(),
                details: description.to_string(),
            });
            Capability {
                name: name.to_string(),
                description: description.to_string(),
                tool: tool.to_string(),
                discovered_at: now_secs(),
                usage_count: 0,
                success_rate: 0.0,
                confidence: 0.5,
            }
        });

        // Update description if it changed
        entry.description = description.to_string();
        true
    }

    /// Record usage of a capability.
    pub fn record_usage(&mut self, name: &str, success: bool) {
        if let Some(cap) = self.capabilities.get_mut(name) {
            cap.usage_count += 1;
            // Exponential moving average for success rate
            let alpha = 0.2;
            let outcome = if success { 1.0 } else { 0.0 };
            cap.success_rate = alpha * outcome + (1.0 - alpha) * cap.success_rate;

            // Confidence increases with usage
            cap.confidence = (cap.usage_count as f64 / (cap.usage_count as f64 + 5.0)).min(1.0);
        }
    }

    /// Get a capability by name.
    pub fn get(&self, name: &str) -> Option<&Capability> {
        self.capabilities.get(name)
    }

    /// Get all capabilities sorted by confidence.
    pub fn all_capabilities(&self) -> Vec<&Capability> {
        let mut caps: Vec<&Capability> = self.capabilities.values().collect();
        caps.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        caps
    }

    /// Get capabilities for a specific tool.
    pub fn capabilities_for_tool(&self, tool: &str) -> Vec<&Capability> {
        self.capabilities
            .values()
            .filter(|c| c.tool == tool)
            .collect()
    }

    /// Number of known capabilities.
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Whether there are no capabilities.
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Get summary for prompting.
    pub fn summary(&self) -> String {
        let caps = self.all_capabilities();
        if caps.is_empty() {
            return "No capabilities discovered yet.".to_string();
        }
        caps.iter()
            .take(10)
            .map(|c| format!("- {} (tool: {}, confidence: {:.0}%)", c.name, c.tool, c.confidence * 100.0))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get discovery log.
    pub fn log(&self) -> &[DiscoveryEvent] {
        &self.log
    }
}

impl Default for DiscoveryEngine {
    fn default() -> Self {
        Self::new(200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_capability() {
        let mut engine = DiscoveryEngine::default();
        assert!(engine.register("file_read", "Read files from disk", "code_ops"));
        assert_eq!(engine.len(), 1);
    }

    #[test]
    fn test_duplicate_register() {
        let mut engine = DiscoveryEngine::default();
        engine.register("test", "desc 1", "tool1");
        engine.register("test", "desc 2", "tool1");
        assert_eq!(engine.len(), 1);
        assert_eq!(engine.get("test").unwrap().description, "desc 2");
    }

    #[test]
    fn test_record_usage() {
        let mut engine = DiscoveryEngine::default();
        engine.register("test", "test cap", "tool");
        engine.record_usage("test", true);
        engine.record_usage("test", true);
        let cap = engine.get("test").unwrap();
        assert_eq!(cap.usage_count, 2);
        assert!(cap.success_rate > 0.0);
        assert!(cap.confidence > 0.0);
    }

    #[test]
    fn test_max_capabilities() {
        let mut engine = DiscoveryEngine::new(2);
        engine.register("a", "cap a", "tool");
        engine.register("b", "cap b", "tool");
        assert!(!engine.register("c", "cap c", "tool")); // Should fail
        assert_eq!(engine.len(), 2);
    }

    #[test]
    fn test_capabilities_for_tool() {
        let mut engine = DiscoveryEngine::default();
        engine.register("a", "cap a", "tool1");
        engine.register("b", "cap b", "tool2");
        engine.register("c", "cap c", "tool1");
        assert_eq!(engine.capabilities_for_tool("tool1").len(), 2);
    }

    #[test]
    fn test_summary() {
        let mut engine = DiscoveryEngine::default();
        engine.register("file_read", "Read files", "code_ops");
        let summary = engine.summary();
        assert!(summary.contains("file_read"));
    }

    #[test]
    fn test_discovery_log() {
        let mut engine = DiscoveryEngine::default();
        engine.register("test", "test cap", "tool");
        assert_eq!(engine.log().len(), 1);
        assert_eq!(engine.log()[0].event_type, "discovered");
    }
}
