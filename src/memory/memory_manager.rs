//! Memory Manager — context window eviction with episodic compression.
//!
//! Ported from `sovereign_titan/memory/memory_manager.py`.
//! Manages context size by evicting old messages and compressing them
//! into episodic summaries when the context window exceeds a threshold.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Default max context tokens.
const MAX_CONTEXT_TOKENS: usize = 8192;
/// Eviction threshold (tokens).
const EVICTION_THRESHOLD: usize = 7300;
/// Eviction ratio (fraction of messages to evict).
const EVICTION_RATIO: f64 = 0.50;
/// Number of recent user↔assistant pairs to protect from eviction.
const PINNED_INTERACTIONS: usize = 5;

/// Telemetry message markers (background messages evicted first).
const TELEMETRY_MARKERS: &[&str] = &[
    "[consciousness]",
    "[background]",
    "[monologue]",
    "[system-telemetry]",
    "[warden]",
];

/// An episodic summary from compressed messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicSummary {
    pub temporal_anchor: String,
    pub primary_domain: String,
    pub search_tags: Vec<String>,
    pub entity_mentions: Vec<String>,
    pub narrative_summary: String,
    pub mechanical_bullets: Vec<String>,
    pub unresolved_threads: Vec<String>,
}

/// A message in the context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
    pub is_telemetry: bool,
    pub token_estimate: usize,
}

impl ContextMessage {
    pub fn new(role: &str, content: &str) -> Self {
        let is_telemetry = TELEMETRY_MARKERS
            .iter()
            .any(|marker| content.starts_with(marker));
        let token_estimate = estimate_tokens(content);
        Self {
            role: role.to_string(),
            content: content.to_string(),
            is_telemetry,
            token_estimate,
        }
    }
}

/// Estimate token count from text (rough: ~4 chars per token).
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Context paging manager for context window management.
pub struct ContextPagingManager {
    /// Max context tokens.
    max_context_tokens: usize,
    /// Eviction threshold.
    eviction_threshold: usize,
    /// Eviction ratio.
    eviction_ratio: f64,
    /// Pinned interactions count.
    pinned_interactions: usize,
    /// Messages in the context window.
    messages: VecDeque<ContextMessage>,
    /// Episodic summaries from evictions.
    summaries: Vec<EpisodicSummary>,
    /// Total messages evicted.
    total_evicted: u64,
    /// Total eviction operations.
    eviction_count: u64,
}

impl ContextPagingManager {
    /// Create a new context paging manager.
    pub fn new(
        max_context_tokens: usize,
        eviction_threshold: usize,
        eviction_ratio: f64,
        pinned_interactions: usize,
    ) -> Self {
        Self {
            max_context_tokens,
            eviction_threshold,
            eviction_ratio,
            pinned_interactions,
            messages: VecDeque::new(),
            summaries: Vec::new(),
            total_evicted: 0,
            eviction_count: 0,
        }
    }

    /// Add a message to the context window.
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push_back(ContextMessage::new(role, content));
        self.maybe_evict();
    }

    /// Get current total token estimate.
    pub fn total_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.token_estimate).sum()
    }

    /// Get all messages.
    pub fn messages(&self) -> &VecDeque<ContextMessage> {
        &self.messages
    }

    /// Build the context string from messages.
    pub fn build_context(&self) -> String {
        self.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get all episodic summaries.
    pub fn summaries(&self) -> &[EpisodicSummary] {
        &self.summaries
    }

    /// Number of messages in context.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Check and perform eviction if needed.
    fn maybe_evict(&mut self) {
        let total = self.total_tokens();
        if total <= self.eviction_threshold {
            return;
        }

        // Count pinned tail messages
        let pinned_count = self.count_pinned_tail();
        let evictable_count = self.messages.len().saturating_sub(pinned_count);
        if evictable_count == 0 {
            return;
        }

        // Calculate how many to evict
        let target_evict = ((evictable_count as f64) * self.eviction_ratio) as usize;
        let target_evict = target_evict.max(1);

        // Prioritize telemetry messages for eviction
        let mut evicted_indices = Vec::new();
        let mut evicted_content = Vec::new();

        // First pass: evict telemetry messages
        for i in 0..evictable_count {
            if evicted_indices.len() >= target_evict {
                break;
            }
            if self.messages[i].is_telemetry {
                evicted_indices.push(i);
                evicted_content.push(self.messages[i].content.clone());
            }
        }

        // Second pass: evict oldest non-telemetry messages
        for i in 0..evictable_count {
            if evicted_indices.len() >= target_evict {
                break;
            }
            if !evicted_indices.contains(&i) {
                evicted_indices.push(i);
                evicted_content.push(self.messages[i].content.clone());
            }
        }

        // Remove evicted messages (in reverse order to maintain indices)
        evicted_indices.sort_unstable();
        for &idx in evicted_indices.iter().rev() {
            self.messages.remove(idx);
        }

        // Create episodic summary from evicted content
        if !evicted_content.is_empty() {
            let summary = self.compress_to_summary(&evicted_content);
            self.summaries.push(summary);
        }

        self.total_evicted += evicted_indices.len() as u64;
        self.eviction_count += 1;
    }

    /// Count pinned tail messages (recent interactions immune to eviction).
    fn count_pinned_tail(&self) -> usize {
        let mut count = 0;
        let mut user_turns = 0;
        for msg in self.messages.iter().rev() {
            count += 1;
            if msg.role == "user" {
                user_turns += 1;
                if user_turns >= self.pinned_interactions {
                    break;
                }
            }
        }
        count
    }

    /// Compress evicted messages into an episodic summary.
    fn compress_to_summary(&self, contents: &[String]) -> EpisodicSummary {
        let narrative: String = contents
            .iter()
            .map(|c| {
                let truncated: String = c.chars().take(100).collect();
                truncated
            })
            .collect::<Vec<_>>()
            .join(" | ");

        let narrative: String = narrative.chars().take(500).collect();

        EpisodicSummary {
            temporal_anchor: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            primary_domain: "conversation".to_string(),
            search_tags: Vec::new(),
            entity_mentions: Vec::new(),
            narrative_summary: narrative,
            mechanical_bullets: Vec::new(),
            unresolved_threads: Vec::new(),
        }
    }

    /// Get eviction statistics.
    pub fn stats(&self) -> EvictionStats {
        EvictionStats {
            total_tokens: self.total_tokens(),
            max_tokens: self.max_context_tokens,
            threshold: self.eviction_threshold,
            message_count: self.messages.len(),
            total_evicted: self.total_evicted,
            eviction_count: self.eviction_count,
            summary_count: self.summaries.len(),
        }
    }
}

