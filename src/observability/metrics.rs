//! Prometheus-compatible metrics collection (in-memory, no external deps).
//!
//! Provides counters, histograms, and gauges with Prometheus exposition format
//! output. All metrics are namespaced to avoid collisions between components.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The type of a metric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricType {
    /// Monotonically increasing counter.
    Counter,
    /// Distribution of observed values.
    Histogram,
    /// Point-in-time gauge value.
    Gauge,
}

impl fmt::Display for MetricType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricType::Counter => write!(f, "counter"),
            MetricType::Histogram => write!(f, "histogram"),
            MetricType::Gauge => write!(f, "gauge"),
        }
    }
}

/// A single metric value, supporting counter, histogram, and gauge semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    /// Counter value (monotonically increasing).
    pub counter: f64,
    /// Histogram observed values.
    pub histogram_values: Vec<f64>,
    /// Gauge value (point-in-time).
    pub gauge: f64,
}

impl Default for MetricValue {
    fn default() -> Self {
        Self {
            counter: 0.0,
            histogram_values: Vec::new(),
            gauge: 0.0,
        }
    }
}

/// In-memory metrics collector with Prometheus exposition format output.
///
/// Collects counters, histograms, and gauges for request tracking, inference
/// monitoring, tool call statistics, and system resource usage.
pub struct MetricsCollector {
    /// Namespace prefix for all metric names.
    namespace: String,
    /// Counter values keyed by metric name.
    counters: HashMap<String, f64>,
    /// Histogram values keyed by metric name.
    histograms: HashMap<String, Vec<f64>>,
    /// Gauge values keyed by metric name.
    gauges: HashMap<String, f64>,
}

