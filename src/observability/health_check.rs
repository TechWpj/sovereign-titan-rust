//! Component health check system — register and run health probes.
//!
//! Provides a registry for named health check functions, running them
//! individually or as a batch to produce an overall system health report.

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Result of a single health check execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Whether the component is healthy.
    pub healthy: bool,
    /// How long the check took in milliseconds.
    pub latency_ms: f64,
    /// When the check was performed (seconds since UNIX epoch).
    pub timestamp: f64,
    /// Error message if the check failed.
    pub error: Option<String>,
}

/// Aggregated health status across all components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallHealth {
    /// Whether all components are healthy.
    pub healthy: bool,
    /// Individual check results.
    pub checks: HashMap<String, HealthCheckResult>,
    /// When the aggregate check was performed.
    pub timestamp: f64,
}

/// Health checker with registered probe functions.
///
/// Components register named check functions that return `true` for healthy
/// and `false` for unhealthy. The checker can run individual or batch probes.
pub struct HealthChecker {
    /// Registered check functions keyed by component name.
    checks: HashMap<String, Box<dyn Fn() -> bool + Send + Sync>>,
    /// Cached results from the last check run.
    results: HashMap<String, HealthCheckResult>,
}

impl HealthChecker {
    /// Create a new health checker with no registered checks.
    pub fn new() -> Self {
        Self {
            checks: HashMap::new(),
            results: HashMap::new(),
        }
    }

    /// Register a health check function for a named component.
    ///
    /// The function should return `true` if the component is healthy,
    /// `false` otherwise. Replaces any existing check with the same name.
    pub fn register(
        &mut self,
        name: &str,
        check_fn: impl Fn() -> bool + Send + Sync + 'static,
    ) {
        self.checks.insert(name.to_string(), Box::new(check_fn));
    }

    /// Run a single named health check and return the result.
    pub fn check(&mut self, name: &str) -> Option<HealthCheckResult> {
        let check_fn = self.checks.get(name)?;

        let start = Instant::now();
        let healthy = check_fn();
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

        let result = HealthCheckResult {
            healthy,
            latency_ms,
            timestamp: now_secs(),
            error: if healthy {
                None
            } else {
                Some(format!("Component '{name}' reported unhealthy"))
            },
        };

        self.results.insert(name.to_string(), result.clone());
        Some(result)
    }

    /// Run all registered health checks and return aggregated results.
    pub fn check_all(&mut self) -> OverallHealth {
        let names: Vec<String> = self.checks.keys().cloned().collect();
        let mut checks = HashMap::new();

        for name in &names {
            if let Some(result) = self.check(name) {
                checks.insert(name.clone(), result);
            }
        }

        let healthy = checks.values().all(|r| r.healthy);

        OverallHealth {
            healthy,
            checks,
            timestamp: now_secs(),
        }
    }

    /// Get cached results from the most recent check runs.
    pub fn get_status(&self) -> &HashMap<String, HealthCheckResult> {
        &self.results
    }

    /// Get the names of all registered checks.
    pub fn registered_checks(&self) -> Vec<&str> {
        self.checks.keys().map(|s| s.as_str()).collect()
    }

    /// Get the number of registered checks.
    pub fn check_count(&self) -> usize {
        self.checks.len()
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current time as seconds since UNIX epoch.
fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_checker() {
        let checker = HealthChecker::new();
        assert_eq!(checker.check_count(), 0);
        assert!(checker.get_status().is_empty());
    }

    #[test]
    fn test_register_and_check_healthy() {
        let mut checker = HealthChecker::new();
        checker.register("gpu", || true);

        let result = checker.check("gpu").unwrap();
        assert!(result.healthy);
        assert!(result.error.is_none());
        assert!(result.latency_ms >= 0.0);
        assert!(result.timestamp > 0.0);
    }

    #[test]
    fn test_check_unhealthy() {
        let mut checker = HealthChecker::new();
        checker.register("database", || false);

        let result = checker.check("database").unwrap();
        assert!(!result.healthy);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("database"));
    }

    #[test]
    fn test_check_nonexistent() {
        let mut checker = HealthChecker::new();
        assert!(checker.check("missing").is_none());
    }

    #[test]
    fn test_check_all_healthy() {
        let mut checker = HealthChecker::new();
        checker.register("gpu", || true);
        checker.register("memory", || true);
        checker.register("model", || true);

        let overall = checker.check_all();
        assert!(overall.healthy);
        assert_eq!(overall.checks.len(), 3);
        assert!(overall.checks.values().all(|r| r.healthy));
    }

    #[test]
    fn test_check_all_mixed() {
        let mut checker = HealthChecker::new();
        checker.register("gpu", || true);
        checker.register("model", || false);

        let overall = checker.check_all();
        assert!(!overall.healthy);
        assert_eq!(overall.checks.len(), 2);
        assert!(overall.checks["gpu"].healthy);
        assert!(!overall.checks["model"].healthy);
    }

    #[test]
    fn test_check_all_empty() {
        let mut checker = HealthChecker::new();
        let overall = checker.check_all();
        // No checks registered = vacuously healthy.
        assert!(overall.healthy);
        assert!(overall.checks.is_empty());
    }

    #[test]
    fn test_results_cached() {
        let mut checker = HealthChecker::new();
        checker.register("gpu", || true);

        assert!(checker.get_status().is_empty());
        checker.check("gpu");
        assert_eq!(checker.get_status().len(), 1);
        assert!(checker.get_status()["gpu"].healthy);
    }

    #[test]
    fn test_registered_checks() {
        let mut checker = HealthChecker::new();
        checker.register("a", || true);
        checker.register("b", || true);

        let names = checker.registered_checks();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }
}
