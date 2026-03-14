//! Autodidactic Loader — training example loading and feature extraction.
//!
//! Manages collections of SFT and DPO training examples with quality
//! filtering, deduplication, statistics, and JSONL serialization.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// ExampleType
// ─────────────────────────────────────────────────────────────────────────────

/// The kind of training example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExampleType {
    /// Supervised fine-tuning (prompt + response).
    Sft,
    /// Direct Preference Optimization (prompt + chosen + rejected).
    Dpo,
    /// Reinforcement Learning from Human Feedback.
    Rlhf,
}

// ─────────────────────────────────────────────────────────────────────────────
// TrainingExample
// ─────────────────────────────────────────────────────────────────────────────

/// A single training example.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingExample {
    /// Unique identifier.
    pub id: String,
    /// The type of this example.
    pub example_type: ExampleType,
    /// The prompt / user message.
    pub prompt: String,
    /// The chosen response (SFT or DPO chosen).
    pub response: String,
    /// The rejected response (DPO only).
    pub rejected_response: Option<String>,
    /// Optional system prompt.
    pub system_prompt: Option<String>,
    /// Arbitrary metadata key-value pairs.
    pub metadata: HashMap<String, String>,
    /// Quality score in [0.0, 1.0].
    pub quality_score: f64,
    /// Source file this example came from.
    pub source_file: String,
}

impl TrainingExample {
    /// Create a new SFT example.
    pub fn sft(prompt: &str, response: &str) -> Self {
        Self {
            id: format!("ex_{}", crate::autonomous::types::now_secs() as u64),
            example_type: ExampleType::Sft,
            prompt: prompt.to_string(),
            response: response.to_string(),
            rejected_response: None,
            system_prompt: None,
            metadata: HashMap::new(),
            quality_score: 1.0,
            source_file: String::new(),
        }
    }

    /// Create a new DPO example with chosen and rejected responses.
    pub fn dpo(prompt: &str, chosen: &str, rejected: &str) -> Self {
        Self {
            id: format!("ex_{}", crate::autonomous::types::now_secs() as u64),
            example_type: ExampleType::Dpo,
            prompt: prompt.to_string(),
            response: chosen.to_string(),
            rejected_response: Some(rejected.to_string()),
            system_prompt: None,
            metadata: HashMap::new(),
            quality_score: 1.0,
            source_file: String::new(),
        }
    }

    /// Whether this is a DPO example.
    pub fn is_dpo(&self) -> bool {
        self.example_type == ExampleType::Dpo
    }

    /// Whether this is an SFT example.
    pub fn is_sft(&self) -> bool {
        self.example_type == ExampleType::Sft
    }

    /// Rough token count estimate (word count * 4/3).
    pub fn total_tokens_estimate(&self) -> usize {
        let word_count =
            self.prompt.split_whitespace().count() + self.response.split_whitespace().count();
        word_count * 4 / 3
    }

    /// Set the quality score (clamped to [0.0, 1.0]).
    pub fn with_quality(mut self, score: f64) -> Self {
        self.quality_score = score.clamp(0.0, 1.0);
        self
    }

    /// Set the source file.
    pub fn with_source(mut self, source: &str) -> Self {
        self.source_file = source.to_string();
        self
    }

