//! Cognitive Threading — multi-step reasoning chains across consciousness cycles.
//!
//! Ported from `sovereign_titan/cognitive/cognitive_threading.py`.
//! Instead of isolated one-shot thoughts, threads allow multi-step reasoning
//! chains that develop ideas over several cycles, concluding with synthesis.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Status of a cognitive thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ThreadStatus {
    Open,
    Concluded,
    Stale,
}

/// A single step in a cognitive thread's chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtStep {
    pub text: String,
    pub category: String,
    pub timestamp: f64,
    pub depth: usize,
}

/// A cognitive thread — a chain of related thoughts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveThread {
    pub id: String,
    pub seed_thought: String,
    pub seed_category: String,
    pub chain: Vec<ThoughtStep>,
    pub status: ThreadStatus,
    pub salience: f64,
    pub max_depth: usize,
    pub created: f64,
    pub last_touched: f64,
    pub conclusion: Option<String>,
    untouched_cycles: usize,
}

impl CognitiveThread {
    /// Create a new cognitive thread from a seed thought.
    pub fn new(seed_thought: &str, seed_category: &str, max_depth: usize) -> Self {
        let now = now_secs();
        let id = format!("thread_{:08x}", rand_u32());
        Self {
            id,
            seed_thought: seed_thought.to_string(),
            seed_category: seed_category.to_string(),
            chain: vec![ThoughtStep {
                text: seed_thought.to_string(),
                category: seed_category.to_string(),
                timestamp: now,
                depth: 0,
            }],
            status: ThreadStatus::Open,
            salience: 1.0,
            max_depth,
            created: now,
            last_touched: now,
            conclusion: None,
            untouched_cycles: 0,
        }
    }

    /// Current depth of the thought chain.
    pub fn depth(&self) -> usize {
        self.chain.len()
    }

    /// Whether the thread has reached max depth.
    pub fn is_at_max_depth(&self) -> bool {
        self.depth() >= self.max_depth
    }

    /// Add a continuation thought to the chain.
    pub fn add_thought(&mut self, text: &str, category: &str) {
        let depth = self.chain.len();
        self.chain.push(ThoughtStep {
            text: text.to_string(),
            category: category.to_string(),
            timestamp: now_secs(),
            depth,
        });
        self.last_touched = now_secs();
        self.untouched_cycles = 0;
    }

    /// Conclude the thread with a synthesis.
    pub fn conclude(&mut self, conclusion: &str) {
        self.conclusion = Some(conclusion.to_string());
        self.status = ThreadStatus::Concluded;
        self.last_touched = now_secs();
    }

    /// Mark thread as stale.
    pub fn mark_stale(&mut self) {
        self.status = ThreadStatus::Stale;
    }

    /// Record that a cycle passed without touching this thread.
    pub fn tick_untouched(&mut self) {
        self.untouched_cycles += 1;
    }

    /// Number of cycles since last touch.
    pub fn untouched_cycles(&self) -> usize {
        self.untouched_cycles
    }

