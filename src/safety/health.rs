//! Health Monitor — system health tracking and diagnostics.
//!
//! Tracks component health, uptime, and provides diagnostic reports.

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Component health level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HealthLevel {
    Healthy,
    Degraded,
    Critical,
    Unknown,
}

/// Health record for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub level: HealthLevel,
    pub message: String,
    pub last_check: f64,
}

/// System health report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub overall: HealthLevel,
    pub components: Vec<ComponentHealth>,
    pub uptime_secs: f64,
    pub timestamp: f64,
}

/// Health monitor for tracking component status.
pub struct HealthMonitor {
    /// Component health states.
    components: HashMap<String, ComponentHealth>,
    /// System start time.
    start_time: Instant,
}

impl HealthMonitor {
    /// Create a new health monitor.
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            start_time: Instant::now(),
        }
    }

    /// Update a component's health status.
    pub fn update(&mut self, name: &str, level: HealthLevel, message: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        self.components.insert(name.to_string(), ComponentHealth {
            name: name.to_string(),
            level,
            message: message.to_string(),
            last_check: timestamp,
        });
    }

    /// Get the current health report.
    pub fn report(&self) -> HealthReport {
        let components: Vec<ComponentHealth> = self.components.values().cloned().collect();

        let overall = if components.iter().any(|c| c.level == HealthLevel::Critical) {
            HealthLevel::Critical
        } else if components.iter().any(|c| c.level == HealthLevel::Degraded) {
            HealthLevel::Degraded
        } else if components.is_empty() {
            HealthLevel::Unknown
        } else {
            HealthLevel::Healthy
        };

        HealthReport {
            overall,
            components,
            uptime_secs: self.start_time.elapsed().as_secs_f64(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        }
    }

    /// Get health of a specific component.
    pub fn get_component(&self, name: &str) -> Option<&ComponentHealth> {
        self.components.get(name)
    }

    /// System uptime in seconds.
    pub fn uptime_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_health() {
        let monitor = HealthMonitor::new();
        let report = monitor.report();
        assert_eq!(report.overall, HealthLevel::Unknown);
    }

    #[test]
    fn test_update_component() {
        let mut monitor = HealthMonitor::new();
        monitor.update("gpu", HealthLevel::Healthy, "GPU is operational");
        assert_eq!(monitor.get_component("gpu").unwrap().level, HealthLevel::Healthy);
    }

    #[test]
    fn test_overall_degrades() {
        let mut monitor = HealthMonitor::new();
        monitor.update("gpu", HealthLevel::Healthy, "OK");
        monitor.update("memory", HealthLevel::Critical, "OOM");
        let report = monitor.report();
        assert_eq!(report.overall, HealthLevel::Critical);
    }

    #[test]
    fn test_uptime() {
        let monitor = HealthMonitor::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(monitor.uptime_secs() > 0.0);
    }
}