    /// Add a metadata entry.
    pub fn with_meta(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = Some(prompt.to_string());
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DatasetStats
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate statistics for a dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetStats {
    /// Total number of examples.
    pub total_examples: usize,
    /// Number of SFT examples.
    pub sft_count: usize,
    /// Number of DPO examples.
    pub dpo_count: usize,
    /// Average prompt length in characters.
    pub avg_prompt_length: f64,
    /// Average response length in characters.
    pub avg_response_length: f64,
    /// Estimated total token count.
    pub total_tokens_estimate: usize,
    /// Unique source files.
    pub source_files: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// TrainingDataLoader
// ─────────────────────────────────────────────────────────────────────────────

/// Loads, filters, and manages training examples.
pub struct TrainingDataLoader {
    examples: Vec<TrainingExample>,
    quality_threshold: f64,
}

impl TrainingDataLoader {
    /// Create a new loader with the given quality threshold.
    ///
    /// Examples below this score are rejected by `add_example`.
    pub fn new(quality_threshold: f64) -> Self {
        Self {
            examples: Vec::new(),
            quality_threshold: quality_threshold.clamp(0.0, 1.0),
        }
    }

    /// Add an example if it meets the quality threshold.
    pub fn add_example(&mut self, example: TrainingExample) {
        if example.quality_score >= self.quality_threshold {
            self.examples.push(example);
        }
    }

    /// Add a batch of examples, filtering by quality.
    pub fn add_batch(&mut self, examples: Vec<TrainingExample>) {
        for ex in examples {
            self.add_example(ex);
        }
    }

    /// Get all SFT examples.
    pub fn sft_examples(&self) -> Vec<&TrainingExample> {
        self.examples.iter().filter(|e| e.is_sft()).collect()
    }

    /// Get all DPO examples.
    pub fn dpo_examples(&self) -> Vec<&TrainingExample> {
        self.examples.iter().filter(|e| e.is_dpo()).collect()
    }

    /// Get examples at or above a minimum quality score.
    pub fn filter_by_quality(&self, min_score: f64) -> Vec<&TrainingExample> {
        self.examples
            .iter()
            .filter(|e| e.quality_score >= min_score)
            .collect()
    }

    /// Compute aggregate statistics.
    pub fn stats(&self) -> DatasetStats {
        let sft_count = self.sft_examples().len();
        let dpo_count = self.dpo_examples().len();
        let total = self.examples.len();
        let avg_prompt = if total > 0 {
            self.examples.iter().map(|e| e.prompt.len() as f64).sum::<f64>() / total as f64
        } else {
            0.0
        };
        let avg_response = if total > 0 {
            self.examples
                .iter()
                .map(|e| e.response.len() as f64)
                .sum::<f64>()
                / total as f64
        } else {
            0.0
        };
        let total_tokens: usize = self.examples.iter().map(|e| e.total_tokens_estimate()).sum();
        let mut sources: Vec<String> = self
            .examples
            .iter()
            .map(|e| e.source_file.clone())
            .filter(|s| !s.is_empty())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        sources.sort();

        DatasetStats {
            total_examples: total,
            sft_count,
            dpo_count,
            avg_prompt_length: avg_prompt,
            avg_response_length: avg_response,
            total_tokens_estimate: total_tokens,
            source_files: sources,
        }
    }

    /// Total number of loaded examples.
    pub fn total_count(&self) -> usize {
        self.examples.len()
    }

    /// Remove all examples.
    pub fn clear(&mut self) {
        self.examples.clear();
    }

    /// Get the current quality threshold.
    pub fn quality_threshold(&self) -> f64 {
        self.quality_threshold
    }

    /// Serialize all examples to JSONL entries.
    pub fn to_jsonl_entries(&self) -> Vec<String> {
        self.examples
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect()
    }

    /// Remove duplicate examples (by prompt+response key).
    pub fn deduplicate(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.examples.retain(|e| {
            let key = format!("{}:{}", e.prompt, e.response);
            seen.insert(key)
        });
    }
}

impl Default for TrainingDataLoader {
    fn default() -> Self {
        Self::new(0.5)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── TrainingExample tests ─────────────────────────────────────────────

    #[test]
    fn test_sft_example() {
        let ex = TrainingExample::sft("What is Rust?", "A systems programming language.");
        assert!(ex.is_sft());
        assert!(!ex.is_dpo());
        assert_eq!(ex.example_type, ExampleType::Sft);
        assert_eq!(ex.prompt, "What is Rust?");
        assert_eq!(ex.response, "A systems programming language.");
        assert!(ex.rejected_response.is_none());
        assert!(ex.id.starts_with("ex_"));
    }

    #[test]
    fn test_dpo_example() {
        let ex = TrainingExample::dpo(
            "What is Rust?",
            "A systems programming language.",
            "I don't know.",
        );
        assert!(ex.is_dpo());
        assert!(!ex.is_sft());
        assert_eq!(ex.example_type, ExampleType::Dpo);
        assert_eq!(ex.rejected_response, Some("I don't know.".to_string()));
    }

    #[test]
    fn test_example_token_estimate() {
        let ex = TrainingExample::sft("one two three", "four five");
        let estimate = ex.total_tokens_estimate();
        // 5 words * 4/3 = 6 (integer division: 20/3 = 6)
        assert_eq!(estimate, 6);
    }

    #[test]
    fn test_example_builder_methods() {
        let ex = TrainingExample::sft("p", "r")
            .with_quality(0.8)
            .with_source("data/train.jsonl")
            .with_meta("category", "coding")
            .with_system_prompt("You are helpful.");

        assert!((ex.quality_score - 0.8).abs() < f64::EPSILON);
        assert_eq!(ex.source_file, "data/train.jsonl");
        assert_eq!(ex.metadata.get("category"), Some(&"coding".to_string()));
        assert_eq!(ex.system_prompt, Some("You are helpful.".to_string()));
    }

    #[test]
    fn test_example_quality_clamped() {
        let ex_high = TrainingExample::sft("p", "r").with_quality(2.0);
        assert!((ex_high.quality_score - 1.0).abs() < f64::EPSILON);

        let ex_low = TrainingExample::sft("p", "r").with_quality(-0.5);
        assert!((ex_low.quality_score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_example_serialization() {
        let ex = TrainingExample::sft("prompt", "response");
        let json = serde_json::to_string(&ex).unwrap();
        let parsed: TrainingExample = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt, "prompt");
        assert_eq!(parsed.response, "response");
        assert_eq!(parsed.example_type, ExampleType::Sft);
    }

    // ── TrainingDataLoader tests ──────────────────────────────────────────

    #[test]
    fn test_loader_default() {
        let loader = TrainingDataLoader::default();
        assert_eq!(loader.total_count(), 0);
        assert!((loader.quality_threshold() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_loader_add_example_above_threshold() {
        let mut loader = TrainingDataLoader::new(0.5);
        loader.add_example(TrainingExample::sft("p", "r").with_quality(0.8));
        assert_eq!(loader.total_count(), 1);
    }

    #[test]
    fn test_loader_rejects_below_threshold() {
        let mut loader = TrainingDataLoader::new(0.5);
        loader.add_example(TrainingExample::sft("p", "r").with_quality(0.3));
        assert_eq!(loader.total_count(), 0);
    }

    #[test]
    fn test_loader_add_batch() {
        let mut loader = TrainingDataLoader::new(0.0);
        let batch = vec![
            TrainingExample::sft("p1", "r1"),
            TrainingExample::dpo("p2", "r2a", "r2b"),
            TrainingExample::sft("p3", "r3"),
        ];
        loader.add_batch(batch);
        assert_eq!(loader.total_count(), 3);
    }

    #[test]
    fn test_loader_sft_dpo_filters() {
        let mut loader = TrainingDataLoader::new(0.0);
        loader.add_example(TrainingExample::sft("p1", "r1"));
        loader.add_example(TrainingExample::dpo("p2", "r2a", "r2b"));
        loader.add_example(TrainingExample::sft("p3", "r3"));

        assert_eq!(loader.sft_examples().len(), 2);
        assert_eq!(loader.dpo_examples().len(), 1);
    }

    #[test]
    fn test_loader_filter_by_quality() {
        let mut loader = TrainingDataLoader::new(0.0);
        loader.add_example(TrainingExample::sft("p1", "r1").with_quality(0.3));
        loader.add_example(TrainingExample::sft("p2", "r2").with_quality(0.7));
        loader.add_example(TrainingExample::sft("p3", "r3").with_quality(0.9));

        assert_eq!(loader.filter_by_quality(0.5).len(), 2);
        assert_eq!(loader.filter_by_quality(0.8).len(), 1);
        assert_eq!(loader.filter_by_quality(1.0).len(), 0);
    }

    #[test]
    fn test_loader_stats() {
        let mut loader = TrainingDataLoader::new(0.0);
        loader.add_example(
            TrainingExample::sft("hello world", "goodbye world")
                .with_source("file1.jsonl"),
        );
        loader.add_example(
            TrainingExample::dpo("prompt", "chosen", "rejected")
                .with_source("file2.jsonl"),
        );

        let stats = loader.stats();
        assert_eq!(stats.total_examples, 2);
        assert_eq!(stats.sft_count, 1);
        assert_eq!(stats.dpo_count, 1);
        assert!(stats.avg_prompt_length > 0.0);
        assert!(stats.avg_response_length > 0.0);
        assert!(stats.total_tokens_estimate > 0);
        assert_eq!(stats.source_files.len(), 2);
    }

    #[test]
    fn test_loader_stats_empty() {
        let loader = TrainingDataLoader::new(0.5);
        let stats = loader.stats();
        assert_eq!(stats.total_examples, 0);
        assert!((stats.avg_prompt_length - 0.0).abs() < f64::EPSILON);
        assert!((stats.avg_response_length - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_loader_clear() {
        let mut loader = TrainingDataLoader::new(0.0);
        loader.add_example(TrainingExample::sft("p", "r"));
        assert_eq!(loader.total_count(), 1);

        loader.clear();
        assert_eq!(loader.total_count(), 0);
    }

    #[test]
    fn test_loader_to_jsonl() {
        let mut loader = TrainingDataLoader::new(0.0);
        loader.add_example(TrainingExample::sft("p1", "r1"));
        loader.add_example(TrainingExample::sft("p2", "r2"));

        let entries = loader.to_jsonl_entries();
        assert_eq!(entries.len(), 2);
        // Each should be valid JSON.
        for entry in &entries {
            let parsed: serde_json::Value = serde_json::from_str(entry).unwrap();
            assert!(parsed.get("prompt").is_some());
        }
    }

    #[test]
    fn test_loader_deduplicate() {
        let mut loader = TrainingDataLoader::new(0.0);
        loader.add_example(TrainingExample::sft("hello", "world"));
        loader.add_example(TrainingExample::sft("hello", "world"));
        loader.add_example(TrainingExample::sft("hello", "different"));
        assert_eq!(loader.total_count(), 3);

        loader.deduplicate();
        assert_eq!(loader.total_count(), 2);
    }

    #[test]
    fn test_example_type_equality() {
        assert_eq!(ExampleType::Sft, ExampleType::Sft);
        assert_eq!(ExampleType::Dpo, ExampleType::Dpo);
        assert_ne!(ExampleType::Sft, ExampleType::Dpo);
        assert_ne!(ExampleType::Rlhf, ExampleType::Sft);
    }
}
