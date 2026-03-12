//! Truth Engine — LLM-as-judge draft selection.
//!
//! Ported from `sovereign_titan/cognitive/truth_engine.py`.
//! Takes multiple candidate drafts and uses the 14B model to evaluate
//! and select the most accurate, well-formatted response.

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use crate::nexus::{ModelNexus, ModelTarget};

use super::thompson::Draft;

/// Maximum tokens for the judge's verdict.
const JUDGE_MAX_TOKENS: u32 = 64;

/// Low temperature for deterministic judging.
const JUDGE_TEMPERATURE: f32 = 0.1;

/// The judge prompt template.
///
/// The model is asked to select the best draft by number. Using a strict
/// format ensures we can parse the response reliably.
const JUDGE_PROMPT: &str = "\
You are an expert quality judge. Below are candidate responses to a user question.
Evaluate each for: accuracy, completeness, clarity, and formatting.

User question: {question}

{drafts}

Select the BEST response. Reply with ONLY the number (1, 2, or 3) of the best response.
If all responses are poor, select the least flawed one.

Best response number:";

/// Truth Engine that selects the best draft from Thompson Sampling candidates.
pub struct TruthEngine;

impl TruthEngine {
    /// Evaluate the given drafts and return the best one.
    ///
    /// Sends all drafts to the 14B model in a judge prompt, parses the
    /// selection, and returns the winning draft. Falls back to the first
    /// draft (lowest temperature / most precise) if judging fails.
    pub async fn select_best(
        nexus: &Arc<ModelNexus>,
        drafts: &[Draft],
        user_question: &str,
    ) -> Result<Draft> {
        if drafts.is_empty() {
            anyhow::bail!("TruthEngine: no drafts to evaluate");
        }

        // Single draft — no need to judge.
        if drafts.len() == 1 {
            info!("TruthEngine: single draft, skipping judgment");
            return Ok(drafts[0].clone());
        }

        // Build the drafts section for the prompt.
        let drafts_text = drafts
            .iter()
            .enumerate()
            .map(|(i, d)| {
                // Truncate very long drafts to keep the judge prompt reasonable.
                let text = if d.text.len() > 2000 {
                    format!("{}...[truncated]", &d.text[..2000])
                } else {
                    d.text.clone()
                };
                format!("--- Response {} ---\n{}", i + 1, text)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = JUDGE_PROMPT
            .replace("{question}", user_question)
            .replace("{drafts}", &drafts_text);

        // Ask the model to judge.
        match nexus
            .generate(&prompt, ModelTarget::Prime, JUDGE_MAX_TOKENS, JUDGE_TEMPERATURE)
            .await
        {
            Ok(verdict) => {
                let selection = parse_selection(&verdict, drafts.len());
                info!(
                    "TruthEngine: judge selected draft {} (verdict: '{}')",
                    selection + 1,
                    verdict.trim()
                );
                Ok(drafts[selection].clone())
            }
            Err(e) => {
                warn!("TruthEngine: judge failed ({e}), falling back to draft 0");
                Ok(drafts[0].clone())
            }
        }
    }
}

/// Parse the judge's verdict to extract a draft index.
///
/// Looks for the first digit in the verdict that maps to a valid draft index.
/// Falls back to 0 (the most precise/low-temperature draft) on parse failure.
fn parse_selection(verdict: &str, num_drafts: usize) -> usize {
    // Look for the first digit 1..=num_drafts in the response.
    for ch in verdict.chars() {
        if let Some(digit) = ch.to_digit(10) {
            let idx = digit as usize;
            if idx >= 1 && idx <= num_drafts {
                return idx - 1; // Convert 1-based to 0-based.
            }
        }
    }

    // Fallback: return the first (most precise) draft.
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_selection_digit() {
        assert_eq!(parse_selection("2", 3), 1);
        assert_eq!(parse_selection("Response 3 is the best", 3), 2);
        assert_eq!(parse_selection("1", 3), 0);
    }

    #[test]
    fn test_parse_selection_fallback() {
        // No valid digit — should return 0.
        assert_eq!(parse_selection("none", 3), 0);
        assert_eq!(parse_selection("", 3), 0);
    }

    #[test]
    fn test_parse_selection_out_of_range() {
        // Digit 5 is out of range for 3 drafts — should skip it.
        assert_eq!(parse_selection("5", 3), 0);
        // But "5 is best, actually 2" should find the 2.
        assert_eq!(parse_selection("5 actually 2", 3), 1);
    }

    #[test]
    fn test_parse_selection_zero_ignored() {
        // 0 is not a valid 1-based selection.
        assert_eq!(parse_selection("0", 3), 0); // falls through to default
    }

    #[test]
    fn test_judge_prompt_template() {
        let prompt = JUDGE_PROMPT
            .replace("{question}", "What is Rust?")
            .replace("{drafts}", "--- Response 1 ---\nRust is a language.");
        assert!(prompt.contains("What is Rust?"));
        assert!(prompt.contains("Response 1"));
        assert!(prompt.contains("Best response number:"));
    }
}
