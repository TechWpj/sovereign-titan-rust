//! Workflow Engine — schedule-based and event-based background automation.
//!
//! Manages workflows that fire on time intervals or event matches.
//! Ported from Python `engine.py` and `triggers.py`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use regex::Regex;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Triggers
// ─────────────────────────────────────────────────────────────────────────────

/// A trigger condition that determines when a workflow should execute.
#[derive(Debug, Clone)]
pub enum WorkflowTrigger {
    /// Fire on a time interval (e.g., every N seconds).
    Schedule {
        interval_secs: u64,
        last_run: Option<Instant>,
    },
    /// Fire when a matching event is received.
    Event {
        event_type: String,
        conditions: HashMap<String, MatchCondition>,
    },
}

/// Matching condition for event fields.
#[derive(Debug, Clone)]
pub enum MatchCondition {
    /// Exact equality.
    Exact(String),
    /// Substring containment.
    Contains(String),
    /// Regex pattern match.
    Pattern(String),
}

impl WorkflowTrigger {
    /// Create a schedule trigger that fires every `secs` seconds.
    pub fn every_secs(secs: u64) -> Self {
        Self::Schedule {
            interval_secs: secs,
            last_run: None,
        }
    }

    /// Create an event trigger for the given event type.
    pub fn on_event(event_type: impl Into<String>) -> Self {
        Self::Event {
            event_type: event_type.into(),
            conditions: HashMap::new(),
        }
    }

    /// Builder: add an exact-match condition (event trigger only).
    pub fn with_condition(mut self, key: impl Into<String>, cond: MatchCondition) -> Self {
        if let Self::Event { ref mut conditions, .. } = self {
            conditions.insert(key.into(), cond);
        }
        self
    }

    /// Check if this trigger matches the given event.
    fn check_event(&self, event: &WorkflowEvent) -> bool {
        match self {
            Self::Schedule { interval_secs, last_run } => {
                if event.event_type != "schedule_tick" {
                    return false;
                }
                match last_run {
                    None => true,
                    Some(last) => last.elapsed().as_secs() >= *interval_secs,
                }
            }
            Self::Event { event_type, conditions } => {
                if event.event_type != *event_type {
                    return false;
                }
                // Check all conditions.
                for (key, cond) in conditions {
                    let actual = event.data.get(key).map(|s| s.as_str()).unwrap_or("");
                    match cond {
                        MatchCondition::Exact(expected) => {
                            if actual != expected {
                                return false;
                            }
                        }
                        MatchCondition::Contains(substr) => {
                            if !actual.contains(substr.as_str()) {
                                return false;
                            }
                        }
                        MatchCondition::Pattern(pattern) => {
                            if let Ok(re) = Regex::new(pattern) {
                                if !re.is_match(actual) {
                                    return false;
                                }
                            } else {
                                return false;
                            }
                        }
                    }
                }
                true
            }
        }
    }

