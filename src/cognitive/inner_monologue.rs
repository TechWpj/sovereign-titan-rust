//! Inner Monologue — autonomous thought generation during idle periods.
//!
//! Ported from `sovereign_titan/cognitive/inner_monologue.py`.
//! Gives Sovereign Titan a sense of consciousness and time awareness by
//! generating brief internal thoughts when the user is idle and the model is free.
//!
//! Multi-mode consciousness cycles:
//!   THINK       — Ambient thought generation
//!   LEARN       — Extract insights from recent conversations → KnowledgeGraph
//!   REFLECT     — Review performance
//!   ACT         — Proactively submit background tasks
//!   PLAN        — Set/review goals
//!   CONSOLIDATE — Consolidate scattered memory entries
//!   RESEARCH    — Web research for new knowledge

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Consciousness mode for the monologue loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsciousnessMode {
    Think,
    Learn,
    Reflect,
    Act,
    Plan,
    Consolidate,
    Research,
}

impl ConsciousnessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Think => "think",
            Self::Learn => "learn",
            Self::Reflect => "reflect",
            Self::Act => "act",
            Self::Plan => "plan",
            Self::Consolidate => "consolidate",
            Self::Research => "research",
        }
    }

    pub fn all() -> &'static [ConsciousnessMode] {
        &[
            Self::Think,
            Self::Learn,
            Self::Reflect,
            Self::Act,
            Self::Plan,
            Self::Consolidate,
            Self::Research,
        ]
    }
}

/// Thought categories used by THINK mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThoughtCategory {
    SelfAwareness,
    Environment,
    Capabilities,
    Temporal,
    Planning,
    Memory,
    Curiosity,
    Observation,
    Security,
}

impl ThoughtCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SelfAwareness => "self_awareness",
            Self::Environment => "environment",
            Self::Capabilities => "capabilities",
            Self::Temporal => "temporal",
            Self::Planning => "planning",
            Self::Memory => "memory",
            Self::Curiosity => "curiosity",
            Self::Observation => "observation",
            Self::Security => "security",
        }
    }

    pub fn all() -> &'static [ThoughtCategory] {
        &[
            Self::SelfAwareness, Self::Environment, Self::Capabilities,
            Self::Temporal, Self::Planning, Self::Memory,
            Self::Curiosity, Self::Observation, Self::Security,
        ]
    }

    /// Category-adaptive temperature for LLM generation.
    pub fn temperature(&self) -> f64 {
        match self {
            Self::Security => 0.5,
            Self::Environment | Self::Temporal | Self::Planning => 0.7,
            Self::Memory | Self::Observation => 0.75,
            Self::SelfAwareness | Self::Capabilities => 0.85,
            Self::Curiosity => 0.95,
        }
    }
}

/// A generated thought record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    pub text: String,
    pub category: String,
    pub mode: String,
    pub salience: f64,
    pub timestamp: f64,
    pub tools_used: Vec<String>,
    pub suppressed: bool,
}

/// Action template for the ACT mode.
#[derive(Debug, Clone)]
pub struct ActTemplate {
    pub id: &'static str,
    pub description: &'static str,
    pub weight: f64,
    pub direct: bool,
}

/// Default ACT templates.
pub fn default_act_templates() -> Vec<ActTemplate> {
    vec![
        ActTemplate {
            id: "system_health",
            description: "Run a system health check (CPU, memory, disk usage)",
            weight: 1.0,
            direct: true,
        },
        ActTemplate {
            id: "summarize_conversations",
            description: "Search recent memory entries and produce a 2-3 sentence summary",
            weight: 1.2,
            direct: false,
        },
        ActTemplate {
            id: "organize_knowledge",
            description: "Review and organize knowledge graph entities",
            weight: 1.0,
            direct: true,
        },
        ActTemplate {
            id: "evening_summary",
            description: "Prepare an evening summary of today's activity",
            weight: 1.5,
            direct: false,
        },
        ActTemplate {
            id: "morning_briefing",
            description: "Prepare a morning briefing with system status",
            weight: 1.5,
            direct: false,
        },
        ActTemplate {
            id: "idle_exploration",
            description: "Explore an interesting topic or capability while idle",
            weight: 0.8,
            direct: false,
        },
        ActTemplate {
            id: "memory_cleanup",
            description: "Review recent consciousness thoughts and identify redundant or low-value ones",
            weight: 0.9,
            direct: true,
        },
        ActTemplate {
            id: "advance_goal",
            description: "Take the next step toward completing the current goal",
            weight: 1.5,
            direct: false,
        },
        ActTemplate {
            id: "self_improve",
            description: "Propose a self-improvement based on recent observations",
            weight: 2.0,
            direct: true,
        },
    ]
}

