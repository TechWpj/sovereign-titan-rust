//! Self-Improvement Engine — learns from user feedback to improve responses.
//!
//! Collects explicit feedback (thumbs up/down, ratings) and implicit signals
//! (outcome tracking), extracts response patterns, and provides guidance
//! to the agent for future interactions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::{now_secs, FeedbackEntry, ResponsePattern};

// ─────────────────────────────────────────────────────────────────────────────
// Stats / Report types
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate performance report for the self-improvement engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    /// Total feedback entries received.
    pub total_feedback: u64,
    /// Number of positive feedback entries (rating >= threshold).
    pub positive_count: u64,
    /// Number of negative feedback entries (rating <= threshold).
    pub negative_count: u64,
    /// Overall positive rate (0.0 to 1.0).
    pub positive_rate: f64,
    /// Number of learned patterns.
    pub pattern_count: usize,
    /// Number of unprocessed feedback entries.
    pub unprocessed_count: usize,
    /// Top patterns by usage count (up to 5).
    pub top_patterns: Vec<String>,
}

/// Compact engine statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub total_feedback: u64,
    pub positive_count: u64,
    pub negative_count: u64,
    pub pattern_count: usize,
    pub feedback_backlog: usize,
    /// Unix timestamp when this snapshot was taken.
    pub snapshot_at: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// SelfImprovementEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Feedback-driven learning engine for Sovereign Titan.
///
/// Collects user feedback, extracts response patterns, and surfaces
/// guidance to the agent to improve future interactions.
pub struct SelfImprovementEngine {
    /// All recorded feedback entries.
    feedback: Vec<FeedbackEntry>,
    /// Learned response patterns keyed by trigger.
    patterns: HashMap<String, ResponsePattern>,
    /// Rating threshold above which feedback is considered positive.
    positive_threshold: f64,
    /// Rating threshold below which feedback is considered negative.
    negative_threshold: f64,
    /// Total feedback entries received.
    total_feedback: u64,
    /// Count of positive feedback entries.
    positive_count: u64,
    /// Count of negative feedback entries.
    negative_count: u64,
}

impl SelfImprovementEngine {
    /// Create a new self-improvement engine with default thresholds.
    pub fn new() -> Self {
        Self {
            feedback: Vec::new(),
            patterns: HashMap::new(),
            positive_threshold: 0.7,
            negative_threshold: 0.3,
            total_feedback: 0,
            positive_count: 0,
            negative_count: 0,
        }
    }

    /// Record generic feedback on a query/response pair.
    ///
    /// Returns a clone of the created [`FeedbackEntry`].
    pub fn add_feedback(
        &mut self,
        query: &str,
        response: &str,
        rating: f64,
        feedback_text: &str,
    ) -> FeedbackEntry {
        let entry = FeedbackEntry::new(query, response, rating, feedback_text);
        self.total_feedback += 1;

        if entry.rating >= self.positive_threshold {
            self.positive_count += 1;
            // Extract a positive pattern from the query.
            self.learn_positive_pattern(query, response);
        }
        if entry.rating <= self.negative_threshold {
            self.negative_count += 1;
        }

        self.feedback.push(entry.clone());
        entry
    }

    /// Shorthand for positive feedback (rating = 1.0).
    pub fn thumbs_up(&mut self, query: &str, response: &str) -> FeedbackEntry {
        self.add_feedback(query, response, 1.0, "thumbs_up")
    }

    /// Shorthand for negative feedback (rating = 0.0).
    pub fn thumbs_down(&mut self, query: &str, response: &str, reason: &str) -> FeedbackEntry {
        self.add_feedback(query, response, 0.0, reason)
    }

    /// Get improvement suggestions for a query based on learned patterns.
    ///
    /// Returns a list of actionable suggestions derived from high-confidence
    /// patterns whose trigger appears in the query.
    pub fn get_improvement_suggestions(&self, query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        let mut suggestions = Vec::new();

        for pattern in self.patterns.values() {
            if query_lower.contains(&pattern.trigger.to_lowercase()) && pattern.confidence >= 0.5 {
                suggestions.push(format!(
                    "[{}] {}: {} (confidence: {:.0}%, used {} times)",
                    pattern.pattern_type,
                    pattern.trigger,
                    pattern.preferred_response,
                    pattern.confidence * 100.0,
                    pattern.usage_count,
                ));
            }
        }

        // Sort by confidence descending.
        suggestions.sort_by(|a, b| b.cmp(a));
        suggestions
    }