    /// Mark this trigger as fired (updates last_run for schedule triggers).
    fn mark_fired(&mut self) {
        if let Self::Schedule { last_run, .. } = self {
            *last_run = Some(Instant::now());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Event
// ─────────────────────────────────────────────────────────────────────────────

/// An event that can trigger workflows.
#[derive(Debug, Clone)]
pub struct WorkflowEvent {
    pub event_type: String,
    pub data: HashMap<String, String>,
}

impl WorkflowEvent {
    /// Create a new event.
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            data: HashMap::new(),
        }
    }

    /// Builder: add a data field.
    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Workflow
// ─────────────────────────────────────────────────────────────────────────────

/// Action function type: receives event data, returns a result string.
pub type WorkflowAction = Arc<
    dyn Fn(
            WorkflowEvent,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// A workflow definition with triggers and an action.
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger: WorkflowTrigger,
    pub action: WorkflowAction,
    pub enabled: bool,
    pub run_count: u64,
    pub error_count: u64,
}

impl Workflow {
    /// Create a new workflow.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        trigger: WorkflowTrigger,
        action: WorkflowAction,
    ) -> Self {
        let name = name.into();
        let id = format!("wf_{}_{}", name.to_lowercase().replace(' ', "_"), uuid::Uuid::new_v4().as_simple());
        Self {
            id,
            name,
            description: description.into(),
            trigger,
            action,
            enabled: true,
            run_count: 0,
            error_count: 0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WorkflowEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Manages and executes workflows based on events and schedules.
pub struct WorkflowEngine {
    workflows: HashMap<String, Workflow>,
    event_tx: mpsc::Sender<WorkflowEvent>,
    event_rx: Option<mpsc::Receiver<WorkflowEvent>>,
    running: Arc<Mutex<bool>>,
}

impl WorkflowEngine {
    /// Create a new workflow engine.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<WorkflowEvent>(256);
        Self {
            workflows: HashMap::new(),
            event_tx: tx,
            event_rx: Some(rx),
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Add a workflow to the engine. Returns the workflow ID.
    pub fn add_workflow(&mut self, workflow: Workflow) -> String {
        let id = workflow.id.clone();
        info!("Workflow registered: {} ({})", workflow.name, id);
        self.workflows.insert(id.clone(), workflow);
        id
    }

    /// Remove a workflow by ID.
    pub fn remove_workflow(&mut self, id: &str) -> bool {
        self.workflows.remove(id).is_some()
    }

    /// Enable or disable a workflow.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(wf) = self.workflows.get_mut(id) {
            wf.enabled = enabled;
        }
    }

    /// Get a sender to emit events into the engine.
    pub fn event_sender(&self) -> mpsc::Sender<WorkflowEvent> {
        self.event_tx.clone()
    }

    /// List all workflows with their stats.
    pub fn list_workflows(&self) -> Vec<WorkflowInfo> {
        self.workflows
            .values()
            .map(|wf| WorkflowInfo {
                id: wf.id.clone(),
                name: wf.name.clone(),
                description: wf.description.clone(),
                enabled: wf.enabled,
                run_count: wf.run_count,
                error_count: wf.error_count,
            })
            .collect()
    }

    /// Get aggregate stats.
    pub fn stats(&self) -> EngineStats {
        EngineStats {
            total_workflows: self.workflows.len(),
            enabled: self.workflows.values().filter(|w| w.enabled).count(),
            total_runs: self.workflows.values().map(|w| w.run_count).sum(),
            total_errors: self.workflows.values().map(|w| w.error_count).sum(),
        }
    }

    /// Process a single event through all workflows.
    pub async fn process_event(&mut self, event: &WorkflowEvent) -> Vec<WorkflowResult> {
        let mut results = Vec::new();

        // Collect IDs of workflows that should fire.
        let to_fire: Vec<String> = self
            .workflows
            .iter()
            .filter(|(_, wf)| wf.enabled && wf.trigger.check_event(event))
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_fire {
            if let Some(wf) = self.workflows.get_mut(&id) {
                wf.trigger.mark_fired();
                wf.run_count += 1;

                let action = Arc::clone(&wf.action);
                let name = wf.name.clone();
                let wf_id = wf.id.clone();

                info!("Running workflow: {}", name);
                match action(event.clone()).await {
                    Ok(output) => {
                        results.push(WorkflowResult {
                            workflow_id: wf_id,
                            success: true,
                            output: Some(output),
                            error: None,
                        });
                    }
                    Err(e) => {
                        wf.error_count += 1;
                        warn!("Workflow '{}' failed: {}", name, e);
                        results.push(WorkflowResult {
                            workflow_id: wf_id,
                            success: false,
                            output: None,
                            error: Some(e),
                        });
                    }
                }
            }
        }

        results
    }

    /// Start the scheduler loop. Processes events from the channel and
    /// emits periodic schedule ticks.
    ///
    /// This takes ownership of the event receiver, so it can only be
    /// called once. Runs until `stop_scheduler()` is called.
    pub async fn start_scheduler(&mut self) {
        let Some(mut rx) = self.event_rx.take() else {
            warn!("Scheduler already started (receiver consumed)");
            return;
        };

        *self.running.lock().await = true;
        info!("Workflow scheduler started");

        while *self.running.lock().await {
            // Process any pending events (non-blocking drain).
            while let Ok(event) = rx.try_recv() {
                self.process_event(&event).await;
            }

            // Emit a schedule tick for time-based workflows.
            let tick = WorkflowEvent::new("schedule_tick");
            self.process_event(&tick).await;

            // Sleep before next tick.
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        info!("Workflow scheduler stopped");
    }

    /// Stop the scheduler loop.
    pub async fn stop_scheduler(&self) {
        *self.running.lock().await = false;
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Info types
// ─────────────────────────────────────────────────────────────────────────────

/// Summary info about a workflow.
#[derive(Debug, Clone)]
pub struct WorkflowInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub run_count: u64,
    pub error_count: u64,
}

/// Aggregate engine stats.
#[derive(Debug, Clone)]
pub struct EngineStats {
    pub total_workflows: usize,
    pub enabled: usize,
    pub total_runs: u64,
    pub total_errors: u64,
}

/// Result of a workflow execution.
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub workflow_id: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_noop_action() -> WorkflowAction {
        Arc::new(|_event| Box::pin(async { Ok("done".into()) }))
    }

    fn make_failing_action() -> WorkflowAction {
        Arc::new(|_event| Box::pin(async { Err("boom".into()) }))
    }

    #[test]
    fn test_workflow_creation() {
        let wf = Workflow::new(
            "Test Workflow",
            "A test workflow",
            WorkflowTrigger::every_secs(60),
            make_noop_action(),
        );
        assert!(wf.id.starts_with("wf_test_workflow_"));
        assert_eq!(wf.name, "Test Workflow");
        assert!(wf.enabled);
        assert_eq!(wf.run_count, 0);
    }

    #[test]
    fn test_schedule_trigger_first_fire() {
        let trigger = WorkflowTrigger::every_secs(60);
        let tick = WorkflowEvent::new("schedule_tick");

        // First check should fire (last_run is None).
        assert!(trigger.check_event(&tick));

        // Non-tick events should not fire.
        let other = WorkflowEvent::new("custom_event");
        assert!(!trigger.check_event(&other));
    }

    #[test]
    fn test_event_trigger_exact_match() {
        let trigger = WorkflowTrigger::on_event("security_alert")
            .with_condition("level", MatchCondition::Exact("HIGH".into()));

        let matching = WorkflowEvent::new("security_alert")
            .with_data("level", "HIGH");
        assert!(trigger.check_event(&matching));

        let non_matching = WorkflowEvent::new("security_alert")
            .with_data("level", "LOW");
        assert!(!trigger.check_event(&non_matching));

        let wrong_type = WorkflowEvent::new("other_event")
            .with_data("level", "HIGH");
        assert!(!trigger.check_event(&wrong_type));
    }

    #[test]
    fn test_event_trigger_contains() {
        let trigger = WorkflowTrigger::on_event("log")
            .with_condition("message", MatchCondition::Contains("error".into()));

        let matching = WorkflowEvent::new("log")
            .with_data("message", "An error occurred in module X");
        assert!(trigger.check_event(&matching));

        let non_matching = WorkflowEvent::new("log")
            .with_data("message", "All systems nominal");
        assert!(!trigger.check_event(&non_matching));
    }

    #[test]
    fn test_event_trigger_regex() {
        let trigger = WorkflowTrigger::on_event("log")
            .with_condition("code", MatchCondition::Pattern(r"^ERR-\d{3}$".into()));

        let matching = WorkflowEvent::new("log")
            .with_data("code", "ERR-404");
        assert!(trigger.check_event(&matching));

        let non_matching = WorkflowEvent::new("log")
            .with_data("code", "OK-200");
        assert!(!trigger.check_event(&non_matching));
    }

    #[tokio::test]
    async fn test_engine_add_remove_workflow() {
        let mut engine = WorkflowEngine::new();
        let wf = Workflow::new(
            "Test",
            "desc",
            WorkflowTrigger::on_event("test"),
            make_noop_action(),
        );
        let id = engine.add_workflow(wf);

        assert_eq!(engine.list_workflows().len(), 1);
        assert!(engine.remove_workflow(&id));
        assert!(engine.list_workflows().is_empty());
    }

    #[tokio::test]
    async fn test_engine_process_event() {
        let mut engine = WorkflowEngine::new();
        engine.add_workflow(Workflow::new(
            "Alert Handler",
            "Handle alerts",
            WorkflowTrigger::on_event("alert"),
            make_noop_action(),
        ));

        let event = WorkflowEvent::new("alert");
        let results = engine.process_event(&event).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output, Some("done".into()));
    }

    #[tokio::test]
    async fn test_engine_process_event_failure() {
        let mut engine = WorkflowEngine::new();
        let wf = Workflow::new(
            "Failing",
            "Will fail",
            WorkflowTrigger::on_event("test"),
            make_failing_action(),
        );
        let id = engine.add_workflow(wf);

        let event = WorkflowEvent::new("test");
        let results = engine.process_event(&event).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert_eq!(results[0].error, Some("boom".into()));

        // Error count should increment.
        let stats = engine.stats();
        assert_eq!(stats.total_errors, 1);
        assert_eq!(stats.total_runs, 1);

        // Remove to avoid unused warning.
        engine.remove_workflow(&id);
    }

    #[tokio::test]
    async fn test_engine_disabled_workflow_skipped() {
        let mut engine = WorkflowEngine::new();
        let wf = Workflow::new(
            "Disabled",
            "Should not run",
            WorkflowTrigger::on_event("test"),
            make_noop_action(),
        );
        let id = engine.add_workflow(wf);
        engine.set_enabled(&id, false);

        let event = WorkflowEvent::new("test");
        let results = engine.process_event(&event).await;
        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_stats() {
        let mut engine = WorkflowEngine::new();
        engine.add_workflow(Workflow::new(
            "A",
            "",
            WorkflowTrigger::on_event("x"),
            make_noop_action(),
        ));
        let id = engine.add_workflow(Workflow::new(
            "B",
            "",
            WorkflowTrigger::on_event("y"),
            make_noop_action(),
        ));
        engine.set_enabled(&id, false);

        let stats = engine.stats();
        assert_eq!(stats.total_workflows, 2);
        assert_eq!(stats.enabled, 1);
    }

    #[test]
    fn test_trigger_mark_fired() {
        let mut trigger = WorkflowTrigger::every_secs(5);
        trigger.mark_fired();
        if let WorkflowTrigger::Schedule { last_run, .. } = &trigger {
            assert!(last_run.is_some());
        } else {
            panic!("expected Schedule trigger");
        }
    }

    #[test]
    fn test_workflow_event_builder() {
        let event = WorkflowEvent::new("test_event")
            .with_data("key1", "value1")
            .with_data("key2", "value2");

        assert_eq!(event.event_type, "test_event");
        assert_eq!(event.data.get("key1").unwrap(), "value1");
        assert_eq!(event.data.get("key2").unwrap(), "value2");
    }
}