    /// Build a summary of the thought chain for prompting.
    pub fn chain_summary(&self) -> String {
        self.chain
            .iter()
            .map(|step| format!("[depth {}] {}", step.depth, step.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Manages cognitive threads — multi-step reasoning chains across cycles.
pub struct CognitiveThreadManager {
    /// Active threads.
    active: Vec<CognitiveThread>,
    /// Concluded threads (recent history).
    concluded: VecDeque<CognitiveThread>,
    /// Max number of active threads.
    max_active: usize,
    /// Default max depth for new threads.
    max_depth: usize,
    /// Probability of continuing an existing thread vs starting new.
    continuation_probability: f64,
    /// Cycles before a thread goes stale.
    stale_after_cycles: usize,
}

impl CognitiveThreadManager {
    /// Create a new thread manager.
    pub fn new(
        max_active: usize,
        max_depth: usize,
        continuation_probability: f64,
        stale_after_cycles: usize,
    ) -> Self {
        Self {
            active: Vec::new(),
            concluded: VecDeque::with_capacity(20),
            max_active,
            max_depth,
            continuation_probability,
            stale_after_cycles,
        }
    }

    /// Start a new cognitive thread. Returns the thread ID, or None if at capacity.
    pub fn start_thread(&mut self, seed_thought: &str, seed_category: &str) -> Option<String> {
        if self.active.len() >= self.max_active {
            return None;
        }
        let thread = CognitiveThread::new(seed_thought, seed_category, self.max_depth);
        let id = thread.id.clone();
        self.active.push(thread);
        Some(id)
    }

    /// Pick the best thread to continue, or None if we should start a new one.
    /// Uses a simple heuristic: highest salience among open threads.
    pub fn pick_thread_to_continue(&self) -> Option<&CognitiveThread> {
        // Simple random check against continuation probability
        let random_val = (now_secs() * 1000.0) as u64 % 100;
        if random_val >= (self.continuation_probability * 100.0) as u64 {
            return None;
        }

        self.active
            .iter()
            .filter(|t| t.status == ThreadStatus::Open && !t.is_at_max_depth())
            .max_by(|a, b| a.salience.partial_cmp(&b.salience).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Add a continuation thought to a specific thread.
    pub fn continue_thread(&mut self, thread_id: &str, text: &str, category: &str) -> bool {
        if let Some(thread) = self.active.iter_mut().find(|t| t.id == thread_id) {
            thread.add_thought(text, category);
            true
        } else {
            false
        }
    }

    /// Conclude a thread with synthesis text.
    pub fn conclude_thread(&mut self, thread_id: &str, conclusion: &str) -> bool {
        if let Some(idx) = self.active.iter().position(|t| t.id == thread_id) {
            self.active[idx].conclude(conclusion);
            let thread = self.active.remove(idx);
            if self.concluded.len() >= 20 {
                self.concluded.pop_front();
            }
            self.concluded.push_back(thread);
            true
        } else {
            false
        }
    }

    /// Tick all active threads — mark stale ones, decay salience.
    pub fn tick_cycle(&mut self) {
        let mut stale_ids = Vec::new();

        for thread in &mut self.active {
            thread.tick_untouched();
            thread.salience *= 0.95; // Decay salience

            if thread.untouched_cycles() >= self.stale_after_cycles {
                thread.mark_stale();
                stale_ids.push(thread.id.clone());
            }
        }

        // Move stale threads to concluded
        for id in stale_ids {
            if let Some(idx) = self.active.iter().position(|t| t.id == id) {
                let thread = self.active.remove(idx);
                if self.concluded.len() >= 20 {
                    self.concluded.pop_front();
                }
                self.concluded.push_back(thread);
            }
        }
    }

    /// Number of active threads.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Get all active threads.
    pub fn active_threads(&self) -> &[CognitiveThread] {
        &self.active
    }

    /// Get recent concluded threads.
    pub fn concluded_threads(&self) -> &VecDeque<CognitiveThread> {
        &self.concluded
    }

    /// Get a specific thread by ID (searches active and concluded).
    pub fn get_thread(&self, id: &str) -> Option<&CognitiveThread> {
        self.active
            .iter()
            .chain(self.concluded.iter())
            .find(|t| t.id == id)
    }

    /// Get summaries of all active threads for prompting.
    pub fn active_summaries(&self) -> Vec<String> {
        self.active
            .iter()
            .filter(|t| t.status == ThreadStatus::Open)
            .map(|t| format!("Thread '{}' (depth {}/{}): {}", t.id, t.depth(), t.max_depth, t.chain_summary()))
            .collect()
    }
}

impl Default for CognitiveThreadManager {
    fn default() -> Self {
        Self::new(4, 5, 0.4, 5)
    }
}

/// Simple pseudo-random u32 based on timestamp.
fn rand_u32() -> u32 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (t ^ (t >> 16)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_thread() {
        let thread = CognitiveThread::new("test thought", "curiosity", 5);
        assert_eq!(thread.depth(), 1);
        assert_eq!(thread.status, ThreadStatus::Open);
        assert!(thread.id.starts_with("thread_"));
    }

    #[test]
    fn test_add_thought() {
        let mut thread = CognitiveThread::new("initial", "curiosity", 5);
        thread.add_thought("continuation", "exploration");
        assert_eq!(thread.depth(), 2);
        assert_eq!(thread.chain[1].category, "exploration");
    }

    #[test]
    fn test_conclude_thread() {
        let mut thread = CognitiveThread::new("start", "planning", 5);
        thread.conclude("synthesis result");
        assert_eq!(thread.status, ThreadStatus::Concluded);
        assert_eq!(thread.conclusion, Some("synthesis result".to_string()));
    }

    #[test]
    fn test_max_depth() {
        let mut thread = CognitiveThread::new("start", "test", 3);
        thread.add_thought("step 2", "test");
        thread.add_thought("step 3", "test");
        assert!(thread.is_at_max_depth());
    }

    #[test]
    fn test_manager_start_thread() {
        let mut mgr = CognitiveThreadManager::new(2, 5, 0.4, 5);
        let id1 = mgr.start_thread("thought 1", "curiosity");
        assert!(id1.is_some());
        let id2 = mgr.start_thread("thought 2", "planning");
        assert!(id2.is_some());
        // At capacity
        let id3 = mgr.start_thread("thought 3", "security");
        assert!(id3.is_none());
        assert_eq!(mgr.active_count(), 2);
    }

    #[test]
    fn test_manager_conclude() {
        let mut mgr = CognitiveThreadManager::default();
        let id = mgr.start_thread("test", "test").unwrap();
        assert!(mgr.conclude_thread(&id, "done"));
        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.concluded_threads().len(), 1);
    }

    #[test]
    fn test_manager_tick_stale() {
        let mut mgr = CognitiveThreadManager::new(4, 5, 0.4, 2);
        mgr.start_thread("test", "test");
        mgr.tick_cycle();
        mgr.tick_cycle();
        // After 2 untouched cycles with stale_after=2, thread should be moved
        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.concluded_threads().len(), 1);
    }

    #[test]
    fn test_chain_summary() {
        let mut thread = CognitiveThread::new("first", "test", 5);
        thread.add_thought("second", "test");
        let summary = thread.chain_summary();
        assert!(summary.contains("[depth 0] first"));
        assert!(summary.contains("[depth 1] second"));
    }

    #[test]
    fn test_continue_thread() {
        let mut mgr = CognitiveThreadManager::default();
        let id = mgr.start_thread("seed", "curiosity").unwrap();
        assert!(mgr.continue_thread(&id, "next thought", "exploration"));
        let thread = mgr.get_thread(&id).unwrap();
        assert_eq!(thread.depth(), 2);
    }
}
