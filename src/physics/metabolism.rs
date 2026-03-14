//! Metabolic Engine — thermodynamic energy/entropy engine.
//!
//! Ported from `sovereign_titan/physics/metabolism.py`.
//! Features:
//! - Energy tracking with time-based regeneration
//! - Entropy accumulation and decay
//! - Automatic state transitions based on energy/entropy thresholds
//! - Task acceptance gating based on available energy
//! - ASCII energy bar visualization

use std::collections::VecDeque;
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// Metabolic state of the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetabolicState {
    /// Peak performance — energy is high, entropy is low.
    Optimal,
    /// Normal operation — moderate energy levels.
    Active,
    /// High entropy — performance is degraded.
    Fatigued,
    /// Low energy — non-essential tasks are rejected.
    Conservation,
    /// High entropy induced sleep — consolidation mode.
    Sleeping,
    /// Dangerously low energy — only critical tasks accepted.
    Critical,
}

impl std::fmt::Display for MetabolicState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetabolicState::Optimal => write!(f, "OPTIMAL"),
            MetabolicState::Active => write!(f, "ACTIVE"),
            MetabolicState::Fatigued => write!(f, "FATIGUED"),
            MetabolicState::Conservation => write!(f, "CONSERVATION"),
            MetabolicState::Sleeping => write!(f, "SLEEPING"),
            MetabolicState::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Snapshot of current metabolic status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetabolicStatus {
    pub energy: f64,
    pub entropy: f64,
    pub state: MetabolicState,
    pub last_update: f64,
    pub energy_trend: f64,
    pub entropy_trend: f64,
}

/// Configuration for the metabolic engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetabolicConfig {
    pub initial_energy: f64,
    pub max_energy: f64,
    pub regen_rate: f64,
    pub base_consumption: f64,
    pub complexity_multiplier: f64,
    pub entropy_decay: f64,
    pub entropy_rise_on_error: f64,
    pub conservation_threshold: f64,
    pub critical_threshold: f64,
    pub sleep_entropy_threshold: f64,
    pub fatigue_threshold: f64,
}

