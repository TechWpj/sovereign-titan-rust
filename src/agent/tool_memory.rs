//! Tool Outcome Memory — per-tool success/failure tracking with persistence.
//!
//! Ported from `sovereign_titan/agents/tool_memory.py`. Records tool usage
//! outcomes and provides hints to the agent about which tools work best
//! for different types of queries.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Action type patterns
// ─────────────────────────────────────────────────────────────────────────────

/// Classify a query into an action type for memory lookup.
pub fn classify_action_type(query: &str) -> &'static str {
    let q = query.to_lowercase();

    let patterns: &[(&[&str], &str)] = &[
        (&["open ", "launch ", "start ", "run "], "open_program"),
        (&["close ", "kill ", "stop ", "end "], "kill_process"),
        (&["ping ", "dns ", "network", "ip "], "network"),
        (&["search ", "look up", "find info", "google"], "web_search"),
        (&["find file", "search file", "locate "], "file_search"),
        (&["read file", "show file", "cat ", "display "], "file_read"),
        (&["write file", "save ", "create file"], "file_write"),
        (&["list process", "show process", "ps "], "list_processes"),
        (&["what time", "current time", "time "], "time_query"),
        (&["what date", "today", "date "], "date_query"),
        (&["calculate ", "compute ", "math "], "calculate"),
        (&["volume ", "mute", "unmute", "audio"], "audio_control"),
        (&["screenshot", "screen capture", "capture screen"], "screenshot"),
        (&["clipboard", "copy ", "paste "], "clipboard"),
        (&["window", "minimize", "maximize", "snap"], "window_control"),
        (&["system info", "cpu", "ram", "disk", "gpu"], "system_info"),
        (&["install ", "uninstall ", "update "], "software_mgmt"),
        (&["git ", "code ", "compile ", "build "], "development"),
        (&["encrypt", "decrypt", "hash ", "encode"], "encoding"),
        (&["play ", "music", "video", "youtube"], "media"),
        (&["navigate ", "go to ", "browse ", "url "], "web_navigation"),
        (&["help ", "how to", "what is", "explain"], "knowledge"),
    ];

    for (keywords, action_type) in patterns {
        if keywords.iter().any(|kw| q.contains(kw)) {
            return action_type;
        }
    }

    "general"
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool outcome record
// ─────────────────────────────────────────────────────────────────────────────

/// A single tool outcome record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub tool: String,
    pub action_type: String,
    pub success_count: u32,
    pub fail_count: u32,
    pub last_error: Option<String>,
}

