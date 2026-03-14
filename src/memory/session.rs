//! Session Memory — multi-turn merging recency + relevance + entity tracking.
//!
//! Ported from `sovereign_titan/memory/session.py`.
//! Tracks entities, recent turns, and builds augmented context for queries.

use std::collections::VecDeque;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// A session turn record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurn {
    pub role: String,
    pub content: String,
    pub timestamp: f64,
    pub metadata: serde_json::Value,
}

/// Tracked entity (name → description).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub name: String,
    pub description: String,
}

/// Conversation session with entity tracking and context augmentation.
pub struct ConversationSession {
    /// Local turn buffer.
    turns: Vec<SessionTurn>,
    /// Sliding window size for recent turns.
    window_size: usize,
    /// Max entities to track (LRU eviction).
    max_entities: usize,
    /// Tracked entities (recent first).
    entities: VecDeque<Entity>,
}

impl ConversationSession {
    /// Create a new session.
    pub fn new(window_size: usize, max_entities: usize) -> Self {
        Self {
            turns: Vec::new(),
            window_size,
            max_entities,
            entities: VecDeque::with_capacity(max_entities),
        }
    }

    /// Record a turn and extract entities.
    pub fn record_turn(&mut self, role: &str, content: &str, metadata: serde_json::Value) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        self.turns.push(SessionTurn {
            role: role.to_string(),
            content: content.to_string(),
            timestamp,
            metadata,
        });

        // Extract entities from user messages
        if role == "user" {
            self.extract_entities(content);
        }
    }

    /// Build augmented context combining recent turns + entity context.
    pub fn get_augmented_context(&self, _query: &str) -> String {
        let mut parts = Vec::new();

        // Recent turns (sliding window, compressed)
        let start = self.turns.len().saturating_sub(self.window_size);
        for turn in &self.turns[start..] {
            let prefix = if turn.role == "user" { "U" } else { "A" };
            let truncated: String = turn.content.chars().take(200).collect();
            parts.push(format!("{prefix}:{truncated}"));
        }

        // Entity context
        let entity_str: String = self
            .entities
            .iter()
            .rev()
            .take(3)
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
            .join(";");

        let hist = parts.join(" | ");
        if entity_str.is_empty() {
            format!("HIST: {hist}")
        } else {
            format!("HIST: {hist} | ENT:{entity_str}")
        }
    }

    /// Get a tracked entity by name.
    pub fn get_entity(&self, name: &str) -> Option<&Entity> {
        let name_lower = name.to_lowercase();
        self.entities.iter().find(|e| e.name.to_lowercase() == name_lower)
    }

    /// Get recent turns.
    pub fn recent_turns(&self, n: usize) -> &[SessionTurn] {
        let start = self.turns.len().saturating_sub(n);
        &self.turns[start..]
    }

    /// Number of turns.
    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    /// Number of tracked entities.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Clear session state.
    pub fn clear(&mut self) {
        self.turns.clear();
        self.entities.clear();
    }

    /// Extract entities from text using regex patterns.
    fn extract_entities(&mut self, text: &str) {
        // Pattern 1: Noun phrases after articles
        let noun_re = Regex::new(r"(?i)\b(?:the|my|a|an)\s+(\w[\w\s]{1,30}?)(?:\.|,|\?|!|$)").unwrap();
        for cap in noun_re.captures_iter(text) {
            let name = cap[1].trim().to_string();
            if name.len() > 2 {
                self.add_entity(&name, "noun phrase");
            }
        }

        // Pattern 2: Proper nouns (capitalized words, simplified without look-behind)
        let proper_re = Regex::new(r"\b([A-Z][a-z]{2,}(?:\s+[A-Z][a-z]+)*)").unwrap();
        for cap in proper_re.captures_iter(text) {
            let name = cap[1].to_string();
            if name.len() > 1 {
                self.add_entity(&name, "proper noun");
            }
        }
    }

    fn add_entity(&mut self, name: &str, description: &str) {
        // Remove existing if present (LRU: move to back)
        self.entities.retain(|e| e.name.to_lowercase() != name.to_lowercase());

        // Add to back (most recent)
        if self.entities.len() >= self.max_entities {
            self.entities.pop_front(); // Evict oldest
        }
        self.entities.push_back(Entity {
            name: name.to_string(),
            description: description.to_string(),
        });
    }
}

impl Default for ConversationSession {
    fn default() -> Self {
        Self::new(12, 20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_record_turn() {
        let mut session = ConversationSession::default();
        session.record_turn("user", "Hello", json!({}));
        assert_eq!(session.turn_count(), 1);
    }

    #[test]
    fn test_recent_turns() {
        let mut session = ConversationSession::default();
        session.record_turn("user", "msg 1", json!({}));
        session.record_turn("assistant", "reply 1", json!({}));
        session.record_turn("user", "msg 2", json!({}));
        let recent = session.recent_turns(2);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_entity_extraction() {
        let mut session = ConversationSession::default();
        session.record_turn("user", "Tell me about the Rust programming language.", json!({}));
        // Should extract "Rust" as proper noun
        assert!(session.entity_count() > 0);
    }

    #[test]
    fn test_entity_lru_eviction() {
        let mut session = ConversationSession::new(12, 3);
        session.add_entity("Entity1", "desc1");
        session.add_entity("Entity2", "desc2");
        session.add_entity("Entity3", "desc3");
        session.add_entity("Entity4", "desc4"); // Should evict Entity1
        assert_eq!(session.entity_count(), 3);
        assert!(session.get_entity("Entity1").is_none());
        assert!(session.get_entity("Entity4").is_some());
    }

    #[test]
    fn test_augmented_context() {
        let mut session = ConversationSession::default();
        session.record_turn("user", "Hello", json!({}));
        session.record_turn("assistant", "Hi there!", json!({}));
        let ctx = session.get_augmented_context("test");
        assert!(ctx.starts_with("HIST:"));
        assert!(ctx.contains("U:Hello"));
        assert!(ctx.contains("A:Hi there!"));
    }

    #[test]
    fn test_clear() {
        let mut session = ConversationSession::default();
        session.record_turn("user", "test", json!({}));
        session.clear();
        assert_eq!(session.turn_count(), 0);
        assert_eq!(session.entity_count(), 0);
    }

    #[test]
    fn test_entity_context_in_hist() {
        let mut session = ConversationSession::default();
        session.add_entity("Discord", "app");
        let ctx = session.get_augmented_context("test");
        assert!(ctx.contains("ENT:Discord"));
    }
}
