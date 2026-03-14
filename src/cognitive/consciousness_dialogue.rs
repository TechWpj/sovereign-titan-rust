//! Consciousness Dialogue — inner monologue generation and persistence.
//!
//! Ported from `sovereign_titan/cognitive/consciousness_dialogue.py`.
//! Features:
//! - Multiple dialogue types (reflection, planning, evaluation, etc.)
//! - Template-based generation
//! - Capped history with importance scoring
//! - Querying by type, recency, and importance

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// The category of an inner dialogue entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DialogueType {
    Reflection,
    Planning,
    Evaluation,
    Curiosity,
    Concern,
    Satisfaction,
    Frustration,
}

/// A single inner-dialogue entry with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueEntry {
    pub dialogue_type: DialogueType,
    pub content: String,
    pub context: String,
    pub timestamp: f64,
    pub importance: f64,
}

impl DialogueEntry {
    /// Create a new dialogue entry with the current timestamp.
    pub fn new(dtype: DialogueType, content: &str, context: &str, importance: f64) -> Self {
        Self {
            dialogue_type: dtype,
            content: content.to_string(),
            context: context.to_string(),
            timestamp: now_secs(),
            importance: importance.clamp(0.0, 1.0),
        }
    }
}

/// Inner monologue engine with template-based generation and bounded history.
pub struct ConsciousnessDialogue {
    history: VecDeque<DialogueEntry>,
    max_history: usize,
    reflection_count: u64,
    templates: HashMap<DialogueType, Vec<String>>,
}

impl ConsciousnessDialogue {
    /// Create a new dialogue engine with the given history capacity.
    pub fn new(max_history: usize) -> Self {
        Self {
            history: VecDeque::new(),
            max_history,
            reflection_count: 0,
            templates: Self::default_templates(),
        }
    }

    fn default_templates() -> HashMap<DialogueType, Vec<String>> {
        let mut t = HashMap::new();
        t.insert(
            DialogueType::Reflection,
            vec![
                "I notice that {context}...".to_string(),
                "Looking back at this, {context} suggests...".to_string(),
                "This reminds me of a pattern: {context}".to_string(),
            ],
        );
        t.insert(
            DialogueType::Planning,
            vec![
                "To accomplish this, I should first {context}...".to_string(),
                "The best approach for {context} would be...".to_string(),
            ],
        );
        t.insert(
            DialogueType::Evaluation,
            vec![
                "The result of {context} was...".to_string(),
                "Evaluating {context}, I find that...".to_string(),
            ],
        );
        t.insert(
            DialogueType::Curiosity,
            vec![
                "I wonder about {context}...".to_string(),
                "What if {context} could be done differently?".to_string(),
            ],
        );
        t.insert(
            DialogueType::Concern,
            vec![
                "I'm concerned about {context}...".to_string(),
                "This might be problematic: {context}".to_string(),
            ],
        );
        t.insert(
            DialogueType::Satisfaction,
            vec!["This turned out well: {context}".to_string()],
        );
        t.insert(
            DialogueType::Frustration,
            vec!["This is challenging: {context}".to_string()],
        );
        t
    }

    /// Add a dialogue entry, evicting the oldest if at capacity.
    pub fn add_entry(&mut self, entry: DialogueEntry) {
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(entry);
    }

    /// Generate a reflection entry and add it to history.
    pub fn reflect(&mut self, context: &str, importance: f64) -> DialogueEntry {
        self.reflection_count += 1;
        let entry = DialogueEntry::new(
            DialogueType::Reflection,
            "Reflecting...",
            context,
            importance,
        );
        self.add_entry(entry.clone());
        entry
    }

    /// Fill the first template for the given dialogue type with the context.
    pub fn generate_from_template(&self, dtype: &DialogueType, context: &str) -> Option<String> {
        self.templates
            .get(dtype)
            .and_then(|templates| templates.first())
            .map(|t| t.replace("{context}", context))
    }

    /// Fill the template at `index` for the given dialogue type with the context.
    pub fn generate_from_template_index(
        &self,
        dtype: &DialogueType,
        context: &str,
        index: usize,
    ) -> Option<String> {
        self.templates
            .get(dtype)
            .and_then(|templates| templates.get(index))
            .map(|t| t.replace("{context}", context))
    }

    /// Return the N most recent entries (newest first).
    pub fn recent(&self, n: usize) -> Vec<&DialogueEntry> {
        self.history.iter().rev().take(n).collect()
    }

    /// Return all entries matching the given dialogue type.
    pub fn by_type(&self, dtype: &DialogueType) -> Vec<&DialogueEntry> {
        self.history
            .iter()
            .filter(|e| &e.dialogue_type == dtype)
            .collect()
    }

    /// Return entries with importance at or above the threshold.
    pub fn high_importance(&self, threshold: f64) -> Vec<&DialogueEntry> {
        self.history
            .iter()
            .filter(|e| e.importance >= threshold)
            .collect()
    }

    /// Total number of reflections generated.
    pub fn reflection_count(&self) -> u64 {
        self.reflection_count
    }

    /// Current history length.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Clear all history entries.
    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Return the number of templates registered for a given dialogue type.
    pub fn template_count(&self, dtype: &DialogueType) -> usize {
        self.templates.get(dtype).map_or(0, |v| v.len())
    }

    /// Register a custom template for a dialogue type.
    pub fn add_template(&mut self, dtype: DialogueType, template: String) {
        self.templates.entry(dtype).or_default().push(template);
    }

    /// Return all distinct dialogue types present in history.
    pub fn active_types(&self) -> Vec<DialogueType> {
        let mut seen = std::collections::HashSet::new();
        let mut types = Vec::new();
        for entry in &self.history {
            if seen.insert(entry.dialogue_type.clone()) {
                types.push(entry.dialogue_type.clone());
            }
        }
        types
    }
}

