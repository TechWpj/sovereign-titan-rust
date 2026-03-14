//! Proposal Lifecycle Management — create, review, approve, or reject
//! self-improvement proposals.
//!
//! Each [`Proposal`] tracks a suggested code change through its lifecycle:
//! pending review, approved for application, rejected, failed during apply,
//! or rolled back after a regression.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::types::now_secs;

static PROPOSAL_COUNTER: AtomicU64 = AtomicU64::new(0);

// ─────────────────────────────────────────────────────────────────────────────
// Enums
// ─────────────────────────────────────────────────────────────────────────────

/// Current status of a proposal in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Awaiting human review.
    Pending,
    /// Approved and ready to apply (or already applied).
    Approved,
    /// Rejected by the reviewer.
    Rejected,
    /// Application was attempted but failed.
    Failed,
    /// Was applied but rolled back due to regressions.
    RolledBack,
}

/// What triggered the proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalTrigger {
    /// Proposed by a human user.
    User,
    /// Proposed autonomously by the system.
    Autonomous,
}

// ─────────────────────────────────────────────────────────────────────────────
// Proposal
// ─────────────────────────────────────────────────────────────────────────────

/// A self-improvement proposal describing a potential code change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique identifier (e.g., `prop_1710000000`).
    pub id: String,
    /// The prompt or instruction that generated this proposal.
    pub prompt: String,
    /// Human-readable description of the change.
    pub description: String,
    /// The code diff or patch content.
    pub diff: String,
    /// Test results after applying the change, if available.
    pub test_results: Option<String>,
    /// Current lifecycle status.
    pub status: ProposalStatus,
    /// What triggered this proposal.
    pub trigger: ProposalTrigger,
    /// Git branch name for the change (if applicable).
    pub branch_name: String,
    /// List of files affected by this proposal.
    pub files_changed: Vec<String>,
    /// Unix timestamp when the proposal was created.
    pub created_at: f64,
    /// Unix timestamp when the proposal was resolved (approved/rejected/etc.).
    pub resolved_at: Option<f64>,
}

impl Proposal {
    /// Create a new proposal in `Pending` status.
    pub fn new(description: &str, prompt: &str, trigger: ProposalTrigger) -> Self {
        let ts = now_secs();
        let id = format!("prop_{}_{}", ts as u64, PROPOSAL_COUNTER.fetch_add(1, Ordering::Relaxed));
        let branch_name = format!(
            "si/{}",
            description
                .to_lowercase()
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join("-")
        );

        Self {
            id,
            prompt: prompt.to_string(),
            description: description.to_string(),
            diff: String::new(),
            test_results: None,
            status: ProposalStatus::Pending,
            trigger,
            branch_name,
            files_changed: Vec::new(),
            created_at: ts,
            resolved_at: None,
        }
    }

    /// Whether this proposal is still awaiting review.
    pub fn is_pending(&self) -> bool {
        self.status == ProposalStatus::Pending
    }

    /// Whether this proposal has been resolved (approved, rejected, failed, or rolled back).
    pub fn is_resolved(&self) -> bool {
        !self.is_pending()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ProposalManager
// ─────────────────────────────────────────────────────────────────────────────

/// Manages the lifecycle of self-improvement proposals.
pub struct ProposalManager {
    /// All proposals keyed by ID.
    proposals: HashMap<String, Proposal>,
    /// Directory where proposals are persisted (informational).
    proposals_dir: String,
}

impl ProposalManager {
    /// Create a new proposal manager.
    ///
    /// If `proposals_dir` is `None`, defaults to `"./proposals"`.
    pub fn new(proposals_dir: Option<String>) -> Self {
        Self {
            proposals: HashMap::new(),
            proposals_dir: proposals_dir.unwrap_or_else(|| "./proposals".to_string()),
        }
    }

    /// Create a new proposal and register it. Returns a clone of the proposal.
    pub fn create(
        &mut self,
        description: &str,
        prompt: &str,
        trigger: ProposalTrigger,
    ) -> Proposal {
        let proposal = Proposal::new(description, prompt, trigger);
        let clone = proposal.clone();
        self.proposals.insert(proposal.id.clone(), proposal);
        clone
    }

    /// Look up a proposal by ID.
    pub fn get(&self, id: &str) -> Option<&Proposal> {
        self.proposals.get(id)
    }

    /// List all proposals (in arbitrary order).
    pub fn list_all(&self) -> Vec<&Proposal> {
        self.proposals.values().collect()
    }

    /// List only pending proposals.
    pub fn list_pending(&self) -> Vec<&Proposal> {
        self.proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Pending)
            .collect()
    }

    /// Count of pending proposals.
    pub fn pending_count(&self) -> usize {
        self.proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Pending)
            .count()
    }

    /// Total number of proposals.
    pub fn total_count(&self) -> usize {
        self.proposals.len()
    }

    /// The proposals directory path.
    pub fn proposals_dir(&self) -> &str {
        &self.proposals_dir
    }

    /// Approve a pending proposal. Returns the updated proposal or an error.
    pub fn approve(&mut self, id: &str) -> Result<&Proposal, String> {
        let proposal = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| format!("Proposal not found: {id}"))?;

        if proposal.status != ProposalStatus::Pending {
            return Err(format!(
                "Cannot approve proposal {id}: status is {:?}, expected Pending",
                proposal.status
            ));
        }

