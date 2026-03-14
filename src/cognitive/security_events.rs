//! Security Event Log — structured security event recording with dedup.
//!
//! Features:
//! - Multiple event types and severity levels
//! - Deduplication window to suppress repeated events
//! - Querying by severity, type, and acknowledgement status
//! - Bounded event storage with FIFO eviction

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Classification of a security event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityEventType {
    PromptInjection,
    UnauthorizedAccess,
    RateLimitExceeded,
    SuspiciousPattern,
    DataExfiltration,
    PrivilegeEscalation,
    MaliciousPayload,
    AnomalousRequest,
}

/// Severity level for a security event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Numeric severity (1=Low .. 4=Critical).
    pub fn numeric(&self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

/// A recorded security event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub id: String,
    pub event_type: SecurityEventType,
    pub severity: Severity,
    pub source: String,
    pub description: String,
    pub timestamp: f64,
    pub metadata: HashMap<String, String>,
    pub acknowledged: bool,
}

/// Bounded security event log with deduplication.
pub struct SecurityEventLog {
    events: VecDeque<SecurityEvent>,
    max_events: usize,
    event_counts: HashMap<SecurityEventType, u64>,
    dedup_window_secs: f64,
    recent_hashes: HashMap<String, f64>,
    counter: u64,
}

impl SecurityEventLog {
    /// Create a new log with the given capacity and dedup window.
    pub fn new(max_events: usize, dedup_window_secs: f64) -> Self {
        Self {
            events: VecDeque::new(),
            max_events,
            event_counts: HashMap::new(),
            dedup_window_secs,
            recent_hashes: HashMap::new(),
            counter: 0,
        }
    }

    /// Record a new security event. Returns `None` if the event was
    /// deduplicated (suppressed), otherwise returns the event ID.
    pub fn record(
        &mut self,
        event_type: SecurityEventType,
        severity: Severity,
        source: &str,
        description: &str,
    ) -> Option<String> {
        let now = now_secs();

        // Dedup check — same type + description within window
        let hash = format!("{:?}:{}", event_type, description);
        if let Some(ts) = self.recent_hashes.get(&hash) {
            if now - ts < self.dedup_window_secs {
                return None; // duplicate within window
            }
        }
        self.recent_hashes.insert(hash, now);

        self.counter += 1;
        let id = format!("sec_{}_{}", now as u64, self.counter);

        let event = SecurityEvent {
            id: id.clone(),
            event_type: event_type.clone(),
            severity,
            source: source.to_string(),
            description: description.to_string(),
            timestamp: now,
            metadata: HashMap::new(),
            acknowledged: false,
        };

        *self.event_counts.entry(event_type).or_insert(0) += 1;

        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
        Some(id)
    }

    /// Record an event with additional metadata.
    pub fn record_with_metadata(
        &mut self,
        event_type: SecurityEventType,
        severity: Severity,
        source: &str,
        description: &str,
        metadata: HashMap<String, String>,
    ) -> Option<String> {
        let id = self.record(event_type, severity, source, description)?;
        // Attach metadata to the just-inserted event
        if let Some(evt) = self.events.back_mut() {
            evt.metadata = metadata;
        }
        Some(id)
    }

    /// Acknowledge a security event by ID.
    pub fn acknowledge(&mut self, id: &str) -> bool {
        if let Some(evt) = self.events.iter_mut().find(|e| e.id == id) {
            evt.acknowledged = true;
            true
        } else {
            false
        }
    }

    /// Return events matching the given severity.
    pub fn by_severity(&self, sev: &Severity) -> Vec<&SecurityEvent> {
        self.events.iter().filter(|e| &e.severity == sev).collect()
    }

    /// Return events matching the given type.
    pub fn by_type(&self, et: &SecurityEventType) -> Vec<&SecurityEvent> {
        self.events
            .iter()
            .filter(|e| &e.event_type == et)
            .collect()
    }

    /// Return all unacknowledged events.
    pub fn unacknowledged(&self) -> Vec<&SecurityEvent> {
        self.events.iter().filter(|e| !e.acknowledged).collect()
    }

    /// Return unacknowledged critical events.
    pub fn critical_unacked(&self) -> Vec<&SecurityEvent> {
        self.events
            .iter()
            .filter(|e| e.severity == Severity::Critical && !e.acknowledged)
            .collect()
    }

