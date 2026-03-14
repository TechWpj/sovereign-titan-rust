//! Security Response — active response actions for security events.
//!
//! Features:
//! - Rule-based evaluation (pattern + severity threshold)
//! - Source blocking / unblocking
//! - Response action logging
//! - Default rules for common threat types

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// The type of response action to take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseAction {
    LogOnly,
    RateLimit,
    BlockRequest,
    BlockSource,
    AlertAdmin,
    Quarantine,
    Shutdown,
}

/// A rule that maps event patterns to response actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseRule {
    pub event_pattern: String,
    pub min_severity: u8,
    pub action: ResponseAction,
    pub cooldown_secs: f64,
    pub enabled: bool,
}

/// A record of a response action that was executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseRecord {
    pub event_id: String,
    pub action: ResponseAction,
    pub timestamp: f64,
    pub success: bool,
    pub details: String,
}

impl ResponseRecord {
    /// Create a new response record with the current timestamp.
    pub fn new(event_id: &str, action: ResponseAction, success: bool, details: &str) -> Self {
        Self {
            event_id: event_id.to_string(),
            action,
            timestamp: now_secs(),
            success,
            details: details.to_string(),
        }
    }
}

/// Engine that evaluates security events against rules and executes responses.
pub struct SecurityResponseEngine {
    rules: Vec<ResponseRule>,
    blocked_sources: HashMap<String, f64>,
    rate_limits: HashMap<String, (u32, f64)>,
    response_log: Vec<ResponseRecord>,
    max_log: usize,
}

