//! Change Log — audit trail for all self-improvement changes.
//!
//! Every change applied (or proposed) by the self-improvement system is
//! recorded as a [`ChangeEntry`] with a sequential tracking code
//! (e.g., SI-0001, SI-0002). The [`SelfImprovementLog`] provides
//! querying, filtering, and summary statistics.

use serde::{Deserialize, Serialize};

use super::types::now_secs;

// ─────────────────────────────────────────────────────────────────────────────
// ChangeStatus
// ─────────────────────────────────────────────────────────────────────────────

/// Status of a change entry in the audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeStatus {
    /// Change has been proposed but not yet reviewed.
    Proposed,
    /// Change has been approved.
    Approved,
    /// Change has been rejected.
    Rejected,
    /// Change has been applied to the codebase.
    Applied,
}

// ─────────────────────────────────────────────────────────────────────────────
// ChangeEntry
// ─────────────────────────────────────────────────────────────────────────────

/// A single entry in the self-improvement audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEntry {
    /// Sequential tracking code (e.g., `SI-0001`).
    pub tracking_code: String,
    /// Unix timestamp when this entry was created.
    pub timestamp: f64,
    /// Source of the change (e.g., `"autonomous"`, `"user"`, `"feedback_engine"`).
    pub source: String,
    /// ID of the associated proposal.
    pub proposal_id: String,
    /// Human-readable description of the change.
    pub description: String,
    /// List of files affected.
    pub files_changed: Vec<String>,
    /// Number of tests that passed after applying the change.
    pub test_passed: u32,
    /// Number of tests that failed after applying the change.
    pub test_failed: u32,
    /// Current status of this change.
    pub status: ChangeStatus,
}

impl ChangeEntry {
    /// Whether the change is in a terminal state (Applied or Rejected).
    pub fn is_terminal(&self) -> bool {
        matches!(self.status, ChangeStatus::Applied | ChangeStatus::Rejected)
    }

    /// Whether all tests passed (none failed).
    pub fn tests_clean(&self) -> bool {
        self.test_failed == 0
    }

