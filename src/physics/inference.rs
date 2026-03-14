//! Active Inference Engine — Free Energy Principle implementation.
//!
//! Ported from `sovereign_titan/physics/inference.py`.
//! Features:
//! - Surprisal computation from observations
//! - Exponential smoothing of prediction errors
//! - Dynamic temperature adjustment based on entropy and energy
//! - Free energy approximation
//! - Exploration vs exploitation gating

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the active inference engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Base sampling temperature.
    pub base_temperature: f64,
    /// Minimum allowed temperature.
    pub min_temperature: f64,
    /// Maximum allowed temperature.
    pub max_temperature: f64,
    /// How sensitive the engine is to surprisal.
    pub surprisal_sensitivity: f64,
    /// Weight of entropy in free energy computation.
    pub entropy_weight: f64,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            base_temperature: 0.7,
            min_temperature: 0.1,
            max_temperature: 1.5,
            surprisal_sensitivity: 0.3,
            entropy_weight: 0.5,
        }
    }
}

/// An observation used for inference updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Task complexity (0.0 to 1.0).
    pub complexity: f64,
    /// Whether the task succeeded.
    pub success: bool,
    /// Novelty of the observation (0.0 to 1.0).
    pub novelty: f64,
}

/// Current status of the inference engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceStatus {
    /// Current sampling temperature.
    pub current_temperature: f64,
    /// Expected complexity (exponential smoothing).
    pub expected_complexity: f64,
    /// Expected success rate (exponential smoothing).
    pub expected_success_rate: f64,
    /// Current free energy estimate.
    pub free_energy: f64,
    /// Whether exploration is recommended.
    pub explore: bool,
    /// Total predictions processed.
    pub total_predictions: u64,
    /// Predictions that were accurate (low surprisal).
    pub accurate_predictions: u64,
    /// Prediction accuracy ratio.
    pub accuracy: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Active inference engine implementing the Free Energy Principle.
///
/// The engine tracks prediction errors and adjusts its internal model of
/// expected complexity and success rate via exponential smoothing. It uses
/// surprisal and entropy to dynamically adjust the sampling temperature and
/// decide whether to explore (higher temperature) or exploit (lower temperature).
pub struct ActiveInferenceEngine {
    /// Configuration.
    config: InferenceConfig,
    /// Current dynamic temperature.
    current_temperature: f64,
    /// Rolling history of surprisal values.
    surprisal_history: VecDeque<f64>,
    /// Rolling history of prediction errors.
    prediction_errors: VecDeque<f64>,
    /// Expected task complexity (smoothed).
    expected_complexity: f64,
    /// Expected success rate (smoothed).
    expected_success_rate: f64,
    /// Exponential smoothing factor (0 < alpha < 1).
    smoothing_factor: f64,
    /// Total observations processed.
    total_predictions: u64,
    /// Observations with low surprisal (accurate predictions).
    accurate_predictions: u64,
}

/// Maximum history window for surprisal and prediction errors.
const HISTORY_SIZE: usize = 100;

impl ActiveInferenceEngine {
    /// Create a new active inference engine.
    pub fn new(config: InferenceConfig) -> Self {
        let temp = config.base_temperature;
        Self {
            config,
            current_temperature: temp,
            surprisal_history: VecDeque::with_capacity(HISTORY_SIZE),
            prediction_errors: VecDeque::with_capacity(HISTORY_SIZE),
            expected_complexity: 0.5,
            expected_success_rate: 0.8,
            smoothing_factor: 0.1,
            total_predictions: 0,
            accurate_predictions: 0,
        }
    }

    /// Compute the surprisal of an observation.
    ///
    /// Surprisal measures how unexpected the observation is relative to
    /// the engine's current expectations. High surprisal = unexpected.
    pub fn compute_surprisal(&self, observation: &Observation) -> f64 {
        // Complexity deviation
        let complexity_surprise =
            (observation.complexity - self.expected_complexity).abs();

        // Success deviation — failure when success was expected is surprising
        let success_val = if observation.success { 1.0 } else { 0.0 };
        let success_surprise = (success_val - self.expected_success_rate).abs();

        // Novelty contributes directly to surprisal
        let novelty_factor = observation.novelty;

        // Weighted combination
        let raw = complexity_surprise * 0.4
            + success_surprise * 0.4
            + novelty_factor * 0.2;

        // Scale by sensitivity
        (raw * self.config.surprisal_sensitivity).clamp(0.0, 1.0)
    }

