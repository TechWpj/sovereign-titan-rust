//! Data structures for autonomous task execution.
//!
//! Defines the core types that underpin the autonomous runner:
//! task status lifecycle, checkpointing, approval requests,
//! step definitions, and task configuration.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static TASK_COUNTER: AtomicU64 = AtomicU64::new(0);

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Current Unix epoch timestamp in seconds (fractional).
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ─────────────────────────────────────────────────────────────────────────────
// TaskStatus
// ─────────────────────────────────────────────────────────────────────────────

/// Lifecycle status of an autonomous task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task has been created but not yet started.
    Pending,
    /// Task is being planned (step decomposition).
    Planning,
    /// Task is actively executing steps.
    Running,
    /// Task execution is paused.
    Paused,
    /// Task is waiting for human approval on a risky step.
    WaitingApproval,
    /// Task completed successfully.
    Completed,
    /// Task failed during execution.
    Failed,
    /// Task was cancelled by the user or system.
    Cancelled,
}

impl TaskStatus {
    /// Whether the task is in a terminal state (completed, failed, or cancelled).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    /// Whether the task is actively running or planning.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            TaskStatus::Planning | TaskStatus::Running | TaskStatus::WaitingApproval
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TaskCheckpoint
// ─────────────────────────────────────────────────────────────────────────────

/// A snapshot of task state at a particular step, enabling recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCheckpoint {
    /// Unique checkpoint identifier.
    pub id: String,
    /// ID of the task this checkpoint belongs to.
    pub task_id: String,
    /// Step index at the time of checkpointing.
    pub step: usize,
    /// Serialized task state at checkpoint time.
    pub state: serde_json::Value,
    /// Unix timestamp when the checkpoint was created.
    pub timestamp: f64,
}

