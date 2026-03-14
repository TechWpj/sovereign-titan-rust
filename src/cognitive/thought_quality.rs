//! Thought Quality Scoring — scores consciousness thoughts on 4 axes.
//!
//! Ported from `sovereign_titan/cognitive/thought_quality.py`.
//! Provides a quality gate that suppresses low-value thoughts and tracks
//! rolling quality averages per category.

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Quality score for a single thought on 4 axes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtQualityScore {
    pub novelty: f64,
    pub depth: f64,
    pub actionability: f64,
    pub grounding: f64,
    pub composite: f64,
    pub suppressed: bool,
}

impl ThoughtQualityScore {
    /// Compute composite score from individual axes.
    pub fn compute(novelty: f64, depth: f64, actionability: f64, grounding: f64) -> Self {
        let composite = novelty * WEIGHT_NOVELTY
            + depth * WEIGHT_DEPTH
            + actionability * WEIGHT_ACTIONABILITY
            + grounding * WEIGHT_GROUNDING;
        Self {
            novelty,
            depth,
            actionability,
            grounding,
            composite,
            suppressed: false,
        }
    }
}

// Axis weights for composite score (depth-weighted for reasoning quality).
const WEIGHT_NOVELTY: f64 = 0.25;
const WEIGHT_DEPTH: f64 = 0.35;
const WEIGHT_ACTIONABILITY: f64 = 0.20;
const WEIGHT_GROUNDING: f64 = 0.20;

/// Scored thought record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredThought {
    pub text: String,
    pub category: String,
    pub score: ThoughtQualityScore,
    pub timestamp: f64,
}

/// Thought quality scorer with rolling history.
pub struct ThoughtQualityScorer {
    /// Quality threshold — thoughts below this composite score are suppressed.
    threshold: f64,
    /// Rolling quality history per category (maxlen 50 each).
    category_history: HashMap<String, VecDeque<f64>>,
    /// All scored thoughts (maxlen 200).
    all_scores: VecDeque<ScoredThought>,
}

impl ThoughtQualityScorer {
    /// Create a new scorer with the given suppression threshold.
    pub fn new(threshold: f64) -> Self {
        Self {
            threshold,
            category_history: HashMap::new(),
            all_scores: VecDeque::with_capacity(200),
        }
    }

    /// Score a thought using heuristic analysis (no LLM needed).
    /// Returns the score with `suppressed` set if below threshold.
    pub fn score_heuristic(
        &mut self,
        text: &str,
        category: &str,
        recent_thoughts: &[String],
    ) -> ThoughtQualityScore {
        let novelty = self.heuristic_novelty(text, recent_thoughts);
        let depth = self.heuristic_depth(text);
        let actionability = self.heuristic_actionability(text);
        let grounding = self.heuristic_grounding(text);

        let mut score = ThoughtQualityScore::compute(novelty, depth, actionability, grounding);

        if score.composite < self.threshold {
            score.suppressed = true;
        }

        // Record in history
        let entry = self.category_history
            .entry(category.to_string())
            .or_insert_with(|| VecDeque::with_capacity(50));
        if entry.len() >= 50 {
            entry.pop_front();
        }
        entry.push_back(score.composite);

        if self.all_scores.len() >= 200 {
            self.all_scores.pop_front();
        }
        self.all_scores.push_back(ScoredThought {
            text: text.to_string(),
            category: category.to_string(),
            score: score.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        });

        score
    }

    /// Parse LLM response of "novelty depth actionability grounding" floats.
    pub fn parse_llm_scores(response: &str) -> Option<ThoughtQualityScore> {
        let numbers: Vec<f64> = response
            .split_whitespace()
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();

        if numbers.len() >= 4 {
            Some(ThoughtQualityScore::compute(
                numbers[0].clamp(0.0, 1.0),
                numbers[1].clamp(0.0, 1.0),
                numbers[2].clamp(0.0, 1.0),
                numbers[3].clamp(0.0, 1.0),
            ))
        } else {
            None
        }
    }

    /// Get average quality score for a category.
    pub fn category_average(&self, category: &str) -> Option<f64> {
        self.category_history.get(category).and_then(|hist| {
            if hist.is_empty() {
                None
            } else {
                Some(hist.iter().sum::<f64>() / hist.len() as f64)
            }
        })
    }

    /// Get overall average quality across all thoughts.
    pub fn overall_average(&self) -> Option<f64> {
        if self.all_scores.is_empty() {
            None
        } else {
            let sum: f64 = self.all_scores.iter().map(|s| s.score.composite).sum();
            Some(sum / self.all_scores.len() as f64)
        }
    }

    /// Get the count of suppressed thoughts.
    pub fn suppressed_count(&self) -> usize {
        self.all_scores.iter().filter(|s| s.score.suppressed).count()
    }

    /// Total scored thoughts.
    pub fn total_scored(&self) -> usize {
        self.all_scores.len()
    }

    /// Get recent scored thoughts (last N).
    pub fn recent_scores(&self, n: usize) -> Vec<&ScoredThought> {
        self.all_scores.iter().rev().take(n).collect()
    }

    // ── Heuristic scoring functions ─────────────────────────────────────

    fn heuristic_novelty(&self, text: &str, recent: &[String]) -> f64 {
        if recent.is_empty() {
            return 0.7; // No context → assume moderate novelty
        }

        let text_words: std::collections::HashSet<&str> = text.split_whitespace().collect();
        let mut max_overlap = 0.0;

        for prev in recent {
            let prev_words: std::collections::HashSet<&str> = prev.split_whitespace().collect();
            let intersection = text_words.intersection(&prev_words).count();
            let union = text_words.union(&prev_words).count();
            if union > 0 {
                let jaccard = intersection as f64 / union as f64;
                if jaccard > max_overlap {
                    max_overlap = jaccard;
                }
            }
        }

        // High overlap → low novelty
        (1.0 - max_overlap).clamp(0.0, 1.0)
    }

