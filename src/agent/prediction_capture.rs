//! Prediction Capture — autonomous retraining data generator.
//!
//! When Titan logs an `[ERROR DELTA: HIGH]` during execution, this module
//! captures the full interaction (prompt, failed prediction, delta analysis,
//! and the eventual correct path) and archives it as a training-ready SFT
//! row in `failed_predictions.jsonl`.
//!
//! This enables the "Titan Fallback" autonomous retraining loop:
//! 1. TRIGGER — ERROR DELTA: HIGH logged during tool execution
//! 2. CAPTURE — Wait for task completion, package the full interaction
//! 3. ARCHIVE — Append to `failed_predictions.jsonl`
//! 4. EVOLVE — Periodically retrain via LoRA on Lambda Labs
//!
//! Ported from `sovereign_titan/agents/prediction_capture.py`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Prediction event
// ─────────────────────────────────────────────────────────────────────────────

/// A single prediction attempt within a tool-calling interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionEvent {
    /// ISO timestamp of this event.
    pub timestamp: String,
    /// The SYSTEM 2 brainstormed options.
    pub system2_ttc: String,
    /// Which option was selected.
    pub ttc_selection: String,
    /// The PREDICTIVE ANCHOR text.
    pub prediction: String,
    /// Tool name.
    pub action: String,
    /// Action input parameters.
    pub action_input: serde_json::Value,
    /// What the system actually returned (capped at 2000 chars).
    pub observation: String,
    /// The ERROR DELTA text.
    pub error_delta: String,
    /// Whether delta was HIGH.
    pub delta_is_high: bool,
}

impl PredictionEvent {
    /// Create a new prediction event with the current timestamp.
    pub fn new() -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            system2_ttc: String::new(),
            ttc_selection: String::new(),
            prediction: String::new(),
            action: String::new(),
            action_input: serde_json::json!({}),
            observation: String::new(),
            error_delta: String::new(),
            delta_is_high: false,
        }
    }
}

impl Default for PredictionEvent {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Captured interaction
// ─────────────────────────────────────────────────────────────────────────────

/// A complete interaction that contained at least one HIGH delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedInteraction {
    /// Unique capture ID.
    pub capture_id: String,
    /// ISO timestamp when the interaction started.
    pub timestamp: String,
    /// The user's original prompt.
    pub user_prompt: String,
    /// Hash of the system prompt (for dedup, not the full prompt).
    pub system_prompt_hash: String,
    /// All prediction events in this interaction.
    pub events: Vec<PredictionEvent>,
    /// The final answer provided.
    pub final_answer: String,
    /// Whether the task ultimately succeeded.
    pub task_succeeded: bool,
    /// Total number of steps.
    pub total_steps: usize,
    /// Number of HIGH delta events.
    pub high_delta_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// PredictionCaptureEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Stats for the prediction capture engine.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureStats {
    pub enabled: bool,
    pub total_high_deltas: usize,
    pub total_zero_deltas: usize,
    pub total_captures_written: usize,
    pub active_interactions: usize,
    pub capture_path: String,
}

/// Captures HIGH delta interactions for autonomous retraining.
///
/// Thread-safe via `Mutex`. Writes to `failed_predictions.jsonl` atomically.
pub struct PredictionCaptureEngine {
    inner: Mutex<CaptureInner>,
}

struct CaptureInner {
    capture_path: PathBuf,
    enabled: bool,
    active_interactions: HashMap<String, CapturedInteraction>,
    total_high_deltas: usize,
    total_zero_deltas: usize,
    total_captures_written: usize,
}

impl PredictionCaptureEngine {
    /// Create a new prediction capture engine.
    ///
    /// If `capture_path` is `None`, defaults to `%APPDATA%/Sovereign Titan/failed_predictions.jsonl`.
    pub fn new(capture_path: Option<PathBuf>, enabled: bool) -> Self {
        let path = capture_path.unwrap_or_else(default_capture_path);

        if enabled {
            info!(
                "PredictionCapture initialized — writing to {}",
                path.display()
            );
        }

        Self {
            inner: Mutex::new(CaptureInner {
                capture_path: path,
                enabled,
                active_interactions: HashMap::new(),
                total_high_deltas: 0,
                total_zero_deltas: 0,
                total_captures_written: 0,
            }),
        }
    }

