//! Temporal Pattern Store — learns user interaction patterns over time.
//!
//! Ported from `sovereign_titan/cognitive/temporal_patterns.py`.
//! Tracks interaction histogram by day-of-week and hour, enabling idle
//! probability prediction and time-aware mode selection.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};

const DAYS: [&str; 7] = [
    "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
];

const MODE_DEQUE_MAXLEN: usize = 20;

/// Time period of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimePeriod {
    Morning,
    Afternoon,
    Evening,
    Night,
}

impl TimePeriod {
    /// Get the current time period.
    pub fn current() -> Self {
        let hour = Local::now().hour();
        match hour {
            5..=11 => Self::Morning,
            12..=16 => Self::Afternoon,
            17..=20 => Self::Evening,
            _ => Self::Night,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Morning => "morning",
            Self::Afternoon => "afternoon",
            Self::Evening => "evening",
            Self::Night => "night",
        }
    }
}

/// Peak hour record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeakHour {
    pub day: String,
    pub hour: u32,
    pub count: u32,
}

/// Stats for the temporal pattern store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalStats {
    pub total_interactions: u64,
    pub peak_hours: Vec<PeakHour>,
    pub idle_probability: f64,
}

/// Serializable state for persistence.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedState {
    histogram: HashMap<String, HashMap<String, u32>>,
    mode_success: HashMap<String, HashMap<String, Vec<bool>>>,
    total_interactions: u64,
}

/// Learns temporal interaction patterns and predicts idle probability.
pub struct TemporalPatternStore {
    /// Day-of-week → hour (0–23) → interaction count.
    histogram: HashMap<String, HashMap<String, u32>>,
    /// Time period → mode → bounded deque of bool outcomes.
    mode_success: HashMap<String, HashMap<String, VecDeque<bool>>>,
    /// Total interaction count.
    total_interactions: u64,
    /// Persistence path.
    persist_path: PathBuf,
}

impl TemporalPatternStore {
    /// Create a new store with the given persistence path.
    pub fn new(persist_path: PathBuf) -> Self {
        let mut histogram = HashMap::new();
        for day in &DAYS {
            let mut hours = HashMap::new();
            for h in 0..24 {
                hours.insert(h.to_string(), 0u32);
            }
            histogram.insert(day.to_string(), hours);
        }

        let mut mode_success = HashMap::new();
        for period in &["morning", "afternoon", "evening", "night"] {
            mode_success.insert(period.to_string(), HashMap::new());
        }

        let mut store = Self {
            histogram,
            mode_success,
            total_interactions: 0,
            persist_path,
        };
        store.load();
        store
    }

    /// Record an interaction at the current day-of-week and hour.
    pub fn record_interaction(&mut self) {
        let now = Local::now();
        let day = DAYS[now.weekday().num_days_from_monday() as usize];
        let hour = now.hour().to_string();

        if let Some(hours) = self.histogram.get_mut(day) {
            *hours.entry(hour).or_insert(0) += 1;
        }
        self.total_interactions += 1;

        if self.total_interactions % 10 == 0 {
            self.persist();
        }
    }

    /// Record whether a mode succeeded during the current time period.
    pub fn record_mode_outcome(&mut self, mode: &str, success: bool) {
        let period = TimePeriod::current().as_str().to_string();

        if let Some(modes) = self.mode_success.get_mut(&period) {
            let dq = modes
                .entry(mode.to_string())
                .or_insert_with(|| VecDeque::with_capacity(MODE_DEQUE_MAXLEN));

            if dq.len() >= MODE_DEQUE_MAXLEN {
                dq.pop_front();
            }
            dq.push_back(success);
        }
    }

    /// Predict idle probability [0.0–1.0] based on current day/hour histogram.
    pub fn predict_idle_probability(&self) -> f64 {
        if self.total_interactions == 0 {
            return 0.5;
        }

        let now = Local::now();
        let day = DAYS[now.weekday().num_days_from_monday() as usize];
        let hour = now.hour().to_string();

        let current_count = self
            .histogram
            .get(day)
            .and_then(|h| h.get(&hour))
            .copied()
            .unwrap_or(0) as f64;

        // Average across all non-zero slots
        let all_counts: Vec<f64> = self
            .histogram
            .values()
            .flat_map(|h| h.values())
            .filter(|&&c| c > 0)
            .map(|&c| c as f64)
            .collect();

        if all_counts.is_empty() {
            return 0.5;
        }

        let average: f64 = all_counts.iter().sum::<f64>() / all_counts.len() as f64;
        if average == 0.0 {
            return 0.5;
        }

        if current_count < average * 0.3 {
            return 0.8; // Likely idle
        }
        if current_count > average * 1.5 {
            return 0.2; // Likely active
        }

        // Linear interpolation
        let low = average * 0.3;
        let high = average * 1.5;
        let ratio = (current_count - low) / (high - low);
        0.8 - ratio * 0.6
    }

