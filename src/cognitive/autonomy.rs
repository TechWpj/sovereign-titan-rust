//! Autonomy Engine — goal-directed planning and action.
//!
//! Ported from `sovereign_titan/cognitive/autonomy.py`.
//! Features:
//! - Hierarchical task decomposition
//! - Goal tracking with dedup + cap + stale pruning
//! - Action rate limiting
//! - Goal progress tracking

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn compute_hash(input: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Status of a goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Active,
    Completed,
    Failed,
    Stale,
}

/// A tracked goal with progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub text: String,
    pub priority: f64,
    pub status: GoalStatus,
    pub created: f64,
    pub last_touched: f64,
    pub progress: f64,
    pub steps_completed: usize,
    pub steps_total: usize,
    pub metadata: serde_json::Value,
}

/// An action record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub goal_id: Option<String>,
    pub description: String,
    pub timestamp: f64,
    pub success: bool,
}

/// Autonomy engine for goal-directed planning.
pub struct AutonomyEngine {
    /// Maximum active goals.
    max_active_goals: usize,
    /// Seconds before a goal is considered stale.
    goal_stale_seconds: u64,
    /// Similarity threshold for dedup (Jaccard).
    similarity_threshold: f64,
    /// Rate limit (actions per minute).
    rate_limit: usize,
    /// Active goals.
    active_goals: Vec<Goal>,
    /// Completed goals (recent history).
    completed_goals: VecDeque<Goal>,
    /// Action history.
    action_history: VecDeque<ActionRecord>,
    /// Actions this minute counter.
    actions_this_minute: usize,
    /// Last minute reset timestamp.
    last_minute_reset: f64,
}

impl AutonomyEngine {
    /// Create a new autonomy engine.
    pub fn new(
        max_active_goals: usize,
        goal_stale_seconds: u64,
        similarity_threshold: f64,
        rate_limit: usize,
    ) -> Self {
        Self {
            max_active_goals,
            goal_stale_seconds,
            similarity_threshold,
            rate_limit,
            active_goals: Vec::new(),
            completed_goals: VecDeque::with_capacity(100),
            action_history: VecDeque::with_capacity(1000),
            actions_this_minute: 0,
            last_minute_reset: now_secs(),
        }
    }

