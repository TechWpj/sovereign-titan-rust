//! Metacognition — multi-draft generation with LLM-as-judge verification.
//!
//! Combines Thompson Sampling (diverse draft generation) with the Truth Engine
//! (LLM-based draft selection) to produce higher-quality responses than
//! single-pass greedy generation.
//!
//! Pipeline:
//! 1. Thompson Sampler generates 3 drafts at varied temperatures
//! 2. Truth Engine judges the drafts and selects the best one
//! 3. Hallucination verifier checks the winning draft
//! 4. If verification fails, retry with the next-best draft

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use crate::nexus::{ModelNexus, ModelTarget};

use super::thompson::ThompsonSampler;
use super::truth::TruthEngine;

/// Maximum number of verification retries before accepting the response.
const MAX_RETRIES: u32 = 2;

/// The internal verification prompt template.
const VERIFICATION_PROMPT: &str = "\
You are a strict fact-checker. Evaluate the following AI response for:
1. Hallucinated facts or made-up information
2. Logical contradictions
3. Claims that cannot be supported by the original question
4. Fabricated names, dates, numbers, or citations

Original question: {question}

AI response to verify:
{response}

Reply with EXACTLY one word:
- PASS if the response is factually grounded and logically consistent
- FAIL if the response contains hallucinations or unsupported claims";

/// Result of a metacognitive verification.
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyResult {
    /// Response passed verification.
    Pass,
    /// Response failed — contains potential hallucinations.
    Fail { reason: String },
}

/// Run the full metacognitive pipeline on a prompt.
///
/// 1. Thompson Sampling: generate 3 diverse drafts
/// 2. Truth Engine: select the best draft via LLM judgment
/// 3. Verification: check the winner for hallucinations
/// 4. Retry: if verification fails, fall back to next-best draft
pub async fn verified_generate(
    nexus: &Arc<ModelNexus>,
    prompt: &str,
    user_question: &str,
    max_tokens: u32,
    temperature: f32,
) -> Result<String> {
    // Step 1: Generate diverse drafts via Thompson Sampling.
    info!("Metacognition: generating {} diverse drafts", 3);
    let drafts = ThompsonSampler::sample_drafts(nexus, prompt, max_tokens, temperature).await;

    if drafts.is_empty() {
        warn!("Metacognition: all drafts failed, falling back to single generation");
        return nexus
            .generate(prompt, ModelTarget::Prime, max_tokens, temperature)
            .await;
    }

    // Step 2: Truth Engine selects the best draft.
    let winner = TruthEngine::select_best(nexus, &drafts, user_question).await?;
    info!(
        "Metacognition: Truth Engine selected draft {} (temp={:.2}, {} chars)",
        winner.index,
        winner.temperature,
        winner.text.len()
    );

    // Step 3: Verify the winning draft for hallucinations.
    match verify_response(nexus, user_question, &winner.text).await {
        Ok(VerifyResult::Pass) => {
            info!("Metacognition: winner passed verification");
            return Ok(winner.text);
        }
        Ok(VerifyResult::Fail { reason }) => {
            warn!("Metacognition: winner FAILED verification — {reason}");
        }
        Err(e) => {
            // Verification error — accept the winner anyway.
            warn!("Metacognition: verification error ({e}), accepting winner");
            return Ok(winner.text);
        }
    }

    // Step 4: Retry with remaining drafts, skipping the failed winner.
    let remaining: Vec<_> = drafts
        .iter()
        .filter(|d| d.index != winner.index)
        .collect();

    for (retry, draft) in remaining.iter().enumerate() {
        if retry as u32 >= MAX_RETRIES {
            break;
        }

        info!("Metacognition: trying fallback draft {} (retry {})", draft.index, retry + 1);

        match verify_response(nexus, user_question, &draft.text).await {
            Ok(VerifyResult::Pass) => {
                info!("Metacognition: fallback draft {} passed on retry {}", draft.index, retry + 1);
                return Ok(draft.text.clone());
            }
            Ok(VerifyResult::Fail { reason }) => {
                warn!("Metacognition: fallback draft {} also failed — {reason}", draft.index);
            }
            Err(e) => {
                warn!("Metacognition: verification error on fallback ({e}), accepting");
                return Ok(draft.text.clone());
            }
        }
    }

    // All drafts failed verification — return the Truth Engine's pick as best effort.
    warn!("Metacognition: all drafts failed verification, returning Truth Engine winner");
    Ok(winner.text)
}