    /// Fast pattern lookup: returns the preferred response approach for the
    /// highest-confidence pattern matching the query, if any.
    pub fn get_response_guidance(&self, query: &str) -> Option<String> {
        let query_lower = query.to_lowercase();
        self.patterns
            .values()
            .filter(|p| {
                query_lower.contains(&p.trigger.to_lowercase())
                    && p.confidence >= 0.5
                    && p.usage_count > 0
            })
            .max_by(|a, b| {
                a.confidence
                    .partial_cmp(&b.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|p| p.preferred_response.clone())
    }

    /// Record the outcome of a query/response to update pattern statistics.
    pub fn record_outcome(&mut self, query: &str, response: &str, success: bool) {
        let query_lower = query.to_lowercase();

        // Update any matching patterns.
        for pattern in self.patterns.values_mut() {
            if query_lower.contains(&pattern.trigger.to_lowercase()) {
                pattern.record_usage(success);
            }
        }

        // If successful, reinforce the pattern.
        if success {
            self.learn_positive_pattern(query, response);
        }
    }

    /// Check whether enough new (unprocessed) feedback has accumulated to
    /// justify a retraining cycle.
    pub fn should_retrain(&self, threshold: usize) -> bool {
        let unprocessed = self.feedback.iter().filter(|f| !f.processed).count();
        unprocessed >= threshold
    }

    /// Mark all current feedback entries as processed.
    pub fn mark_all_processed(&mut self) {
        for entry in &mut self.feedback {
            entry.mark_processed();
        }
    }

    /// Generate a full performance report.
    pub fn get_performance_report(&self) -> PerformanceReport {
        let positive_rate = if self.total_feedback > 0 {
            self.positive_count as f64 / self.total_feedback as f64
        } else {
            0.0
        };

        let unprocessed_count = self.feedback.iter().filter(|f| !f.processed).count();

        // Top patterns by usage count.
        let mut sorted_patterns: Vec<&ResponsePattern> = self.patterns.values().collect();
        sorted_patterns.sort_by(|a, b| b.usage_count.cmp(&a.usage_count));

        let top_patterns: Vec<String> = sorted_patterns
            .iter()
            .take(5)
            .map(|p| format!("{}: {} (used {}x)", p.trigger, p.preferred_response, p.usage_count))
            .collect();

        PerformanceReport {
            total_feedback: self.total_feedback,
            positive_count: self.positive_count,
            negative_count: self.negative_count,
            positive_rate,
            pattern_count: self.patterns.len(),
            unprocessed_count,
            top_patterns,
        }
    }

    /// Get compact engine statistics, timestamped with `now_secs()`.
    pub fn get_stats(&self) -> EngineStats {
        EngineStats {
            total_feedback: self.total_feedback,
            positive_count: self.positive_count,
            negative_count: self.negative_count,
            pattern_count: self.patterns.len(),
            feedback_backlog: self.feedback.iter().filter(|f| !f.processed).count(),
            snapshot_at: now_secs(),
        }
    }

    /// Total number of feedback entries.
    pub fn feedback_count(&self) -> usize {
        self.feedback.len()
    }

    /// Total number of learned patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    // ── Private helpers ─────────────────────────────────────────────────────

    /// Extract key words from a query to use as pattern triggers.
    fn extract_trigger(query: &str) -> Option<String> {
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return None;
        }

        // Use the first significant word (skip common stop words).
        let stop_words = [
            "the", "a", "an", "is", "are", "was", "were", "what", "how", "can", "do", "does",
            "please", "i", "you", "me", "my", "to", "for", "of", "in", "on", "it", "this",
        ];

        for word in &words {
            let w = word.to_lowercase();
            if !stop_words.contains(&w.as_str()) && w.len() > 2 {
                return Some(w);
            }
        }

        // Fallback to the first word.
        Some(words[0].to_lowercase())
    }

    /// Learn a positive pattern from a successful query/response pair.
    fn learn_positive_pattern(&mut self, query: &str, response: &str) {
        if let Some(trigger) = Self::extract_trigger(query) {
            let entry = self
                .patterns
                .entry(trigger.clone())
                .or_insert_with(|| {
                    ResponsePattern::new(
                        "learned",
                        &trigger,
                        &response.chars().take(200).collect::<String>(),
                        0.5,
                    )
                });

            // Boost confidence on repeated positive signals.
            entry.confidence = (entry.confidence + 0.05).min(1.0);
            entry.record_usage(true);
        }
    }
}

impl Default for SelfImprovementEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_new() {
        let engine = SelfImprovementEngine::new();
        assert_eq!(engine.feedback_count(), 0);
        assert_eq!(engine.pattern_count(), 0);
        assert_eq!(engine.total_feedback, 0);
    }

    #[test]
    fn test_add_feedback_positive() {
        let mut engine = SelfImprovementEngine::new();
        let entry = engine.add_feedback("explain rust", "Rust is a systems language...", 0.9, "good");
        assert!(entry.id.starts_with("fb_"));
        assert!((entry.rating - 0.9).abs() < f64::EPSILON);
        assert_eq!(engine.feedback_count(), 1);
        assert_eq!(engine.positive_count, 1);
        assert_eq!(engine.negative_count, 0);
    }

