//! Conversation Memory — manages conversation history with summarization.
//!
//! Ported from `sovereign_titan/memory/conversation.py`.
//! Keeps a bounded in-memory deque of recent turns + summarized history.

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

fn gen_conversation_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("conv_{ts}_{:04x}", (ts & 0xFFFF) as u16)
}

/// A single conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
    pub timestamp: f64,
    pub metadata: serde_json::Value,
}

/// Conversation memory with bounded history and summarization.
pub struct ConversationMemory {
    /// In-memory recent turns.
    history: Mutex<VecDeque<Turn>>,
    /// Max history size.
    max_history: usize,
    /// Summarization threshold (when to compress older turns).
    summarize_threshold: usize,
    /// Current conversation ID.
    conversation_id: String,
    /// Stored summaries.
    summaries: Mutex<Vec<String>>,
}

impl ConversationMemory {
    /// Create a new conversation memory.
    pub fn new(max_history: usize, summarize_threshold: usize) -> Self {
        Self {
            history: Mutex::new(VecDeque::with_capacity(max_history)),
            max_history,
            summarize_threshold,
            conversation_id: gen_conversation_id(),
            summaries: Mutex::new(Vec::new()),
        }
    }

    /// Add a turn to the conversation.
    pub fn add_turn(&self, role: &str, content: &str, metadata: serde_json::Value) {
        let turn = Turn {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: now_secs(),
            metadata,
        };

        let mut history = self.history.lock().unwrap();
        if history.len() >= self.max_history {
            history.pop_front();
        }
        history.push_back(turn);

        // Check if we need to summarize
        if history.len() >= self.summarize_threshold {
            self.maybe_summarize_inner(&mut history);
        }
    }

    /// Get the N most recent turns.
    pub fn get_recent(&self, n: usize) -> Vec<Turn> {
        let history = self.history.lock().unwrap();
        history.iter().rev().take(n).cloned().collect::<Vec<_>>().into_iter().rev().collect()
    }

    /// Build context string combining recent history and relevant past.
    pub fn get_context_for_query(&self, _query: &str, n_recent: usize) -> String {
        let recent = self.get_recent(n_recent);
        let summaries = self.summaries.lock().unwrap();

        let mut context = String::new();

        // Add summaries
        if !summaries.is_empty() {
            context.push_str("Previous conversation summaries:\n");
            for summary in summaries.iter().rev().take(3) {
                context.push_str("- ");
                context.push_str(summary);
                context.push('\n');
            }
            context.push('\n');
        }

        // Add recent turns
        if !recent.is_empty() {
            context.push_str("Recent conversation:\n");
            for turn in &recent {
                let prefix = if turn.role == "user" { "User" } else { "Assistant" };
                let truncated: String = turn.content.chars().take(200).collect();
                context.push_str(&format!("{prefix}: {truncated}\n"));
            }
        }

        context
    }

    /// Search conversation history by keyword.
    pub fn search_history(&self, query: &str, n_results: usize) -> Vec<Turn> {
        let history = self.history.lock().unwrap();
        let query_lower = query.to_lowercase();

        let mut matches: Vec<Turn> = history
            .iter()
            .filter(|turn| turn.content.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();

        matches.truncate(n_results);
        matches
    }

    /// Clear all history and start a new conversation.
    pub fn clear(&self) {
        let mut history = self.history.lock().unwrap();
        history.clear();
        let mut summaries = self.summaries.lock().unwrap();
        summaries.clear();
    }

    /// Get the current conversation ID.
    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    /// Number of turns in history.
    pub fn len(&self) -> usize {
        self.history.lock().unwrap().len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.history.lock().unwrap().is_empty()
    }

    /// Summarize oldest half of history.
    fn maybe_summarize_inner(&self, history: &mut VecDeque<Turn>) {
        let count = history.len() / 2;
        if count == 0 {
            return;
        }

        // Extract oldest turns and summarize
        let mut summary_parts = Vec::new();
        for _ in 0..count {
            if let Some(turn) = history.pop_front() {
                let truncated: String = turn.content.chars().take(100).collect();
                let prefix = if turn.role == "user" { "U" } else { "A" };
                summary_parts.push(format!("{prefix}:{truncated}"));
            }
        }

        let summary = summary_parts.join(" | ");
        let summary: String = summary.chars().take(500).collect();

        let mut summaries = self.summaries.lock().unwrap();
        summaries.push(summary);
    }
}

impl Default for ConversationMemory {
    fn default() -> Self {
        Self::new(100, 20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_add_turn() {
        let mem = ConversationMemory::default();
        mem.add_turn("user", "Hello", json!({}));
        assert_eq!(mem.len(), 1);
    }

    #[test]
    fn test_get_recent() {
        let mem = ConversationMemory::default();
        mem.add_turn("user", "Hello", json!({}));
        mem.add_turn("assistant", "Hi there!", json!({}));
        mem.add_turn("user", "How are you?", json!({}));
        let recent = mem.get_recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].role, "assistant");
        assert_eq!(recent[1].role, "user");
    }

    #[test]
    fn test_max_history() {
        let mem = ConversationMemory::new(3, 100);
        for i in 0..5 {
            mem.add_turn("user", &format!("msg {i}"), json!({}));
        }
        assert!(mem.len() <= 3);
    }

    #[test]
    fn test_search_history() {
        let mem = ConversationMemory::default();
        mem.add_turn("user", "Tell me about Rust programming", json!({}));
        mem.add_turn("assistant", "Rust is a systems language", json!({}));
        mem.add_turn("user", "What about Python?", json!({}));
        let results = mem.search_history("Rust", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_clear() {
        let mem = ConversationMemory::default();
        mem.add_turn("user", "test", json!({}));
        mem.clear();
        assert!(mem.is_empty());
    }

    #[test]
    fn test_context_for_query() {
        let mem = ConversationMemory::default();
        mem.add_turn("user", "Hello", json!({}));
        mem.add_turn("assistant", "Hi!", json!({}));
        let ctx = mem.get_context_for_query("test", 5);
        assert!(ctx.contains("User: Hello"));
        assert!(ctx.contains("Assistant: Hi!"));
    }

    #[test]
    fn test_conversation_id() {
        let mem = ConversationMemory::default();
        assert!(mem.conversation_id().starts_with("conv_"));
    }
}