    fn heuristic_depth(&self, text: &str) -> f64 {
        let words = text.split_whitespace().count();
        let has_reasoning = text.contains("because")
            || text.contains("therefore")
            || text.contains("however")
            || text.contains("which means")
            || text.contains("indicates")
            || text.contains("suggests");

        let sentences = text.matches(". ").count() + 1;

        let mut score: f64 = 0.3; // baseline

        if words > 30 {
            score += 0.2;
        }
        if has_reasoning {
            score += 0.2;
        }
        if sentences >= 3 {
            score += 0.1;
        }
        if words > 60 {
            score += 0.1;
        }

        score.min(1.0)
    }

    fn heuristic_actionability(&self, text: &str) -> f64 {
        let action_words = [
            "should", "could", "will", "need to", "plan to",
            "next step", "investigate", "check", "verify",
            "run", "test", "try", "monitor", "scan",
        ];

        let count = action_words
            .iter()
            .filter(|w| text.to_lowercase().contains(**w))
            .count();

        match count {
            0 => 0.2,
            1 => 0.5,
            2 => 0.7,
            _ => 0.9,
        }
    }

    fn heuristic_grounding(&self, text: &str) -> f64 {
        let grounding_indicators = [
            // Specific data references
            text.contains('%'),
            text.contains("MB") || text.contains("GB") || text.contains("KB"),
            text.contains("ms") || text.contains("seconds"),
            // Process/file references
            text.contains(".exe") || text.contains(".dll"),
            text.contains("port ") || text.contains("PID"),
            // Timestamps/numbers
            text.chars().filter(|c| c.is_ascii_digit()).count() > 3,
            // Tool output references
            text.contains("observed") || text.contains("detected"),
        ];

        let count = grounding_indicators.iter().filter(|&&v| v).count();
        match count {
            0 => 0.2,
            1 => 0.4,
            2 => 0.6,
            3 => 0.8,
            _ => 0.95,
        }
    }
}

impl Default for ThoughtQualityScorer {
    fn default() -> Self {
        Self::new(0.3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_composite() {
        let score = ThoughtQualityScore::compute(0.8, 0.6, 0.5, 0.7);
        // 0.8*0.25 + 0.6*0.35 + 0.5*0.20 + 0.7*0.20 = 0.2+0.21+0.1+0.14 = 0.65
        assert!((score.composite - 0.65).abs() < 0.01);
    }

    #[test]
    fn test_parse_llm_scores() {
        let score = ThoughtQualityScorer::parse_llm_scores("0.7 0.8 0.5 0.6");
        assert!(score.is_some());
        let s = score.unwrap();
        assert!((s.novelty - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_parse_invalid_llm_scores() {
        assert!(ThoughtQualityScorer::parse_llm_scores("invalid response").is_none());
    }

    #[test]
    fn test_heuristic_scoring() {
        let mut scorer = ThoughtQualityScorer::default();
        let score = scorer.score_heuristic(
            "I observed that CPU usage is at 45% which suggests the system is under moderate load. \
             I should investigate which processes are consuming the most resources because \
             this could indicate a background task running.",
            "observation",
            &[],
        );
        // Should score reasonably well on all axes
        assert!(score.composite > 0.3);
        assert!(!score.suppressed);
    }

    #[test]
    fn test_suppression() {
        let mut scorer = ThoughtQualityScorer::new(0.9); // Very high threshold
        let score = scorer.score_heuristic("hello", "test", &[]);
        assert!(score.suppressed);
    }

    #[test]
    fn test_novelty_with_overlap() {
        let mut scorer = ThoughtQualityScorer::default();
        let recent = vec!["the system is running fine right now".to_string()];
        let score = scorer.score_heuristic(
            "the system is running fine right now",
            "observation",
            &recent,
        );
        // Same text → low novelty
        assert!(score.novelty < 0.3);
    }

    #[test]
    fn test_category_average() {
        let mut scorer = ThoughtQualityScorer::default();
        scorer.score_heuristic("thought one about security scanning", "security", &[]);
        scorer.score_heuristic("thought two about security monitoring", "security", &[]);
        let avg = scorer.category_average("security");
        assert!(avg.is_some());
    }

    #[test]
    fn test_overall_average() {
        let mut scorer = ThoughtQualityScorer::default();
        scorer.score_heuristic("test thought", "test", &[]);
        assert!(scorer.overall_average().is_some());
    }

    #[test]
    fn test_suppressed_count() {
        let mut scorer = ThoughtQualityScorer::new(0.99);
        scorer.score_heuristic("hi", "test", &[]);
        assert_eq!(scorer.suppressed_count(), 1);
    }

    #[test]
    fn test_depth_scoring() {
        let mut scorer = ThoughtQualityScorer::default();
        // Short, simple text → low depth
        let s1 = scorer.score_heuristic("hello world", "test", &[]);
        // Long, reasoning text → higher depth
        let s2 = scorer.score_heuristic(
            "I noticed that the CPU usage has been consistently high because there are multiple \
             background processes running. This suggests that the system may need optimization. \
             Therefore I should investigate which processes are consuming the most resources.",
            "test",
            &[],
        );
        assert!(s2.depth > s1.depth);
    }
}
