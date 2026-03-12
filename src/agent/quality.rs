//! Answer Quality Gate — validates agent answers before returning to user.
//!
//! Ported from `sovereign_titan/agents/react.py` quality checks.
//! Rejects answers that are too short, echo the query, or are simple refusals.
//! Provides re-prompt text when an answer fails the quality check.

/// Minimum acceptable answer length (chars).
const DEFAULT_MIN_LENGTH: usize = 10;

/// Maximum number of re-prompts before accepting any answer.
const DEFAULT_MAX_REPROMPTS: usize = 1;

/// Refusal phrases that indicate the model is refusing to help.
const REFUSAL_PATTERNS: &[&str] = &[
    "i cannot",
    "i can't",
    "i'm not able",
    "i am not able",
    "as an ai",
    "i don't have the ability",
    "i'm unable to",
    "i am unable to",
];

/// Result of a quality check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityVerdict {
    /// Answer is acceptable.
    Accept,
    /// Answer is rejected — includes reason and re-prompt text.
    Reject {
        reason: &'static str,
        reprompt: &'static str,
    },
}

/// Validates agent answers before returning to the user.
pub struct AnswerQualityGate {
    /// Minimum acceptable answer length.
    min_length: usize,
    /// Maximum re-prompts before force-accepting.
    max_reprompts: usize,
    /// Current re-prompt count for the active query.
    reprompt_count: usize,
}

impl AnswerQualityGate {
    /// Create a new quality gate with default settings.
    pub fn new() -> Self {
        Self {
            min_length: DEFAULT_MIN_LENGTH,
            max_reprompts: DEFAULT_MAX_REPROMPTS,
            reprompt_count: 0,
        }
    }

    /// Create a quality gate with custom minimum length.
    pub fn with_min_length(min_length: usize) -> Self {
        Self {
            min_length,
            max_reprompts: DEFAULT_MAX_REPROMPTS,
            reprompt_count: 0,
        }
    }

    /// Reset the re-prompt counter (call at the start of each new query).
    pub fn reset(&mut self) {
        self.reprompt_count = 0;
    }

    /// Check if an answer passes the quality gate.
    ///
    /// Returns `QualityVerdict::Accept` if the answer is good enough,
    /// or `QualityVerdict::Reject` with a reason and re-prompt text.
    pub fn check(&mut self, answer: &str, query: &str) -> QualityVerdict {
        // If we've already re-prompted the max number of times, accept anything
        if self.reprompt_count >= self.max_reprompts {
            return QualityVerdict::Accept;
        }

        let a = answer.trim();

        // Too short
        if a.len() < self.min_length {
            self.reprompt_count += 1;
            return QualityVerdict::Reject {
                reason: "too_short",
                reprompt: "SYSTEM: Your answer was too short. Please provide a more detailed and useful response.",
            };
        }

        // Echo detection — answer is just the query repeated
        if a.to_lowercase() == query.trim().to_lowercase() {
            self.reprompt_count += 1;
            return QualityVerdict::Reject {
                reason: "echo",
                reprompt: "SYSTEM: You repeated the question back. Please provide an actual answer.",
            };
        }

        // Refusal patterns (only reject short refusals)
        if a.len() < 100 {
            let lower = a.to_lowercase();
            if REFUSAL_PATTERNS.iter().any(|p| lower.contains(p)) {
                self.reprompt_count += 1;
                return QualityVerdict::Reject {
                    reason: "refusal",
                    reprompt: "SYSTEM: Instead of refusing, try using available tools to accomplish the task.",
                };
            }
        }

        QualityVerdict::Accept
    }

    /// Number of times re-prompt has been triggered for the current query.
    pub fn reprompt_count(&self) -> usize {
        self.reprompt_count
    }

    /// Maximum re-prompts allowed.
    pub fn max_reprompts(&self) -> usize {
        self.max_reprompts
    }
}

impl Default for AnswerQualityGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accept_good_answer() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("The capital of France is Paris.", "what is the capital of France");
        assert_eq!(v, QualityVerdict::Accept);
    }

    #[test]
    fn test_reject_too_short() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("Yes", "what is quantum computing");
        assert!(matches!(v, QualityVerdict::Reject { reason: "too_short", .. }));
    }

    #[test]
    fn test_reject_echo() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("what time is it", "what time is it");
        assert!(matches!(v, QualityVerdict::Reject { reason: "echo", .. }));
    }

    #[test]
    fn test_reject_refusal() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("I cannot help with that.", "open notepad");
        assert!(matches!(v, QualityVerdict::Reject { reason: "refusal", .. }));
    }

    #[test]
    fn test_accept_long_refusal() {
        let mut gate = AnswerQualityGate::new();
        let long_answer = format!(
            "I cannot tell you the exact result because the process is still running, \
             but here's what I found so far: {}",
            "a".repeat(100)
        );
        let v = gate.check(&long_answer, "check status");
        assert_eq!(v, QualityVerdict::Accept);
    }

    #[test]
    fn test_force_accept_after_max_reprompts() {
        let mut gate = AnswerQualityGate::new();
        // First rejection
        let v = gate.check("Yes", "explain quantum computing");
        assert!(matches!(v, QualityVerdict::Reject { .. }));
        assert_eq!(gate.reprompt_count(), 1);

        // After max reprompts (1), should accept anything
        let v = gate.check("Yes", "explain quantum computing");
        assert_eq!(v, QualityVerdict::Accept);
    }

    #[test]
    fn test_reset_counter() {
        let mut gate = AnswerQualityGate::new();
        gate.check("Yes", "query");
        assert_eq!(gate.reprompt_count(), 1);

        gate.reset();
        assert_eq!(gate.reprompt_count(), 0);
    }

    #[test]
    fn test_custom_min_length() {
        let mut gate = AnswerQualityGate::with_min_length(50);
        let v = gate.check("This is a short answer.", "query");
        assert!(matches!(v, QualityVerdict::Reject { reason: "too_short", .. }));

        gate.reset();
        let v = gate.check(
            "This is a much longer answer that exceeds the fifty character minimum length requirement.",
            "query",
        );
        assert_eq!(v, QualityVerdict::Accept);
    }

    #[test]
    fn test_case_insensitive_echo() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("WHAT TIME IS IT", "what time is it");
        assert!(matches!(v, QualityVerdict::Reject { reason: "echo", .. }));
    }

    #[test]
    fn test_whitespace_handling() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("  Yes  ", "explain something");
        assert!(matches!(v, QualityVerdict::Reject { reason: "too_short", .. }));
    }

    #[test]
    fn test_various_refusal_patterns() {
        let patterns = [
            "I can't do that.",
            "I'm not able to help.",
            "As an AI, I cannot.",
            "I'm unable to assist.",
        ];
        for pattern in patterns {
            let mut gate = AnswerQualityGate::new();
            let v = gate.check(pattern, "do something");
            assert!(
                matches!(v, QualityVerdict::Reject { reason: "refusal", .. }),
                "Expected refusal rejection for: {pattern}"
            );
        }
    }

    #[test]
    fn test_empty_answer() {
        let mut gate = AnswerQualityGate::new();
        let v = gate.check("", "query");
        assert!(matches!(v, QualityVerdict::Reject { reason: "too_short", .. }));
    }
}
