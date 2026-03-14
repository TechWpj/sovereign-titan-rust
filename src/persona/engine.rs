//! Persona Engine — unified personality and response formatting.
//!
//! Ported from `sovereign_titan/persona/engine.py`.
//! Features:
//! - Configurable personality (name, voice, self-reference)
//! - Complexity estimation from query text
//! - Response formatting with optional energy bar
//! - Rejection generation with alternatives
//! - System prompt generation with state summary
//! - Few-shot persona examples

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the persona engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaConfig {
    /// The persona's name.
    pub name: String,
    /// Voice style descriptor.
    pub voice: String,
    /// How the persona refers to itself.
    pub self_reference: String,
    /// Whether to include the energy bar in formatted output.
    pub show_energy_bar: bool,
    /// Maximum number of few-shot examples to include in prompts.
    pub few_shot_window: usize,
}

impl Default for PersonaConfig {
    fn default() -> Self {
        Self {
            name: "Titan".to_string(),
            voice: "confident_precise".to_string(),
            self_reference: "I".to_string(),
            show_energy_bar: true,
            few_shot_window: 10,
        }
    }
}

/// A few-shot persona example (query/response pair).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaExample {
    /// User query.
    pub query: String,
    /// Expected response.
    pub response: String,
}

/// Statistics about the persona engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaStats {
    /// Total responses generated.
    pub responses_generated: u64,
    /// Total tokens processed (estimated).
    pub total_tokens: u64,
    /// Number of loaded persona examples.
    pub example_count: usize,
    /// Number of reasoning templates.
    pub template_count: usize,
    /// Persona name.
    pub name: String,
    /// Voice style.
    pub voice: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Keyword tables for complexity estimation
// ─────────────────────────────────────────────────────────────────────────────

/// Keywords that indicate higher complexity and their additive weights.
const HIGH_COMPLEXITY_KEYWORDS: &[(&str, f64)] = &[
    ("explain", 0.15),
    ("analyze", 0.2),
    ("compare", 0.2),
    ("implement", 0.25),
    ("design", 0.25),
    ("optimize", 0.2),
    ("debug", 0.15),
    ("refactor", 0.2),
    ("architecture", 0.25),
    ("algorithm", 0.2),
    ("security", 0.15),
    ("performance", 0.15),
    ("concurrent", 0.2),
    ("distributed", 0.25),
    ("machine learning", 0.25),
];

/// Keywords that indicate lower complexity and their subtractive weights.
const LOW_COMPLEXITY_KEYWORDS: &[(&str, f64)] = &[
    ("what is", -0.1),
    ("open", -0.15),
    ("close", -0.15),
    ("time", -0.1),
    ("hello", -0.2),
    ("thanks", -0.2),
    ("hi", -0.2),
];

// ─────────────────────────────────────────────────────────────────────────────
// Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Unified personality engine for response generation and formatting.
pub struct PersonaEngine {
    /// Personality configuration.
    config: PersonaConfig,
    /// Few-shot persona examples.
    persona_examples: Vec<PersonaExample>,
    /// Named reasoning templates.
    reasoning_templates: HashMap<String, String>,
    /// Total responses formatted.
    responses_generated: u64,
    /// Estimated total tokens.
    total_tokens: u64,
}

impl PersonaEngine {
    /// Create a new persona engine with the given configuration.
    pub fn new(config: PersonaConfig) -> Self {
        let mut reasoning_templates = HashMap::new();

        reasoning_templates.insert(
            "analysis".to_string(),
            format!(
                "{ref_} will analyze this systematically, breaking it down into components.",
                ref_ = config.self_reference
            ),
        );
        reasoning_templates.insert(
            "implementation".to_string(),
            format!(
                "{ref_} will implement this step by step, ensuring correctness at each stage.",
                ref_ = config.self_reference
            ),
        );
        reasoning_templates.insert(
            "explanation".to_string(),
            format!(
                "{ref_} will explain this clearly, starting from fundamentals.",
                ref_ = config.self_reference
            ),
        );
        reasoning_templates.insert(
            "troubleshooting".to_string(),
            format!(
                "{ref_} will diagnose this methodically, checking the most likely causes first.",
                ref_ = config.self_reference
            ),
        );
        reasoning_templates.insert(
            "creative".to_string(),
            format!(
                "{ref_} will explore this creatively, considering multiple approaches.",
                ref_ = config.self_reference
            ),
        );

        Self {
            config,
            persona_examples: Vec::new(),
            reasoning_templates,
            responses_generated: 0,
            total_tokens: 0,
        }
    }

    /// Load persona examples. Returns the number of examples loaded.
    pub fn load_examples(&mut self, examples: Vec<PersonaExample>) -> usize {
        let count = examples.len();
        self.persona_examples.extend(examples);
        count
    }