    /// Get success rate for a mode in the current time period.
    pub fn get_mode_success_rate(&self, mode: &str) -> Option<f64> {
        let period = TimePeriod::current().as_str();
        let dq = self.mode_success.get(period)?.get(mode)?;
        if dq.len() < 3 {
            return None;
        }
        let successes = dq.iter().filter(|&&v| v).count();
        Some(successes as f64 / dq.len() as f64)
    }

    /// Get summary statistics.
    pub fn get_stats(&self) -> TemporalStats {
        let mut slots: Vec<(u32, String, u32)> = self
            .histogram
            .iter()
            .flat_map(|(day, hours)| {
                hours.iter().filter(|(_, c)| **c > 0).map(move |(hour, count)| {
                    (*count, day.clone(), hour.parse::<u32>().unwrap_or(0))
                })
            })
            .collect();
        slots.sort_by(|a, b| b.0.cmp(&a.0));

        let peak_hours: Vec<PeakHour> = slots
            .iter()
            .take(3)
            .map(|(count, day, hour)| PeakHour {
                day: day.clone(),
                hour: *hour,
                count: *count,
            })
            .collect();

        TemporalStats {
            total_interactions: self.total_interactions,
            peak_hours,
            idle_probability: self.predict_idle_probability(),
        }
    }

    /// Total interactions recorded.
    pub fn total_interactions(&self) -> u64 {
        self.total_interactions
    }

    // ── Persistence ─────────────────────────────────────────────────────

    fn persist(&self) {
        let state = PersistedState {
            histogram: self.histogram.clone(),
            mode_success: self
                .mode_success
                .iter()
                .map(|(period, modes)| {
                    let modes_map: HashMap<String, Vec<bool>> = modes
                        .iter()
                        .map(|(mode, dq)| (mode.clone(), dq.iter().copied().collect()))
                        .collect();
                    (period.clone(), modes_map)
                })
                .collect(),
            total_interactions: self.total_interactions,
        };

        if let Some(parent) = self.persist_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        if let Ok(json) = serde_json::to_string_pretty(&state) {
            std::fs::write(&self.persist_path, json).ok();
        }
    }

    fn load(&mut self) {
        let data = match std::fs::read_to_string(&self.persist_path) {
            Ok(d) => d,
            Err(_) => return,
        };

        let state: PersistedState = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Restore histogram
        for (day, hours) in &state.histogram {
            if let Some(existing) = self.histogram.get_mut(day) {
                for (hour, count) in hours {
                    existing.insert(hour.clone(), *count);
                }
            }
        }

        // Restore mode success
        for (period, modes) in &state.mode_success {
            if let Some(existing) = self.mode_success.get_mut(period) {
                for (mode, outcomes) in modes {
                    let mut dq = VecDeque::with_capacity(MODE_DEQUE_MAXLEN);
                    for &val in outcomes.iter().rev().take(MODE_DEQUE_MAXLEN).collect::<Vec<_>>().iter().rev() {
                        dq.push_back(*val);
                    }
                    existing.insert(mode.clone(), dq);
                }
            }
        }

        self.total_interactions = state.total_interactions;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_store() -> TemporalPatternStore {
        TemporalPatternStore::new(PathBuf::from("__test_temporal_patterns_nonexistent.json"))
    }

    #[test]
    fn test_initial_idle_probability() {
        let store = test_store();
        assert!((store.predict_idle_probability() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_record_interaction() {
        let mut store = test_store();
        store.record_interaction();
        assert_eq!(store.total_interactions(), 1);
    }

    #[test]
    fn test_record_mode_outcome() {
        let mut store = test_store();
        for _ in 0..5 {
            store.record_mode_outcome("think", true);
        }
        let rate = store.get_mode_success_rate("think");
        assert!(rate.is_some());
        assert!((rate.unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_mode_success_insufficient_data() {
        let mut store = test_store();
        store.record_mode_outcome("test_mode", true);
        assert!(store.get_mode_success_rate("test_mode").is_none());
    }

    #[test]
    fn test_get_stats() {
        let mut store = test_store();
        store.record_interaction();
        let stats = store.get_stats();
        assert_eq!(stats.total_interactions, 1);
    }

    #[test]
    fn test_time_period() {
        let period = TimePeriod::current();
        // Just verify it returns a valid period
        let _ = period.as_str();
    }
}