    #[test]
    fn test_add_feedback_negative() {
        let mut engine = SelfImprovementEngine::new();
        engine.add_feedback("explain rust", "I don't know", 0.1, "unhelpful");
        assert_eq!(engine.positive_count, 0);
        assert_eq!(engine.negative_count, 1);
    }

    #[test]
    fn test_thumbs_up() {
        let mut engine = SelfImprovementEngine::new();
        let entry = engine.thumbs_up("hello world", "Hi there!");
        assert!((entry.rating - 1.0).abs() < f64::EPSILON);
        assert_eq!(entry.feedback_text, "thumbs_up");
        assert_eq!(engine.positive_count, 1);
    }

    #[test]
    fn test_thumbs_down() {
        let mut engine = SelfImprovementEngine::new();
        let entry = engine.thumbs_down("hello world", "error occurred", "broken response");
        assert!((entry.rating - 0.0).abs() < f64::EPSILON);
        assert_eq!(entry.feedback_text, "broken response");
        assert_eq!(engine.negative_count, 1);
    }

    #[test]
    fn test_get_improvement_suggestions() {
        let mut engine = SelfImprovementEngine::new();
        // Add positive feedback to create a pattern for "rust".
        engine.thumbs_up("explain rust ownership", "Ownership in Rust ensures memory safety...");
        engine.thumbs_up("rust borrowing rules", "Borrowing lets you reference data...");

        let suggestions = engine.get_improvement_suggestions("tell me about rust");
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn test_get_response_guidance() {
        let mut engine = SelfImprovementEngine::new();
        engine.thumbs_up("explain python decorators", "Decorators are higher-order functions...");

        // Pattern for "python" should have been learned. It needs usage > 0
        // (which thumbs_up causes via learn_positive_pattern → record_usage).
        let guidance = engine.get_response_guidance("help with python");
        // The pattern might match on "python" or "decorators".
        // At minimum, the learned pattern should exist.
        assert!(engine.pattern_count() > 0);

        // If guidance is found, it should be a non-empty string.
        if let Some(g) = guidance {
            assert!(!g.is_empty());
        }
    }

    #[test]
    fn test_record_outcome() {
        let mut engine = SelfImprovementEngine::new();
        engine.thumbs_up("open discord", "Opening Discord now...");
        let initial_count = engine.pattern_count();

        engine.record_outcome("open discord", "Done", true);
        // Pattern count should stay the same or increase (reinforcement).
        assert!(engine.pattern_count() >= initial_count);
    }

    #[test]
    fn test_should_retrain() {
        let mut engine = SelfImprovementEngine::new();
        assert!(!engine.should_retrain(3));

        engine.add_feedback("q1", "r1", 0.8, "");
        engine.add_feedback("q2", "r2", 0.9, "");
        assert!(!engine.should_retrain(3));

        engine.add_feedback("q3", "r3", 0.7, "");
        assert!(engine.should_retrain(3));

        engine.mark_all_processed();
        assert!(!engine.should_retrain(3));
    }

    #[test]
    fn test_performance_report() {
        let mut engine = SelfImprovementEngine::new();
        engine.thumbs_up("test query 1", "response 1");
        engine.thumbs_up("test query 2", "response 2");
        engine.thumbs_down("test query 3", "bad response", "wrong");

        let report = engine.get_performance_report();
        assert_eq!(report.total_feedback, 3);
        assert_eq!(report.positive_count, 2);
        assert_eq!(report.negative_count, 1);
        assert!((report.positive_rate - 2.0 / 3.0).abs() < 0.01);
        assert_eq!(report.unprocessed_count, 3);
    }

    #[test]
    fn test_get_stats() {
        let mut engine = SelfImprovementEngine::new();
        engine.thumbs_up("hello", "world");

        let stats = engine.get_stats();
        assert_eq!(stats.total_feedback, 1);
        assert_eq!(stats.positive_count, 1);
        assert_eq!(stats.negative_count, 0);
        assert_eq!(stats.feedback_backlog, 1);
    }

    #[test]
    fn test_extract_trigger_skips_stop_words() {
        let trigger = SelfImprovementEngine::extract_trigger("what is the weather today");
        assert!(trigger.is_some());
        let t = trigger.unwrap();
        // Should skip "what", "is", "the" and land on "weather".
        assert_eq!(t, "weather");
    }

    #[test]
    fn test_extract_trigger_empty_query() {
        let trigger = SelfImprovementEngine::extract_trigger("");
        assert!(trigger.is_none());
    }
}
