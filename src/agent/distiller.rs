//! Observation Distiller — distills long tool outputs into concise summaries.
//!
//! Ported from `sovereign_titan/agents/distiller.py`. When tool observations
//! exceed a threshold, the distiller extracts the most relevant information
//! using tool-specific templates and word overlap scoring.

use std::collections::HashMap;

/// Maximum observation length before distillation is triggered.
const DISTILL_THRESHOLD: usize = 3000;

/// Maximum distilled output length.
const MAX_DISTILLED_LEN: usize = 1500;

/// Tool-specific extraction templates.
struct ExtractionTemplate {
    /// Keywords to prioritize when extracting.
    priority_keywords: Vec<&'static str>,
    /// How many lines to keep from the start.
    head_lines: usize,
    /// How many lines to keep from the end.
    tail_lines: usize,
}

/// The observation distiller.
pub struct ObservationDistiller {
    templates: HashMap<&'static str, ExtractionTemplate>,
}

impl ObservationDistiller {
    pub fn new() -> Self {
        let mut templates = HashMap::new();

        templates.insert(
            "web_search",
            ExtractionTemplate {
                priority_keywords: vec!["result", "title", "url", "snippet", "description", "answer"],
                head_lines: 15,
                tail_lines: 3,
            },
        );

        templates.insert(
            "file_search",
            ExtractionTemplate {
                priority_keywords: vec!["found", "match", "path", "file", "directory"],
                head_lines: 20,
                tail_lines: 2,
            },
        );

        templates.insert(
            "shell",
            ExtractionTemplate {
                priority_keywords: vec!["error", "warning", "success", "fail", "output"],
                head_lines: 10,
                tail_lines: 10,
            },
        );

        templates.insert(
            "code_ops",
            ExtractionTemplate {
                priority_keywords: vec!["function", "class", "def", "fn", "struct", "impl", "error"],
                head_lines: 30,
                tail_lines: 5,
            },
        );

        templates.insert(
            "system_control",
            ExtractionTemplate {
                priority_keywords: vec!["launched", "killed", "started", "stopped", "failed", "error", "success"],
                head_lines: 5,
                tail_lines: 3,
            },
        );

        templates.insert(
            "process_manager",
            ExtractionTemplate {
                priority_keywords: vec!["pid", "name", "memory", "cpu", "killed"],
                head_lines: 15,
                tail_lines: 3,
            },
        );

        templates.insert(
            "system_map",
            ExtractionTemplate {
                priority_keywords: vec!["cpu", "gpu", "ram", "disk", "memory", "usage"],
                head_lines: 20,
                tail_lines: 3,
            },
        );

        Self { templates }
    }

    /// Distill an observation if it exceeds the threshold.
    pub fn distill(&self, tool_name: &str, observation: &str, query: &str) -> String {
        if observation.len() <= DISTILL_THRESHOLD {
            return observation.to_string();
        }

        let template = self.templates.get(tool_name);

        // Strategy 1: Tool-specific template extraction
        if let Some(tmpl) = template {
            let distilled = self.template_extract(observation, tmpl, query);
            if !distilled.is_empty() && self.confidence_score(&distilled, query) > 0.3 {
                return self.cap_length(&distilled);
            }
        }

        // Strategy 2: Keyword-prioritized line extraction
        let keyword_extract = self.keyword_extract(observation, query);
        if !keyword_extract.is_empty() {
            return self.cap_length(&keyword_extract);
        }

        // Strategy 3: Simple head/tail truncation
        self.simple_truncate(observation)
    }

    /// Extract using tool-specific template.
    fn template_extract(
        &self,
        observation: &str,
        template: &ExtractionTemplate,
        _query: &str,
    ) -> String {
        let lines: Vec<&str> = observation.lines().collect();
        let total = lines.len();

        if total == 0 {
            return String::new();
        }

        let mut selected = Vec::new();

        // Head lines
        let head_count = template.head_lines.min(total);
        for line in lines.iter().take(head_count) {
            selected.push(*line);
        }

        // Priority keyword lines (from the middle)
        if total > template.head_lines + template.tail_lines {
            let middle_start = template.head_lines;
            let middle_end = total.saturating_sub(template.tail_lines);

            for &line in lines[middle_start..middle_end].iter() {
                let lower = line.to_lowercase();
                if template
                    .priority_keywords
                    .iter()
                    .any(|kw| lower.contains(kw))
                {
                    selected.push(line);
                }
            }
        }

        // Tail lines
        if total > template.head_lines {
            let tail_start = total.saturating_sub(template.tail_lines);
            for &line in lines[tail_start..].iter() {
                if !selected.contains(&line) {
                    selected.push(line);
                }
            }
        }

        selected.join("\n")
    }