/// Run the metacognitive pipeline with ChatML system prompt wrapping.
///
/// This variant is designed for the ReAct agent pipeline:
/// - Drafts are generated with `generate_with_system` (ChatML + stop sequences)
/// - Truth Engine judges and verification use raw `generate` (separate evaluation tasks)
///
/// This produces higher-quality ReAct-formatted responses by generating
/// multiple diverse drafts and selecting the best one.
pub async fn verified_generate_with_system(
    nexus: &Arc<ModelNexus>,
    system_prompt: &str,
    user_message: &str,
    user_question: &str,
    max_tokens: u32,
    temperature: f32,
) -> Result<String> {
    // Step 1: Generate diverse drafts via Thompson Sampling (with system prompt).
    info!("Metacognition: generating {} diverse drafts (system-prompt mode)", 3);
    let drafts = ThompsonSampler::sample_drafts_with_system(
        nexus,
        system_prompt,
        user_message,
        max_tokens,
        temperature,
    )
    .await;

    if drafts.is_empty() {
        warn!("Metacognition: all drafts failed, falling back to single generation");
        return nexus
            .generate_with_system(system_prompt, user_message, ModelTarget::Prime, max_tokens, temperature)
            .await;
    }

    // If only one draft succeeded, skip judging.
    if drafts.len() == 1 {
        info!("Metacognition: only 1 draft, skipping judgment");
        return Ok(drafts[0].text.clone());
    }

    // Step 2: Truth Engine selects the best draft.
    let winner = TruthEngine::select_best(nexus, &drafts, user_question).await?;
    info!(
        "Metacognition: Truth Engine selected draft {} (temp={:.2}, {} chars)",
        winner.index,
        winner.temperature,
        winner.text.len()
    );

    // Step 3: Verify the winning draft for hallucinations.
    match verify_response(nexus, user_question, &winner.text).await {
        Ok(VerifyResult::Pass) => {
            info!("Metacognition: winner passed verification");
            return Ok(winner.text);
        }
        Ok(VerifyResult::Fail { reason }) => {
            warn!("Metacognition: winner FAILED verification — {reason}");
        }
        Err(e) => {
            warn!("Metacognition: verification error ({e}), accepting winner");
            return Ok(winner.text);
        }
    }

    // Step 4: Retry with remaining drafts.
    let remaining: Vec<_> = drafts
        .iter()
        .filter(|d| d.index != winner.index)
        .collect();

    for (retry, draft) in remaining.iter().enumerate() {
        if retry as u32 >= MAX_RETRIES {
            break;
        }

        info!("Metacognition: trying fallback draft {} (retry {})", draft.index, retry + 1);

        match verify_response(nexus, user_question, &draft.text).await {
            Ok(VerifyResult::Pass) => {
                info!("Metacognition: fallback draft {} passed on retry {}", draft.index, retry + 1);
                return Ok(draft.text.clone());
            }
            Ok(VerifyResult::Fail { reason }) => {
                warn!("Metacognition: fallback draft {} also failed — {reason}", draft.index);
            }
            Err(e) => {
                warn!("Metacognition: verification error on fallback ({e}), accepting");
                return Ok(draft.text.clone());
            }
        }
    }

    warn!("Metacognition: all drafts failed verification, returning Truth Engine winner");
    Ok(winner.text)
}

/// Verify a single response using the internal fact-checking prompt.
async fn verify_response(
    nexus: &Arc<ModelNexus>,
    question: &str,
    response: &str,
) -> Result<VerifyResult> {
    let check_prompt = VERIFICATION_PROMPT
        .replace("{question}", question)
        .replace("{response}", response);

    let verdict = nexus
        .generate(&check_prompt, ModelTarget::Prime, 32, 0.1)
        .await?;

    let verdict_trimmed = verdict.trim().to_uppercase();

    if verdict_trimmed.contains("PASS") {
        Ok(VerifyResult::Pass)
    } else if verdict_trimmed.contains("FAIL") {
        Ok(VerifyResult::Fail {
            reason: verdict.trim().to_string(),
        })
    } else {
        // Ambiguous — treat as pass (don't block on unclear verdicts).
        info!("Metacognition: ambiguous verdict '{verdict_trimmed}', treating as PASS");
        Ok(VerifyResult::Pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verification_prompt_template() {
        let prompt = VERIFICATION_PROMPT
            .replace("{question}", "What is 2+2?")
            .replace("{response}", "2+2 is 4");
        assert!(prompt.contains("What is 2+2?"));
        assert!(prompt.contains("2+2 is 4"));
        assert!(prompt.contains("PASS"));
        assert!(prompt.contains("FAIL"));
    }
}
