//! Metacognition — self-verification loop for hallucination detection.
//!
//! Ported from `sovereign_titan/cognitive/thompson_sampling.py` (verification aspect).
//! Before the PrimeActor sends its final response, this module evaluates the
//! output for hallucinations using a strict internal prompt. If it fails,
//! forces a retry (up to MAX_RETRIES times).

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use crate::nexus::{ModelNexus, ModelTarget};

/// Maximum number of verification retries before accepting the response.
const MAX_RETRIES: u32 = 3;

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

/// Run the metacognitive verification loop on a candidate response.
///
/// Generates the response, verifies it, and retries up to MAX_RETRIES times
/// if it fails verification. Returns the best response available.
pub async fn verified_generate(
    nexus: &Arc<ModelNexus>,
    prompt: &str,
    user_question: &str,
    max_tokens: u32,
    temperature: f32,
) -> Result<String> {
    let mut best_response = String::new();

    for attempt in 0..=MAX_RETRIES {
        // Generate (or re-generate) the response.
        let response = if attempt == 0 {
            nexus
                .generate(prompt, ModelTarget::Prime, max_tokens, temperature)
                .await?
        } else {
            // On retry, add a hint to avoid the previous failure.
            let retry_prompt = format!(
                "{prompt}\n\n[System: Your previous response may have contained inaccuracies. \
                 Please provide a careful, well-grounded answer. Only state facts you are confident about.]"
            );
            nexus
                .generate(&retry_prompt, ModelTarget::Prime, max_tokens, temperature * 0.8)
                .await?
        };

        best_response = response.clone();

        // Verify the response.
        match verify_response(nexus, user_question, &response).await {
            Ok(VerifyResult::Pass) => {
                if attempt > 0 {
                    info!("Metacognition: response passed on retry #{attempt}");
                }
                return Ok(response);
            }
            Ok(VerifyResult::Fail { reason }) => {
                warn!(
                    "Metacognition: FAIL on attempt {} — {reason}",
                    attempt + 1
                );
                if attempt == MAX_RETRIES {
                    warn!("Metacognition: max retries reached, returning best effort");
                }
            }
            Err(e) => {
                // Verification itself failed (e.g., model error) — skip verification.
                warn!("Metacognition: verification error ({e}), accepting response");
                return Ok(response);
            }
        }
    }

    // Return whatever we have after exhausting retries.
    Ok(best_response)
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
