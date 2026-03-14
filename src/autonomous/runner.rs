//! Autonomous Runner — orchestrates multi-step task execution.
//!
//! The [`AutonomousRunner`] manages the lifecycle of autonomous tasks:
//! creation, planning, step-by-step advancement, checkpointing,
//! approval gating, failure handling, and cancellation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::{
    now_secs, AutonomousTask, TaskCheckpoint, TaskConfig, TaskStatus, TaskStep,
};

// ─────────────────────────────────────────────────────────────────────────────
// RunnerStats
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate statistics for the autonomous runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerStats {
    /// Total tasks created.
    pub tasks_created: u64,
    /// Tasks that completed successfully.
    pub tasks_completed: u64,
    /// Tasks that failed.
    pub tasks_failed: u64,
    /// Total steps executed across all tasks.
    pub steps_executed: u64,
    /// Total approval requests issued.
    pub approvals_requested: u64,
}

impl RunnerStats {
    fn new() -> Self {
        Self {
            tasks_created: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            steps_executed: 0,
            approvals_requested: 0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AutonomousRunner
// ─────────────────────────────────────────────────────────────────────────────

/// Orchestrates the execution of autonomous multi-step tasks.
///
/// Manages task creation, planning, step advancement with checkpointing,
/// approval gates for risky operations, and failure/cancellation handling.
pub struct AutonomousRunner {
    /// All tasks keyed by ID.
    tasks: HashMap<String, AutonomousTask>,
    /// ID of the currently active task (if any).
    active_task: Option<String>,
    /// Risk threshold below which steps are auto-approved.
    auto_approve_threshold: f64,
    /// Aggregate statistics.
    stats: RunnerStats,
}

impl AutonomousRunner {
    /// Create a new autonomous runner.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            active_task: None,
            auto_approve_threshold: 0.3,
            stats: RunnerStats::new(),
        }
    }

    /// Create a new task and register it. Returns a reference to the created task.
    pub fn create_task(
        &mut self,
        description: &str,
        config: Option<TaskConfig>,
    ) -> &AutonomousTask {
        let cfg = config.unwrap_or_default();
        let task = AutonomousTask::new(description, cfg);
        let id = task.id.clone();
        self.tasks.insert(id.clone(), task);
        self.stats.tasks_created += 1;
        self.tasks.get(&id).unwrap()
    }

    /// Assign a plan (list of steps) to a task. Transitions the task to `Planning`.
    pub fn plan_task(&mut self, task_id: &str, steps: Vec<TaskStep>) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        if task.status != TaskStatus::Pending {
            return Err(format!(
                "Cannot plan task {task_id}: status is {:?}, expected Pending",
                task.status
            ));
        }

        task.total_steps = steps.len();
        task.plan = steps;
        task.status = TaskStatus::Planning;
        Ok(())
    }

    /// Advance the task by one step, recording the result.
    ///
    /// Returns the new task status. If the task needs approval for the
    /// current step, it transitions to `WaitingApproval` instead.
    pub fn advance_step(&mut self, task_id: &str, result: &str) -> Result<TaskStatus, String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        // First advancement transitions from Planning to Running.
        if task.status == TaskStatus::Planning {
            task.status = TaskStatus::Running;
            self.active_task = Some(task_id.to_string());
        }

        if task.status != TaskStatus::Running {
            return Err(format!(
                "Cannot advance task {task_id}: status is {:?}, expected Running",
                task.status
            ));
        }

        // Timestamp the step transition.
        let transition_ts = now_secs();

        // Record the result.
        task.results.push(format!("[{:.3}] {}", transition_ts, result));
        task.current_step += 1;
        self.stats.steps_executed += 1;

        // Create checkpoint if needed.
        if task.should_checkpoint() {
            let state = serde_json::json!({
                "current_step": task.current_step,
                "results_count": task.results.len(),
            });
            let ckpt = TaskCheckpoint::new(task_id, task.current_step, state);
            task.checkpoints.push(ckpt);
        }

        // Check if task is complete.
        if task.current_step >= task.total_steps {
            task.status = TaskStatus::Completed;
            self.stats.tasks_completed += 1;
            self.active_task = None;
            return Ok(TaskStatus::Completed);
        }

        // Check if next step needs approval.
        if task.needs_approval() {
            task.status = TaskStatus::WaitingApproval;
            self.stats.approvals_requested += 1;
            return Ok(TaskStatus::WaitingApproval);
        }