    /// Get current stats.
    pub fn stats(&self) -> CaptureStats {
        let inner = self.inner.lock().unwrap();
        CaptureStats {
            enabled: inner.enabled,
            total_high_deltas: inner.total_high_deltas,
            total_zero_deltas: inner.total_zero_deltas,
            total_captures_written: inner.total_captures_written,
            active_interactions: inner.active_interactions.len(),
            capture_path: inner.capture_path.display().to_string(),
        }
    }

    /// Begin tracking a new user interaction.
    pub fn start_interaction(
        &self,
        interaction_id: &str,
        user_prompt: &str,
        system_prompt_hash: &str,
    ) {
        let mut inner = self.inner.lock().unwrap();
        if !inner.enabled {
            return;
        }

        inner.active_interactions.insert(
            interaction_id.to_string(),
            CapturedInteraction {
                capture_id: interaction_id.to_string(),
                timestamp: Utc::now().to_rfc3339(),
                user_prompt: user_prompt.to_string(),
                system_prompt_hash: system_prompt_hash.to_string(),
                events: Vec::new(),
                final_answer: String::new(),
                task_succeeded: false,
                total_steps: 0,
                high_delta_count: 0,
            },
        );
    }

    /// Record a single step (prediction + observation) in an interaction.
    pub fn record_event(&self, interaction_id: &str, event: PredictionEvent) {
        let mut inner = self.inner.lock().unwrap();
        if !inner.enabled {
            return;
        }

        let is_high = event.delta_is_high;
        let delta_preview = if is_high {
            event.error_delta[..event.error_delta.len().min(200)].to_string()
        } else {
            String::new()
        };

        // Remove interaction, modify it, then re-insert to avoid split borrows.
        let Some(mut interaction) = inner.active_interactions.remove(interaction_id) else {
            return;
        };

        interaction.total_steps += 1;
        let step_num = interaction.total_steps;

        if is_high {
            interaction.high_delta_count += 1;
            inner.total_high_deltas += 1;
            info!(
                "HIGH ERROR DELTA captured in {} step {}: {}",
                interaction_id, step_num, delta_preview
            );
        } else {
            inner.total_zero_deltas += 1;
        }

        interaction.events.push(event);
        inner
            .active_interactions
            .insert(interaction_id.to_string(), interaction);
    }

    /// End an interaction. If it contained HIGH deltas, write to JSONL.
    ///
    /// Returns the capture file path if written, `None` otherwise.
    pub fn end_interaction(
        &self,
        interaction_id: &str,
        final_answer: &str,
        task_succeeded: bool,
    ) -> Option<PathBuf> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.enabled {
            return None;
        }

        let mut interaction = inner.active_interactions.remove(interaction_id)?;
        interaction.final_answer = final_answer.chars().take(2000).collect();
        interaction.task_succeeded = task_succeeded;

        // Only capture if there were HIGH deltas
        if interaction.high_delta_count == 0 {
            return None;
        }

        write_capture(&inner.capture_path, &interaction).ok()?;
        inner.total_captures_written += 1;

        info!(
            "Captured failed prediction #{}: {} ({} high deltas, {} steps, {})",
            inner.total_captures_written,
            interaction.capture_id,
            interaction.high_delta_count,
            interaction.total_steps,
            if interaction.task_succeeded {
                "succeeded"
            } else {
                "FAILED"
            },
        );