impl Default for ConsciousnessDialogue {
    fn default() -> Self {
        Self::new(1000)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default() {
        let cd = ConsciousnessDialogue::default();
        assert_eq!(cd.history_len(), 0);
        assert_eq!(cd.reflection_count(), 0);
    }

    #[test]
    fn test_add_entry() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "plan",
            "ctx",
            0.5,
        ));
        assert_eq!(cd.history_len(), 1);
    }

    #[test]
    fn test_eviction_at_capacity() {
        let mut cd = ConsciousnessDialogue::new(3);
        for i in 0..5 {
            cd.add_entry(DialogueEntry::new(
                DialogueType::Planning,
                &format!("entry_{i}"),
                "ctx",
                0.5,
            ));
        }
        assert_eq!(cd.history_len(), 3);
        // Oldest should have been evicted
        let recent = cd.recent(3);
        assert_eq!(recent[0].content, "entry_4");
        assert_eq!(recent[2].content, "entry_2");
    }

    #[test]
    fn test_reflect_increments_count() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.reflect("ctx", 0.9);
        cd.reflect("ctx2", 0.8);
        assert_eq!(cd.reflection_count(), 2);
        assert_eq!(cd.history_len(), 2);
    }

    #[test]
    fn test_reflect_entry_properties() {
        let mut cd = ConsciousnessDialogue::new(10);
        let entry = cd.reflect("test context", 0.75);
        assert_eq!(entry.dialogue_type, DialogueType::Reflection);
        assert_eq!(entry.content, "Reflecting...");
        assert_eq!(entry.context, "test context");
        assert!((entry.importance - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_importance_clamping() {
        let entry = DialogueEntry::new(DialogueType::Concern, "c", "ctx", 2.0);
        assert!((entry.importance - 1.0).abs() < f64::EPSILON);

        let entry2 = DialogueEntry::new(DialogueType::Concern, "c", "ctx", -0.5);
        assert!(entry2.importance.abs() < f64::EPSILON);
    }

    #[test]
    fn test_generate_from_template() {
        let cd = ConsciousnessDialogue::new(10);
        let result = cd
            .generate_from_template(&DialogueType::Reflection, "user input")
            .unwrap();
        assert_eq!(result, "I notice that user input...");
    }

    #[test]
    fn test_generate_from_template_index() {
        let cd = ConsciousnessDialogue::new(10);
        let result = cd
            .generate_from_template_index(&DialogueType::Reflection, "patterns", 2)
            .unwrap();
        assert_eq!(result, "This reminds me of a pattern: patterns");
    }

    #[test]
    fn test_generate_from_template_missing_type() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.templates.clear();
        assert!(cd
            .generate_from_template(&DialogueType::Reflection, "ctx")
            .is_none());
    }

    #[test]
    fn test_recent_ordering() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "first",
            "ctx",
            0.1,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Evaluation,
            "second",
            "ctx",
            0.2,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Curiosity,
            "third",
            "ctx",
            0.3,
        ));
        let recent = cd.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "third");
        assert_eq!(recent[1].content, "second");
    }

    #[test]
    fn test_by_type_filtering() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "p1",
            "ctx",
            0.5,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Concern,
            "c1",
            "ctx",
            0.5,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "p2",
            "ctx",
            0.5,
        ));
        let planning = cd.by_type(&DialogueType::Planning);
        assert_eq!(planning.len(), 2);
        assert_eq!(planning[0].content, "p1");
        assert_eq!(planning[1].content, "p2");
    }

    #[test]
    fn test_high_importance() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Satisfaction,
            "low",
            "ctx",
            0.2,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Satisfaction,
            "high",
            "ctx",
            0.9,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Satisfaction,
            "mid",
            "ctx",
            0.5,
        ));
        let important = cd.high_importance(0.5);
        assert_eq!(important.len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Frustration,
            "f",
            "ctx",
            0.5,
        ));
        cd.reflect("ctx", 0.5);
        cd.clear();
        assert_eq!(cd.history_len(), 0);
        // reflection_count is not cleared
        assert_eq!(cd.reflection_count(), 1);
    }

    #[test]
    fn test_add_custom_template() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_template(
            DialogueType::Curiosity,
            "Fascinating: {context}!".to_string(),
        );
        // Should now have 3 curiosity templates (2 default + 1 custom)
        assert_eq!(cd.template_count(&DialogueType::Curiosity), 3);
    }

    #[test]
    fn test_active_types() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "p",
            "ctx",
            0.5,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Concern,
            "c",
            "ctx",
            0.5,
        ));
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "p2",
            "ctx",
            0.5,
        ));
        let types = cd.active_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&DialogueType::Planning));
        assert!(types.contains(&DialogueType::Concern));
    }

    #[test]
    fn test_template_count() {
        let cd = ConsciousnessDialogue::new(10);
        assert_eq!(cd.template_count(&DialogueType::Reflection), 3);
        assert_eq!(cd.template_count(&DialogueType::Satisfaction), 1);
        assert_eq!(cd.template_count(&DialogueType::Frustration), 1);
        assert_eq!(cd.template_count(&DialogueType::Planning), 2);
    }

    #[test]
    fn test_entry_timestamp_positive() {
        let entry = DialogueEntry::new(DialogueType::Evaluation, "e", "ctx", 0.5);
        assert!(entry.timestamp > 0.0);
    }

    #[test]
    fn test_recent_more_than_available() {
        let mut cd = ConsciousnessDialogue::new(10);
        cd.add_entry(DialogueEntry::new(
            DialogueType::Planning,
            "only",
            "ctx",
            0.5,
        ));
        let recent = cd.recent(100);
        assert_eq!(recent.len(), 1);
    }
}
