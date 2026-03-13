//! Thompson Sampling — multi-draft generation with temperature diversity.
//!
//! Ported from `sovereign_titan/cognitive/thompson_sampling.py`.
//! Instead of single-pass greedy generation, spawns N parallel generation
//! tasks with varied temperatures to produce diverse candidate drafts.
//! The best draft is then selected by the Truth Engine.

use std::sync::Arc;

use tracing::{info, warn};

use crate::nexus::{ModelNexus, ModelTarget};

/// Number of candidate drafts to generate.
const NUM_DRAFTS: usize = 3;

/// Temperature offsets for diversity: [precise, balanced, creative].
const TEMPERATURE_SPREAD: [f32; NUM_DRAFTS] = [0.3, 0.6, 0.9];

/// Result of multi-draft generation including the system prompt used.
#[derive(Debug, Clone)]
pub struct DraftBatch {
    pub drafts: Vec<Draft>,
    pub system_prompt: String,
    pub user_message: String,
}

/// A single candidate draft with its generation metadata.
#[derive(Debug, Clone)]
pub struct Draft {
    /// The generated text.
    pub text: String,
    /// The temperature used to generate this draft.
    pub temperature: f32,
    /// Index (0-based) of this draft.
    pub index: usize,
}

/// Thompson Sampler that generates diverse draft responses.
///
/// Uses temperature spreading to explore the response space:
/// - Draft 0: low temperature (precise, factual)
/// - Draft 1: medium temperature (balanced)
/// - Draft 2: high temperature (creative, exploratory)
pub struct ThompsonSampler;

impl ThompsonSampler {
    /// Generate [`NUM_DRAFTS`] candidate responses with varied temperatures.
    ///
    /// All drafts are spawned as concurrent tasks, though the model's internal
    /// write lock will serialize actual GPU inference.
    pub async fn sample_drafts(
        nexus: &Arc<ModelNexus>,
        prompt: &str,
        max_tokens: u32,
        base_temperature: f32,
    ) -> Vec<Draft> {
        let mut handles = Vec::with_capacity(NUM_DRAFTS);

        for (i, &offset) in TEMPERATURE_SPREAD.iter().enumerate() {
            let nexus = Arc::clone(nexus);
            let prompt = prompt.to_string();

            // Scale temperature: blend the base with the offset.
            // Clamp to [0.1, 1.5] to stay in a reasonable range.
            let temp = (base_temperature * 0.5 + offset * 0.5).clamp(0.1, 1.5);

            let handle = tokio::spawn(async move {
                let result = nexus
                    .generate(&prompt, ModelTarget::Prime, max_tokens, temp)
                    .await;

                match result {
                    Ok(text) => Some(Draft {
                        text,
                        temperature: temp,
                        index: i,
                    }),
                    Err(e) => {
                        warn!("Thompson: draft {i} (temp={temp:.2}) failed: {e}");
                        None
                    }
                }
            });

            handles.push(handle);
        }

        // Collect results, filtering out failures.
        let mut drafts = Vec::with_capacity(NUM_DRAFTS);
        for handle in handles {
            match handle.await {
                Ok(Some(draft)) => {
                    info!(
                        "Thompson: draft {} generated ({} chars, temp={:.2})",
                        draft.index,
                        draft.text.len(),
                        draft.temperature
                    );
                    drafts.push(draft);
                }
                Ok(None) => {} // Already logged in the spawn
                Err(e) => warn!("Thompson: join error: {e}"),
            }
        }

        drafts
    }

    /// Generate [`NUM_DRAFTS`] candidate responses using ChatML system prompt wrapping.
    ///
    /// Unlike [`sample_drafts`], this uses `generate_with_system` which applies
    /// ChatML formatting and stop sequences (`\nOBSERVATION:`), making it
    /// compatible with the ReAct agent pipeline.
    pub async fn sample_drafts_with_system(
        nexus: &Arc<ModelNexus>,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
        base_temperature: f32,
    ) -> Vec<Draft> {
        let mut handles = Vec::with_capacity(NUM_DRAFTS);

        for (i, &offset) in TEMPERATURE_SPREAD.iter().enumerate() {
            let nexus = Arc::clone(nexus);
            let sys = system_prompt.to_string();
            let usr = user_message.to_string();

            // Scale temperature: blend the base with the offset.
            let temp = (base_temperature * 0.5 + offset * 0.5).clamp(0.1, 1.5);

            let handle = tokio::spawn(async move {
                let result = nexus
                    .generate_with_system(&sys, &usr, ModelTarget::Prime, max_tokens, temp)
                    .await;

                match result {
                    Ok(text) => Some(Draft {
                        text,
                        temperature: temp,
                        index: i,
                    }),
                    Err(e) => {
                        warn!("Thompson: draft {i} (temp={temp:.2}) failed: {e}");
                        None
                    }
                }
            });

            handles.push(handle);
        }

        let mut drafts = Vec::with_capacity(NUM_DRAFTS);
        for handle in handles {
            match handle.await {
                Ok(Some(draft)) => {
                    info!(
                        "Thompson: draft {} generated ({} chars, temp={:.2})",
                        draft.index,
                        draft.text.len(),
                        draft.temperature
                    );
                    drafts.push(draft);
                }
                Ok(None) => {}
                Err(e) => warn!("Thompson: join error: {e}"),
            }
        }

        drafts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temperature_spread_valid() {
        // All temperatures should be in a usable range.
        for &t in &TEMPERATURE_SPREAD {
            assert!(t > 0.0 && t <= 1.5, "temperature {t} out of range");
        }
    }

    #[test]
    fn test_temperature_clamping() {
        // Very high base temperature should still clamp.
        let base = 3.0_f32;
        for &offset in &TEMPERATURE_SPREAD {
            let temp = (base * 0.5 + offset * 0.5).clamp(0.1, 1.5);
            assert!(temp <= 1.5);
            assert!(temp >= 0.1);
        }

        // Very low base temperature should still clamp.
        let base = 0.0_f32;
        for &offset in &TEMPERATURE_SPREAD {
            let temp = (base * 0.5 + offset * 0.5).clamp(0.1, 1.5);
            assert!(temp >= 0.1);
        }
    }

    #[test]
    fn test_draft_struct() {
        let draft = Draft {
            text: "Hello world".to_string(),
            temperature: 0.5,
            index: 0,
        };
        assert_eq!(draft.index, 0);
        assert_eq!(draft.temperature, 0.5);
        assert!(!draft.text.is_empty());
    }
}