impl MetricsCollector {
    /// Create a new metrics collector with the given namespace.
    pub fn new(namespace: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            counters: HashMap::new(),
            histograms: HashMap::new(),
            gauges: HashMap::new(),
        }
    }

    /// Build a fully-qualified metric name.
    fn fq_name(&self, name: &str) -> String {
        format!("{}_{}", self.namespace, name)
    }

    /// Increment a counter by the given amount.
    fn increment_counter(&mut self, name: &str, amount: f64) {
        let key = self.fq_name(name);
        *self.counters.entry(key).or_insert(0.0) += amount;
    }

    /// Record a value into a histogram.
    fn record_histogram(&mut self, name: &str, value: f64) {
        let key = self.fq_name(name);
        self.histograms.entry(key).or_default().push(value);
    }

    /// Set a gauge to the given value.
    fn set_gauge(&mut self, name: &str, value: f64) {
        let key = self.fq_name(name);
        self.gauges.insert(key, value);
    }

    // ── High-level recording methods ────────────────────────────────────

    /// Record an HTTP request with endpoint, status code, and latency.
    pub fn record_request(&mut self, endpoint: &str, status: u16, latency_ms: f64) {
        let counter_name = format!("http_requests_total_{endpoint}_{status}");
        self.increment_counter(&counter_name, 1.0);
        self.increment_counter("http_requests_total", 1.0);

        let hist_name = format!("http_request_duration_ms_{endpoint}");
        self.record_histogram(&hist_name, latency_ms);
        self.record_histogram("http_request_duration_ms", latency_ms);
    }

    /// Record token usage for an inference call.
    pub fn record_tokens(&mut self, input_tokens: u64, output_tokens: u64) {
        self.increment_counter("tokens_input_total", input_tokens as f64);
        self.increment_counter("tokens_output_total", output_tokens as f64);
        self.increment_counter("tokens_total", (input_tokens + output_tokens) as f64);
    }

    /// Record an inference call with latency and token count.
    pub fn record_inference(&mut self, latency_ms: f64, tokens_generated: u64) {
        self.increment_counter("inference_calls_total", 1.0);
        self.record_histogram("inference_latency_ms", latency_ms);

        if tokens_generated > 0 && latency_ms > 0.0 {
            let tokens_per_sec = (tokens_generated as f64) / (latency_ms / 1000.0);
            self.record_histogram("inference_tokens_per_sec", tokens_per_sec);
        }
    }

    /// Record an error from a component.
    pub fn record_error(&mut self, component: &str, error_type: &str) {
        let name = format!("errors_total_{component}_{error_type}");
        self.increment_counter(&name, 1.0);
        self.increment_counter("errors_total", 1.0);
    }

    /// Record a tool call result.
    pub fn record_tool_call(&mut self, tool_name: &str, success: bool) {
        self.increment_counter("tool_calls_total", 1.0);
        let status = if success { "success" } else { "failure" };
        let name = format!("tool_calls_{tool_name}_{status}");
        self.increment_counter(&name, 1.0);
    }

    /// Set the number of currently active requests.
    pub fn set_active_requests(&mut self, count: u64) {
        self.set_gauge("active_requests", count as f64);
    }

    /// Set memory usage gauges (RSS, VMS, GPU in bytes).
    pub fn set_memory_usage(&mut self, rss: u64, vms: u64, gpu: u64) {
        self.set_gauge("memory_rss_bytes", rss as f64);
        self.set_gauge("memory_vms_bytes", vms as f64);
        self.set_gauge("memory_gpu_bytes", gpu as f64);
    }

    /// Set the system energy level gauge (0.0 - 1.0).
    pub fn set_energy_level(&mut self, energy: f64) {
        self.set_gauge("energy_level", energy);
    }

    /// Set component health gauge (1.0 = healthy, 0.0 = unhealthy).
    pub fn set_component_health(&mut self, component: &str, healthy: bool) {
        let name = format!("component_health_{component}");
        self.set_gauge(&name, if healthy { 1.0 } else { 0.0 });
    }

    // ── Query methods ───────────────────────────────────────────────────

    /// Get the current value of a counter by short name.
    pub fn get_counter(&self, name: &str) -> f64 {
        let key = self.fq_name(name);
        self.counters.get(&key).copied().unwrap_or(0.0)
    }

    /// Get the current value of a gauge by short name.
    pub fn get_gauge(&self, name: &str) -> f64 {
        let key = self.fq_name(name);
        self.gauges.get(&key).copied().unwrap_or(0.0)
    }

    /// Get the average of a histogram by short name.
    pub fn get_histogram_avg(&self, name: &str) -> Option<f64> {
        let key = self.fq_name(name);
        let values = self.histograms.get(&key)?;
        if values.is_empty() {
            return None;
        }
        let sum: f64 = values.iter().sum();
        Some(sum / values.len() as f64)
    }

    /// Get the number of observations in a histogram by short name.
    pub fn get_histogram_count(&self, name: &str) -> usize {
        let key = self.fq_name(name);
        self.histograms
            .get(&key)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Get the minimum value in a histogram by short name.
    pub fn get_histogram_min(&self, name: &str) -> Option<f64> {
        let key = self.fq_name(name);
        self.histograms
            .get(&key)
            .and_then(|v| v.iter().copied().reduce(f64::min))
    }

    /// Get the maximum value in a histogram by short name.
    pub fn get_histogram_max(&self, name: &str) -> Option<f64> {
        let key = self.fq_name(name);
        self.histograms
            .get(&key)
            .and_then(|v| v.iter().copied().reduce(f64::max))
    }

    /// Render all metrics in Prometheus exposition text format.
    pub fn get_metrics_text(&self) -> String {
        let mut lines = Vec::new();

        // Counters.
        let mut counter_keys: Vec<&String> = self.counters.keys().collect();
        counter_keys.sort();
        for key in counter_keys {
            let value = self.counters[key];
            lines.push(format!("# TYPE {key} counter"));
            lines.push(format!("{key} {value}"));
        }

        // Histograms (summary style: count, sum, avg).
        let mut hist_keys: Vec<&String> = self.histograms.keys().collect();
        hist_keys.sort();
        for key in hist_keys {
            let values = &self.histograms[key];
            if values.is_empty() {
                continue;
            }
            let count = values.len();
            let sum: f64 = values.iter().sum();
            lines.push(format!("# TYPE {key} histogram"));
            lines.push(format!("{key}_count {count}"));
            lines.push(format!("{key}_sum {sum}"));
        }

        // Gauges.
        let mut gauge_keys: Vec<&String> = self.gauges.keys().collect();
        gauge_keys.sort();
        for key in gauge_keys {
            let value = self.gauges[key];
            lines.push(format!("# TYPE {key} gauge"));
            lines.push(format!("{key} {value}"));
        }

        lines.join("\n")
    }

    /// Get the namespace of this collector.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_collector() {
        let mc = MetricsCollector::new("titan");
        assert_eq!(mc.namespace(), "titan");
        assert_eq!(mc.get_counter("http_requests_total"), 0.0);
    }

    #[test]
    fn test_record_request() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_request("chat", 200, 45.5);
        mc.record_request("chat", 200, 55.0);
        mc.record_request("chat", 500, 10.0);

        assert_eq!(mc.get_counter("http_requests_total"), 3.0);
        assert_eq!(mc.get_counter("http_requests_total_chat_200"), 2.0);
        assert_eq!(mc.get_counter("http_requests_total_chat_500"), 1.0);
    }

    #[test]
    fn test_record_tokens() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_tokens(100, 50);
        mc.record_tokens(200, 75);

        assert_eq!(mc.get_counter("tokens_input_total"), 300.0);
        assert_eq!(mc.get_counter("tokens_output_total"), 125.0);
        assert_eq!(mc.get_counter("tokens_total"), 425.0);
    }

    #[test]
    fn test_record_inference() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_inference(1000.0, 78);
        mc.record_inference(500.0, 39);

        assert_eq!(mc.get_counter("inference_calls_total"), 2.0);

        let avg_latency = mc.get_histogram_avg("inference_latency_ms").unwrap();
        assert!((avg_latency - 750.0).abs() < 0.001);
    }

    #[test]
    fn test_record_error() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_error("nexus", "timeout");
        mc.record_error("nexus", "timeout");
        mc.record_error("tools", "not_found");

        assert_eq!(mc.get_counter("errors_total"), 3.0);
        assert_eq!(mc.get_counter("errors_total_nexus_timeout"), 2.0);
        assert_eq!(mc.get_counter("errors_total_tools_not_found"), 1.0);
    }

    #[test]
    fn test_record_tool_call() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_tool_call("web_search", true);
        mc.record_tool_call("web_search", true);
        mc.record_tool_call("web_search", false);

        assert_eq!(mc.get_counter("tool_calls_total"), 3.0);
        assert_eq!(mc.get_counter("tool_calls_web_search_success"), 2.0);
        assert_eq!(mc.get_counter("tool_calls_web_search_failure"), 1.0);
    }

    #[test]
    fn test_gauges() {
        let mut mc = MetricsCollector::new("titan");
        mc.set_active_requests(5);
        assert_eq!(mc.get_gauge("active_requests"), 5.0);

        mc.set_active_requests(3);
        assert_eq!(mc.get_gauge("active_requests"), 3.0);
    }

    #[test]
    fn test_memory_usage_gauges() {
        let mut mc = MetricsCollector::new("titan");
        mc.set_memory_usage(1_000_000, 2_000_000, 8_000_000);

        assert_eq!(mc.get_gauge("memory_rss_bytes"), 1_000_000.0);
        assert_eq!(mc.get_gauge("memory_vms_bytes"), 2_000_000.0);
        assert_eq!(mc.get_gauge("memory_gpu_bytes"), 8_000_000.0);
    }

    #[test]
    fn test_energy_level() {
        let mut mc = MetricsCollector::new("titan");
        mc.set_energy_level(0.85);
        assert!((mc.get_gauge("energy_level") - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn test_component_health_gauge() {
        let mut mc = MetricsCollector::new("titan");
        mc.set_component_health("prime", true);
        mc.set_component_health("warden", false);

        assert_eq!(mc.get_gauge("component_health_prime"), 1.0);
        assert_eq!(mc.get_gauge("component_health_warden"), 0.0);
    }

    #[test]
    fn test_histogram_stats() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_inference(100.0, 10);
        mc.record_inference(200.0, 20);
        mc.record_inference(300.0, 30);

        assert_eq!(mc.get_histogram_count("inference_latency_ms"), 3);
        assert_eq!(mc.get_histogram_min("inference_latency_ms"), Some(100.0));
        assert_eq!(mc.get_histogram_max("inference_latency_ms"), Some(300.0));

        let avg = mc.get_histogram_avg("inference_latency_ms").unwrap();
        assert!((avg - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_histogram_avg_empty() {
        let mc = MetricsCollector::new("titan");
        assert!(mc.get_histogram_avg("nonexistent").is_none());
    }

    #[test]
    fn test_prometheus_text_format() {
        let mut mc = MetricsCollector::new("titan");
        mc.record_request("chat", 200, 50.0);
        mc.set_active_requests(2);

        let text = mc.get_metrics_text();
        assert!(text.contains("# TYPE titan_http_requests_total counter"));
        assert!(text.contains("titan_http_requests_total 1"));
        assert!(text.contains("# TYPE titan_active_requests gauge"));
        assert!(text.contains("titan_active_requests 2"));
        assert!(text.contains("# TYPE titan_http_request_duration_ms histogram"));
        assert!(text.contains("titan_http_request_duration_ms_count 1"));
    }

    #[test]
    fn test_metric_type_display() {
        assert_eq!(format!("{}", MetricType::Counter), "counter");
        assert_eq!(format!("{}", MetricType::Histogram), "histogram");
        assert_eq!(format!("{}", MetricType::Gauge), "gauge");
    }
}
