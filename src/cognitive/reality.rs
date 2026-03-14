//! Reality Check — validates claims against observable system state.
//!
//! Ported from `sovereign_titan/cognitive/reality.py`.
//! Provides lightweight grounding checks for consciousness thoughts.

use serde::{Deserialize, Serialize};

/// Reality check verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RealityVerdict {
    /// Claim is consistent with observations.
    Consistent,
    /// Claim cannot be verified.
    Unverifiable,
    /// Claim contradicts observations.
    Contradicted,
}

/// A reality check record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealityCheck {
    pub claim: String,
    pub verdict: RealityVerdict,
    pub evidence: String,
    pub confidence: f64,
}

/// Reality check engine for grounding claims.
pub struct RealityEngine {
    /// Recent checks for dedup and learning.
    checks: Vec<RealityCheck>,
    /// Max history size.
    max_history: usize,
}

impl RealityEngine {
    /// Create a new reality engine.
    pub fn new(max_history: usize) -> Self {
        Self {
            checks: Vec::new(),
            max_history,
        }
    }

    /// Check a numeric claim against observed data.
    pub fn check_numeric(
        &mut self,
        claim: &str,
        claimed_value: f64,
        observed_value: f64,
        tolerance: f64,
    ) -> RealityVerdict {
        let diff = (claimed_value - observed_value).abs();
        let verdict = if diff <= tolerance {
            RealityVerdict::Consistent
        } else {
            RealityVerdict::Contradicted
        };

        let confidence = if diff == 0.0 {
            1.0
        } else {
            (1.0 - diff / (tolerance + diff)).max(0.0)
        };

        self.record(claim, verdict.clone(), &format!(
            "claimed={claimed_value}, observed={observed_value}, diff={diff:.2}"
        ), confidence);

        verdict
    }

    /// Check a boolean claim (process running, service available, etc.).
    pub fn check_boolean(
        &mut self,
        claim: &str,
        claimed: bool,
        observed: bool,
    ) -> RealityVerdict {
        let verdict = if claimed == observed {
            RealityVerdict::Consistent
        } else {
            RealityVerdict::Contradicted
        };

        self.record(claim, verdict.clone(), &format!(
            "claimed={claimed}, observed={observed}"
        ), 1.0);

        verdict
    }

    /// Check a string claim (presence, contains, equals).
    pub fn check_contains(
        &mut self,
        claim: &str,
        haystack: &str,
        needle: &str,
    ) -> RealityVerdict {
        let verdict = if haystack.to_lowercase().contains(&needle.to_lowercase()) {
            RealityVerdict::Consistent
        } else {
            RealityVerdict::Contradicted
        };

        self.record(claim, verdict.clone(), &format!(
            "searching for '{needle}' in data ({} chars)",
            haystack.len()
        ), 0.9);

        verdict
    }

    /// Record a check result.
    fn record(&mut self, claim: &str, verdict: RealityVerdict, evidence: &str, confidence: f64) {
        self.checks.push(RealityCheck {
            claim: claim.to_string(),
            verdict,
            evidence: evidence.to_string(),
            confidence,
        });

        if self.checks.len() > self.max_history {
            self.checks.drain(..self.checks.len() - self.max_history);
        }
    }

    /// Get recent checks.
    pub fn recent_checks(&self, n: usize) -> &[RealityCheck] {
        let start = self.checks.len().saturating_sub(n);
        &self.checks[start..]
    }

    /// Contradiction rate across all checks.
    pub fn contradiction_rate(&self) -> f64 {
        if self.checks.is_empty() {
            return 0.0;
        }
        let contradictions = self
            .checks
            .iter()
            .filter(|c| c.verdict == RealityVerdict::Contradicted)
            .count();
        contradictions as f64 / self.checks.len() as f64
    }

    /// Total checks performed.
    pub fn total_checks(&self) -> usize {
        self.checks.len()
    }

    /// Clear all check history.
    pub fn clear(&mut self) {
        self.checks.clear();
    }
}

impl Default for RealityEngine {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_consistent() {
        let mut engine = RealityEngine::default();
        let v = engine.check_numeric("CPU is 45%", 45.0, 44.5, 2.0);
        assert_eq!(v, RealityVerdict::Consistent);
    }

    #[test]
    fn test_numeric_contradicted() {
        let mut engine = RealityEngine::default();
        let v = engine.check_numeric("CPU is 90%", 90.0, 10.0, 5.0);
        assert_eq!(v, RealityVerdict::Contradicted);
    }

    #[test]
    fn test_boolean_consistent() {
        let mut engine = RealityEngine::default();
        let v = engine.check_boolean("Chrome is running", true, true);
        assert_eq!(v, RealityVerdict::Consistent);
    }

    #[test]
    fn test_boolean_contradicted() {
        let mut engine = RealityEngine::default();
        let v = engine.check_boolean("Chrome is running", true, false);
        assert_eq!(v, RealityVerdict::Contradicted);
    }

    #[test]
    fn test_contains_consistent() {
        let mut engine = RealityEngine::default();
        let v = engine.check_contains("Discord appears in processes", "chrome.exe\ndiscord.exe\nnotepad.exe", "discord");
        assert_eq!(v, RealityVerdict::Consistent);
    }

    #[test]
    fn test_contains_contradicted() {
        let mut engine = RealityEngine::default();
        let v = engine.check_contains("Discord appears in processes", "chrome.exe\nnotepad.exe", "discord");
        assert_eq!(v, RealityVerdict::Contradicted);
    }

    #[test]
    fn test_contradiction_rate() {
        let mut engine = RealityEngine::default();
        engine.check_boolean("test1", true, true); // consistent
        engine.check_boolean("test2", true, false); // contradicted
        assert!((engine.contradiction_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_recent_checks() {
        let mut engine = RealityEngine::default();
        engine.check_boolean("a", true, true);
        engine.check_boolean("b", true, true);
        let recent = engine.recent_checks(1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].claim, "b");
    }

    #[test]
    fn test_clear() {
        let mut engine = RealityEngine::default();
        engine.check_boolean("test", true, true);
        engine.clear();
        assert_eq!(engine.total_checks(), 0);
    }
}