impl ToolOutcome {
    fn success_rate(&self) -> f64 {
        let total = self.success_count + self.fail_count;
        if total == 0 {
            0.5 // no data → neutral
        } else {
            self.success_count as f64 / total as f64
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ToolOutcomeMemory
// ─────────────────────────────────────────────────────────────────────────────

/// Persistent per-tool success/failure memory.
pub struct ToolOutcomeMemory {
    /// (tool_name, action_type) → ToolOutcome
    records: HashMap<(String, String), ToolOutcome>,
    /// Persistence path.
    persist_path: Option<PathBuf>,
    /// Whether there are unsaved changes.
    dirty: bool,
}

impl ToolOutcomeMemory {
    /// Create a new memory, optionally loading from disk.
    pub fn new() -> Self {
        let persist_path = Self::default_path();
        let mut memory = Self {
            records: HashMap::new(),
            persist_path: persist_path.clone(),
            dirty: false,
        };

        if let Some(ref path) = persist_path {
            memory.load(path);
        }

        memory
    }

    /// Create a memory without persistence (for testing).
    pub fn in_memory() -> Self {
        Self {
            records: HashMap::new(),
            persist_path: None,
            dirty: false,
        }
    }

    /// Default persistence path: %APPDATA%\Sovereign Titan\tool_memory.json
    fn default_path() -> Option<PathBuf> {
        std::env::var("APPDATA").ok().map(|appdata| {
            PathBuf::from(appdata)
                .join("Sovereign Titan")
                .join("tool_memory.json")
        })
    }

    /// Record a successful tool execution.
    pub fn record_success(&mut self, tool: &str, action_type: &str) {
        let key = (tool.to_string(), action_type.to_string());
        let entry = self.records.entry(key).or_insert_with(|| ToolOutcome {
            tool: tool.to_string(),
            action_type: action_type.to_string(),
            success_count: 0,
            fail_count: 0,
            last_error: None,
        });
        entry.success_count += 1;
        self.dirty = true;
    }

    /// Record a failed tool execution.
    pub fn record_failure(&mut self, tool: &str, action_type: &str, error: &str) {
        let key = (tool.to_string(), action_type.to_string());
        let entry = self.records.entry(key).or_insert_with(|| ToolOutcome {
            tool: tool.to_string(),
            action_type: action_type.to_string(),
            success_count: 0,
            fail_count: 0,
            last_error: None,
        });
        entry.fail_count += 1;
        entry.last_error = Some(error.chars().take(200).collect());
        self.dirty = true;
    }

    /// Get hints for the best tools for a given query.
    pub fn get_hints(&self, query: &str) -> String {
        let action_type = classify_action_type(query);

        let mut relevant: Vec<&ToolOutcome> = self
            .records
            .values()
            .filter(|r| r.action_type == action_type && (r.success_count + r.fail_count) > 0)
            .collect();

        if relevant.is_empty() {
            return String::new();
        }

        relevant.sort_by(|a, b| {
            b.success_rate()
                .partial_cmp(&a.success_rate())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut hints = Vec::new();
        for r in relevant.iter().take(3) {
            let rate = (r.success_rate() * 100.0) as u32;
            let total = r.success_count + r.fail_count;
            let mut hint = format!(
                "- {} ({rate}% success rate over {total} uses for '{}')",
                r.tool, r.action_type
            );
            if let Some(ref err) = r.last_error {
                if r.success_rate() < 0.5 {
                    hint.push_str(&format!(" [last error: {err}]"));
                }
            }
            hints.push(hint);
        }

        if hints.is_empty() {
            String::new()
        } else {
            format!("[Tool History for '{action_type}']\n{}", hints.join("\n"))
        }
    }

    /// Number of recorded tool/action pairs.
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Save to disk if dirty.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }

        if let Some(ref path) = self.persist_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            let records: Vec<&ToolOutcome> = self.records.values().collect();
            match serde_json::to_string_pretty(&records) {
                Ok(json) => {
                    if std::fs::write(path, json).is_ok() {
                        self.dirty = false;
                        info!("ToolOutcomeMemory: saved {} records", records.len());
                    }
                }
                Err(e) => warn!("Failed to serialize tool memory: {e}"),
            }
        }
    }

    /// Load from disk.
    fn load(&mut self, path: &PathBuf) {
        if !path.exists() {
            return;
        }

        match std::fs::read_to_string(path) {
            Ok(json) => {
                if let Ok(records) = serde_json::from_str::<Vec<ToolOutcome>>(&json) {
                    for r in records {
                        let key = (r.tool.clone(), r.action_type.clone());
                        self.records.insert(key, r);
                    }
                    info!("ToolOutcomeMemory: loaded {} records", self.records.len());
                }
            }
            Err(e) => warn!("Failed to read tool memory: {e}"),
        }
    }
}

impl Default for ToolOutcomeMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ToolOutcomeMemory {
    fn drop(&mut self) {
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_open_program() {
        assert_eq!(classify_action_type("open discord"), "open_program");
        assert_eq!(classify_action_type("launch chrome"), "open_program");
    }

    #[test]
    fn test_classify_web_search() {
        assert_eq!(classify_action_type("search for rust programming"), "web_search");
    }

    #[test]
    fn test_classify_time_query() {
        assert_eq!(classify_action_type("what time is it"), "time_query");
    }

    #[test]
    fn test_classify_file_search() {
        assert_eq!(classify_action_type("find file readme.md"), "file_search");
    }

    #[test]
    fn test_classify_unknown() {
        assert_eq!(classify_action_type("hello there"), "general");
    }

    #[test]
    fn test_record_success() {
        let mut mem = ToolOutcomeMemory::in_memory();
        mem.record_success("system_control", "open_program");
        assert_eq!(mem.record_count(), 1);

        let key = ("system_control".to_string(), "open_program".to_string());
        let record = mem.records.get(&key).unwrap();
        assert_eq!(record.success_count, 1);
        assert_eq!(record.fail_count, 0);
    }

    #[test]
    fn test_record_failure() {
        let mut mem = ToolOutcomeMemory::in_memory();
        mem.record_failure("web_search", "web_search", "timeout");
        assert_eq!(mem.record_count(), 1);

        let key = ("web_search".to_string(), "web_search".to_string());
        let record = mem.records.get(&key).unwrap();
        assert_eq!(record.success_count, 0);
        assert_eq!(record.fail_count, 1);
        assert_eq!(record.last_error, Some("timeout".to_string()));
    }

    #[test]
    fn test_record_mixed() {
        let mut mem = ToolOutcomeMemory::in_memory();
        mem.record_success("shell", "development");
        mem.record_success("shell", "development");
        mem.record_failure("shell", "development", "exit code 1");

        let key = ("shell".to_string(), "development".to_string());
        let record = mem.records.get(&key).unwrap();
        assert_eq!(record.success_count, 2);
        assert_eq!(record.fail_count, 1);
        assert!((record.success_rate() - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_get_hints_empty() {
        let mem = ToolOutcomeMemory::in_memory();
        assert!(mem.get_hints("open discord").is_empty());
    }

    #[test]
    fn test_get_hints_with_data() {
        let mut mem = ToolOutcomeMemory::in_memory();
        mem.record_success("system_control", "open_program");
        mem.record_success("system_control", "open_program");
        mem.record_success("system_control", "open_program");

        let hints = mem.get_hints("open discord");
        assert!(hints.contains("system_control"));
        assert!(hints.contains("100%"));
    }

    #[test]
    fn test_get_hints_shows_failures() {
        let mut mem = ToolOutcomeMemory::in_memory();
        mem.record_failure("web_search", "web_search", "connection refused");
        mem.record_failure("web_search", "web_search", "timeout");

        let hints = mem.get_hints("search for something");
        assert!(hints.contains("web_search"));
        assert!(hints.contains("0%"));
    }

    #[test]
    fn test_success_rate_no_data() {
        let outcome = ToolOutcome {
            tool: "test".to_string(),
            action_type: "test".to_string(),
            success_count: 0,
            fail_count: 0,
            last_error: None,
        };
        assert!((outcome.success_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_all_success() {
        let outcome = ToolOutcome {
            tool: "test".to_string(),
            action_type: "test".to_string(),
            success_count: 10,
            fail_count: 0,
            last_error: None,
        };
        assert!((outcome.success_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_all_fail() {
        let outcome = ToolOutcome {
            tool: "test".to_string(),
            action_type: "test".to_string(),
            success_count: 0,
            fail_count: 5,
            last_error: None,
        };
        assert!((outcome.success_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_classify_audio() {
        assert_eq!(classify_action_type("volume up"), "audio_control");
        assert_eq!(classify_action_type("mute the speakers"), "audio_control");
    }

    #[test]
    fn test_classify_screenshot() {
        assert_eq!(classify_action_type("take a screenshot"), "screenshot");
    }

    #[test]
    fn test_classify_clipboard() {
        assert_eq!(classify_action_type("copy this to clipboard"), "clipboard");
    }

    #[test]
    fn test_classify_network() {
        assert_eq!(classify_action_type("ping google.com"), "network");
    }

    #[test]
    fn test_classify_system_info() {
        assert_eq!(classify_action_type("show cpu usage"), "system_info");
        assert_eq!(classify_action_type("how much ram"), "system_info");
    }
}