        Ok(TaskStatus::Running)
    }

    /// Record a step failure. Transitions the task to `Failed`.
    pub fn fail_step(&mut self, task_id: &str, error: &str) -> Result<TaskStatus, String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        task.errors.push(error.to_string());
        task.status = TaskStatus::Failed;
        self.stats.tasks_failed += 1;

        if self.active_task.as_deref() == Some(task_id) {
            self.active_task = None;
        }

        Ok(TaskStatus::Failed)
    }

    /// Resume a task that was waiting for approval (after approval is granted).
    pub fn resume_task(&mut self, task_id: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        if task.status != TaskStatus::WaitingApproval && task.status != TaskStatus::Paused {
            return Err(format!(
                "Cannot resume task {task_id}: status is {:?}, expected WaitingApproval or Paused",
                task.status
            ));
        }

        task.status = TaskStatus::Running;
        Ok(())
    }

    /// Pause a running task.
    pub fn pause_task(&mut self, task_id: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        if task.status != TaskStatus::Running {
            return Err(format!(
                "Cannot pause task {task_id}: status is {:?}, expected Running",
                task.status
            ));
        }

        task.status = TaskStatus::Paused;
        Ok(())
    }

    /// Create a manual checkpoint for the given task.
    pub fn checkpoint(&mut self, task_id: &str) -> Option<TaskCheckpoint> {
        let task = self.tasks.get_mut(task_id)?;

        let state = serde_json::json!({
            "current_step": task.current_step,
            "results_count": task.results.len(),
            "errors_count": task.errors.len(),
            "status": format!("{:?}", task.status),
        });
        let ckpt = TaskCheckpoint::new(task_id, task.current_step, state);
        task.checkpoints.push(ckpt.clone());
        Some(ckpt)
    }

    /// Cancel a task. Returns `true` if the task was found and cancelled.
    pub fn cancel_task(&mut self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            if task.status.is_terminal() {
                return false;
            }
            task.status = TaskStatus::Cancelled;
            if self.active_task.as_deref() == Some(task_id) {
                self.active_task = None;
            }
            true
        } else {
            false
        }
    }

    /// Get a reference to a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<&AutonomousTask> {
        self.tasks.get(task_id)
    }

    /// List all tasks.
    pub fn list_tasks(&self) -> Vec<&AutonomousTask> {
        self.tasks.values().collect()
    }

    /// Get the ID of the currently active task.
    pub fn active_task_id(&self) -> Option<&str> {
        self.active_task.as_deref()
    }

    /// Get aggregate runner statistics.
    pub fn get_stats(&self) -> &RunnerStats {
        &self.stats
    }

    /// Total number of tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

