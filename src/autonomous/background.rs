//! Background Task Manager — concurrent background work queue.
//!
//! Manages a pool of background tasks with concurrency limits,
//! progress tracking, and lifecycle management (submit, start,
//! complete, fail, cancel).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::types::now_secs;

static BG_TASK_COUNTER: AtomicU64 = AtomicU64::new(0);

// ─────────────────────────────────────────────────────────────────────────────
// BGTaskStatus
// ─────────────────────────────────────────────────────────────────────────────

/// Status of a background task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BGTaskStatus {
    /// Task is in the queue waiting to start.
    Queued,
    /// Task is currently executing.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed with an error.
    Failed,
    /// Task was cancelled before completion.
    Cancelled,
}

impl BGTaskStatus {
    /// Whether the task is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            BGTaskStatus::Completed | BGTaskStatus::Failed | BGTaskStatus::Cancelled
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ProgressEntry
// ─────────────────────────────────────────────────────────────────────────────

/// A progress update for a background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEntry {
    /// Human-readable progress message.
    pub message: String,
    /// Unix timestamp when the update was recorded.
    pub timestamp: f64,
}

impl ProgressEntry {
    /// Create a new progress entry with the current timestamp.
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            timestamp: now_secs(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BGTask
// ─────────────────────────────────────────────────────────────────────────────

/// A background task with lifecycle tracking and progress updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BGTask {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Current status.
    pub status: BGTaskStatus,
    /// Unix timestamp when the task was created (submitted).
    pub created_at: f64,
    /// Unix timestamp when the task started running.
    pub started_at: Option<f64>,
    /// Unix timestamp when the task completed (success or failure).
    pub completed_at: Option<f64>,
    /// Result value on successful completion.
    pub result: Option<String>,
    /// Error message on failure.
    pub error: Option<String>,
    /// Progress updates in chronological order.
    pub progress: Vec<ProgressEntry>,
}

impl BGTask {
    /// Create a new background task in `Queued` status.
    pub fn new(description: &str) -> Self {
        let ts = now_secs();
        Self {
            id: format!("bg_{}_{}", ts as u64, BG_TASK_COUNTER.fetch_add(1, Ordering::Relaxed)),
            description: description.to_string(),
            status: BGTaskStatus::Queued,
            created_at: ts,
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            progress: Vec::new(),
        }
    }

    /// Duration in seconds from creation to completion (or now if still running).
    pub fn elapsed_secs(&self) -> f64 {
        let end = self.completed_at.unwrap_or_else(now_secs);
        let start = self.started_at.unwrap_or(self.created_at);
        (end - start).max(0.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BGStats
// ─────────────────────────────────────────────────────────────────────────────

/// Summary statistics for the background task manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BGStats {
    /// Total tasks submitted.
    pub total_submitted: usize,
    /// Currently queued tasks.
    pub queued_count: usize,
    /// Currently running tasks.
    pub running_count: usize,
    /// Completed tasks.
    pub completed_count: usize,
    /// Failed tasks.
    pub failed_count: usize,
    /// Cancelled tasks.
    pub cancelled_count: usize,
    /// Maximum concurrent tasks allowed.
    pub max_concurrent: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// BackgroundTaskManager
// ─────────────────────────────────────────────────────────────────────────────

/// Manages background tasks with concurrency control and progress tracking.
pub struct BackgroundTaskManager {
    /// All tasks keyed by ID.
    tasks: HashMap<String, BGTask>,
    /// Maximum number of concurrently running tasks.
    max_concurrent: usize,
    /// Current number of running tasks.
    running_count: usize,
}

impl BackgroundTaskManager {
    /// Create a new background task manager with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            tasks: HashMap::new(),
            max_concurrent: max_concurrent.max(1),
            running_count: 0,
        }
    }

    /// Submit a new task to the queue. Returns the task ID.
    pub fn submit(&mut self, description: &str) -> String {
        let task = BGTask::new(description);
        let id = task.id.clone();
        self.tasks.insert(id.clone(), task);
        id
    }

    /// Mark a queued task as running. Fails if concurrency limit is reached
    /// or the task is not in `Queued` status.
    pub fn start_task(&mut self, task_id: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        if task.status != BGTaskStatus::Queued {
            return Err(format!(
                "Cannot start task {task_id}: status is {:?}, expected Queued",
                task.status
            ));
        }

        if self.running_count >= self.max_concurrent {
            return Err(format!(
                "Concurrency limit reached ({}/{})",
                self.running_count, self.max_concurrent
            ));
        }

        let task = self.tasks.get_mut(task_id).unwrap();
        task.status = BGTaskStatus::Running;
        task.started_at = Some(now_secs());
        self.running_count += 1;
        Ok(())
    }

    /// Mark a running task as completed with a result.
    pub fn complete_task(&mut self, task_id: &str, result: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        if task.status != BGTaskStatus::Running {
            return Err(format!(
                "Cannot complete task {task_id}: status is {:?}, expected Running",
                task.status
            ));
        }

        task.status = BGTaskStatus::Completed;
        task.result = Some(result.to_string());
        task.completed_at = Some(now_secs());
        self.running_count = self.running_count.saturating_sub(1);
        Ok(())
    }

    /// Mark a running task as failed with an error message.
    pub fn fail_task(&mut self, task_id: &str, error: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))?;

