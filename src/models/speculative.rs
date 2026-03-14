//! Speculative Decoding — draft+verify acceleration (stub for vLLM integration).
//!
//! Implements the speculative decoding pattern where a small draft model
//! proposes tokens quickly and a larger target model verifies them in batch.
//! This module provides configuration, statistics tracking, and the
//! verification logic.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// SpeculativeConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for speculative decoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculativeConfig {
    /// Name or path of the target (verifier) model.
    pub target_model: String,
    /// Name or path of the draft (proposer) model.
    pub draft_model: String,
    /// Number of tokens the draft model proposes per step.
    pub num_speculative_tokens: usize,
    /// Minimum probability ratio for accepting a draft token.
    pub acceptance_threshold: f64,
    /// Maximum retries on rejection before falling back to serial decoding.
    pub max_retries: usize,
}

impl Default for SpeculativeConfig {
    fn default() -> Self {
        Self {
            target_model: String::new(),
            draft_model: String::new(),
            num_speculative_tokens: 5,
            acceptance_threshold: 0.9,
            max_retries: 3,
        }
    }
}

impl SpeculativeConfig {
    /// Create a config with the given model names.
    pub fn with_models(mut self, target: &str, draft: &str) -> Self {
        self.target_model = target.to_string();
        self.draft_model = draft.to_string();
        self
    }

    /// Set the number of speculative tokens.
    pub fn with_num_tokens(mut self, n: usize) -> Self {
        self.num_speculative_tokens = n;
        self
    }

    /// Set the acceptance threshold.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.acceptance_threshold = threshold.clamp(0.0, 1.0);
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SpeculativeStats
// ─────────────────────────────────────────────────────────────────────────────

/// Cumulative statistics for speculative decoding performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculativeStats {
    /// Total draft tokens proposed.
    pub total_tokens: u64,
    /// Draft tokens that were accepted by the verifier.
    pub accepted_tokens: u64,
    /// Draft tokens that were rejected.
    pub rejected_tokens: u64,
    /// Number of draft calls made.
    pub draft_calls: u64,
    /// Number of verify calls made.
    pub verify_calls: u64,
    /// Running acceptance rate.
    pub avg_acceptance_rate: f64,
    /// Estimated speedup over serial decoding.
    pub speedup_ratio: f64,
}

impl SpeculativeStats {
    /// Create fresh zero-valued stats.
    pub fn new() -> Self {
        Self {
            total_tokens: 0,
            accepted_tokens: 0,
            rejected_tokens: 0,
            draft_calls: 0,
            verify_calls: 0,
            avg_acceptance_rate: 0.0,
            speedup_ratio: 1.0,
        }
    }

    /// Current acceptance rate (accepted / total).
    pub fn acceptance_rate(&self) -> f64 {
        if self.total_tokens == 0 {
            0.0
        } else {
            self.accepted_tokens as f64 / self.total_tokens as f64
        }
    }

    /// Record a batch of drafted+verified tokens.
    pub fn record_batch(&mut self, drafted: usize, accepted: usize) {
        self.draft_calls += 1;
        self.verify_calls += 1;
        self.total_tokens += drafted as u64;
        self.accepted_tokens += accepted as u64;
        self.rejected_tokens += (drafted - accepted) as u64;
        self.avg_acceptance_rate = self.acceptance_rate();
        // Speedup = tokens produced per verify call (accepted + 1 serial token each call).
        if self.verify_calls > 0 {
            self.speedup_ratio =
                (self.accepted_tokens as f64 + self.verify_calls as f64) / self.verify_calls as f64;
        }
    }

    /// Whether speculative decoding is providing meaningful speedup.
    pub fn is_beneficial(&self) -> bool {
        self.speedup_ratio > 1.2 && self.acceptance_rate() > 0.3
    }
}

impl Default for SpeculativeStats {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DraftToken
// ─────────────────────────────────────────────────────────────────────────────

/// A token proposed by the draft model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftToken {
    /// Token ID in the vocabulary.
    pub token_id: u32,
    /// Text representation of the token.
    pub text: String,
    /// Log probability assigned by the draft model.
    pub log_prob: f64,
}