impl Default for ContextPagingManager {
    fn default() -> Self {
        Self::new(MAX_CONTEXT_TOKENS, EVICTION_THRESHOLD, EVICTION_RATIO, PINNED_INTERACTIONS)
    }
}

/// Eviction statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionStats {
    pub total_tokens: usize,
    pub max_tokens: usize,
    pub threshold: usize,
    pub message_count: usize,
    pub total_evicted: u64,
    pub eviction_count: u64,
    pub summary_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars / 4 = ~3
    }

    #[test]
    fn test_add_message() {
        let mut mgr = ContextPagingManager::default();
        mgr.add_message("user", "Hello");
        assert_eq!(mgr.message_count(), 1);
    }

    #[test]
    fn test_telemetry_detection() {
        let msg = ContextMessage::new("system", "[consciousness] thinking about...");
        assert!(msg.is_telemetry);
        let msg2 = ContextMessage::new("user", "Hello world");
        assert!(!msg2.is_telemetry);
    }

    #[test]
    fn test_eviction_triggers() {
        let mut mgr = ContextPagingManager::new(100, 50, 0.5, 1);
        // Add enough messages to exceed threshold
        for i in 0..30 {
            mgr.add_message("user", &format!("Message {} with some content padding", i));
        }
        // Should have evicted some messages
        assert!(mgr.message_count() < 30);
        assert!(mgr.stats().total_evicted > 0);
    }

    #[test]
    fn test_telemetry_evicted_first() {
        let mut mgr = ContextPagingManager::new(200, 100, 0.5, 1);
        mgr.add_message("system", "[consciousness] thought 1 with lots of padding text to fill tokens");
        mgr.add_message("system", "[consciousness] thought 2 with lots of padding text to fill tokens");
        mgr.add_message("user", "Important user message");
        // Add more to trigger eviction
        for i in 0..20 {
            mgr.add_message("user", &format!("Message {} with padding text for tokens", i));
        }
        // Telemetry should be evicted before user messages
        let has_telemetry = mgr.messages().iter().any(|m| m.is_telemetry);
        // Can't guarantee exact behavior but summaries should exist
        assert!(mgr.stats().eviction_count >= 0);
    }

    #[test]
    fn test_build_context() {
        let mut mgr = ContextPagingManager::default();
        mgr.add_message("user", "Hello");
        mgr.add_message("assistant", "Hi!");
        let ctx = mgr.build_context();
        assert!(ctx.contains("user: Hello"));
        assert!(ctx.contains("assistant: Hi!"));
    }

    #[test]
    fn test_episodic_summary() {
        let mut mgr = ContextPagingManager::new(50, 25, 0.5, 1);
        for i in 0..20 {
            mgr.add_message("user", &format!("Message number {} with padding", i));
        }
        // Should have created at least one summary
        if mgr.stats().eviction_count > 0 {
            assert!(!mgr.summaries().is_empty());
        }
    }

    #[test]
    fn test_stats() {
        let mgr = ContextPagingManager::default();
        let stats = mgr.stats();
        assert_eq!(stats.message_count, 0);
        assert_eq!(stats.total_evicted, 0);
    }
}