        Some(inner.capture_path.clone())
    }

    /// Generate a unique interaction ID.
    pub fn new_interaction_id() -> String {
        Uuid::new_v4().to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Default capture path: `%APPDATA%/Sovereign Titan/failed_predictions.jsonl`.
fn default_capture_path() -> PathBuf {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Sovereign Titan")
        .join("failed_predictions.jsonl")
}

/// Atomically append a capture to the JSONL file.
fn write_capture(path: &PathBuf, interaction: &CapturedInteraction) -> std::io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let record = serde_json::json!({
        "capture_id": interaction.capture_id,
        "timestamp": interaction.timestamp,
        "user_prompt": interaction.user_prompt,
        "high_delta_count": interaction.high_delta_count,
        "total_steps": interaction.total_steps,
        "task_succeeded": interaction.task_succeeded,
        "final_answer": interaction.final_answer,
        "events": interaction.events,
    });

    let mut line = serde_json::to_string(&record)?;
    line.push('\n');

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prediction_event_default() {
        let event = PredictionEvent::new();
        assert!(!event.delta_is_high);
        assert!(event.action.is_empty());
        assert!(!event.timestamp.is_empty());
    }

    #[test]
    fn test_capture_engine_disabled() {
        let engine = PredictionCaptureEngine::new(None, false);
        engine.start_interaction("test-1", "hello", "");
        let event = PredictionEvent {
            delta_is_high: true,
            ..PredictionEvent::new()
        };
        engine.record_event("test-1", event);
        let result = engine.end_interaction("test-1", "answer", true);
        assert!(result.is_none());

        let stats = engine.stats();
        assert!(!stats.enabled);
        assert_eq!(stats.total_high_deltas, 0);
    }

    #[test]
    fn test_capture_engine_no_high_deltas() {
        let tmp = std::env::temp_dir().join("titan_test_capture_no_high.jsonl");
        let _ = fs::remove_file(&tmp);

        let engine = PredictionCaptureEngine::new(Some(tmp.clone()), true);
        engine.start_interaction("test-2", "hello", "");

        let event = PredictionEvent {
            delta_is_high: false,
            ..PredictionEvent::new()
        };
        engine.record_event("test-2", event);

        let result = engine.end_interaction("test-2", "answer", true);
        assert!(result.is_none()); // No HIGH deltas → nothing written

        let stats = engine.stats();
        assert_eq!(stats.total_zero_deltas, 1);
        assert_eq!(stats.total_captures_written, 0);

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_capture_engine_with_high_delta() {
        let tmp = std::env::temp_dir().join("titan_test_capture_high.jsonl");
        let _ = fs::remove_file(&tmp);

        let engine = PredictionCaptureEngine::new(Some(tmp.clone()), true);
        engine.start_interaction("test-3", "search for rust", "hash123");

        let event = PredictionEvent {
            action: "web_search".to_string(),
            action_input: serde_json::json!({"query": "rust"}),
            observation: "some results".to_string(),
            error_delta: "Prediction mismatch: expected python results".to_string(),
            delta_is_high: true,
            ..PredictionEvent::new()
        };
        engine.record_event("test-3", event);

        let result = engine.end_interaction("test-3", "Here are the results", true);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), tmp);

        let stats = engine.stats();
        assert_eq!(stats.total_high_deltas, 1);
        assert_eq!(stats.total_captures_written, 1);

        // Verify JSONL content
        let content = fs::read_to_string(&tmp).unwrap();
        let record: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(record["capture_id"], "test-3");
        assert_eq!(record["high_delta_count"], 1);
        assert_eq!(record["task_succeeded"], true);

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_capture_engine_unknown_interaction() {
        let engine = PredictionCaptureEngine::new(None, true);
        // Recording to unknown interaction should be a no-op
        engine.record_event(
            "nonexistent",
            PredictionEvent {
                delta_is_high: true,
                ..PredictionEvent::new()
            },
        );
        let result = engine.end_interaction("nonexistent", "", false);
        assert!(result.is_none());
    }

    #[test]
    fn test_interaction_id_unique() {
        let id1 = PredictionCaptureEngine::new_interaction_id();
        let id2 = PredictionCaptureEngine::new_interaction_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_stats() {
        let engine = PredictionCaptureEngine::new(None, true);
        let stats = engine.stats();
        assert!(stats.enabled);
        assert_eq!(stats.active_interactions, 0);
        assert_eq!(stats.total_high_deltas, 0);
    }

    #[test]
    fn test_final_answer_truncation() {
        let tmp = std::env::temp_dir().join("titan_test_truncation.jsonl");
        let _ = fs::remove_file(&tmp);

        let engine = PredictionCaptureEngine::new(Some(tmp.clone()), true);
        engine.start_interaction("test-trunc", "test", "");

        engine.record_event(
            "test-trunc",
            PredictionEvent {
                delta_is_high: true,
                ..PredictionEvent::new()
            },
        );

        let long_answer = "a".repeat(5000);
        engine.end_interaction("test-trunc", &long_answer, true);

        let content = fs::read_to_string(&tmp).unwrap();
        let record: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        let stored_answer = record["final_answer"].as_str().unwrap();
        assert_eq!(stored_answer.len(), 2000);

        let _ = fs::remove_file(&tmp);
    }
}