impl DraftToken {
    /// Create a new draft token.
    pub fn new(token_id: u32, text: &str, log_prob: f64) -> Self {
        Self {
            token_id,
            text: text.to_string(),
            log_prob,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SpeculativeDecoder
// ─────────────────────────────────────────────────────────────────────────────

/// Speculative decoder that manages draft-and-verify token generation.
pub struct SpeculativeDecoder {
    config: SpeculativeConfig,
    stats: SpeculativeStats,
    enabled: bool,
}

impl SpeculativeDecoder {
    /// Create a new decoder with the given config.
    pub fn new(config: SpeculativeConfig) -> Self {
        Self {
            config,
            stats: SpeculativeStats::new(),
            enabled: true,
        }
    }

    /// Whether speculative decoding is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable speculative decoding.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get the current config.
    pub fn config(&self) -> &SpeculativeConfig {
        &self.config
    }

    /// Get the current stats.
    pub fn stats(&self) -> &SpeculativeStats {
        &self.stats
    }

    /// Whether it makes sense to try speculative decoding right now.
    ///
    /// Returns `true` if enabled and acceptance rate is not catastrophically low.
    pub fn should_speculate(&self) -> bool {
        self.enabled
            && (self.stats.total_tokens == 0
                || self.stats.acceptance_rate() >= self.config.acceptance_threshold * 0.5)
    }

    /// Verify draft tokens against target model log probabilities.
    ///
    /// Returns a boolean mask indicating which tokens were accepted.
    pub fn verify_tokens(
        &mut self,
        draft_tokens: &[DraftToken],
        target_log_probs: &[f64],
    ) -> Vec<bool> {
        let mut accepted = Vec::new();
        let mut all_accepted = true;

        for (draft, &target_lp) in draft_tokens.iter().zip(target_log_probs) {
            let ratio = (target_lp - draft.log_prob).exp();
            let accept =
                ratio >= self.config.acceptance_threshold || (all_accepted && ratio >= 0.5);
            if !accept {
                all_accepted = false;
            }
            accepted.push(accept);
        }

        let num_accepted = accepted.iter().filter(|&&a| a).count();
        self.stats.record_batch(draft_tokens.len(), num_accepted);
        accepted
    }

    /// Reset all stats to zero.
    pub fn reset_stats(&mut self) {
        self.stats = SpeculativeStats::new();
    }
}

impl Default for SpeculativeDecoder {
    fn default() -> Self {
        Self::new(SpeculativeConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── SpeculativeConfig tests ───────────────────────────────────────────

    #[test]
    fn test_config_default() {
        let config = SpeculativeConfig::default();
        assert!(config.target_model.is_empty());
        assert!(config.draft_model.is_empty());
        assert_eq!(config.num_speculative_tokens, 5);
        assert!((config.acceptance_threshold - 0.9).abs() < f64::EPSILON);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_config_builder() {
        let config = SpeculativeConfig::default()
            .with_models("big-model", "small-model")
            .with_num_tokens(8)
            .with_threshold(0.7);
        assert_eq!(config.target_model, "big-model");
        assert_eq!(config.draft_model, "small-model");
        assert_eq!(config.num_speculative_tokens, 8);
        assert!((config.acceptance_threshold - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_config_threshold_clamped() {
        let config = SpeculativeConfig::default().with_threshold(1.5);
        assert!((config.acceptance_threshold - 1.0).abs() < f64::EPSILON);

        let config2 = SpeculativeConfig::default().with_threshold(-0.3);
        assert!((config2.acceptance_threshold - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_config_serialization() {
        let config = SpeculativeConfig::default().with_models("target", "draft");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: SpeculativeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.target_model, "target");
        assert_eq!(parsed.draft_model, "draft");
    }

    // ── SpeculativeStats tests ────────────────────────────────────────────

    #[test]
    fn test_stats_new() {
        let stats = SpeculativeStats::new();
        assert_eq!(stats.total_tokens, 0);
        assert_eq!(stats.accepted_tokens, 0);
        assert_eq!(stats.rejected_tokens, 0);
        assert!((stats.acceptance_rate() - 0.0).abs() < f64::EPSILON);
        assert!((stats.speedup_ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_record_batch() {
        let mut stats = SpeculativeStats::new();
        stats.record_batch(5, 4);

        assert_eq!(stats.total_tokens, 5);
        assert_eq!(stats.accepted_tokens, 4);
        assert_eq!(stats.rejected_tokens, 1);
        assert_eq!(stats.draft_calls, 1);
        assert_eq!(stats.verify_calls, 1);
        assert!((stats.acceptance_rate() - 0.8).abs() < f64::EPSILON);
        assert!(stats.speedup_ratio > 1.0);
    }

    #[test]
    fn test_stats_multiple_batches() {
        let mut stats = SpeculativeStats::new();
        stats.record_batch(5, 5); // 100% acceptance
        stats.record_batch(5, 3); // 60% acceptance

        assert_eq!(stats.total_tokens, 10);
        assert_eq!(stats.accepted_tokens, 8);
        assert_eq!(stats.rejected_tokens, 2);
        assert_eq!(stats.draft_calls, 2);
        assert!((stats.acceptance_rate() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_is_beneficial() {
        let mut stats = SpeculativeStats::new();
        // Good acceptance: beneficial.
        stats.record_batch(10, 8);
        assert!(stats.is_beneficial());

        // Reset and record bad acceptance: not beneficial.
        let mut stats2 = SpeculativeStats::new();
        stats2.record_batch(10, 1);
        assert!(!stats2.is_beneficial());
    }

    #[test]
    fn test_stats_serialization() {
        let mut stats = SpeculativeStats::new();
        stats.record_batch(5, 4);
        let json = serde_json::to_string(&stats).unwrap();
        let parsed: SpeculativeStats = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_tokens, 5);
        assert_eq!(parsed.accepted_tokens, 4);
    }

    // ── DraftToken tests ──────────────────────────────────────────────────

    #[test]
    fn test_draft_token_new() {
        let token = DraftToken::new(42, "hello", -0.5);
        assert_eq!(token.token_id, 42);
        assert_eq!(token.text, "hello");
        assert!((token.log_prob - (-0.5)).abs() < f64::EPSILON);
    }

    // ── SpeculativeDecoder tests ──────────────────────────────────────────

    #[test]
    fn test_decoder_default() {
        let decoder = SpeculativeDecoder::default();
        assert!(decoder.is_enabled());
        assert_eq!(decoder.stats().total_tokens, 0);
    }

    #[test]
    fn test_decoder_enable_disable() {
        let mut decoder = SpeculativeDecoder::default();
        assert!(decoder.is_enabled());
        decoder.set_enabled(false);
        assert!(!decoder.is_enabled());
        decoder.set_enabled(true);
        assert!(decoder.is_enabled());
    }

    #[test]
    fn test_decoder_should_speculate_initial() {
        let decoder = SpeculativeDecoder::default();
        // With no history, should speculate (no evidence against it).
        assert!(decoder.should_speculate());
    }

    #[test]
    fn test_decoder_should_speculate_disabled() {
        let mut decoder = SpeculativeDecoder::default();
        decoder.set_enabled(false);
        assert!(!decoder.should_speculate());
    }

    #[test]
    fn test_decoder_verify_tokens_all_accepted() {
        let config = SpeculativeConfig::default().with_threshold(0.5);
        let mut decoder = SpeculativeDecoder::new(config);

        let drafts = vec![
            DraftToken::new(1, "a", -1.0),
            DraftToken::new(2, "b", -1.0),
            DraftToken::new(3, "c", -1.0),
        ];
        // Target log probs identical => ratio = exp(0) = 1.0 >= 0.5
        let target_probs = vec![-1.0, -1.0, -1.0];
        let accepted = decoder.verify_tokens(&drafts, &target_probs);

        assert_eq!(accepted, vec![true, true, true]);
        assert_eq!(decoder.stats().accepted_tokens, 3);
    }

    #[test]
    fn test_decoder_verify_tokens_some_rejected() {
        let config = SpeculativeConfig::default().with_threshold(0.9);
        let mut decoder = SpeculativeDecoder::new(config);

        let drafts = vec![
            DraftToken::new(1, "a", -1.0),
            DraftToken::new(2, "b", -1.0),
        ];
        // First: target much worse than draft => ratio = exp(-5 - (-1)) = exp(-4) ~ 0.018
        // Second: target equal => ratio = 1.0
        let target_probs = vec![-5.0, -1.0];
        let accepted = decoder.verify_tokens(&drafts, &target_probs);

        // First should be rejected (ratio ~0.018 < 0.9, and since all_accepted starts true,
        // it checks ratio >= 0.5 => also false).
        assert!(!accepted[0]);
        // After first rejection, all_accepted is false, so second needs ratio >= 0.9.
        // ratio = exp(0) = 1.0 >= 0.9 => accepted.
        assert!(accepted[1]);
    }

    #[test]
    fn test_decoder_reset_stats() {
        let mut decoder = SpeculativeDecoder::default();
        let drafts = vec![DraftToken::new(1, "a", -1.0)];
        let target_probs = vec![-1.0];
        decoder.verify_tokens(&drafts, &target_probs);
        assert!(decoder.stats().total_tokens > 0);

        decoder.reset_stats();
        assert_eq!(decoder.stats().total_tokens, 0);
        assert_eq!(decoder.stats().accepted_tokens, 0);
    }

    #[test]
    fn test_decoder_config_access() {
        let config = SpeculativeConfig::default().with_models("target", "draft");
        let decoder = SpeculativeDecoder::new(config);
        assert_eq!(decoder.config().target_model, "target");
        assert_eq!(decoder.config().draft_model, "draft");
    }
}