    /// Total number of events currently stored.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Cumulative count for a given event type (including evicted events).
    pub fn count_by_type(&self, et: &SecurityEventType) -> u64 {
        self.event_counts.get(et).copied().unwrap_or(0)
    }

    /// Return the N most recent events (newest first).
    pub fn recent(&self, n: usize) -> Vec<&SecurityEvent> {
        self.events.iter().rev().take(n).collect()
    }

    /// Get an event by its ID.
    pub fn get_by_id(&self, id: &str) -> Option<&SecurityEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    /// Return events with severity at or above the given numeric threshold.
    pub fn at_or_above_severity(&self, min_numeric: u8) -> Vec<&SecurityEvent> {
        self.events
            .iter()
            .filter(|e| e.severity.numeric() >= min_numeric)
            .collect()
    }

    /// Clear all events, counts, and dedup hashes.
    pub fn clear(&mut self) {
        self.events.clear();
        self.event_counts.clear();
        self.recent_hashes.clear();
    }
}

impl Default for SecurityEventLog {
    fn default() -> Self {
        Self::new(10000, 5.0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_log() -> SecurityEventLog {
        SecurityEventLog::new(100, 5.0)
    }

    #[test]
    fn test_record_basic() {
        let mut log = make_log();
        let id = log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "api",
            "injection attempt",
        );
        assert!(id.is_some());
        assert_eq!(log.total_events(), 1);
    }

    #[test]
    fn test_dedup_within_window() {
        let mut log = make_log();
        let id1 = log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "api",
            "injection attempt",
        );
        let id2 = log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "api",
            "injection attempt",
        );
        assert!(id1.is_some());
        assert!(id2.is_none()); // deduplicated
        assert_eq!(log.total_events(), 1);
    }

    #[test]
    fn test_different_descriptions_not_deduped() {
        let mut log = make_log();
        log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "api",
            "attempt 1",
        );
        let id2 = log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "api",
            "attempt 2",
        );
        assert!(id2.is_some());
        assert_eq!(log.total_events(), 2);
    }

    #[test]
    fn test_expired_dedup_window() {
        let mut log = SecurityEventLog::new(100, 0.0); // 0-second dedup window
        let id1 = log.record(
            SecurityEventType::SuspiciousPattern,
            Severity::Low,
            "scan",
            "same event",
        );
        let id2 = log.record(
            SecurityEventType::SuspiciousPattern,
            Severity::Low,
            "scan",
            "same event",
        );
        assert!(id1.is_some());
        assert!(id2.is_some()); // not deduped because window is 0
    }

    #[test]
    fn test_eviction_at_capacity() {
        let mut log = SecurityEventLog::new(3, 0.0);
        for i in 0..5 {
            log.record(
                SecurityEventType::AnomalousRequest,
                Severity::Low,
                "src",
                &format!("event_{i}"),
            );
        }
        assert_eq!(log.total_events(), 3);
    }

    #[test]
    fn test_acknowledge() {
        let mut log = make_log();
        let id = log
            .record(
                SecurityEventType::DataExfiltration,
                Severity::Critical,
                "dlp",
                "data leak",
            )
            .unwrap();
        assert_eq!(log.unacknowledged().len(), 1);
        assert!(log.acknowledge(&id));
        assert_eq!(log.unacknowledged().len(), 0);
    }

    #[test]
    fn test_acknowledge_nonexistent() {
        let mut log = make_log();
        assert!(!log.acknowledge("bogus_id"));
    }

    #[test]
    fn test_by_severity() {
        let mut log = SecurityEventLog::new(100, 0.0);
        log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "a",
            "h1",
        );
        log.record(
            SecurityEventType::RateLimitExceeded,
            Severity::Low,
            "b",
            "l1",
        );
        log.record(
            SecurityEventType::MaliciousPayload,
            Severity::High,
            "c",
            "h2",
        );
        assert_eq!(log.by_severity(&Severity::High).len(), 2);
        assert_eq!(log.by_severity(&Severity::Low).len(), 1);
        assert_eq!(log.by_severity(&Severity::Critical).len(), 0);
    }

    #[test]
    fn test_by_type() {
        let mut log = SecurityEventLog::new(100, 0.0);
        log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "a",
            "pi1",
        );
        log.record(
            SecurityEventType::PromptInjection,
            Severity::Medium,
            "b",
            "pi2",
        );
        log.record(
            SecurityEventType::RateLimitExceeded,
            Severity::Low,
            "c",
            "rl1",
        );
        assert_eq!(log.by_type(&SecurityEventType::PromptInjection).len(), 2);
        assert_eq!(log.by_type(&SecurityEventType::RateLimitExceeded).len(), 1);
    }

    #[test]
    fn test_critical_unacked() {
        let mut log = SecurityEventLog::new(100, 0.0);
        let id_crit = log
            .record(
                SecurityEventType::PrivilegeEscalation,
                Severity::Critical,
                "auth",
                "esc1",
            )
            .unwrap();
        log.record(
            SecurityEventType::SuspiciousPattern,
            Severity::High,
            "scan",
            "pat1",
        );
        assert_eq!(log.critical_unacked().len(), 1);
        log.acknowledge(&id_crit);
        assert_eq!(log.critical_unacked().len(), 0);
    }

    #[test]
    fn test_count_by_type() {
        let mut log = SecurityEventLog::new(100, 0.0);
        log.record(
            SecurityEventType::RateLimitExceeded,
            Severity::Medium,
            "gw",
            "rl_a",
        );
        log.record(
            SecurityEventType::RateLimitExceeded,
            Severity::Medium,
            "gw",
            "rl_b",
        );
        assert_eq!(
            log.count_by_type(&SecurityEventType::RateLimitExceeded),
            2
        );
        assert_eq!(
            log.count_by_type(&SecurityEventType::DataExfiltration),
            0
        );
    }

    #[test]
    fn test_recent() {
        let mut log = SecurityEventLog::new(100, 0.0);
        log.record(
            SecurityEventType::AnomalousRequest,
            Severity::Low,
            "s",
            "first",
        );
        log.record(
            SecurityEventType::AnomalousRequest,
            Severity::Low,
            "s",
            "second",
        );
        let recent = log.recent(1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].description, "second");
    }

    #[test]
    fn test_severity_numeric() {
        assert_eq!(Severity::Low.numeric(), 1);
        assert_eq!(Severity::Medium.numeric(), 2);
        assert_eq!(Severity::High.numeric(), 3);
        assert_eq!(Severity::Critical.numeric(), 4);
    }

    #[test]
    fn test_at_or_above_severity() {
        let mut log = SecurityEventLog::new(100, 0.0);
        log.record(
            SecurityEventType::SuspiciousPattern,
            Severity::Low,
            "a",
            "lo",
        );
        log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "b",
            "hi",
        );
        log.record(
            SecurityEventType::DataExfiltration,
            Severity::Critical,
            "c",
            "crit",
        );
        assert_eq!(log.at_or_above_severity(3).len(), 2); // High + Critical
        assert_eq!(log.at_or_above_severity(4).len(), 1); // Critical only
    }

    #[test]
    fn test_clear() {
        let mut log = make_log();
        log.record(
            SecurityEventType::PromptInjection,
            Severity::High,
            "x",
            "y",
        );
        log.clear();
        assert_eq!(log.total_events(), 0);
        assert_eq!(
            log.count_by_type(&SecurityEventType::PromptInjection),
            0
        );
    }

    #[test]
    fn test_record_with_metadata() {
        let mut log = SecurityEventLog::new(100, 0.0);
        let mut meta = HashMap::new();
        meta.insert("ip".to_string(), "10.0.0.1".to_string());
        let id = log
            .record_with_metadata(
                SecurityEventType::UnauthorizedAccess,
                Severity::High,
                "firewall",
                "blocked",
                meta,
            )
            .unwrap();
        let evt = log.get_by_id(&id).unwrap();
        assert_eq!(evt.metadata.get("ip").unwrap(), "10.0.0.1");
    }

    #[test]
    fn test_default_log() {
        let log = SecurityEventLog::default();
        assert_eq!(log.max_events, 10000);
        assert!((log.dedup_window_secs - 5.0).abs() < f64::EPSILON);
    }
}