    /// Extract lines that contain words from the query.
    fn keyword_extract(&self, observation: &str, query: &str) -> String {
        let query_words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(String::from)
            .collect();

        if query_words.is_empty() {
            return String::new();
        }

        let lines: Vec<&str> = observation.lines().collect();
        let mut scored: Vec<(&str, usize)> = lines
            .iter()
            .map(|line| {
                let lower = line.to_lowercase();
                let score = query_words
                    .iter()
                    .filter(|w| lower.contains(w.as_str()))
                    .count();
                (*line, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));

        // Take top-scoring lines
        let result: Vec<&str> = scored
            .iter()
            .filter(|(_, score)| *score > 0)
            .take(20)
            .map(|(line, _)| *line)
            .collect();

        result.join("\n")
    }

    /// Confidence score: ratio of query words found in the distilled output.
    fn confidence_score(&self, distilled: &str, query: &str) -> f64 {
        let query_words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(String::from)
            .collect();

        if query_words.is_empty() {
            return 1.0;
        }

        let lower = distilled.to_lowercase();
        let found = query_words
            .iter()
            .filter(|w| lower.contains(w.as_str()))
            .count();

        found as f64 / query_words.len() as f64
    }

    /// Simple head/tail truncation with marker.
    fn simple_truncate(&self, observation: &str) -> String {
        let half = MAX_DISTILLED_LEN / 2;
        if observation.len() <= MAX_DISTILLED_LEN {
            return observation.to_string();
        }

        let head = &observation[..half];
        let tail = &observation[observation.len() - half..];
        let omitted = observation.len() - MAX_DISTILLED_LEN;
        format!("{head}\n[...truncated {omitted} chars...]\n{tail}")
    }

    /// Cap the output length.
    fn cap_length(&self, text: &str) -> String {
        if text.len() <= MAX_DISTILLED_LEN {
            text.to_string()
        } else {
            let truncated = &text[..MAX_DISTILLED_LEN];
            let remaining = text.len() - MAX_DISTILLED_LEN;
            format!("{truncated}\n[...truncated {remaining} chars...]")
        }
    }
}

impl Default for ObservationDistiller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distiller() -> ObservationDistiller {
        ObservationDistiller::new()
    }

    #[test]
    fn test_short_observation_unchanged() {
        let d = distiller();
        let obs = "Found 3 files matching 'readme'.";
        assert_eq!(d.distill("file_search", obs, "find readme"), obs);
    }

    #[test]
    fn test_long_observation_distilled() {
        let d = distiller();
        let obs = "result line\n".repeat(500);
        let distilled = d.distill("web_search", &obs, "search query");
        assert!(distilled.len() < obs.len());
    }

    #[test]
    fn test_long_shell_output_distilled() {
        let d = distiller();
        let mut obs = String::new();
        for i in 0..500 {
            obs.push_str(&format!("line {i}: some output data\n"));
        }
        obs.push_str("error: something failed\n");
        let distilled = d.distill("shell", &obs, "run tests");
        assert!(distilled.len() <= MAX_DISTILLED_LEN + 100); // some tolerance
    }

    #[test]
    fn test_confidence_score_full_match() {
        let d = distiller();
        let score = d.confidence_score("hello world programming", "hello world");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_confidence_score_partial_match() {
        let d = distiller();
        let score = d.confidence_score("hello everyone", "hello world");
        assert!(score > 0.0 && score < 1.0);
    }

    #[test]
    fn test_confidence_score_no_match() {
        let d = distiller();
        let score = d.confidence_score("completely unrelated text", "quantum physics theory");
        assert!(score < 0.5);
    }

    #[test]
    fn test_confidence_score_empty_query() {
        let d = distiller();
        assert!((d.confidence_score("anything", "") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_simple_truncate() {
        let d = distiller();
        let obs = "a".repeat(5000);
        let truncated = d.simple_truncate(&obs);
        assert!(truncated.len() < obs.len());
        assert!(truncated.contains("truncated"));
    }

    #[test]
    fn test_simple_truncate_short() {
        let d = distiller();
        let obs = "short";
        assert_eq!(d.simple_truncate(obs), obs);
    }

    #[test]
    fn test_keyword_extract_finds_relevant() {
        let d = distiller();
        let obs = "irrelevant line 1\nirrelevant line 2\nrust programming is great\nanother line\n";
        let result = d.keyword_extract(obs, "rust programming");
        assert!(result.contains("rust programming"));
    }

    #[test]
    fn test_keyword_extract_empty_query() {
        let d = distiller();
        let result = d.keyword_extract("some text", "");
        assert!(result.is_empty());
    }

    #[test]
    fn test_template_exists_for_common_tools() {
        let d = distiller();
        assert!(d.templates.contains_key("web_search"));
        assert!(d.templates.contains_key("shell"));
        assert!(d.templates.contains_key("file_search"));
        assert!(d.templates.contains_key("code_ops"));
        assert!(d.templates.contains_key("system_control"));
    }

    #[test]
    fn test_unknown_tool_still_distills() {
        let d = distiller();
        let obs = "x".repeat(5000);
        let result = d.distill("unknown_tool", &obs, "some query");
        assert!(result.len() < obs.len());
    }

    #[test]
    fn test_cap_length_short() {
        let d = distiller();
        assert_eq!(d.cap_length("short"), "short");
    }

    #[test]
    fn test_cap_length_long() {
        let d = distiller();
        let long = "a".repeat(3000);
        let capped = d.cap_length(&long);
        assert!(capped.len() < long.len());
        assert!(capped.contains("truncated"));
    }
}
