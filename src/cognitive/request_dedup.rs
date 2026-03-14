//! Request Deduplication — prevents duplicate concurrent requests.
//!
//! Ported from `sovereign_titan/cognitive/request_dedup.py`.
//! Features:
//! - Leader election: first submitter becomes leader, subsequent ones wait
//! - Completed-request cache with TTL
//! - Deduplication counting for metrics

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// A request that is currently being processed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub key: String,
    pub created_at: f64,
    pub requester_count: usize,
    pub resolved: bool,
    pub result: Option<String>,
}

/// The result of submitting a request key to the dedup engine.
#[derive(Debug, Clone, PartialEq)]
pub enum DedupResult {
    /// This caller is the leader (first submitter) — should execute the work.
    Leader,
    /// Another caller is already executing — wait for the result.
    Waiting,
    /// A cached result was found within the TTL.
    Cached(String),
}

/// Deduplication engine that coalesces identical concurrent requests.
pub struct RequestDedup {
    pending: HashMap<String, PendingRequest>,
    completed_cache: HashMap<String, (String, f64)>,
    cache_ttl_secs: f64,
    dedup_count: u64,
}

impl RequestDedup {
    /// Create a new dedup engine with the given cache TTL in seconds.
    pub fn new(cache_ttl_secs: f64) -> Self {
        Self {
            pending: HashMap::new(),
            completed_cache: HashMap::new(),
            cache_ttl_secs,
            dedup_count: 0,
        }
    }

    /// Submit a request key. Returns whether this caller is the leader,
    /// should wait, or can use a cached result.
    pub fn submit(&mut self, key: &str) -> DedupResult {
        let now = now_secs();

        // Check completed cache first
        if let Some((result, ts)) = self.completed_cache.get(key) {
            if now - ts < self.cache_ttl_secs {
                return DedupResult::Cached(result.clone());
            } else {
                // Expired — remove
                self.completed_cache.remove(&key.to_string());
            }
        }

        // Check if another caller is already working on this
        if let Some(req) = self.pending.get_mut(key) {
            req.requester_count += 1;
            self.dedup_count += 1;
            return DedupResult::Waiting;
        }

        // New request — this caller is the leader
        self.pending.insert(
            key.to_string(),
            PendingRequest {
                key: key.to_string(),
                created_at: now,
                requester_count: 1,
                resolved: false,
                result: None,
            },
        );
        DedupResult::Leader
    }

    /// Resolve a pending request with a result. Returns the number of
    /// callers that were waiting (including the leader).
    pub fn resolve(&mut self, key: &str, result: &str) -> usize {
        let now = now_secs();
        let waiters = if let Some(req) = self.pending.remove(key) {
            req.requester_count
        } else {
            0
        };
        self.completed_cache
            .insert(key.to_string(), (result.to_string(), now));
        waiters
    }

    /// Whether a request is currently pending.
    pub fn is_pending(&self, key: &str) -> bool {
        self.pending.contains_key(key)
    }

    /// Number of currently pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Total number of deduplicated (non-leader) submissions.
    pub fn dedup_count(&self) -> u64 {
        self.dedup_count
    }

    /// Number of entries in the completed cache.
    pub fn cache_size(&self) -> usize {
        self.completed_cache.len()
    }

    /// Remove expired entries from the completed cache.
    pub fn cleanup_expired(&mut self) {
        let now = now_secs();
        self.completed_cache
            .retain(|_, (_, ts)| now - *ts < self.cache_ttl_secs);
    }

    /// Remove stale pending requests older than the given age in seconds.
    pub fn cleanup_stale_pending(&mut self, max_age_secs: f64) {
        let now = now_secs();
        self.pending
            .retain(|_, req| now - req.created_at < max_age_secs);
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.completed_cache.clear();
    }

    /// Get the requester count for a pending request.
    pub fn requester_count(&self, key: &str) -> usize {
        self.pending
            .get(key)
            .map_or(0, |req| req.requester_count)
    }
}

impl Default for RequestDedup {
    fn default() -> Self {
        Self::new(60.0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_submit_is_leader() {
        let mut dd = RequestDedup::new(60.0);
        assert_eq!(dd.submit("k1"), DedupResult::Leader);
    }

    #[test]
    fn test_second_submit_is_waiting() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        assert_eq!(dd.submit("k1"), DedupResult::Waiting);
    }

    #[test]
    fn test_different_keys_both_leader() {
        let mut dd = RequestDedup::new(60.0);
        assert_eq!(dd.submit("k1"), DedupResult::Leader);
        assert_eq!(dd.submit("k2"), DedupResult::Leader);
    }

    #[test]
    fn test_resolve_returns_waiters() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        dd.submit("k1");
        dd.submit("k1");
        let waiters = dd.resolve("k1", "result_data");
        assert_eq!(waiters, 3); // 1 leader + 2 waiting
    }

    #[test]
    fn test_resolve_populates_cache() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        dd.resolve("k1", "answer");
        assert_eq!(dd.cache_size(), 1);
        assert!(!dd.is_pending("k1"));
    }

    #[test]
    fn test_cached_result() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        dd.resolve("k1", "cached_answer");
        // Next submit should get cached result
        assert_eq!(dd.submit("k1"), DedupResult::Cached("cached_answer".to_string()));
    }

    #[test]
    fn test_dedup_count() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        dd.submit("k1");
        dd.submit("k1");
        assert_eq!(dd.dedup_count(), 2); // 2 non-leader submissions
    }

    #[test]
    fn test_pending_count() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("a");
        dd.submit("b");
        assert_eq!(dd.pending_count(), 2);
        dd.resolve("a", "done");
        assert_eq!(dd.pending_count(), 1);
    }

    #[test]
    fn test_clear() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        dd.resolve("k1", "val");
        dd.submit("k2");
        dd.clear();
        assert_eq!(dd.pending_count(), 0);
        assert_eq!(dd.cache_size(), 0);
    }

    #[test]
    fn test_requester_count() {
        let mut dd = RequestDedup::new(60.0);
        dd.submit("k1");
        dd.submit("k1");
        assert_eq!(dd.requester_count("k1"), 2);
        assert_eq!(dd.requester_count("nonexistent"), 0);
    }

    #[test]
    fn test_resolve_nonexistent_returns_zero() {
        let mut dd = RequestDedup::new(60.0);
        assert_eq!(dd.resolve("nope", "val"), 0);
    }

    #[test]
    fn test_default_ttl() {
        let dd = RequestDedup::default();
        assert!((dd.cache_ttl_secs - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_expired_cache_not_returned() {
        let mut dd = RequestDedup::new(0.0); // 0-second TTL = always expired
        dd.submit("k1");
        dd.resolve("k1", "answer");
        // With TTL=0, the cache entry is immediately expired
        let result = dd.submit("k1");
        // Should be Leader again, not Cached
        assert_eq!(result, DedupResult::Leader);
    }
}
