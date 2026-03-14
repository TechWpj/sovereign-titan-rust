//! Integrated observability stack — composites metrics, logging, and health.
//!
//! Provides a single entry point for observability concerns, tracking active
//! requests and delegating to the metrics collector, structured logger, and
//! health checker subsystems.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::health_check::HealthChecker;
use super::metrics::MetricsCollector;
use super::structured_logging::StructuredLogger;

/// An in-flight request being tracked by the observability stack.
#[derive(Debug, Clone)]
pub struct ActiveRequest {
    /// Unique request identifier.
    pub request_id: String,
    /// API endpoint being served.
    pub endpoint: String,
    /// When the request started (seconds since UNIX epoch).
    pub started_at: f64,
}

/// Integrated observability stack combining metrics, logging, and health checks.
///
/// Tracks active requests and provides unified access to all observability
/// subsystems for the Sovereign Titan runtime.
pub struct ObservabilityStack {
    /// Prometheus-compatible metrics collector.
    metrics: MetricsCollector,
    /// Structured JSON logger.
    logger: StructuredLogger,
    /// Component health checker.
    health: HealthChecker,
    /// Currently active (in-flight) requests.
    active_requests: HashMap<String, ActiveRequest>,
}

impl ObservabilityStack {
    /// Create a new observability stack with the given namespace.
    pub fn new(namespace: &str) -> Self {
        Self {
            metrics: MetricsCollector::new(namespace),
            logger: StructuredLogger::new(&format!("{namespace}.stack")),
            health: HealthChecker::new(),
            active_requests: HashMap::new(),
        }
    }

    /// Begin tracking an incoming request.
    ///
    /// Records the request start time and updates the active request gauge.
    pub fn start_request(&mut self, request_id: &str, endpoint: &str) {
        let now = now_secs();

        self.active_requests.insert(
            request_id.to_string(),
            ActiveRequest {
                request_id: request_id.to_string(),
                endpoint: endpoint.to_string(),
                started_at: now,
            },
        );

        self.metrics
            .set_active_requests(self.active_requests.len() as u64);

        self.logger.log(
            super::structured_logging::LogLevel::Info,
            "request_started",
            Some(HashMap::from([
                ("request_id".to_string(), serde_json::json!(request_id)),
                ("endpoint".to_string(), serde_json::json!(endpoint)),
            ])),
        );
    }

    /// End tracking a request, recording its duration and status.
    ///
    /// Computes latency from the stored start time and records it as both
    /// a metric and a log entry. Returns the latency in milliseconds, or
    /// `None` if the request ID was not found.
    pub fn end_request(&mut self, request_id: &str, status: u16) -> Option<f64> {
        let request = self.active_requests.remove(request_id)?;
        let now = now_secs();
        let latency_ms = (now - request.started_at) * 1000.0;

        self.metrics
            .record_request(&request.endpoint, status, latency_ms);
        self.metrics
            .set_active_requests(self.active_requests.len() as u64);

        self.logger.log(
            super::structured_logging::LogLevel::Info,
            "request_completed",
            Some(HashMap::from([
                ("request_id".to_string(), serde_json::json!(request_id)),
                ("endpoint".to_string(), serde_json::json!(request.endpoint)),
                ("status".to_string(), serde_json::json!(status)),
                ("latency_ms".to_string(), serde_json::json!(latency_ms)),
            ])),
        );

        Some(latency_ms)
    }

    /// Get Prometheus exposition format text for all collected metrics.
    pub fn get_prometheus_metrics(&self) -> String {
        self.metrics.get_metrics_text()
    }

    /// Get a reference to the metrics collector.
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics
    }

    /// Get a mutable reference to the metrics collector.
    pub fn metrics_mut(&mut self) -> &mut MetricsCollector {
        &mut self.metrics
    }

    /// Get a mutable reference to the structured logger.
    pub fn logger(&mut self) -> &mut StructuredLogger {
        &mut self.logger
    }

    /// Get a reference to the health checker.
    pub fn health(&self) -> &HealthChecker {
        &self.health
    }

    /// Get a mutable reference to the health checker (for registration).
    pub fn health_mut(&mut self) -> &mut HealthChecker {
        &mut self.health
    }

    /// Get the number of currently active requests.
    pub fn active_request_count(&self) -> usize {
        self.active_requests.len()
    }

    /// Get all currently active requests.
    pub fn active_requests(&self) -> &HashMap<String, ActiveRequest> {
        &self.active_requests
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
    fn test_new_stack() {
        let stack = ObservabilityStack::new("titan");
        assert_eq!(stack.active_request_count(), 0);
        assert_eq!(stack.metrics().namespace(), "titan");
        assert!(stack.health().get_status().is_empty());
    }

    #[test]
    fn test_start_and_end_request() {
        let mut stack = ObservabilityStack::new("titan");
        stack.start_request("req-1", "chat");
        assert_eq!(stack.active_request_count(), 1);
        assert!(stack.active_requests().contains_key("req-1"));

        let latency = stack.end_request("req-1", 200);
        assert!(latency.is_some());
        assert!(latency.unwrap() >= 0.0);
        assert_eq!(stack.active_request_count(), 0);
    }

    #[test]
    fn test_end_unknown_request() {
        let mut stack = ObservabilityStack::new("titan");
        let result = stack.end_request("nonexistent", 200);
        assert!(result.is_none());
    }

    #[test]
    fn test_request_records_metrics() {
        let mut stack = ObservabilityStack::new("titan");
        stack.start_request("req-1", "chat");
        stack.end_request("req-1", 200);

        assert_eq!(stack.metrics().get_counter("http_requests_total"), 1.0);
    }

    #[test]
    fn test_request_records_logs() {
        let mut stack = ObservabilityStack::new("titan");
        stack.start_request("req-1", "chat");
        stack.end_request("req-1", 200);

        // Should have at least start + end log entries.
        let log_json = stack.logger().to_json_lines();
        assert!(log_json.contains("request_started"));
        assert!(log_json.contains("request_completed"));
    }

    #[test]
    fn test_multiple_concurrent_requests() {
        let mut stack = ObservabilityStack::new("titan");
        stack.start_request("req-1", "chat");
        stack.start_request("req-2", "status");
        stack.start_request("req-3", "chat");

        assert_eq!(stack.active_request_count(), 3);

        stack.end_request("req-2", 200);
        assert_eq!(stack.active_request_count(), 2);

        stack.end_request("req-1", 200);
        stack.end_request("req-3", 500);
        assert_eq!(stack.active_request_count(), 0);
    }

    #[test]
    fn test_health_check_integration() {
        let mut stack = ObservabilityStack::new("titan");
        stack.health_mut().register("gpu", || true);
        stack.health_mut().register("model", || true);

        let overall = stack.health_mut().check_all();
        assert!(overall.healthy);
        assert_eq!(overall.checks.len(), 2);
    }

    #[test]
    fn test_prometheus_output() {
        let mut stack = ObservabilityStack::new("titan");
        stack.start_request("req-1", "chat");
        stack.end_request("req-1", 200);
        stack.metrics_mut().set_energy_level(0.9);

        let text = stack.get_prometheus_metrics();
        assert!(text.contains("titan_http_requests_total"));
        assert!(text.contains("titan_energy_level"));
    }
}