        proposal.status = ProposalStatus::Approved;
        proposal.resolved_at = Some(now_secs());
        Ok(self.proposals.get(id).unwrap())
    }

    /// Reject a pending proposal. Returns the updated proposal or an error.
    pub fn reject(&mut self, id: &str) -> Result<&Proposal, String> {
        let proposal = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| format!("Proposal not found: {id}"))?;

        if proposal.status != ProposalStatus::Pending {
            return Err(format!(
                "Cannot reject proposal {id}: status is {:?}, expected Pending",
                proposal.status
            ));
        }

        proposal.status = ProposalStatus::Rejected;
        proposal.resolved_at = Some(now_secs());
        Ok(self.proposals.get(id).unwrap())
    }

    /// Mark a proposal as failed (e.g., tests didn't pass after apply).
    pub fn mark_failed(&mut self, id: &str) -> Result<&Proposal, String> {
        let proposal = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| format!("Proposal not found: {id}"))?;

        proposal.status = ProposalStatus::Failed;
        proposal.resolved_at = Some(now_secs());
        Ok(self.proposals.get(id).unwrap())
    }

    /// Mark a proposal as rolled back.
    pub fn mark_rolled_back(&mut self, id: &str) -> Result<&Proposal, String> {
        let proposal = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| format!("Proposal not found: {id}"))?;

        proposal.status = ProposalStatus::RolledBack;
        proposal.resolved_at = Some(now_secs());
        Ok(self.proposals.get(id).unwrap())
    }
}

impl Default for ProposalManager {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_new() {
        let prop = Proposal::new("Add caching layer", "Implement LRU cache", ProposalTrigger::User);
        assert!(prop.id.starts_with("prop_"));
        assert_eq!(prop.status, ProposalStatus::Pending);
        assert_eq!(prop.trigger, ProposalTrigger::User);
        assert!(prop.branch_name.starts_with("si/"));
        assert!(prop.is_pending());
        assert!(!prop.is_resolved());
        assert!(prop.resolved_at.is_none());
    }

    #[test]
    fn test_proposal_branch_name_generated() {
        let prop = Proposal::new(
            "Fix memory leak in parser module",
            "prompt",
            ProposalTrigger::Autonomous,
        );
        assert_eq!(prop.branch_name, "si/fix-memory-leak-in");
    }

    #[test]
    fn test_manager_create() {
        let mut mgr = ProposalManager::new(None);
        let prop = mgr.create("Test proposal", "prompt", ProposalTrigger::User);
        assert_eq!(mgr.total_count(), 1);
        assert!(mgr.get(&prop.id).is_some());
    }

    #[test]
    fn test_manager_list_all() {
        let mut mgr = ProposalManager::new(None);
        mgr.create("Proposal A", "prompt a", ProposalTrigger::User);
        mgr.create("Proposal B", "prompt b", ProposalTrigger::Autonomous);
        assert_eq!(mgr.list_all().len(), 2);
    }

    #[test]
    fn test_manager_list_pending() {
        let mut mgr = ProposalManager::new(Some("/tmp/proposals".to_string()));
        let p1 = mgr.create("Pending one", "p", ProposalTrigger::User);
        let p2 = mgr.create("Pending two", "p", ProposalTrigger::User);

        assert_eq!(mgr.list_pending().len(), 2);
        assert_eq!(mgr.pending_count(), 2);

        mgr.approve(&p1.id).unwrap();
        assert_eq!(mgr.list_pending().len(), 1);
        assert_eq!(mgr.pending_count(), 1);

        mgr.reject(&p2.id).unwrap();
        assert_eq!(mgr.list_pending().len(), 0);
        assert_eq!(mgr.pending_count(), 0);
    }

    #[test]
    fn test_manager_approve() {
        let mut mgr = ProposalManager::new(None);
        let prop = mgr.create("Approval test", "prompt", ProposalTrigger::User);
        let approved = mgr.approve(&prop.id).unwrap();
        assert_eq!(approved.status, ProposalStatus::Approved);
        assert!(approved.resolved_at.is_some());
    }

    #[test]
    fn test_manager_reject() {
        let mut mgr = ProposalManager::new(None);
        let prop = mgr.create("Reject test", "prompt", ProposalTrigger::Autonomous);
        let rejected = mgr.reject(&prop.id).unwrap();
        assert_eq!(rejected.status, ProposalStatus::Rejected);
        assert!(rejected.resolved_at.is_some());
    }

    #[test]
    fn test_manager_cannot_approve_non_pending() {
        let mut mgr = ProposalManager::new(None);
        let prop = mgr.create("Already resolved", "p", ProposalTrigger::User);
        mgr.approve(&prop.id).unwrap();

        // Second approval should fail.
        let result = mgr.approve(&prop.id);
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_not_found() {
        let mut mgr = ProposalManager::new(None);
        assert!(mgr.approve("nonexistent").is_err());
        assert!(mgr.reject("nonexistent").is_err());
    }

    #[test]
    fn test_manager_mark_failed() {
        let mut mgr = ProposalManager::new(None);
        let prop = mgr.create("Failure test", "p", ProposalTrigger::User);
        let failed = mgr.mark_failed(&prop.id).unwrap();
        assert_eq!(failed.status, ProposalStatus::Failed);
    }

    #[test]
    fn test_manager_mark_rolled_back() {
        let mut mgr = ProposalManager::new(None);
        let prop = mgr.create("Rollback test", "p", ProposalTrigger::Autonomous);
        mgr.approve(&prop.id).unwrap();
        let rolled = mgr.mark_rolled_back(&prop.id).unwrap();
        assert_eq!(rolled.status, ProposalStatus::RolledBack);
    }

    #[test]
    fn test_manager_proposals_dir() {
        let mgr = ProposalManager::new(Some("/custom/dir".to_string()));
        assert_eq!(mgr.proposals_dir(), "/custom/dir");

        let default_mgr = ProposalManager::default();
        assert_eq!(default_mgr.proposals_dir(), "./proposals");
    }
}