        if task.status != BGTaskStatus::Running {
            return Err(format!(
                "Cannot fail task {task_id}: status is {:?}, expected Running",
                task.status
            ));
        }

        task.status = BGTaskStatus::Failed;
        task.error = Some(error.to_string());
        task.completed_at = Some(now_secs());
        self.running_count = self.running_count.saturating_sub(1);
        Ok(())
    }

    /// Cancel a task. Returns `true` if cancelled, `false` if already terminal
    /// or not found.
    pub fn cancel(&mut self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            if task.status.is_terminal() {
                return false;
            }
            let was_running = task.status == BGTaskStatus::Running;
            task.status = BGTaskStatus::Cancelled;
            task.completed_at = Some(now_secs());
            if was_running {
                self.running_count = self.running_count.saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    /// Add a progress update to a task.
    pub fn add_progress(&mut self, task_id: &str, message: &str) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.progress.push(ProgressEntry::new(message));
        }
    }

    /// Get a reference to a task by ID.
    pub fn get(&self, task_id: &str) -> Option<&BGTask> {
        self.tasks.get(task_id)
    }

    /// List tasks, optionally filtered by status.
    pub fn list_tasks(&self, status: Option<BGTaskStatus>) -> Vec<&BGTask> {
        self.tasks
            .values()
            .filter(|t| {
                if let Some(ref s) = status {
                    &t.status == s
                } else {
                    true
                }
            })
            .collect()
    }

    /// Total number of tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Number of currently running tasks.
    pub fn current_running(&self) -> usize {
        self.running_count
    }

    /// Get summary statistics.
    pub fn get_stats(&self) -> BGStats {
        let mut queued = 0;
        let mut completed = 0;
        let mut failed = 0;
        let mut cancelled = 0;

        for task in self.tasks.values() {
            match task.status {
                BGTaskStatus::Queued => queued += 1,
                BGTaskStatus::Running => {} // tracked by running_count
                BGTaskStatus::Completed => completed += 1,
                BGTaskStatus::Failed => failed += 1,
                BGTaskStatus::Cancelled => cancelled += 1,
            }
        }

        BGStats {
            total_submitted: self.tasks.len(),
            queued_count: queued,
            running_count: self.running_count,
            completed_count: completed,
            failed_count: failed,
            cancelled_count: cancelled,
            max_concurrent: self.max_concurrent,
        }
    }
}

impl Default for BackgroundTaskManager {
    fn default() -> Self {
        Self::new(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_new() {
        let mgr = BackgroundTaskManager::new(4);
        assert_eq!(mgr.task_count(), 0);
        assert_eq!(mgr.current_running(), 0);
        assert_eq!(mgr.max_concurrent, 4);
    }

    #[test]
    fn test_manager_min_concurrency() {
        let mgr = BackgroundTaskManager::new(0);
        assert_eq!(mgr.max_concurrent, 1); // minimum 1
    }

    #[test]
    fn test_submit_task() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Background scan");
        assert!(id.starts_with("bg_"));
        assert_eq!(mgr.task_count(), 1);

        let task = mgr.get(&id).unwrap();
        assert_eq!(task.status, BGTaskStatus::Queued);
        assert_eq!(task.description, "Background scan");
    }

    #[test]
    fn test_start_task() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Start me");

        mgr.start_task(&id).unwrap();
        let task = mgr.get(&id).unwrap();
        assert_eq!(task.status, BGTaskStatus::Running);
        assert!(task.started_at.is_some());
        assert_eq!(mgr.current_running(), 1);
    }

