//! Data structures for feedback entries and response patterns.
//!
//! These types underpin the self-improvement engine's learning loop:
//! [`FeedbackEntry`] captures user ratings on individual interactions,
//! while [`ResponsePattern`] encodes learned preferences that guide
//! future responses.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Current Unix epoch timestamp in seconds (fractional).
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ─────────────────────────────────────────────────────────────────────────────
// FeedbackEntry
// ─────────────────────────────────────────────────────────────────────────────

/// A single piece of user feedback on a query/response pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    /// Unique identifier (e.g., `fb_1710000000`).
    pub id: String,
    /// The original user query.
    pub query: String,
    /// The system's response that was rated.
    pub response: String,
    /// Rating value: 0.0 (worst) to 1.0 (best).
    pub rating: f64,
    /// Optional free-text feedback from the user.
    pub feedback_text: String,
    /// Unix timestamp when the feedback was recorded.
    pub timestamp: f64,
    /// Whether this feedback has been processed by the learning engine.
    pub processed: bool,
}

impl FeedbackEntry {
    /// Create a new feedback entry with an auto-generated ID and timestamp.
    pub fn new(query: &str, response: &str, rating: f64, feedback_text: &str) -> Self {
        let ts = now_secs();
        Self {
            id: format!("fb_{}", ts as u64),
            query: query.to_string(),
            response: response.to_string(),
            rating: rating.clamp(0.0, 1.0),
            feedback_text: feedback_text.to_string(),
            timestamp: ts,
            processed: false,
        }
    }

    /// Whether this feedback is positive (rating >= 0.7).
    pub fn is_positive(&self) -> bool {
        self.rating >= 0.7
    }

    /// Whether this feedback is negative (rating <= 0.3).
    pub fn is_negative(&self) -> bool {
        self.rating <= 0.3
    }

    /// Mark this entry as processed.
    pub fn mark_processed(&mut self) {
        self.processed = true;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ResponsePattern
// ─────────────────────────────────────────────────────────────────────────────

/// A learned response pattern extracted from user feedback.
///
/// Patterns capture which types of queries elicit positive responses
/// and what style or content the user prefers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsePattern {
    /// Unique identifier (e.g., `rp_1710000000`).
    pub id: String,
    /// Category of the pattern (e.g., `"style"`, `"content"`, `"format"`).
    pub pattern_type: String,
    /// The query keyword or phrase that triggers this pattern.
    pub trigger: String,
    /// The preferred response approach for matching queries.
    pub preferred_response: String,
    /// Confidence in this pattern (0.0 to 1.0).
    pub confidence: f64,
    /// How many times this pattern has been applied.
    pub usage_count: u64,
    /// Observed success rate when this pattern is applied.
    pub success_rate: f64,
}

impl ResponsePattern {
    /// Create a new response pattern with an auto-generated ID.
    pub fn new(
        pattern_type: &str,
        trigger: &str,
        preferred_response: &str,
        confidence: f64,
    ) -> Self {
        let ts = now_secs();
        Self {
            id: format!("rp_{}", ts as u64),
            pattern_type: pattern_type.to_string(),
            trigger: trigger.to_string(),
            preferred_response: preferred_response.to_string(),
            confidence: confidence.clamp(0.0, 1.0),
            usage_count: 0,
            success_rate: 0.0,
        }
    }

    /// Record a usage of this pattern and update the success rate.
    pub fn record_usage(&mut self, success: bool) {
        let total = self.usage_count as f64;
        let successes = self.success_rate * total;
        self.usage_count += 1;
        self.success_rate = if success {
            (successes + 1.0) / self.usage_count as f64
        } else {
            successes / self.usage_count as f64
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feedback_entry_new() {
        let entry = FeedbackEntry::new("hello", "Hi there!", 0.9, "great response");
        assert!(entry.id.starts_with("fb_"));
        assert_eq!(entry.query, "hello");
        assert_eq!(entry.response, "Hi there!");
        assert!((entry.rating - 0.9).abs() < f64::EPSILON);
        assert!(!entry.processed);
    }

    #[test]
    fn test_feedback_rating_clamped() {
        let high = FeedbackEntry::new("q", "r", 1.5, "");
        assert!((high.rating - 1.0).abs() < f64::EPSILON);

        let low = FeedbackEntry::new("q", "r", -0.5, "");
        assert!((low.rating - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_feedback_is_positive() {
        let positive = FeedbackEntry::new("q", "r", 0.8, "");
        assert!(positive.is_positive());
        assert!(!positive.is_negative());

        let negative = FeedbackEntry::new("q", "r", 0.2, "");
        assert!(!negative.is_positive());
        assert!(negative.is_negative());
    }

    #[test]
    fn test_feedback_mark_processed() {
        let mut entry = FeedbackEntry::new("q", "r", 0.5, "");
        assert!(!entry.processed);
        entry.mark_processed();
        assert!(entry.processed);
    }

    #[test]
    fn test_response_pattern_new() {
        let pattern = ResponsePattern::new("style", "greeting", "Be warm and friendly", 0.85);
        assert!(pattern.id.starts_with("rp_"));
        assert_eq!(pattern.pattern_type, "style");
        assert_eq!(pattern.trigger, "greeting");
        assert_eq!(pattern.usage_count, 0);
        assert!((pattern.success_rate - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_response_pattern_record_usage() {
        let mut pattern = ResponsePattern::new("content", "code", "Include examples", 0.9);
        pattern.record_usage(true);
        assert_eq!(pattern.usage_count, 1);
        assert!((pattern.success_rate - 1.0).abs() < f64::EPSILON);

        pattern.record_usage(false);
        assert_eq!(pattern.usage_count, 2);
        assert!((pattern.success_rate - 0.5).abs() < f64::EPSILON);

        pattern.record_usage(true);
        assert_eq!(pattern.usage_count, 3);
        let expected = 2.0 / 3.0;
        assert!((pattern.success_rate - expected).abs() < 0.01);
    }

    #[test]
    fn test_pattern_confidence_clamped() {
        let high = ResponsePattern::new("style", "t", "r", 1.5);
        assert!((high.confidence - 1.0).abs() < f64::EPSILON);

        let low = ResponsePattern::new("style", "t", "r", -0.3);
        assert!((low.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_now_secs_returns_positive() {
        let ts = now_secs();
        assert!(ts > 0.0);
    }
}