impl Default for MetabolicConfig {
    fn default() -> Self {
        Self {
            initial_energy: 850.0,
            max_energy: 1000.0,
            regen_rate: 5.0,
            base_consumption: 10.0,
            complexity_multiplier: 50.0,
            entropy_decay: 0.02,
            entropy_rise_on_error: 0.05,
            conservation_threshold: 200.0,
            critical_threshold: 100.0,
            sleep_entropy_threshold: 0.8,
            fatigue_threshold: 0.5,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Thermodynamic metabolic engine that tracks energy, entropy, and state.
pub struct MetabolicEngine {
    /// Current energy level.
    energy: f64,
    /// Maximum energy capacity.
    max_energy: f64,
    /// Current entropy (0.0 = ordered, 1.0 = chaotic).
    entropy: f64,
    /// Current metabolic state.
    state: MetabolicState,
    /// Energy regenerated per second.
    regeneration_rate: f64,
    /// Base energy cost per task.
    base_consumption: f64,
    /// Extra cost scaling with complexity.
    complexity_multiplier: f64,
    /// Entropy decay per second (natural recovery).
    entropy_decay: f64,
    /// Entropy increase on error.
    entropy_rise_on_error: f64,
    /// Energy below which conservation mode activates.
    conservation_threshold: f64,
    /// Energy below which critical mode activates.
    critical_threshold: f64,
    /// Entropy above which sleep mode activates.
    sleep_entropy_threshold: f64,
    /// Entropy above which fatigue mode activates.
    fatigue_threshold: f64,
    /// Rolling window of energy values for trend analysis.
    energy_history: VecDeque<f64>,
    /// Rolling window of entropy values for trend analysis.
    entropy_history: VecDeque<f64>,
    /// Timestamp of the last update.
    last_update: Instant,
    /// Total tasks completed.
    tasks_completed: u64,
    /// Total tasks rejected.
    tasks_rejected: u64,
    /// Cumulative energy consumed.
    total_energy_consumed: f64,
}

impl MetabolicEngine {
    /// Maximum history window size.
    const HISTORY_SIZE: usize = 50;

    /// Create a new metabolic engine with the given configuration.
    pub fn new(config: MetabolicConfig) -> Self {
        let mut engine = Self {
            energy: config.initial_energy,
            max_energy: config.max_energy,
            entropy: 0.0,
            state: MetabolicState::Optimal,
            regeneration_rate: config.regen_rate,
            base_consumption: config.base_consumption,
            complexity_multiplier: config.complexity_multiplier,
            entropy_decay: config.entropy_decay,
            entropy_rise_on_error: config.entropy_rise_on_error,
            conservation_threshold: config.conservation_threshold,
            critical_threshold: config.critical_threshold,
            sleep_entropy_threshold: config.sleep_entropy_threshold,
            fatigue_threshold: config.fatigue_threshold,
            energy_history: VecDeque::with_capacity(Self::HISTORY_SIZE),
            entropy_history: VecDeque::with_capacity(Self::HISTORY_SIZE),
            last_update: Instant::now(),
            tasks_completed: 0,
            tasks_rejected: 0,
            total_energy_consumed: 0.0,
        };
        engine.state = engine.compute_state();
        engine
    }

    /// Tick the engine — apply time-based regeneration, entropy decay, and
    /// update the metabolic state. Returns the current status snapshot.
    pub fn update(&mut self) -> MetabolicStatus {
        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f64();
        self.last_update = now;

        // Regenerate energy over elapsed time (capped at max)
        if self.state != MetabolicState::Sleeping {
            self.energy = (self.energy + self.regeneration_rate * dt).min(self.max_energy);
        } else {
            // Sleep mode regenerates at 3x rate
            self.energy = (self.energy + self.regeneration_rate * dt * 3.0).min(self.max_energy);
        }

        // Entropy naturally decays over time
        self.entropy = (self.entropy - self.entropy_decay * dt).max(0.0);

        // Record history for trend analysis
        if self.energy_history.len() >= Self::HISTORY_SIZE {
            self.energy_history.pop_front();
        }
        self.energy_history.push_back(self.energy);

        if self.entropy_history.len() >= Self::HISTORY_SIZE {
            self.entropy_history.pop_front();
        }
        self.entropy_history.push_back(self.entropy);

        // State transitions (priority order)
        self.state = self.compute_state();

        self.get_status()
    }

    /// Compute the current state based on energy and entropy thresholds.
    /// Priority: Sleeping > Critical > Conservation > Fatigued > Active > Optimal
    fn compute_state(&self) -> MetabolicState {
        if self.entropy >= self.sleep_entropy_threshold {
            MetabolicState::Sleeping
        } else if self.energy < self.critical_threshold {
            MetabolicState::Critical
        } else if self.energy < self.conservation_threshold {
            MetabolicState::Conservation
        } else if self.entropy >= self.fatigue_threshold {
            MetabolicState::Fatigued
        } else if self.energy < 600.0 {
            MetabolicState::Active
        } else {
            MetabolicState::Optimal
        }
    }

    /// Consume energy for a task. Returns the actual energy consumed.
    pub fn consume_energy(&mut self, base_cost: f64, complexity: f64) -> f64 {
        let cost = base_cost + self.complexity_multiplier * complexity;
        let actual = cost.min(self.energy);
        self.energy -= actual;
        self.total_energy_consumed += actual;
        self.tasks_completed += 1;

        // Update state after consumption
        self.state = self.compute_state();

        actual
    }

    /// Estimate the energy cost for a task of the given complexity.
    pub fn estimate_cost(&self, complexity: f64) -> f64 {
        self.base_consumption + self.complexity_multiplier * complexity
    }

    /// Check whether the engine can accept a task with the estimated cost.
    /// Returns `(accepted, reason)`.
    pub fn can_accept_task(&mut self, estimated_cost: f64) -> (bool, String) {
        match self.state {
            MetabolicState::Sleeping => {
                self.tasks_rejected += 1;
                (false, "Engine is in SLEEPING state — consolidation in progress".to_string())
            }
            MetabolicState::Critical => {
                if estimated_cost < self.energy * 0.5 {
                    (true, "CRITICAL mode — only small tasks accepted".to_string())
                } else {
                    self.tasks_rejected += 1;
                    (false, format!(
                        "CRITICAL mode — task cost {:.1} exceeds safety margin (energy={:.1})",
                        estimated_cost, self.energy
                    ))
                }
            }
            MetabolicState::Conservation => {
                if estimated_cost <= self.energy * 0.8 {
                    (true, "CONSERVATION mode — task accepted within budget".to_string())
                } else {
                    self.tasks_rejected += 1;
                    (false, format!(
                        "CONSERVATION mode — task cost {:.1} too high (energy={:.1})",
                        estimated_cost, self.energy
                    ))
                }
            }
            _ => {
                if estimated_cost <= self.energy {
                    (true, format!("{} mode — task accepted", self.state))
                } else {
                    self.tasks_rejected += 1;
                    (false, format!(
                        "Insufficient energy: need {:.1}, have {:.1}",
                        estimated_cost, self.energy
                    ))
                }
            }
        }
    }

    /// Record an error — increases entropy based on severity (0.0 to 1.0).
    pub fn record_error(&mut self, severity: f64) {
        let severity = severity.clamp(0.0, 1.0);
        self.entropy = (self.entropy + self.entropy_rise_on_error * severity).min(1.0);
        self.state = self.compute_state();
    }

    /// Record a success — decreases entropy based on magnitude (0.0 to 1.0).
    pub fn record_success(&mut self, magnitude: f64) {
        let magnitude = magnitude.clamp(0.0, 1.0);
        self.entropy = (self.entropy - self.entropy_rise_on_error * magnitude * 0.5).max(0.0);
        self.state = self.compute_state();
    }

    /// Force the engine into sleep/consolidation mode for the given duration
    /// in seconds. During sleep, entropy decays faster and energy regenerates
    /// at 3x rate.
    pub fn force_sleep(&mut self, duration: f64) {
        self.entropy = (self.entropy + 0.01).min(self.sleep_entropy_threshold);
        // The actual sleep duration is managed externally; we just set the
        // entropy high enough to trigger sleeping state.
        self.entropy = self.sleep_entropy_threshold;
        self.state = MetabolicState::Sleeping;
        // The caller should wait `duration` seconds then call update() to
        // allow the engine to transition out of sleep.
        let _ = duration; // used by caller
    }

    /// Get the current metabolic status snapshot.
    pub fn get_status(&self) -> MetabolicStatus {
        MetabolicStatus {
            energy: self.energy,
            entropy: self.entropy,
            state: self.state.clone(),
            last_update: self.last_update.elapsed().as_secs_f64(),
            energy_trend: self.compute_energy_trend(),
            entropy_trend: self.compute_entropy_trend(),
        }
    }

    /// Compute the energy trend from the rolling history.
    fn compute_energy_trend(&self) -> f64 {
        if self.energy_history.len() < 2 {
            return 0.0;
        }
        let first = self.energy_history.front().copied().unwrap_or(0.0);
        let last = self.energy_history.back().copied().unwrap_or(0.0);
        last - first
    }

    /// Compute the entropy trend from the rolling history.
    fn compute_entropy_trend(&self) -> f64 {
        if self.entropy_history.len() < 2 {
            return 0.0;
        }
        let first = self.entropy_history.front().copied().unwrap_or(0.0);
        let last = self.entropy_history.back().copied().unwrap_or(0.0);
        last - first
    }

    /// Generate an ASCII energy bar of the given width.
    ///
    /// Example output: `[████████████░░░░░░░░] 62% (620/1000)`
    pub fn energy_bar(&self, width: usize) -> String {
        let ratio = (self.energy / self.max_energy).clamp(0.0, 1.0);
        let filled = (ratio * width as f64).round() as usize;
        let empty = width.saturating_sub(filled);

        let bar_char = if ratio > 0.6 {
            '\u{2588}' // full block
        } else if ratio > 0.3 {
            '\u{2593}' // dark shade
        } else {
            '\u{2591}' // light shade
        };

        let bar: String = std::iter::repeat(bar_char).take(filled).collect();
        let blank: String = std::iter::repeat('\u{2591}').take(empty).collect();
        let pct = (ratio * 100.0).round() as u32;

        format!(
            "[{}{}] {}% ({:.0}/{:.0}) [{}]",
            bar, blank, pct, self.energy, self.max_energy, self.state
        )
    }

    /// Current energy level.
    pub fn energy(&self) -> f64 {
        self.energy
    }

    /// Current entropy level.
    pub fn entropy(&self) -> f64 {
        self.entropy
    }

    /// Current metabolic state.
    pub fn current_state(&self) -> &MetabolicState {
        &self.state
    }

    /// Total tasks completed.
    pub fn tasks_completed(&self) -> u64 {
        self.tasks_completed
    }

    /// Total tasks rejected.
    pub fn tasks_rejected(&self) -> u64 {
        self.tasks_rejected
    }

    /// Total energy consumed since creation.
    pub fn total_energy_consumed(&self) -> f64 {
        self.total_energy_consumed
    }
}

impl Default for MetabolicEngine {
    fn default() -> Self {
        Self::new(MetabolicConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state_is_optimal() {
        let engine = MetabolicEngine::default();
        assert_eq!(*engine.current_state(), MetabolicState::Optimal);
        assert!((engine.energy() - 850.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_update_returns_status() {
        let mut engine = MetabolicEngine::default();
        let status = engine.update();
        assert_eq!(status.state, MetabolicState::Optimal);
        assert!(status.energy > 0.0);
        assert!(status.entropy >= 0.0);
    }

    #[test]
    fn test_consume_energy_reduces_level() {
        let mut engine = MetabolicEngine::default();
        let before = engine.energy();
        let consumed = engine.consume_energy(50.0, 0.5);
        assert!(consumed > 0.0);
        assert!(engine.energy() < before);
    }

    #[test]
    fn test_consume_energy_cannot_go_negative() {
        let mut config = MetabolicConfig::default();
        config.initial_energy = 10.0;
        let mut engine = MetabolicEngine::new(config);
        let consumed = engine.consume_energy(100.0, 1.0);
        assert!((consumed - 10.0).abs() < f64::EPSILON);
        assert!((engine.energy() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_estimate_cost() {
        let engine = MetabolicEngine::default();
        let cost = engine.estimate_cost(0.5);
        // base_consumption(10) + complexity_multiplier(50) * 0.5 = 35
        assert!((cost - 35.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_critical_state_on_low_energy() {
        let mut config = MetabolicConfig::default();
        config.initial_energy = 50.0;
        let engine = MetabolicEngine::new(config);
        assert_eq!(*engine.current_state(), MetabolicState::Critical);
    }

    #[test]
    fn test_conservation_state() {
        let mut config = MetabolicConfig::default();
        config.initial_energy = 150.0;
        let engine = MetabolicEngine::new(config);
        assert_eq!(*engine.current_state(), MetabolicState::Conservation);
    }

    #[test]
    fn test_record_error_increases_entropy() {
        let mut engine = MetabolicEngine::default();
        let before = engine.entropy();
        engine.record_error(1.0);
        assert!(engine.entropy() > before);
    }

    #[test]
    fn test_record_success_decreases_entropy() {
        let mut engine = MetabolicEngine::default();
        engine.record_error(1.0);
        let after_error = engine.entropy();
        engine.record_success(1.0);
        assert!(engine.entropy() < after_error);
    }

    #[test]
    fn test_sleeping_state_on_high_entropy() {
        let mut engine = MetabolicEngine::default();
        // Drive entropy above sleep threshold (0.8)
        for _ in 0..20 {
            engine.record_error(1.0);
        }
        assert_eq!(*engine.current_state(), MetabolicState::Sleeping);
    }

    #[test]
    fn test_force_sleep() {
        let mut engine = MetabolicEngine::default();
        engine.force_sleep(10.0);
        assert_eq!(*engine.current_state(), MetabolicState::Sleeping);
    }

    #[test]
    fn test_can_accept_task_optimal() {
        let mut engine = MetabolicEngine::default();
        let (accepted, _reason) = engine.can_accept_task(50.0);
        assert!(accepted);
    }

    #[test]
    fn test_can_accept_task_sleeping_rejected() {
        let mut engine = MetabolicEngine::default();
        engine.force_sleep(10.0);
        let (accepted, _reason) = engine.can_accept_task(10.0);
        assert!(!accepted);
    }

    #[test]
    fn test_can_accept_task_critical_large_rejected() {
        let mut config = MetabolicConfig::default();
        config.initial_energy = 50.0;
        let mut engine = MetabolicEngine::new(config);
        let (accepted, _reason) = engine.can_accept_task(40.0);
        assert!(!accepted);
    }

    #[test]
    fn test_can_accept_task_critical_small_accepted() {
        let mut config = MetabolicConfig::default();
        config.initial_energy = 50.0;
        let mut engine = MetabolicEngine::new(config);
        let (accepted, _reason) = engine.can_accept_task(10.0);
        assert!(accepted);
    }

    #[test]
    fn test_energy_bar_format() {
        let engine = MetabolicEngine::default();
        let bar = engine.energy_bar(20);
        assert!(bar.contains('['));
        assert!(bar.contains(']'));
        assert!(bar.contains('%'));
        assert!(bar.contains("OPTIMAL"));
    }

    #[test]
    fn test_tasks_tracking() {
        let mut engine = MetabolicEngine::default();
        engine.consume_energy(10.0, 0.1);
        engine.consume_energy(10.0, 0.1);
        assert_eq!(engine.tasks_completed(), 2);
        assert!(engine.total_energy_consumed() > 0.0);
    }

    #[test]
    fn test_entropy_clamped_to_1() {
        let mut engine = MetabolicEngine::default();
        for _ in 0..100 {
            engine.record_error(1.0);
        }
        assert!(engine.entropy() <= 1.0);
    }

    #[test]
    fn test_entropy_clamped_to_0() {
        let mut engine = MetabolicEngine::default();
        engine.record_error(0.5);
        for _ in 0..100 {
            engine.record_success(1.0);
        }
        assert!(engine.entropy() >= 0.0);
    }

    #[test]
    fn test_fatigue_state() {
        let mut engine = MetabolicEngine::default();
        // Push entropy above fatigue threshold (0.5) but below sleep (0.8)
        for _ in 0..12 {
            engine.record_error(1.0);
        }
        // Entropy = 12 * 0.05 = 0.6 (above 0.5 fatigue, below 0.8 sleep)
        assert_eq!(*engine.current_state(), MetabolicState::Fatigued);
    }
}