impl SecurityResponseEngine {
    /// Create a new response engine with default rules.
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
            blocked_sources: HashMap::new(),
            rate_limits: HashMap::new(),
            response_log: Vec::new(),
            max_log: 5000,
        }
    }

    fn default_rules() -> Vec<ResponseRule> {
        vec![
            ResponseRule {
                event_pattern: "PromptInjection".to_string(),
                min_severity: 3,
                action: ResponseAction::BlockRequest,
                cooldown_secs: 60.0,
                enabled: true,
            },
            ResponseRule {
                event_pattern: "DataExfiltration".to_string(),
                min_severity: 3,
                action: ResponseAction::BlockSource,
                cooldown_secs: 300.0,
                enabled: true,
            },
            ResponseRule {
                event_pattern: "RateLimitExceeded".to_string(),
                min_severity: 2,
                action: ResponseAction::RateLimit,
                cooldown_secs: 30.0,
                enabled: true,
            },
        ]
    }

    /// Evaluate which rules match an event type and severity.
    pub fn evaluate(&self, event_type: &str, severity: u8) -> Vec<&ResponseRule> {
        self.rules
            .iter()
            .filter(|r| {
                r.enabled && event_type.contains(&r.event_pattern) && severity >= r.min_severity
            })
            .collect()
    }

    /// Block a source (e.g., an IP or user ID).
    pub fn block_source(&mut self, source: &str) {
        self.blocked_sources
            .insert(source.to_string(), now_secs());
    }

    /// Check whether a source is currently blocked.
    pub fn is_blocked(&self, source: &str) -> bool {
        self.blocked_sources.contains_key(source)
    }

    /// Unblock a source. Returns `true` if it was blocked.
    pub fn unblock_source(&mut self, source: &str) -> bool {
        self.blocked_sources.remove(source).is_some()
    }

    /// Record a response action that was taken.
    pub fn record_response(&mut self, record: ResponseRecord) {
        if self.response_log.len() >= self.max_log {
            self.response_log.remove(0);
        }
        self.response_log.push(record);
    }

    /// Number of currently blocked sources.
    pub fn blocked_count(&self) -> usize {
        self.blocked_sources.len()
    }

    /// Number of configured rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Number of recorded responses.
    pub fn response_count(&self) -> usize {
        self.response_log.len()
    }

    /// Add a custom rule.
    pub fn add_rule(&mut self, rule: ResponseRule) {
        self.rules.push(rule);
    }

    /// Disable all rules matching a pattern.
    pub fn disable_rules(&mut self, pattern: &str) {
        for rule in &mut self.rules {
            if rule.event_pattern.contains(pattern) {
                rule.enabled = false;
            }
        }
    }

    /// Enable all rules matching a pattern.
    pub fn enable_rules(&mut self, pattern: &str) {
        for rule in &mut self.rules {
            if rule.event_pattern.contains(pattern) {
                rule.enabled = true;
            }
        }
    }

    /// Return the most recent N response records (newest first).
    pub fn recent_responses(&self, n: usize) -> Vec<&ResponseRecord> {
        self.response_log.iter().rev().take(n).collect()
    }

    /// Unblock sources that have been blocked longer than `max_age_secs`.
    pub fn expire_blocks(&mut self, max_age_secs: f64) {
        let now = now_secs();
        self.blocked_sources
            .retain(|_, ts| now - *ts < max_age_secs);
    }

    /// Clear all blocked sources and response logs.
    pub fn clear(&mut self) {
        self.blocked_sources.clear();
        self.rate_limits.clear();
        self.response_log.clear();
    }

    /// All currently blocked source identifiers.
    pub fn blocked_sources(&self) -> Vec<&str> {
        self.blocked_sources.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for SecurityResponseEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rules() {
        let engine = SecurityResponseEngine::new();
        assert_eq!(engine.rule_count(), 3);
    }

    #[test]
    fn test_evaluate_matching_rule() {
        let engine = SecurityResponseEngine::new();
        let matches = engine.evaluate("PromptInjection", 3);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].action, ResponseAction::BlockRequest);
    }

    #[test]
    fn test_evaluate_severity_too_low() {
        let engine = SecurityResponseEngine::new();
        let matches = engine.evaluate("PromptInjection", 1);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_evaluate_no_matching_pattern() {
        let engine = SecurityResponseEngine::new();
        let matches = engine.evaluate("UnknownEventType", 4);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_block_and_check_source() {
        let mut engine = SecurityResponseEngine::new();
        assert!(!engine.is_blocked("bad_ip"));
        engine.block_source("bad_ip");
        assert!(engine.is_blocked("bad_ip"));
        assert_eq!(engine.blocked_count(), 1);
    }

    #[test]
    fn test_unblock_source() {
        let mut engine = SecurityResponseEngine::new();
        engine.block_source("bad_ip");
        assert!(engine.unblock_source("bad_ip"));
        assert!(!engine.is_blocked("bad_ip"));
    }

    #[test]
    fn test_unblock_nonexistent() {
        let mut engine = SecurityResponseEngine::new();
        assert!(!engine.unblock_source("never_blocked"));
    }

    #[test]
    fn test_record_response() {
        let mut engine = SecurityResponseEngine::new();
        let record = ResponseRecord::new("evt_1", ResponseAction::BlockRequest, true, "blocked");
        engine.record_response(record);
        assert_eq!(engine.response_count(), 1);
    }

    #[test]
    fn test_response_log_capacity() {
        let mut engine = SecurityResponseEngine::new();
        engine.max_log = 3;
        for i in 0..5 {
            engine.record_response(ResponseRecord::new(
                &format!("evt_{i}"),
                ResponseAction::LogOnly,
                true,
                "ok",
            ));
        }
        assert_eq!(engine.response_count(), 3);
    }

    #[test]
    fn test_add_custom_rule() {
        let mut engine = SecurityResponseEngine::new();
        engine.add_rule(ResponseRule {
            event_pattern: "CustomThreat".to_string(),
            min_severity: 1,
            action: ResponseAction::AlertAdmin,
            cooldown_secs: 10.0,
            enabled: true,
        });
        assert_eq!(engine.rule_count(), 4);
        let matches = engine.evaluate("CustomThreat", 1);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_disable_rules() {
        let mut engine = SecurityResponseEngine::new();
        engine.disable_rules("PromptInjection");
        let matches = engine.evaluate("PromptInjection", 4);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_enable_rules() {
        let mut engine = SecurityResponseEngine::new();
        engine.disable_rules("PromptInjection");
        engine.enable_rules("PromptInjection");
        let matches = engine.evaluate("PromptInjection", 3);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_recent_responses() {
        let mut engine = SecurityResponseEngine::new();
        engine.record_response(ResponseRecord::new(
            "evt_a",
            ResponseAction::LogOnly,
            true,
            "first",
        ));
        engine.record_response(ResponseRecord::new(
            "evt_b",
            ResponseAction::BlockRequest,
            true,
            "second",
        ));
        let recent = engine.recent_responses(1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].event_id, "evt_b");
    }

    #[test]
    fn test_clear() {
        let mut engine = SecurityResponseEngine::new();
        engine.block_source("ip1");
        engine.record_response(ResponseRecord::new(
            "e",
            ResponseAction::LogOnly,
            true,
            "ok",
        ));
        engine.clear();
        assert_eq!(engine.blocked_count(), 0);
        assert_eq!(engine.response_count(), 0);
    }

    #[test]
    fn test_blocked_sources_list() {
        let mut engine = SecurityResponseEngine::new();
        engine.block_source("ip_a");
        engine.block_source("ip_b");
        let sources = engine.blocked_sources();
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&"ip_a"));
        assert!(sources.contains(&"ip_b"));
    }

    #[test]
    fn test_response_record_new() {
        let rec = ResponseRecord::new("evt_99", ResponseAction::Quarantine, false, "failed");
        assert_eq!(rec.event_id, "evt_99");
        assert_eq!(rec.action, ResponseAction::Quarantine);
        assert!(!rec.success);
        assert!(rec.timestamp > 0.0);
    }
}