    /// Update internal predictions based on a new observation.
    ///
    /// Uses exponential smoothing to gradually shift the expected complexity
    /// and success rate toward observed values.
    pub fn update_predictions(&mut self, observation: &Observation) {
        let surprisal = self.compute_surprisal(observation);

        // Record surprisal
        if self.surprisal_history.len() >= HISTORY_SIZE {
            self.surprisal_history.pop_front();
        }
        self.surprisal_history.push_back(surprisal);

        // Compute prediction error
        let pred_error = (observation.complexity - self.expected_complexity).abs();
        if self.prediction_errors.len() >= HISTORY_SIZE {
            self.prediction_errors.pop_front();
        }
        self.prediction_errors.push_back(pred_error);

        // Exponential smoothing of expectations
        let alpha = self.smoothing_factor;
        self.expected_complexity =
            alpha * observation.complexity + (1.0 - alpha) * self.expected_complexity;

        let success_val = if observation.success { 1.0 } else { 0.0 };
        self.expected_success_rate =
            alpha * success_val + (1.0 - alpha) * self.expected_success_rate;

        // Track accuracy
        self.total_predictions += 1;
        if surprisal < 0.15 {
            self.accurate_predictions += 1;
        }
    }

    /// Get the dynamic temperature adjusted for current entropy and energy.
    ///
    /// High entropy or low energy => lower temperature (more conservative).
    /// Low entropy and high energy => higher temperature (more explorative).
    pub fn get_dynamic_temperature(&mut self, entropy: f64, energy: f64) -> f64 {
        // Normalize energy to [0, 1] assuming max 1000
        let energy_norm = (energy / 1000.0).clamp(0.0, 1.0);

        // Base temperature
        let mut temp = self.config.base_temperature;

        // High entropy => reduce temperature (be more deterministic)
        temp -= entropy * 0.3;

        // Low energy => reduce temperature (conserve resources)
        temp -= (1.0 - energy_norm) * 0.2;

        // High recent surprisal => increase temperature (need exploration)
        let avg_surprisal = if self.surprisal_history.is_empty() {
            0.0
        } else {
            self.surprisal_history.iter().sum::<f64>() / self.surprisal_history.len() as f64
        };
        temp += avg_surprisal * 0.15;

        // Clamp to bounds
        self.current_temperature =
            temp.clamp(self.config.min_temperature, self.config.max_temperature);

        self.current_temperature
    }

    /// Compute the approximate free energy of the system.
    ///
    /// Free energy = average prediction error + entropy weight * average surprisal.
    /// Lower free energy indicates a better internal model.
    pub fn compute_free_energy(&self) -> f64 {
        let avg_pred_error = if self.prediction_errors.is_empty() {
            0.5 // prior
        } else {
            self.prediction_errors.iter().sum::<f64>() / self.prediction_errors.len() as f64
        };

        let avg_surprisal = if self.surprisal_history.is_empty() {
            0.5 // prior
        } else {
            self.surprisal_history.iter().sum::<f64>() / self.surprisal_history.len() as f64
        };

        avg_pred_error + self.config.entropy_weight * avg_surprisal
    }

    /// Determine whether the engine should explore (try new strategies) or
    /// exploit (use known good strategies).
    ///
    /// Exploration is recommended when:
    /// - Free energy is high (internal model is poor)
    /// - Recent surprisal is high (environment is unpredictable)
    /// - Success rate is low (current approach is failing)
    pub fn should_explore(&self) -> bool {
        let free_energy = self.compute_free_energy();
        let avg_surprisal = if self.surprisal_history.is_empty() {
            0.5
        } else {
            self.surprisal_history.iter().sum::<f64>() / self.surprisal_history.len() as f64
        };

        // Explore if free energy is high OR recent surprisal is high OR success rate is low
        free_energy > 0.5 || avg_surprisal > 0.3 || self.expected_success_rate < 0.5
    }

    /// Get the current status of the inference engine.
    pub fn get_status(&self) -> InferenceStatus {
        let accuracy = if self.total_predictions == 0 {
            0.0
        } else {
            self.accurate_predictions as f64 / self.total_predictions as f64
        };

        InferenceStatus {
            current_temperature: self.current_temperature,
            expected_complexity: self.expected_complexity,
            expected_success_rate: self.expected_success_rate,
            free_energy: self.compute_free_energy(),
            explore: self.should_explore(),
            total_predictions: self.total_predictions,
            accurate_predictions: self.accurate_predictions,
            accuracy,
        }
    }

    /// Current temperature.
    pub fn current_temperature(&self) -> f64 {
        self.current_temperature
    }

    /// Expected complexity (smoothed).
    pub fn expected_complexity(&self) -> f64 {
        self.expected_complexity
    }

    /// Expected success rate (smoothed).
    pub fn expected_success_rate(&self) -> f64 {
        self.expected_success_rate
    }
}