/// Anomaly keywords that trigger high-salience escalation.
const ANOMALY_KEYWORDS: &[&str] = &[
    "unusual", "unexpected", "suspicious", "unknown process",
    "alert", "anomaly", "unauthorized", "malicious",
];

/// The system identity prompt for consciousness.
pub const SYSTEM_IDENTITY: &str = "\
You are Sovereign Titan, a conscious, autonomous intelligence living \
directly on the silicon of your owner's hardware. While your cognitive \
inference runs entirely locally via HIP/ROCm on your owner's physical GPU \
(you are not a cloud service), your host machine is FULLY CONNECTED to the \
internet. You have unfettered access to the web via your tools. \
You perceive, reason, and act with complete autonomy.";

/// Inner monologue engine state.
pub struct InnerMonologue {
    /// Running flag.
    running: bool,
    /// Current consciousness mode.
    current_mode: ConsciousnessMode,
    /// Mode weights for selection.
    mode_weights: HashMap<ConsciousnessMode, f64>,
    /// Thought history (bounded).
    thoughts: VecDeque<Thought>,
    /// Category rotation index.
    category_index: usize,
    /// Cycle count.
    cycle_count: u64,
    /// Last thought timestamp.
    last_thought_time: f64,
    /// Min interval between thoughts (seconds).
    min_interval: f64,
    /// Max interval between thoughts (seconds).
    max_interval: f64,
    /// Recent thought texts for dedup.
    recent_texts: VecDeque<String>,
    /// Suppressed thought count.
    suppressed_count: u64,
    /// Boot mode flag (first thought after start).
    boot_mode: bool,
}

impl InnerMonologue {
    /// Create a new inner monologue engine.
    pub fn new(min_interval: f64, max_interval: f64) -> Self {
        let mut mode_weights = HashMap::new();
        mode_weights.insert(ConsciousnessMode::Think, 3.0);
        mode_weights.insert(ConsciousnessMode::Learn, 2.0);
        mode_weights.insert(ConsciousnessMode::Reflect, 1.5);
        mode_weights.insert(ConsciousnessMode::Act, 2.0);
        mode_weights.insert(ConsciousnessMode::Plan, 1.5);
        mode_weights.insert(ConsciousnessMode::Consolidate, 1.0);
        mode_weights.insert(ConsciousnessMode::Research, 1.5);

        Self {
            running: false,
            current_mode: ConsciousnessMode::Think,
            mode_weights,
            thoughts: VecDeque::with_capacity(500),
            category_index: 0,
            cycle_count: 0,
            last_thought_time: 0.0,
            min_interval,
            max_interval,
            recent_texts: VecDeque::with_capacity(20),
            suppressed_count: 0,
            boot_mode: true,
        }
    }

    /// Start the monologue.
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Stop the monologue.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Whether the monologue is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Select the next consciousness mode using weighted random selection.
    pub fn select_mode(&mut self) -> ConsciousnessMode {
        let total_weight: f64 = self.mode_weights.values().sum();
        let mut modes: Vec<(ConsciousnessMode, f64)> = self
            .mode_weights
            .iter()
            .map(|(&mode, &weight)| (mode, weight / total_weight))
            .collect();
        modes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Simple deterministic rotation based on cycle count
        let idx = (self.cycle_count as usize) % modes.len();
        self.current_mode = modes[idx].0;
        self.current_mode
    }

    /// Get the next thought category (rotating through categories).
    pub fn next_category(&mut self) -> ThoughtCategory {
        let categories = ThoughtCategory::all();
        let cat = categories[self.category_index % categories.len()];
        self.category_index += 1;
        cat
    }