    /// Set a new goal. Returns goal ID, or None if rejected (cap or duplicate).
    pub fn set_goal(
        &mut self,
        text: &str,
        priority: f64,
        metadata: serde_json::Value,
    ) -> Option<String> {
        // Prune stale goals first
        self.prune_stale_goals();

        // Check cap
        if self.active_goals.len() >= self.max_active_goals {
            return None;
        }

        // Check for duplicates
        if self.is_duplicate_goal(text) {
            return None;
        }

        let id = compute_hash(&format!("{}{}", text, now_secs()))[..12].to_string();
        let now = now_secs();

        self.active_goals.push(Goal {
            id: id.clone(),
            text: text.to_string(),
            priority,
            status: GoalStatus::Active,
            created: now,
            last_touched: now,
            progress: 0.0,
            steps_completed: 0,
            steps_total: 0,
            metadata,
        });

        // Sort by priority (highest first)
        self.active_goals
            .sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal));

        Some(id)
    }

    /// Update goal progress.
    pub fn update_progress(
        &mut self,
        goal_id: &str,
        progress: f64,
        steps_completed: usize,
        steps_total: usize,
    ) -> bool {
        if let Some(goal) = self.active_goals.iter_mut().find(|g| g.id == goal_id) {
            goal.progress = progress.clamp(0.0, 1.0);
            goal.steps_completed = steps_completed;
            goal.steps_total = steps_total;
            goal.last_touched = now_secs();

            if progress >= 1.0 {
                goal.status = GoalStatus::Completed;
            }
            true
        } else {
            false
        }
    }

    /// Complete a goal.
    pub fn complete_goal(&mut self, goal_id: &str) -> bool {
        if let Some(idx) = self.active_goals.iter().position(|g| g.id == goal_id) {
            let mut goal = self.active_goals.remove(idx);
            goal.status = GoalStatus::Completed;
            goal.progress = 1.0;
            goal.last_touched = now_secs();
            if self.completed_goals.len() >= 100 {
                self.completed_goals.pop_front();
            }
            self.completed_goals.push_back(goal);
            true
        } else {
            false
        }
    }

    /// Fail a goal.
    pub fn fail_goal(&mut self, goal_id: &str, reason: &str) -> bool {
        if let Some(idx) = self.active_goals.iter().position(|g| g.id == goal_id) {
            let mut goal = self.active_goals.remove(idx);
            goal.status = GoalStatus::Failed;
            goal.last_touched = now_secs();
            goal.metadata["fail_reason"] = serde_json::Value::String(reason.to_string());
            if self.completed_goals.len() >= 100 {
                self.completed_goals.pop_front();
            }
            self.completed_goals.push_back(goal);
            true
        } else {
            false
        }
    }

    /// Record an action.
    pub fn record_action(
        &mut self,
        goal_id: Option<&str>,
        description: &str,
        success: bool,
    ) -> bool {
        // Rate limiting
        let now = now_secs();
        if now - self.last_minute_reset > 60.0 {
            self.actions_this_minute = 0;
            self.last_minute_reset = now;
        }

        if self.actions_this_minute >= self.rate_limit {
            return false; // Rate limited
        }

        self.actions_this_minute += 1;

        if self.action_history.len() >= 1000 {
            self.action_history.pop_front();
        }
        self.action_history.push_back(ActionRecord {
            goal_id: goal_id.map(|s| s.to_string()),
            description: description.to_string(),
            timestamp: now,
            success,
        });

        true
    }

    /// Get the current highest-priority active goal.
    pub fn current_goal(&self) -> Option<&Goal> {
        self.active_goals
            .iter()
            .filter(|g| g.status == GoalStatus::Active)
            .max_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Get all active goals.
    pub fn active_goals(&self) -> &[Goal] {
        &self.active_goals
    }

    /// Number of active goals.
    pub fn active_count(&self) -> usize {
        self.active_goals.len()
    }

    /// Whether we're rate limited.
    pub fn is_rate_limited(&self) -> bool {
        self.actions_this_minute >= self.rate_limit
    }

    /// Prune stale goals. Returns count of pruned.
    pub fn prune_stale_goals(&mut self) -> usize {
        let now = now_secs();
        let stale_threshold = self.goal_stale_seconds as f64;

        let mut stale_ids = Vec::new();
        for goal in &self.active_goals {
            if now - goal.last_touched > stale_threshold {
                stale_ids.push(goal.id.clone());
            }
        }

        let count = stale_ids.len();
        for id in stale_ids {
            if let Some(idx) = self.active_goals.iter().position(|g| g.id == id) {
                let mut goal = self.active_goals.remove(idx);
                goal.status = GoalStatus::Stale;
                if self.completed_goals.len() >= 100 {
                    self.completed_goals.pop_front();
                }
                self.completed_goals.push_back(goal);
            }
        }
        count
    }

    /// Check if a goal text is a duplicate of an existing active goal.
    fn is_duplicate_goal(&self, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        let text_words: std::collections::HashSet<&str> = text_lower
            .split_whitespace()
            .collect();

        for goal in &self.active_goals {
            let goal_lower = goal.text.to_lowercase();
            let goal_words: std::collections::HashSet<&str> = goal_lower
                .split_whitespace()
                .collect();

            if text_words.is_empty() || goal_words.is_empty() {
                continue;
            }

            let intersection = text_words.intersection(&goal_words).count();
            let union = text_words.union(&goal_words).count();
            let similarity = intersection as f64 / union as f64;

            if similarity >= self.similarity_threshold {
                return true;
            }
        }
        false
    }

    /// Get action history for a specific goal.
    pub fn goal_actions(&self, goal_id: &str) -> Vec<&ActionRecord> {
        self.action_history
            .iter()
            .filter(|a| a.goal_id.as_deref() == Some(goal_id))
            .collect()
    }

    /// Get summary of all active goals for prompting.
    pub fn goals_summary(&self) -> String {
        if self.active_goals.is_empty() {
            return "No active goals.".to_string();
        }
        self.active_goals
            .iter()
            .map(|g| {
                format!(
                    "- [{}] {} (priority: {:.1}, progress: {:.0}%)",
                    g.id, g.text, g.priority, g.progress * 100.0
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for AutonomyEngine {
    fn default() -> Self {
        Self::new(10, 86400, 0.8, 30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_set_goal() {
        let mut engine = AutonomyEngine::default();
        let id = engine.set_goal("Learn about Rust async", 0.5, json!({}));
        assert!(id.is_some());
        assert_eq!(engine.active_count(), 1);
    }

    #[test]
    fn test_goal_cap() {
        let mut engine = AutonomyEngine::new(2, 86400, 0.8, 30);
        engine.set_goal("goal 1", 0.5, json!({}));
        engine.set_goal("goal 2", 0.5, json!({}));
        let id3 = engine.set_goal("goal 3", 0.5, json!({}));
        assert!(id3.is_none());
    }

    #[test]
    fn test_duplicate_rejection() {
        let mut engine = AutonomyEngine::default();
        engine.set_goal("learn about rust programming", 0.5, json!({}));
        let dup = engine.set_goal("learn about rust programming", 0.5, json!({}));
        assert!(dup.is_none());
    }

    #[test]
    fn test_complete_goal() {
        let mut engine = AutonomyEngine::default();
        let id = engine.set_goal("test goal", 0.5, json!({})).unwrap();
        assert!(engine.complete_goal(&id));
        assert_eq!(engine.active_count(), 0);
    }

    #[test]
    fn test_fail_goal() {
        let mut engine = AutonomyEngine::default();
        let id = engine.set_goal("test goal", 0.5, json!({})).unwrap();
        assert!(engine.fail_goal(&id, "not feasible"));
        assert_eq!(engine.active_count(), 0);
    }

    #[test]
    fn test_update_progress() {
        let mut engine = AutonomyEngine::default();
        let id = engine.set_goal("test goal", 0.5, json!({})).unwrap();
        assert!(engine.update_progress(&id, 0.5, 2, 4));
        let goal = engine.active_goals().iter().find(|g| g.id == id).unwrap();
        assert!((goal.progress - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_record_action() {
        let mut engine = AutonomyEngine::default();
        assert!(engine.record_action(None, "test action", true));
    }

    #[test]
    fn test_rate_limiting() {
        let mut engine = AutonomyEngine::new(10, 86400, 0.8, 2);
        assert!(engine.record_action(None, "action 1", true));
        assert!(engine.record_action(None, "action 2", true));
        assert!(!engine.record_action(None, "action 3", true)); // Rate limited
    }

    #[test]
    fn test_current_goal() {
        let mut engine = AutonomyEngine::default();
        engine.set_goal("low priority", 0.3, json!({}));
        engine.set_goal("high priority", 0.9, json!({}));
        let current = engine.current_goal().unwrap();
        assert!((current.priority - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_goals_summary() {
        let mut engine = AutonomyEngine::default();
        engine.set_goal("test goal", 0.5, json!({}));
        let summary = engine.goals_summary();
        assert!(summary.contains("test goal"));
    }

    #[test]
    fn test_no_goals_summary() {
        let engine = AutonomyEngine::default();
        assert_eq!(engine.goals_summary(), "No active goals.");
    }
}
