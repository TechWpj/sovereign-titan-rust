//! Semantic Classifier — intent classification for incoming queries.
//!
//! Ported from `sovereign_titan/routing/classifier.py`.
//! Classifies user queries into intent categories using keyword matching
//! and pattern recognition.

use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Classified intent result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedIntent {
    pub intent: String,
    pub confidence: f64,
    pub sub_intents: Vec<String>,
}

/// Semantic intent categories with example phrases.
pub fn semantic_intents() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m = HashMap::new();
    m.insert("tool_use", vec![
        "open an application", "run a command", "take a screenshot",
        "search for files", "control the system", "manage windows",
    ]);
    m.insert("research", vec![
        "search the web", "find information about", "look up",
        "what is", "who is", "tell me about",
    ]);
    m.insert("analysis", vec![
        "analyze this", "explain why", "compare these",
        "what's the difference", "break down", "investigate",
    ]);
    m.insert("code", vec![
        "write code", "fix this bug", "create a script",
        "refactor", "implement", "debug this",
    ]);
    m.insert("math", vec![
        "calculate", "compute", "what is the sum",
        "solve this equation", "convert units",
    ]);
    m.insert("creative", vec![
        "write a story", "compose", "create a poem",
        "brainstorm", "design", "imagine",
    ]);
    m.insert("simple", vec![
        "hello", "hi", "hey", "thanks", "bye",
        "good morning", "how are you",
    ]);
    m.insert("memory_recall", vec![
        "what did we talk about", "remember when",
        "last time we", "do you recall", "what was that",
    ]);
    m.insert("document_creation", vec![
        "create a document", "write a report", "make a spreadsheet",
        "generate a PDF", "create a file",
    ]);
    m
}

/// Intent classifier using keyword and regex matching.
pub struct IntentClassifier {
    /// Intent patterns (compiled regex).
    patterns: Vec<(String, Regex, f64)>,
    /// Keyword maps.
    keyword_map: HashMap<String, Vec<String>>,
}

impl IntentClassifier {
    /// Create a new classifier with default patterns.
    pub fn new() -> Self {
        let mut patterns = Vec::new();

        // Tool use patterns
        patterns.push((
            "tool_use".to_string(),
            Regex::new(r"(?i)^(?:open|launch|start|run|close|kill|take|click|type|set\s+volume|mute|unmute|list\s+windows)").unwrap(),
            0.9,
        ));

        // Code patterns
        patterns.push((
            "code".to_string(),
            Regex::new(r"(?i)(?:write|create|implement|fix|debug|refactor)\s+(?:a\s+)?(?:code|function|class|script|program|module)").unwrap(),
            0.85,
        ));

        // Math patterns
        patterns.push((
            "math".to_string(),
            Regex::new(r"(?i)^(?:calculate|compute|what\s+is\s+\d|solve|convert\s+\d)").unwrap(),
            0.9,
        ));

        // Research patterns
        patterns.push((
            "research".to_string(),
            Regex::new(r"(?i)^(?:search|find|look\s+up|what\s+is|who\s+is|tell\s+me\s+about|explain)").unwrap(),
            0.7,
        ));

        // Simple/greeting patterns
        patterns.push((
            "simple".to_string(),
            Regex::new(r"(?i)^(?:hello|hi|hey|thanks|thank\s+you|bye|goodbye|good\s+(?:morning|afternoon|evening))").unwrap(),
            0.95,
        ));

        // Memory recall patterns
        patterns.push((
            "memory_recall".to_string(),
            Regex::new(r"(?i)(?:remember|recall|last\s+time|what\s+did\s+we|do\s+you\s+know)").unwrap(),
            0.8,
        ));

        // Document creation
        patterns.push((
            "document_creation".to_string(),
            Regex::new(r"(?i)(?:create|make|write|generate)\s+(?:a\s+)?(?:document|report|file|spreadsheet|csv|pdf)").unwrap(),
            0.85,
        ));

        // Creative
        patterns.push((
            "creative".to_string(),
            Regex::new(r"(?i)(?:write\s+(?:a\s+)?(?:story|poem|song)|brainstorm|imagine|compose|design)").unwrap(),
            0.8,
        ));

        // Build keyword map from semantic intents
        let mut keyword_map = HashMap::new();
        for (intent, phrases) in semantic_intents() {
            let keywords: Vec<String> = phrases
                .iter()
                .flat_map(|p| p.split_whitespace())
                .map(|w| w.to_lowercase())
                .collect();
            keyword_map.insert(intent.to_string(), keywords);
        }

        Self {
            patterns,
            keyword_map,
        }
    }

    /// Classify a query into an intent.
    pub fn classify(&self, query: &str) -> ClassifiedIntent {
        let trimmed = query.trim();

        // Try regex patterns first (highest confidence)
        for (intent, pattern, confidence) in &self.patterns {
            if pattern.is_match(trimmed) {
                return ClassifiedIntent {
                    intent: intent.clone(),
                    confidence: *confidence,
                    sub_intents: Vec::new(),
                };
            }
        }

        // Fall back to keyword matching
        let query_words: Vec<String> = trimmed.to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut best_intent = "research".to_string();
        let mut best_score = 0.0;

        for (intent, keywords) in &self.keyword_map {
            let matches = query_words
                .iter()
                .filter(|w| keywords.contains(w))
                .count();
            let score = if query_words.is_empty() {
                0.0
            } else {
                matches as f64 / query_words.len() as f64
            };
            if score > best_score {
                best_score = score;
                best_intent = intent.clone();
            }
        }

        ClassifiedIntent {
            intent: best_intent,
            confidence: (best_score * 0.7).min(0.7), // Cap keyword-only at 0.7
            sub_intents: Vec::new(),
        }
    }

    /// Number of registered patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

impl Default for IntentClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_use_classification() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("open Discord");
        assert_eq!(result.intent, "tool_use");
        assert!(result.confidence > 0.8);
    }

    #[test]
    fn test_math_classification() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("calculate 2 + 2");
        assert_eq!(result.intent, "math");
    }

    #[test]
    fn test_greeting_classification() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("hello");
        assert_eq!(result.intent, "simple");
        assert!(result.confidence > 0.9);
    }

    #[test]
    fn test_research_classification() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("what is quantum computing");
        assert_eq!(result.intent, "research");
    }

    #[test]
    fn test_code_classification() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("write a function to sort arrays");
        assert_eq!(result.intent, "code");
    }

    #[test]
    fn test_document_classification() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("create a report about sales data");
        assert_eq!(result.intent, "document_creation");
    }

    #[test]
    fn test_memory_recall() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("do you remember what we discussed");
        assert_eq!(result.intent, "memory_recall");
    }

    #[test]
    fn test_fallback() {
        let classifier = IntentClassifier::new();
        let result = classifier.classify("something completely random");
        // Should still return a valid intent
        assert!(!result.intent.is_empty());
    }

    #[test]
    fn test_pattern_count() {
        let classifier = IntentClassifier::new();
        assert!(classifier.pattern_count() >= 7);
    }
}