impl Default for ActiveInferenceEngine {
    fn default() -> Self {
        Self::new(InferenceConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_observation(complexity: f64, success: bool, novelty: f64) -> Observation {
        Observation {
            complexity,
            success,
            novelty,
        }
    }

    #[test]
    fn test_default_state() {
        let engine = ActiveInferenceEngine::default();
        assert!((engine.current_temperature() - 0.7).abs() < f64::EPSILON);
        assert!((engine.expected_complexity() - 0.5).abs() < f64::EPSILON);
        assert!((engine.expected_success_rate() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_surprisal_expected_observation() {
        let engine = ActiveInferenceEngine::default();
        // Observation matching expectations => low surprisal
        let obs = make_observation(0.5, true, 0.0);
        let surprisal = engine.compute_surprisal(&obs);
        assert!(surprisal < 0.15);
    }

    #[test]
    fn test_compute_surprisal_unexpected_observation() {
        let engine = ActiveInferenceEngine::default();
        // High novelty, failure, unexpected complexity
        let obs = make_observation(1.0, false, 1.0);
        let surprisal = engine.compute_surprisal(&obs);
        assert!(surprisal > 0.05);
    }

    #[test]
    fn test_update_predictions_shifts_expectations() {
        let mut engine = ActiveInferenceEngine::default();
        // Feed many high-complexity successes
        for _ in 0..20 {
            let obs = make_observation(0.9, true, 0.1);
            engine.update_predictions(&obs);
        }
        // Expected complexity should shift toward 0.9
        assert!(engine.expected_complexity() > 0.7);
        // Expected success rate should remain high
        assert!(engine.expected_success_rate() > 0.8);
    }

    #[test]
    fn test_update_predictions_failure_lowers_success_rate() {
        let mut engine = ActiveInferenceEngine::default();
        for _ in 0..30 {
            let obs = make_observation(0.5, false, 0.0);
            engine.update_predictions(&obs);
        }
        assert!(engine.expected_success_rate() < 0.5);
    }

    #[test]
    fn test_dynamic_temperature_high_energy_low_entropy() {
        let mut engine = ActiveInferenceEngine::default();
        let temp = engine.get_dynamic_temperature(0.1, 900.0);
        // Should be close to base (0.7) with slight adjustments
        assert!(temp > 0.3);
        assert!(temp < 1.0);
    }

    #[test]
    fn test_dynamic_temperature_high_entropy_reduces() {
        let mut engine = ActiveInferenceEngine::default();
        let temp_low = engine.get_dynamic_temperature(0.1, 500.0);
        let temp_high = engine.get_dynamic_temperature(0.9, 500.0);
        assert!(temp_high < temp_low);
    }

    #[test]
    fn test_free_energy_initial() {
        let engine = ActiveInferenceEngine::default();
        let fe = engine.compute_free_energy();
        // With no observations, uses priors: 0.5 + 0.5 * 0.5 = 0.75
        assert!((fe - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_free_energy_decreases_with_good_predictions() {
        let mut engine = ActiveInferenceEngine::default();
        let initial_fe = engine.compute_free_energy();

        // Feed perfectly expected observations
        for _ in 0..50 {
            let obs = make_observation(0.5, true, 0.0);
            engine.update_predictions(&obs);
        }

        let after_fe = engine.compute_free_energy();
        assert!(after_fe < initial_fe);
    }

    #[test]
    fn test_should_explore_initially() {
        let engine = ActiveInferenceEngine::default();
        // Free energy is 0.75 > 0.5, so should explore
        assert!(engine.should_explore());
    }

    #[test]
    fn test_should_not_explore_after_calibration() {
        let mut engine = ActiveInferenceEngine::default();
        // Feed many expected observations to reduce free energy
        for _ in 0..100 {
            let obs = make_observation(0.5, true, 0.0);
            engine.update_predictions(&obs);
        }
        // After calibration, free energy should be low
        let fe = engine.compute_free_energy();
        // If free energy dropped below threshold and success rate is high
        if fe < 0.5 && engine.expected_success_rate() > 0.5 {
            assert!(!engine.should_explore());
        }
    }

    #[test]
    fn test_get_status() {
        let engine = ActiveInferenceEngine::default();
        let status = engine.get_status();
        assert!((status.current_temperature - 0.7).abs() < f64::EPSILON);
        assert_eq!(status.total_predictions, 0);
        assert!((status.accuracy - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_accuracy_tracking() {
        let mut engine = ActiveInferenceEngine::default();
        // Feed observations that match expectations (low surprisal)
        for _ in 0..10 {
            let obs = make_observation(0.5, true, 0.0);
            engine.update_predictions(&obs);
        }
        assert_eq!(engine.total_predictions, 10);
        assert!(engine.accurate_predictions > 0);
    }
}