    /// Estimate the complexity of a query (0.0 to 1.0).
    ///
    /// Uses a combination of text length, keyword matching, and structural
    /// indicators (question marks, code blocks).
    pub fn estimate_complexity(&self, query: &str) -> f64 {
        let lower = query.to_lowercase();

        // Base complexity from length
        let words = query.split_whitespace().count();
        let length_factor = (words as f64 / 100.0).min(0.3);
        let mut complexity = 0.3 + length_factor;

        // High-complexity keyword boosts
        for (keyword, weight) in HIGH_COMPLEXITY_KEYWORDS {
            if lower.contains(keyword) {
                complexity += weight;
            }
        }

        // Low-complexity keyword reductions
        for (keyword, weight) in LOW_COMPLEXITY_KEYWORDS {
            if lower.contains(keyword) {
                complexity += weight; // weight is negative
            }
        }

        // Structural indicators
        if lower.contains('?') {
            complexity += 0.05;
        }
        if lower.contains("```") {
            complexity += 0.15;
        }
        if lower.contains('\n') {
            complexity += 0.05;
        }

        complexity.clamp(0.0, 1.0)
    }

    /// Get a human-readable complexity label.
    pub fn complexity_label(complexity: f64) -> &'static str {
        if complexity < 0.3 {
            "LOW"
        } else if complexity < 0.5 {
            "MODERATE"
        } else if complexity < 0.75 {
            "HIGH"
        } else {
            "VERY HIGH"
        }
    }

    /// Format a response with the persona's style, optionally including an
    /// energy bar.
    pub fn format_response(&mut self, content: &str, energy_bar: Option<&str>) -> String {
        self.responses_generated += 1;
        // Rough token estimate: ~4 chars per token
        self.total_tokens += (content.len() as u64) / 4;

        if self.config.show_energy_bar {
            if let Some(bar) = energy_bar {
                return format!("{}\n\n{}", content, bar);
            }
        }

        content.to_string()
    }

    /// Generate a rejection message with optional alternatives.
    pub fn generate_rejection(
        &mut self,
        reason: &str,
        alternatives: Option<&[&str]>,
    ) -> String {
        self.responses_generated += 1;

        let mut msg = format!(
            "{} cannot process this request: {}",
            self.config.self_reference, reason
        );

        if let Some(alts) = alternatives {
            if !alts.is_empty() {
                msg.push_str("\n\nAlternatives you could try:");
                for (i, alt) in alts.iter().enumerate() {
                    msg.push_str(&format!("\n  {}. {}", i + 1, alt));
                }
            }
        }

        msg
    }

    /// Generate the system prompt incorporating the persona's identity and
    /// the current state summary.
    pub fn get_system_prompt(&self, state_summary: &str) -> String {
        let mut prompt = format!(
            "You are {name}, a sovereign AI assistant. \
             Your voice is {voice}. You refer to yourself as \"{self_ref}\".\n\n\
             Current state:\n{state}\n",
            name = self.config.name,
            voice = self.config.voice,
            self_ref = self.config.self_reference,
            state = state_summary,
        );

        // Append few-shot examples (up to window size)
        let examples_to_include = self
            .persona_examples
            .iter()
            .rev()
            .take(self.config.few_shot_window)
            .collect::<Vec<_>>();

        if !examples_to_include.is_empty() {
            prompt.push_str("\nExamples of expected behavior:\n");
            for example in examples_to_include.iter().rev() {
                prompt.push_str(&format!(
                    "\nUser: {}\nAssistant: {}\n",
                    example.query, example.response
                ));
            }
        }

        // Append reasoning template hints
        prompt.push_str(
            "\nReasoning approach: Choose the appropriate strategy based on the task — \
             analysis, implementation, explanation, troubleshooting, or creative.\n",
        );

        prompt
    }

    /// Get usage statistics.
    pub fn get_stats(&self) -> PersonaStats {
        PersonaStats {
            responses_generated: self.responses_generated,
            total_tokens: self.total_tokens,
            example_count: self.persona_examples.len(),
            template_count: self.reasoning_templates.len(),
            name: self.config.name.clone(),
            voice: self.config.voice.clone(),
        }
    }

    /// Get the persona name.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Get the voice style.
    pub fn voice(&self) -> &str {
        &self.config.voice
    }

    /// Get the reasoning template for a given strategy.
    pub fn reasoning_template(&self, strategy: &str) -> Option<&str> {
        self.reasoning_templates.get(strategy).map(|s| s.as_str())
    }
}