    /// Total number of tests run.
    pub fn total_tests(&self) -> u32 {
        self.test_passed + self.test_failed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LogSummary
// ─────────────────────────────────────────────────────────────────────────────

/// Summary statistics for the change log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSummary {
    /// Total entries in the log.
    pub total_entries: usize,
    /// Count of proposed (not yet reviewed) entries.
    pub proposed_count: usize,
    /// Count of approved entries.
    pub approved_count: usize,
    /// Count of rejected entries.
    pub rejected_count: usize,
    /// Count of applied entries.
    pub applied_count: usize,
    /// Total tests passed across all entries.
    pub total_tests_passed: u32,
    /// Total tests failed across all entries.
    pub total_tests_failed: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// SelfImprovementLog
// ─────────────────────────────────────────────────────────────────────────────

/// Audit trail for self-improvement changes.
///
/// Maintains an ordered list of [`ChangeEntry`] records with sequential
/// tracking codes for traceability.
pub struct SelfImprovementLog {
    /// All change entries in chronological order.
    entries: Vec<ChangeEntry>,
    /// Next sequential ID for tracking code generation.
    next_id: u32,
}

impl SelfImprovementLog {
    /// Create a new, empty change log.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// Log a new change entry. Returns a clone of the created entry.
    ///
    /// Generates a sequential tracking code (SI-0001, SI-0002, etc.).
    pub fn log_change(
        &mut self,
        proposal_id: &str,
        description: &str,
        source: &str,
        files: Vec<String>,
        test_passed: u32,
        test_failed: u32,
    ) -> ChangeEntry {
        let tracking_code = format!("SI-{:04}", self.next_id);
        self.next_id += 1;

        let entry = ChangeEntry {
            tracking_code,
            timestamp: now_secs(),
            source: source.to_string(),
            proposal_id: proposal_id.to_string(),
            description: description.to_string(),
            files_changed: files,
            test_passed,
            test_failed,
            status: ChangeStatus::Proposed,
        };

        self.entries.push(entry.clone());
        entry
    }

    /// Update the status of an entry by tracking code.
    ///
    /// Returns `true` if the entry was found and updated, `false` otherwise.
    pub fn update_status(&mut self, tracking_code: &str, status: ChangeStatus) -> bool {
        for entry in &mut self.entries {
            if entry.tracking_code == tracking_code {
                entry.status = status;
                return true;
            }
        }
        false
    }

    /// Get entries filtered by optional status, limited to `limit` results.
    ///
    /// Returns entries in reverse chronological order (newest first).
    pub fn get_entries(&self, status: Option<ChangeStatus>, limit: usize) -> Vec<&ChangeEntry> {
        let filtered: Vec<&ChangeEntry> = self
            .entries
            .iter()
            .rev()
            .filter(|e| {
                if let Some(ref s) = status {
                    &e.status == s
                } else {
                    true
                }
            })
            .take(limit)
            .collect();

        filtered
    }

    /// Find the change entry associated with a specific proposal ID.
    pub fn get_by_proposal(&self, proposal_id: &str) -> Option<&ChangeEntry> {
        self.entries.iter().find(|e| e.proposal_id == proposal_id)
    }

    /// Find a change entry by its tracking code.
    pub fn get_by_code(&self, tracking_code: &str) -> Option<&ChangeEntry> {
        self.entries
            .iter()
            .find(|e| e.tracking_code == tracking_code)
    }

    /// Total number of entries in the log.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Generate a summary of the entire log.
    pub fn get_summary(&self) -> LogSummary {
        let mut proposed_count = 0usize;
        let mut approved_count = 0usize;
        let mut rejected_count = 0usize;
        let mut applied_count = 0usize;
        let mut total_tests_passed = 0u32;
        let mut total_tests_failed = 0u32;

        for entry in &self.entries {
            match entry.status {
                ChangeStatus::Proposed => proposed_count += 1,
                ChangeStatus::Approved => approved_count += 1,
                ChangeStatus::Rejected => rejected_count += 1,
                ChangeStatus::Applied => applied_count += 1,
            }
            total_tests_passed += entry.test_passed;
            total_tests_failed += entry.test_failed;
        }

        LogSummary {
            total_entries: self.entries.len(),
            proposed_count,
            approved_count,
            rejected_count,
            applied_count,
            total_tests_passed,
            total_tests_failed,
        }
    }
}

impl Default for SelfImprovementLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_new() {
        let log = SelfImprovementLog::new();
        assert_eq!(log.entry_count(), 0);
    }

    #[test]
    fn test_log_change_sequential_codes() {
        let mut log = SelfImprovementLog::new();
        let e1 = log.log_change("prop_1", "First change", "autonomous", vec![], 10, 0);
        let e2 = log.log_change("prop_2", "Second change", "user", vec![], 20, 1);
        let e3 = log.log_change("prop_3", "Third change", "feedback_engine", vec![], 15, 0);

        assert_eq!(e1.tracking_code, "SI-0001");
        assert_eq!(e2.tracking_code, "SI-0002");
        assert_eq!(e3.tracking_code, "SI-0003");
        assert_eq!(log.entry_count(), 3);
    }

    #[test]
    fn test_log_change_fields() {
        let mut log = SelfImprovementLog::new();
        let entry = log.log_change(
            "prop_42",
            "Add caching",
            "autonomous",
            vec!["src/cache.rs".to_string(), "src/main.rs".to_string()],
            50,
            2,
        );

        assert_eq!(entry.proposal_id, "prop_42");
        assert_eq!(entry.description, "Add caching");
        assert_eq!(entry.source, "autonomous");
        assert_eq!(entry.files_changed.len(), 2);
        assert_eq!(entry.test_passed, 50);
        assert_eq!(entry.test_failed, 2);
        assert_eq!(entry.status, ChangeStatus::Proposed);
        assert!(entry.timestamp > 0.0);
    }

    #[test]
    fn test_update_status() {
        let mut log = SelfImprovementLog::new();
        let entry = log.log_change("p1", "change", "user", vec![], 0, 0);

        assert!(log.update_status(&entry.tracking_code, ChangeStatus::Approved));
        let updated = log.get_by_code(&entry.tracking_code).unwrap();
        assert_eq!(updated.status, ChangeStatus::Approved);

        assert!(!log.update_status("SI-9999", ChangeStatus::Rejected));
    }

    #[test]
    fn test_get_entries_no_filter() {
        let mut log = SelfImprovementLog::new();
        log.log_change("p1", "a", "user", vec![], 0, 0);
        log.log_change("p2", "b", "user", vec![], 0, 0);
        log.log_change("p3", "c", "user", vec![], 0, 0);

        let all = log.get_entries(None, 100);
        assert_eq!(all.len(), 3);
        // Should be newest first.
        assert_eq!(all[0].tracking_code, "SI-0003");
        assert_eq!(all[2].tracking_code, "SI-0001");
    }

    #[test]
    fn test_get_entries_with_filter() {
        let mut log = SelfImprovementLog::new();
        let e1 = log.log_change("p1", "a", "user", vec![], 0, 0);
        log.log_change("p2", "b", "user", vec![], 0, 0);

        log.update_status(&e1.tracking_code, ChangeStatus::Applied);

        let applied = log.get_entries(Some(ChangeStatus::Applied), 100);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].tracking_code, "SI-0001");

