//! Structured JSON logging — typed log entries with context binding.
//!
//! Provides a structured logger that produces JSON-lines output with
//! bound context fields, suitable for log aggregation and analysis.

use std::collections::HashMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Log severity level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warning => write!(f, "WARNING"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// A single structured log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Seconds since UNIX epoch.
    pub timestamp: f64,
    /// Severity level.
    pub level: LogLevel,
    /// Logger name (component identifier).
    pub logger: String,
    /// Human-readable message.
    pub message: String,
    /// Additional structured fields.
    pub fields: HashMap<String, serde_json::Value>,
}

impl LogEntry {
    /// Render the entry as a single JSON line.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| format!("{{\"error\":\"serialize_fail\",\"message\":\"{}\"}}", self.message))
    }
}

/// Structured logger with bound context and in-memory entry storage.
///
/// Supports binding persistent context fields that are attached to every
/// log entry, plus per-entry extra fields.
pub struct StructuredLogger {
    /// Logger name / component identifier.
    name: String,
    /// Persistent context fields bound to every entry.
    context: HashMap<String, serde_json::Value>,
    /// In-memory log entry buffer.
    entries: Vec<LogEntry>,
}

impl StructuredLogger {
    /// Create a new structured logger with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            context: HashMap::new(),
            entries: Vec::new(),
        }
    }

    /// Bind a persistent context field to all future log entries.
    pub fn bind(&mut self, key: &str, value: serde_json::Value) -> &mut Self {
        self.context.insert(key.to_string(), value);
        self
    }

    /// Log a message at the given level with optional extra fields.
    pub fn log(
        &mut self,
        level: LogLevel,
        message: &str,
        extra: Option<HashMap<String, serde_json::Value>>,
    ) {
        let mut fields = self.context.clone();
        if let Some(extra_fields) = extra {
            fields.extend(extra_fields);
        }

        let entry = LogEntry {
            timestamp: now_secs(),
            level,
            logger: self.name.clone(),
            message: message.to_string(),
            fields,
        };

        self.entries.push(entry);
    }

    /// Log an info message.
    pub fn info(&mut self, message: &str) {
        self.log(LogLevel::Info, message, None);
    }

    /// Log an error message.
    pub fn error(&mut self, message: &str) {
        self.log(LogLevel::Error, message, None);
    }

    /// Log a warning message.
    pub fn warning(&mut self, message: &str) {
        self.log(LogLevel::Warning, message, None);
    }

    /// Log a debug message.
    pub fn debug(&mut self, message: &str) {
        self.log(LogLevel::Debug, message, None);
    }

    /// Get all recorded log entries.
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// Render all entries as newline-delimited JSON (JSON Lines format).
    pub fn to_json_lines(&self) -> String {
        self.entries
            .iter()
            .map(|e| e.to_json())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get the logger name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Clear all stored entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if there are no stored entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
    fn test_new_logger() {
        let logger = StructuredLogger::new("test_component");
        assert_eq!(logger.name(), "test_component");
        assert!(logger.is_empty());
        assert_eq!(logger.len(), 0);
    }

    #[test]
    fn test_log_levels() {
        let mut logger = StructuredLogger::new("test");
        logger.debug("debug msg");
        logger.info("info msg");
        logger.warning("warning msg");
        logger.error("error msg");

        assert_eq!(logger.len(), 4);
        assert_eq!(logger.entries()[0].level, LogLevel::Debug);
        assert_eq!(logger.entries()[1].level, LogLevel::Info);
        assert_eq!(logger.entries()[2].level, LogLevel::Warning);
        assert_eq!(logger.entries()[3].level, LogLevel::Error);
    }

    #[test]
    fn test_bind_context() {
        let mut logger = StructuredLogger::new("test");
        logger.bind("request_id", serde_json::json!("abc-123"));
        logger.bind("user", serde_json::json!("titan"));
        logger.info("handling request");

        let entry = &logger.entries()[0];
        assert_eq!(entry.fields["request_id"], "abc-123");
        assert_eq!(entry.fields["user"], "titan");
    }

    #[test]
    fn test_log_with_extra_fields() {
        let mut logger = StructuredLogger::new("test");
        logger.bind("component", serde_json::json!("nexus"));

        let mut extra = HashMap::new();
        extra.insert("latency_ms".to_string(), serde_json::json!(42.5));
        extra.insert("status".to_string(), serde_json::json!(200));

        logger.log(LogLevel::Info, "request complete", Some(extra));

        let entry = &logger.entries()[0];
        assert_eq!(entry.fields["component"], "nexus");
        assert_eq!(entry.fields["latency_ms"], 42.5);
        assert_eq!(entry.fields["status"], 200);
    }

    #[test]
    fn test_to_json_lines() {
        let mut logger = StructuredLogger::new("test");
        logger.info("first");
        logger.error("second");

        let output = logger.to_json_lines();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON.
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("timestamp").is_some());
            assert!(parsed.get("level").is_some());
            assert!(parsed.get("logger").is_some());
            assert!(parsed.get("message").is_some());
        }
    }

    #[test]
    fn test_entry_timestamps_increase() {
        let mut logger = StructuredLogger::new("test");
        logger.info("first");
        logger.info("second");

        let t1 = logger.entries()[0].timestamp;
        let t2 = logger.entries()[1].timestamp;
        assert!(t2 >= t1);
    }

    #[test]
    fn test_clear_entries() {
        let mut logger = StructuredLogger::new("test");
        logger.info("one");
        logger.info("two");
        assert_eq!(logger.len(), 2);

        logger.clear();
        assert!(logger.is_empty());
        assert_eq!(logger.len(), 0);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(format!("{}", LogLevel::Debug), "DEBUG");
        assert_eq!(format!("{}", LogLevel::Info), "INFO");
        assert_eq!(format!("{}", LogLevel::Warning), "WARNING");
        assert_eq!(format!("{}", LogLevel::Error), "ERROR");
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Error);
    }
}