impl Default for PersonaEngine {
    fn default() -> Self {
        Self::new(PersonaConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_persona() {
        let engine = PersonaEngine::default();
        assert_eq!(engine.name(), "Titan");
        assert_eq!(engine.voice(), "confident_precise");
    }

    #[test]
    fn test_estimate_complexity_simple() {
        let engine = PersonaEngine::default();
        let complexity = engine.estimate_complexity("hello");
        assert!(complexity < 0.3);
    }

    #[test]
    fn test_estimate_complexity_complex() {
        let engine = PersonaEngine::default();
        let complexity =
            engine.estimate_complexity("explain the distributed algorithm architecture for this system and optimize performance");
        assert!(complexity > 0.5);
    }

    #[test]
    fn test_complexity_label_low() {
        assert_eq!(PersonaEngine::complexity_label(0.1), "LOW");
    }

    #[test]
    fn test_complexity_label_moderate() {
        assert_eq!(PersonaEngine::complexity_label(0.35), "MODERATE");
    }

    #[test]
    fn test_complexity_label_high() {
        assert_eq!(PersonaEngine::complexity_label(0.6), "HIGH");
    }

    #[test]
    fn test_complexity_label_very_high() {
        assert_eq!(PersonaEngine::complexity_label(0.9), "VERY HIGH");
    }

    #[test]
    fn test_format_response_without_bar() {
        let mut engine = PersonaEngine::default();
        let result = engine.format_response("Hello, world!", None);
        assert_eq!(result, "Hello, world!");
        assert_eq!(engine.responses_generated, 1);
    }

    #[test]
    fn test_format_response_with_bar() {
        let mut engine = PersonaEngine::default();
        let result = engine.format_response("Done.", Some("[####----] 50%"));
        assert!(result.contains("Done."));
        assert!(result.contains("[####----] 50%"));
    }

    #[test]
    fn test_format_response_bar_hidden() {
        let config = PersonaConfig {
            show_energy_bar: false,
            ..Default::default()
        };
        let mut engine = PersonaEngine::new(config);
        let result = engine.format_response("Done.", Some("[####----] 50%"));
        assert!(!result.contains("[####----] 50%"));
    }

    #[test]
    fn test_generate_rejection_no_alternatives() {
        let mut engine = PersonaEngine::default();
        let msg = engine.generate_rejection("insufficient energy", None);
        assert!(msg.contains("cannot process"));
        assert!(msg.contains("insufficient energy"));
    }

    #[test]
    fn test_generate_rejection_with_alternatives() {
        let mut engine = PersonaEngine::default();
        let msg = engine.generate_rejection(
            "conservation mode",
            Some(&["Try a simpler query", "Wait for energy regeneration"]),
        );
        assert!(msg.contains("Alternatives"));
        assert!(msg.contains("Try a simpler query"));
        assert!(msg.contains("Wait for energy regeneration"));
    }

    #[test]
    fn test_get_system_prompt() {
        let engine = PersonaEngine::default();
        let prompt = engine.get_system_prompt("Energy: 850/1000, State: OPTIMAL");
        assert!(prompt.contains("Titan"));
        assert!(prompt.contains("confident_precise"));
        assert!(prompt.contains("Energy: 850/1000"));
    }

    #[test]
    fn test_load_examples() {
        let mut engine = PersonaEngine::default();
        let examples = vec![
            PersonaExample {
                query: "What time is it?".to_string(),
                response: "The current time is 3:00 PM.".to_string(),
            },
            PersonaExample {
                query: "Open Discord".to_string(),
                response: "Opening Discord now.".to_string(),
            },
        ];
        let count = engine.load_examples(examples);
        assert_eq!(count, 2);
        assert_eq!(engine.get_stats().example_count, 2);
    }

    #[test]
    fn test_system_prompt_includes_examples() {
        let mut engine = PersonaEngine::default();
        engine.load_examples(vec![PersonaExample {
            query: "test query".to_string(),
            response: "test response".to_string(),
        }]);
        let prompt = engine.get_system_prompt("State: ACTIVE");
        assert!(prompt.contains("test query"));
        assert!(prompt.contains("test response"));
    }

    #[test]
    fn test_reasoning_template() {
        let engine = PersonaEngine::default();
        assert!(engine.reasoning_template("analysis").is_some());
        assert!(engine.reasoning_template("nonexistent").is_none());
    }

    #[test]
    fn test_stats() {
        let mut engine = PersonaEngine::default();
        engine.format_response("Hello world", None);
        engine.format_response("Another response", None);
        let stats = engine.get_stats();
        assert_eq!(stats.responses_generated, 2);
        assert!(stats.total_tokens > 0);
        assert_eq!(stats.template_count, 5);
    }

    #[test]
    fn test_complexity_clamped() {
        let engine = PersonaEngine::default();
        // Even with many keywords, complexity should not exceed 1.0
        let complexity = engine.estimate_complexity(
            "explain analyze compare implement design optimize debug refactor \
             architecture algorithm security performance concurrent distributed \
             machine learning",
        );
        assert!(complexity <= 1.0);
        assert!(complexity >= 0.0);
    }
}