        let proposed = log.get_entries(Some(ChangeStatus::Proposed), 100);
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].tracking_code, "SI-0002");
    }

    #[test]
    fn test_get_entries_limit() {
        let mut log = SelfImprovementLog::new();
        for i in 0..10 {
            log.log_change(&format!("p{i}"), "change", "user", vec![], 0, 0);
        }

        let limited = log.get_entries(None, 3);
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_get_by_proposal() {
        let mut log = SelfImprovementLog::new();
        log.log_change("prop_abc", "my change", "user", vec![], 5, 0);
        log.log_change("prop_def", "other change", "auto", vec![], 3, 1);

        let found = log.get_by_proposal("prop_abc").unwrap();
        assert_eq!(found.description, "my change");

        assert!(log.get_by_proposal("prop_nonexistent").is_none());
    }

    #[test]
    fn test_get_summary() {
        let mut log = SelfImprovementLog::new();
        let e1 = log.log_change("p1", "a", "user", vec![], 10, 0);
        let e2 = log.log_change("p2", "b", "auto", vec![], 8, 2);
        log.log_change("p3", "c", "user", vec![], 5, 1);

        log.update_status(&e1.tracking_code, ChangeStatus::Applied);
        log.update_status(&e2.tracking_code, ChangeStatus::Rejected);

        let summary = log.get_summary();
        assert_eq!(summary.total_entries, 3);
        assert_eq!(summary.proposed_count, 1);
        assert_eq!(summary.applied_count, 1);
        assert_eq!(summary.rejected_count, 1);
        assert_eq!(summary.approved_count, 0);
        assert_eq!(summary.total_tests_passed, 23);
        assert_eq!(summary.total_tests_failed, 3);
    }

    #[test]
    fn test_change_entry_helpers() {
        let entry = ChangeEntry {
            tracking_code: "SI-0001".to_string(),
            timestamp: 0.0,
            source: "test".to_string(),
            proposal_id: "p1".to_string(),
            description: "test".to_string(),
            files_changed: vec![],
            test_passed: 10,
            test_failed: 0,
            status: ChangeStatus::Proposed,
        };

        assert!(!entry.is_terminal());
        assert!(entry.tests_clean());
        assert_eq!(entry.total_tests(), 10);

        let terminal = ChangeEntry {
            status: ChangeStatus::Applied,
            test_failed: 2,
            ..entry.clone()
        };
        assert!(terminal.is_terminal());
        assert!(!terminal.tests_clean());
        assert_eq!(terminal.total_tests(), 12);
    }
}