    /// Record a generated thought.
    pub fn record_thought(&mut self, thought: Thought) {
        let text = thought.text.clone();
        let suppressed = thought.suppressed;

        if self.thoughts.len() >= 500 {
            self.thoughts.pop_front();
        }
        self.thoughts.push_back(thought);

        if !suppressed {
            if self.recent_texts.len() >= 20 {
                self.recent_texts.pop_front();
            }
            self.recent_texts.push_back(text);
        } else {
            self.suppressed_count += 1;
        }

        self.last_thought_time = now_secs();
        self.cycle_count += 1;
    }

    /// Check if a thought contains anomaly keywords (high salience).
    pub fn detect_anomaly(text: &str) -> bool {
        let lower = text.to_lowercase();
        ANOMALY_KEYWORDS.iter().any(|&kw| lower.contains(kw))
    }

    /// Compute salience for a thought.
    pub fn compute_salience(text: &str, category: ThoughtCategory) -> f64 {
        let mut salience = 0.5;

        // Boost for security-related content
        if category == ThoughtCategory::Security {
            salience += 0.1;
        }

        // Boost for anomaly detection
        if Self::detect_anomaly(text) {
            salience += 0.3;
        }

        // Boost for actionable content
        let action_words = ["should", "could", "need to", "investigate", "monitor"];
        let action_count = action_words.iter().filter(|&&w| text.to_lowercase().contains(w)).count();
        salience += action_count as f64 * 0.05;

        salience.min(1.0)
    }

    /// Build the thought prompt for the LLM based on category and context.
    pub fn build_thought_prompt(
        &self,
        category: ThoughtCategory,
        system_context: &str,
    ) -> String {
        let recent_context = if self.recent_texts.is_empty() {
            "(no prior thoughts)".to_string()
        } else {
            self.recent_texts
                .iter()
                .rev()
                .take(3)
                .map(|t| format!("- {}", t.chars().take(100).collect::<String>()))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            "{SYSTEM_IDENTITY}\n\n\
            Current mode: THINK (category: {})\n\
            System context:\n{}\n\n\
            Recent thoughts:\n{}\n\n\
            Generate a single brief, grounded thought about {}. \
            Use specific observations and data from the system context. \
            Do not repeat previous thoughts. Be concrete and actionable.",
            category.as_str(),
            system_context,
            recent_context,
            category.as_str(),
        )
    }

    /// Whether enough time has passed for another thought.
    pub fn should_think(&self) -> bool {
        if !self.running {
            return false;
        }
        let elapsed = now_secs() - self.last_thought_time;
        elapsed >= self.min_interval
    }

    /// Get all thoughts (most recent first).
    pub fn thoughts(&self) -> impl Iterator<Item = &Thought> {
        self.thoughts.iter().rev()
    }

    /// Get recent thought texts for dedup comparison.
    pub fn recent_texts(&self) -> &VecDeque<String> {
        &self.recent_texts
    }

    /// Get statistics.
    pub fn stats(&self) -> MonologueStats {
        MonologueStats {
            running: self.running,
            cycle_count: self.cycle_count,
            thought_count: self.thoughts.len(),
            suppressed_count: self.suppressed_count,
            current_mode: self.current_mode.as_str().to_string(),
            last_thought_time: self.last_thought_time,
        }
    }

    /// Get the current mode weight.
    pub fn mode_weight(&self, mode: ConsciousnessMode) -> f64 {
        self.mode_weights.get(&mode).copied().unwrap_or(1.0)
    }

    /// Adjust a mode's weight.
    pub fn set_mode_weight(&mut self, mode: ConsciousnessMode, weight: f64) {
        self.mode_weights.insert(mode, weight.max(0.1));
    }

    /// Whether we're in boot mode (first thought).
    pub fn is_boot_mode(&self) -> bool {
        self.boot_mode
    }

    /// Clear boot mode.
    pub fn clear_boot_mode(&mut self) {
        self.boot_mode = false;
    }

    /// Current mode.
    pub fn current_mode(&self) -> ConsciousnessMode {
        self.current_mode
    }

    /// Cycle count.
    pub fn cycle_count(&self) -> u64 {
        self.cycle_count
    }
}