impl TaskCheckpoint {
    /// Create a new checkpoint for the given task and step.
    pub fn new(task_id: &str, step: usize, state: serde_json::Value) -> Self {
        let ts = now_secs();
        Self {
            id: format!("ckpt_{}_{}", task_id, ts as u64),
            task_id: task_id.to_string(),
            step,
            state,
            timestamp: ts,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ApprovalRequest
// ─────────────────────────────────────────────────────────────────────────────

/// A request for human approval before executing a risky step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique request identifier.
    pub id: String,
    /// ID of the task requesting approval.
    pub task_id: String,
    /// Step index that requires approval.
    pub step: usize,
    /// Description of the action requiring approval.
    pub action: String,
    /// Risk level of the action (0.0 = safe, 1.0 = critical).
    pub risk_level: f64,
    /// Additional details about the action.
    pub details: serde_json::Value,
    /// Unix timestamp when the request was created.
    pub created_at: f64,
    /// Whether the request has been resolved.
    pub resolved: bool,
    /// Whether the request was approved (only meaningful if resolved).
    pub approved: bool,
}

impl ApprovalRequest {
    /// Create a new approval request.
    pub fn new(
        task_id: &str,
        step: usize,
        action: &str,
        risk_level: f64,
        details: serde_json::Value,
    ) -> Self {
        let ts = now_secs();
        Self {
            id: format!("appr_{}_{}", task_id, ts as u64),
            task_id: task_id.to_string(),
            step,
            action: action.to_string(),
            risk_level: risk_level.clamp(0.0, 1.0),
            details,
            created_at: ts,
            resolved: false,
            approved: false,
        }
    }

    /// Resolve the approval request.
    pub fn resolve(&mut self, approved: bool) {
        self.resolved = true;
        self.approved = approved;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TaskConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for an autonomous task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskConfig {
    /// Maximum number of steps before the task is forcibly stopped.
    pub max_steps: usize,
    /// Risk threshold below which steps are auto-approved (0.0 to 1.0).
    pub auto_approve_threshold: f64,
    /// How often to create checkpoints (every N steps).
    pub checkpoint_interval: usize,
}

impl TaskConfig {
    /// Create a task config with custom values.
    pub fn new(max_steps: usize, auto_approve_threshold: f64, checkpoint_interval: usize) -> Self {
        Self {
            max_steps,
            auto_approve_threshold: auto_approve_threshold.clamp(0.0, 1.0),
            checkpoint_interval: checkpoint_interval.max(1),
        }
    }
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            max_steps: 50,
            auto_approve_threshold: 0.3,
            checkpoint_interval: 5,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TaskStep
// ─────────────────────────────────────────────────────────────────────────────

/// A single step in an autonomous task plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    /// The action to perform (e.g., tool name or operation).
    pub action: String,
    /// Human-readable description of this step.
    pub description: String,
    /// Parameters for the action.
    pub parameters: serde_json::Value,
    /// Risk level of this step (0.0 = safe, 1.0 = critical).
    pub risk_level: f64,
    /// Description of the expected output.
    pub expected_output: String,
}

impl TaskStep {
    /// Create a new task step.
    pub fn new(
        action: &str,
        description: &str,
        parameters: serde_json::Value,
        risk_level: f64,
        expected_output: &str,
    ) -> Self {
        Self {
            action: action.to_string(),
            description: description.to_string(),
            parameters,
            risk_level: risk_level.clamp(0.0, 1.0),
            expected_output: expected_output.to_string(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AutonomousTask
// ─────────────────────────────────────────────────────────────────────────────

/// A multi-step autonomous task with planning, execution, and checkpointing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousTask {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable description of the task.
    pub description: String,
    /// Configuration for this task.
    pub config: TaskConfig,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Current step index (0-based).
    pub current_step: usize,
    /// Total number of planned steps.
    pub total_steps: usize,
    /// The planned steps.
    pub plan: Vec<TaskStep>,
    /// Results from completed steps.
    pub results: Vec<String>,
    /// Errors encountered during execution.
    pub errors: Vec<String>,
    /// Saved checkpoints.
    pub checkpoints: Vec<TaskCheckpoint>,
    /// Unix timestamp when the task was created.
    pub created_at: f64,
}

impl AutonomousTask {
    /// Create a new task in `Pending` status.
    pub fn new(description: &str, config: TaskConfig) -> Self {
        let ts = now_secs();
        Self {
            id: format!("task_{}_{}", ts as u64, TASK_COUNTER.fetch_add(1, Ordering::Relaxed)),
            description: description.to_string(),
            config,
            status: TaskStatus::Pending,
            current_step: 0,
            total_steps: 0,
            plan: Vec::new(),
            results: Vec::new(),
            errors: Vec::new(),
            checkpoints: Vec::new(),
            created_at: ts,
        }
    }

    /// Whether the task has a plan assigned.
    pub fn has_plan(&self) -> bool {
        !self.plan.is_empty()
    }

    /// Progress as a fraction (0.0 to 1.0).
    pub fn progress(&self) -> f64 {
        if self.total_steps == 0 {
            0.0
        } else {
            self.current_step as f64 / self.total_steps as f64
        }
    }

    /// Whether this step should create a checkpoint based on the interval config.
    pub fn should_checkpoint(&self) -> bool {
        self.current_step > 0 && self.current_step % self.config.checkpoint_interval == 0
    }

    /// Whether the current step's risk level requires human approval.
    pub fn needs_approval(&self) -> bool {
        if let Some(step) = self.plan.get(self.current_step) {
            step.risk_level > self.config.auto_approve_threshold
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_terminal() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
    }

    #[test]
    fn test_task_status_active() {
        assert!(TaskStatus::Planning.is_active());
        assert!(TaskStatus::Running.is_active());
        assert!(TaskStatus::WaitingApproval.is_active());
        assert!(!TaskStatus::Pending.is_active());
        assert!(!TaskStatus::Completed.is_active());
        assert!(!TaskStatus::Paused.is_active());
    }

    #[test]
    fn test_checkpoint_new() {
        let ckpt = TaskCheckpoint::new("task_123", 5, serde_json::json!({"key": "value"}));
        assert!(ckpt.id.starts_with("ckpt_task_123_"));
        assert_eq!(ckpt.task_id, "task_123");
        assert_eq!(ckpt.step, 5);
        assert!(ckpt.timestamp > 0.0);
    }

    #[test]
    fn test_approval_request_new() {
        let req = ApprovalRequest::new(
            "task_456",
            3,
            "delete_file",
            0.8,
            serde_json::json!({"path": "/tmp/data"}),
        );
        assert!(req.id.starts_with("appr_task_456_"));
        assert_eq!(req.step, 3);
        assert!((req.risk_level - 0.8).abs() < f64::EPSILON);
        assert!(!req.resolved);
        assert!(!req.approved);
    }

    #[test]
    fn test_approval_request_resolve() {
        let mut req = ApprovalRequest::new("t", 0, "action", 0.5, serde_json::json!(null));
        req.resolve(true);
        assert!(req.resolved);
        assert!(req.approved);

        let mut req2 = ApprovalRequest::new("t", 0, "action", 0.5, serde_json::json!(null));
        req2.resolve(false);
        assert!(req2.resolved);
        assert!(!req2.approved);
    }

    #[test]
    fn test_task_config_default() {
        let config = TaskConfig::default();
        assert_eq!(config.max_steps, 50);
        assert!((config.auto_approve_threshold - 0.3).abs() < f64::EPSILON);
        assert_eq!(config.checkpoint_interval, 5);
    }

    #[test]
    fn test_task_config_custom() {
        let config = TaskConfig::new(100, 0.5, 10);
        assert_eq!(config.max_steps, 100);
        assert!((config.auto_approve_threshold - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.checkpoint_interval, 10);
    }

    #[test]
    fn test_task_config_clamped() {
        let config = TaskConfig::new(10, 1.5, 0);
        assert!((config.auto_approve_threshold - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.checkpoint_interval, 1); // min 1
    }

    #[test]
    fn test_task_step_new() {
        let step = TaskStep::new(
            "web_search",
            "Search for documentation",
            serde_json::json!({"query": "Rust async"}),
            0.1,
            "Search results",
        );
        assert_eq!(step.action, "web_search");
        assert!((step.risk_level - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_autonomous_task_new() {
        let task = AutonomousTask::new("Build a report", TaskConfig::default());
        assert!(task.id.starts_with("task_"));
        assert_eq!(task.description, "Build a report");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.current_step, 0);
        assert_eq!(task.total_steps, 0);
        assert!(!task.has_plan());
    }

    #[test]
    fn test_task_progress() {
        let mut task = AutonomousTask::new("test", TaskConfig::default());
        assert!((task.progress() - 0.0).abs() < f64::EPSILON);

        task.total_steps = 10;
        task.current_step = 5;
        assert!((task.progress() - 0.5).abs() < f64::EPSILON);

        task.current_step = 10;
        assert!((task.progress() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_task_should_checkpoint() {
        let mut task = AutonomousTask::new("test", TaskConfig::default());
        // Default checkpoint_interval is 5.
        task.current_step = 0;
        assert!(!task.should_checkpoint()); // step 0 never checkpoints

        task.current_step = 5;
        assert!(task.should_checkpoint());

        task.current_step = 7;
        assert!(!task.should_checkpoint());

        task.current_step = 10;
        assert!(task.should_checkpoint());
    }

    #[test]
    fn test_task_needs_approval() {
        let mut task = AutonomousTask::new("test", TaskConfig::default());
        // auto_approve_threshold = 0.3

        // No plan yet — no approval needed.
        assert!(!task.needs_approval());

        // Add a low-risk step.
        task.plan.push(TaskStep::new("read", "read file", serde_json::json!({}), 0.1, "content"));
        task.total_steps = 1;
        assert!(!task.needs_approval()); // 0.1 <= 0.3

        // Add a high-risk step and advance to it.
        task.plan.push(TaskStep::new("delete", "delete file", serde_json::json!({}), 0.9, "deleted"));
        task.total_steps = 2;
        task.current_step = 1;
        assert!(task.needs_approval()); // 0.9 > 0.3
    }

    #[test]
    fn test_risk_level_clamped() {
        let step = TaskStep::new("test", "test", serde_json::json!({}), 2.0, "out");
        assert!((step.risk_level - 1.0).abs() < f64::EPSILON);

        let req = ApprovalRequest::new("t", 0, "a", -0.5, serde_json::json!(null));
        assert!((req.risk_level - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_now_secs_returns_positive() {
        let ts = now_secs();
        assert!(ts > 0.0);
    }
}