impl Default for AutonomousRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_steps(count: usize, risk: f64) -> Vec<TaskStep> {
        (0..count)
            .map(|i| {
                TaskStep::new(
                    &format!("action_{i}"),
                    &format!("Step {i}"),
                    serde_json::json!({"index": i}),
                    risk,
                    "output",
                )
            })
            .collect()
    }

    #[test]
    fn test_runner_new() {
        let runner = AutonomousRunner::new();
        assert_eq!(runner.task_count(), 0);
        assert!(runner.active_task_id().is_none());
        assert_eq!(runner.get_stats().tasks_created, 0);
    }

    #[test]
    fn test_create_task() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Build a report", None);
        assert!(task.id.starts_with("task_"));
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(runner.task_count(), 1);
        assert_eq!(runner.get_stats().tasks_created, 1);
    }

    #[test]
    fn test_create_task_with_config() {
        let mut runner = AutonomousRunner::new();
        let config = TaskConfig::new(10, 0.5, 2);
        let task = runner.create_task("Custom task", Some(config));
        assert_eq!(task.config.max_steps, 10);
        assert!((task.config.auto_approve_threshold - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_plan_task() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Plan me", None);
        let id = task.id.clone();

        let steps = make_steps(3, 0.1);
        runner.plan_task(&id, steps).unwrap();

        let task = runner.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Planning);
        assert_eq!(task.total_steps, 3);
        assert_eq!(task.plan.len(), 3);
    }

    #[test]
    fn test_plan_task_wrong_status() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Test", None);
        let id = task.id.clone();

        let steps = make_steps(2, 0.1);
        runner.plan_task(&id, steps.clone()).unwrap();

        // Planning again should fail (status is Planning, not Pending).
        let result = runner.plan_task(&id, steps);
        assert!(result.is_err());
    }

    #[test]
    fn test_advance_step_full_completion() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Complete me", None);
        let id = task.id.clone();

        runner.plan_task(&id, make_steps(3, 0.1)).unwrap();

        // Advance through all 3 steps.
        let s1 = runner.advance_step(&id, "result_0").unwrap();
        assert_eq!(s1, TaskStatus::Running);

        let s2 = runner.advance_step(&id, "result_1").unwrap();
        assert_eq!(s2, TaskStatus::Running);

        let s3 = runner.advance_step(&id, "result_2").unwrap();
        assert_eq!(s3, TaskStatus::Completed);

        let task = runner.get_task(&id).unwrap();
        assert_eq!(task.results.len(), 3);
        assert_eq!(runner.get_stats().steps_executed, 3);
        assert_eq!(runner.get_stats().tasks_completed, 1);
    }

    #[test]
    fn test_advance_step_triggers_approval() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Risky task", None);
        let id = task.id.clone();

        // First step low-risk, second step high-risk.
        let steps = vec![
            TaskStep::new("safe", "Safe step", serde_json::json!({}), 0.1, "out"),
            TaskStep::new("risky", "Risky step", serde_json::json!({}), 0.9, "out"),
        ];
        runner.plan_task(&id, steps).unwrap();

        // Advance past step 0 — should see WaitingApproval for step 1.
        let status = runner.advance_step(&id, "safe_result").unwrap();
        assert_eq!(status, TaskStatus::WaitingApproval);
        assert_eq!(runner.get_stats().approvals_requested, 1);
    }

    #[test]
    fn test_fail_step() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Fail me", None);
        let id = task.id.clone();

        runner.plan_task(&id, make_steps(3, 0.1)).unwrap();
        runner.advance_step(&id, "step_0_ok").unwrap();

        let status = runner.fail_step(&id, "something broke").unwrap();
        assert_eq!(status, TaskStatus::Failed);

        let task = runner.get_task(&id).unwrap();
        assert_eq!(task.errors.len(), 1);
        assert_eq!(task.errors[0], "something broke");
        assert_eq!(runner.get_stats().tasks_failed, 1);
    }

    #[test]
    fn test_checkpoint_manual() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Checkpoint test", None);
        let id = task.id.clone();

        let ckpt = runner.checkpoint(&id).unwrap();
        assert!(ckpt.id.contains("task_"));
        assert_eq!(ckpt.step, 0);

        let task = runner.get_task(&id).unwrap();
        assert_eq!(task.checkpoints.len(), 1);
    }

    #[test]
    fn test_cancel_task() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Cancel me", None);
        let id = task.id.clone();

        assert!(runner.cancel_task(&id));
        let task = runner.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Cancelled);

        // Cancelling again should fail (already terminal).
        assert!(!runner.cancel_task(&id));
    }

    #[test]
    fn test_cancel_nonexistent() {
        let mut runner = AutonomousRunner::new();
        assert!(!runner.cancel_task("nonexistent"));
    }

    #[test]
    fn test_list_tasks() {
        let mut runner = AutonomousRunner::new();
        runner.create_task("Task A", None);
        runner.create_task("Task B", None);
        runner.create_task("Task C", None);

        assert_eq!(runner.list_tasks().len(), 3);
    }

    #[test]
    fn test_resume_task() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Resume test", None);
        let id = task.id.clone();

        let steps = vec![
            TaskStep::new("safe", "s", serde_json::json!({}), 0.1, ""),
            TaskStep::new("risky", "r", serde_json::json!({}), 0.9, ""),
        ];
        runner.plan_task(&id, steps).unwrap();
        runner.advance_step(&id, "ok").unwrap();

        // Should be waiting for approval.
        assert_eq!(
            runner.get_task(&id).unwrap().status,
            TaskStatus::WaitingApproval
        );

        runner.resume_task(&id).unwrap();
        assert_eq!(runner.get_task(&id).unwrap().status, TaskStatus::Running);
    }

    #[test]
    fn test_pause_and_resume() {
        let mut runner = AutonomousRunner::new();
        let task = runner.create_task("Pause test", None);
        let id = task.id.clone();

        runner.plan_task(&id, make_steps(5, 0.1)).unwrap();
        runner.advance_step(&id, "ok").unwrap(); // transitions to Running

        runner.pause_task(&id).unwrap();
        assert_eq!(runner.get_task(&id).unwrap().status, TaskStatus::Paused);

        runner.resume_task(&id).unwrap();
        assert_eq!(runner.get_task(&id).unwrap().status, TaskStatus::Running);
    }

    #[test]
    fn test_active_task_tracking() {
        let mut runner = AutonomousRunner::new();
        assert!(runner.active_task_id().is_none());

        let task = runner.create_task("Active tracking", None);
        let id = task.id.clone();

        runner.plan_task(&id, make_steps(2, 0.1)).unwrap();
        runner.advance_step(&id, "ok").unwrap();

        assert_eq!(runner.active_task_id(), Some(id.as_str()));

        runner.advance_step(&id, "done").unwrap();
        assert!(runner.active_task_id().is_none()); // completed, no longer active
    }

    #[test]
    fn test_automatic_checkpoint() {
        let mut runner = AutonomousRunner::new();
        let config = TaskConfig::new(50, 0.3, 2); // checkpoint every 2 steps
        let task = runner.create_task("Checkpoint auto", Some(config));
        let id = task.id.clone();

        runner.plan_task(&id, make_steps(6, 0.1)).unwrap();

        // Step 0 -> step 1 (current_step = 1, not checkpoint)
        runner.advance_step(&id, "r0").unwrap();
        assert_eq!(runner.get_task(&id).unwrap().checkpoints.len(), 0);

        // Step 1 -> step 2 (current_step = 2, checkpoint!)
        runner.advance_step(&id, "r1").unwrap();
        assert_eq!(runner.get_task(&id).unwrap().checkpoints.len(), 1);

        // Step 2 -> step 3 (current_step = 3, no checkpoint)
        runner.advance_step(&id, "r2").unwrap();
        assert_eq!(runner.get_task(&id).unwrap().checkpoints.len(), 1);

        // Step 3 -> step 4 (current_step = 4, checkpoint!)
        runner.advance_step(&id, "r3").unwrap();
        assert_eq!(runner.get_task(&id).unwrap().checkpoints.len(), 2);
    }
}