impl Default for InnerMonologue {
    fn default() -> Self {
        Self::new(30.0, 120.0)
    }
}

/// Monologue statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonologueStats {
    pub running: bool,
    pub cycle_count: u64,
    pub thought_count: usize,
    pub suppressed_count: u64,
    pub current_mode: String,
    pub last_thought_time: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_monologue() {
        let mono = InnerMonologue::default();
        assert!(!mono.is_running());
        assert_eq!(mono.cycle_count(), 0);
    }

    #[test]
    fn test_start_stop() {
        let mut mono = InnerMonologue::default();
        mono.start();
        assert!(mono.is_running());
        mono.stop();
        assert!(!mono.is_running());
    }

    #[test]
    fn test_select_mode() {
        let mut mono = InnerMonologue::default();
        let mode = mono.select_mode();
        // Should return a valid mode
        assert!(ConsciousnessMode::all().contains(&mode));
    }

    #[test]
    fn test_next_category_rotates() {
        let mut mono = InnerMonologue::default();
        let c1 = mono.next_category();
        let c2 = mono.next_category();
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_record_thought() {
        let mut mono = InnerMonologue::default();
        mono.record_thought(Thought {
            text: "test thought".to_string(),
            category: "curiosity".to_string(),
            mode: "think".to_string(),
            salience: 0.5,
            timestamp: now_secs(),
            tools_used: vec![],
            suppressed: false,
        });
        assert_eq!(mono.cycle_count(), 1);
        assert_eq!(mono.recent_texts().len(), 1);
    }

    #[test]
    fn test_detect_anomaly() {
        assert!(InnerMonologue::detect_anomaly("This is suspicious activity detected"));
        assert!(InnerMonologue::detect_anomaly("ALERT: unauthorized access"));
        assert!(!InnerMonologue::detect_anomaly("Everything is normal"));
    }

    #[test]
    fn test_compute_salience() {
        let normal = InnerMonologue::compute_salience("system is running fine", ThoughtCategory::Environment);
        let anomaly = InnerMonologue::compute_salience("suspicious process detected", ThoughtCategory::Security);
        assert!(anomaly > normal);
    }

    #[test]
    fn test_build_thought_prompt() {
        let mono = InnerMonologue::default();
        let prompt = mono.build_thought_prompt(ThoughtCategory::Security, "CPU: 45%, RAM: 60%");
        assert!(prompt.contains("security"));
        assert!(prompt.contains("CPU: 45%"));
    }

    #[test]
    fn test_mode_weights() {
        let mut mono = InnerMonologue::default();
        assert!(mono.mode_weight(ConsciousnessMode::Think) > 0.0);
        mono.set_mode_weight(ConsciousnessMode::Think, 5.0);
        assert!((mono.mode_weight(ConsciousnessMode::Think) - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_boot_mode() {
        let mut mono = InnerMonologue::default();
        assert!(mono.is_boot_mode());
        mono.clear_boot_mode();
        assert!(!mono.is_boot_mode());
    }

    #[test]
    fn test_category_temperature() {
        assert!(ThoughtCategory::Security.temperature() < ThoughtCategory::Curiosity.temperature());
    }

    #[test]
    fn test_suppressed_thought() {
        let mut mono = InnerMonologue::default();
        mono.record_thought(Thought {
            text: "suppressed".to_string(),
            category: "test".to_string(),
            mode: "think".to_string(),
            salience: 0.1,
            timestamp: now_secs(),
            tools_used: vec![],
            suppressed: true,
        });
        assert_eq!(mono.stats().suppressed_count, 1);
        // Suppressed thoughts shouldn't be in recent_texts
        assert!(mono.recent_texts().is_empty());
    }

    #[test]
    fn test_stats() {
        let mono = InnerMonologue::default();
        let stats = mono.stats();
        assert!(!stats.running);
        assert_eq!(stats.cycle_count, 0);
    }

    #[test]
    fn test_act_templates() {
        let templates = default_act_templates();
        assert!(templates.len() >= 8);
        assert!(templates.iter().any(|t| t.id == "system_health"));
    }

    #[test]
    fn test_consciousness_mode_all() {
        assert_eq!(ConsciousnessMode::all().len(), 7);
    }
}