    #[test]
    fn test_start_task_concurrency_limit() {
        let mut mgr = BackgroundTaskManager::new(1);
        let id1 = mgr.submit("Task 1");
        let id2 = mgr.submit("Task 2");

        mgr.start_task(&id1).unwrap();
        let result = mgr.start_task(&id2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Concurrency limit"));
    }

    #[test]
    fn test_complete_task() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Complete me");
        mgr.start_task(&id).unwrap();

        mgr.complete_task(&id, "all done").unwrap();
        let task = mgr.get(&id).unwrap();
        assert_eq!(task.status, BGTaskStatus::Completed);
        assert_eq!(task.result, Some("all done".to_string()));
        assert!(task.completed_at.is_some());
        assert_eq!(mgr.current_running(), 0);
    }

    #[test]
    fn test_fail_task() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Fail me");
        mgr.start_task(&id).unwrap();

        mgr.fail_task(&id, "explosion").unwrap();
        let task = mgr.get(&id).unwrap();
        assert_eq!(task.status, BGTaskStatus::Failed);
        assert_eq!(task.error, Some("explosion".to_string()));
        assert!(task.completed_at.is_some());
        assert_eq!(mgr.current_running(), 0);
    }

    #[test]
    fn test_cancel_queued_task() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Cancel me");

        assert!(mgr.cancel(&id));
        let task = mgr.get(&id).unwrap();
        assert_eq!(task.status, BGTaskStatus::Cancelled);
    }

    #[test]
    fn test_cancel_running_task() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Cancel running");
        mgr.start_task(&id).unwrap();
        assert_eq!(mgr.current_running(), 1);

        assert!(mgr.cancel(&id));
        assert_eq!(mgr.current_running(), 0);
    }

    #[test]
    fn test_cancel_completed_task_fails() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Already done");
        mgr.start_task(&id).unwrap();
        mgr.complete_task(&id, "done").unwrap();

        assert!(!mgr.cancel(&id)); // already terminal
    }

    #[test]
    fn test_add_progress() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id = mgr.submit("Progressing");
        mgr.start_task(&id).unwrap();

        mgr.add_progress(&id, "Step 1 done");
        mgr.add_progress(&id, "Step 2 done");

        let task = mgr.get(&id).unwrap();
        assert_eq!(task.progress.len(), 2);
        assert_eq!(task.progress[0].message, "Step 1 done");
        assert_eq!(task.progress[1].message, "Step 2 done");
    }

    #[test]
    fn test_list_tasks_filter() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id1 = mgr.submit("Task 1");
        let id2 = mgr.submit("Task 2");
        mgr.submit("Task 3");

        mgr.start_task(&id1).unwrap();
        mgr.start_task(&id2).unwrap();
        mgr.complete_task(&id1, "done").unwrap();

        assert_eq!(mgr.list_tasks(None).len(), 3);
        assert_eq!(mgr.list_tasks(Some(BGTaskStatus::Queued)).len(), 1);
        assert_eq!(mgr.list_tasks(Some(BGTaskStatus::Running)).len(), 1);
        assert_eq!(mgr.list_tasks(Some(BGTaskStatus::Completed)).len(), 1);
    }

    #[test]
    fn test_get_stats() {
        let mut mgr = BackgroundTaskManager::new(4);
        let id1 = mgr.submit("A");
        let id2 = mgr.submit("B");
        mgr.submit("C");

        mgr.start_task(&id1).unwrap();
        mgr.start_task(&id2).unwrap();
        mgr.complete_task(&id1, "ok").unwrap();
        mgr.fail_task(&id2, "err").unwrap();

        let stats = mgr.get_stats();
        assert_eq!(stats.total_submitted, 3);
        assert_eq!(stats.queued_count, 1);
        assert_eq!(stats.running_count, 0);
        assert_eq!(stats.completed_count, 1);
        assert_eq!(stats.failed_count, 1);
        assert_eq!(stats.max_concurrent, 4);
    }

    #[test]
    fn test_bg_task_status_terminal() {
        assert!(BGTaskStatus::Completed.is_terminal());
        assert!(BGTaskStatus::Failed.is_terminal());
        assert!(BGTaskStatus::Cancelled.is_terminal());
        assert!(!BGTaskStatus::Queued.is_terminal());
        assert!(!BGTaskStatus::Running.is_terminal());
    }

    #[test]
    fn test_progress_entry_timestamp() {
        let entry = ProgressEntry::new("test");
        assert!(entry.timestamp > 0.0);
        assert_eq!(entry.message, "test");
    }
}
