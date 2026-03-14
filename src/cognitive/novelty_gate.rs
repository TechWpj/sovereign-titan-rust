//! Novelty Gate — multi-tier novelty + hallucination + epistemic triage filter.
//!
//! Ported from `sovereign_titan/cognitive/novelty_gate.py`.
//! Prevents the knowledge graph and consciousness loops from storing
//! redundant textbook trivia, near-duplicate facts, or hallucinated claims.
//!
//! Three-tier novelty pipeline:
//!   Tier 0 — Exact/near-duplicate Jaccard > 0.95 → DUPLICATE
//!   Tier 1 — Cosine vector filter (0.70–0.95 danger zone)
//!   Tier 2 — LLM for ambiguous cases in the danger zone

use std::collections::HashSet;

use regex::Regex;

/// Novelty classification result.
#[derive(Debug, Clone, PartialEq)]
pub enum NoveltyVerdict {
    Novel,
    Known,
    Duplicate,
}

impl std::fmt::Display for NoveltyVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Novel => write!(f, "NOVEL"),
            Self::Known => write!(f, "KNOWN"),
            Self::Duplicate => write!(f, "DUPLICATE"),
        }
    }
}

/// Hallucination classification result.
#[derive(Debug, Clone, PartialEq)]
pub enum HallucinationVerdict {
    Grounded,
    Uncertain,
    Hallucinated,
}

/// Epistemic triage result.
#[derive(Debug, Clone, PartialEq)]
pub enum EpistemicVerdict {
    Retain,
    Reject,
}

/// Token-level Jaccard similarity between two strings.
fn jaccard_tokens(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let sa: HashSet<&str> = a_lower.split_whitespace().collect();
    let sb: HashSet<&str> = b_lower.split_whitespace().collect();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let intersection = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    intersection as f64 / union as f64
}

/// Cosine similarity between two embedding vectors.
fn cosine_sim(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Pattern for detecting specific references (CVEs, malware names).
fn specific_pattern() -> Regex {
    Regex::new(
        r"\b(?:CVE-\d{4}-\d+|[A-Z][a-z]+(?:Bot|Locker|Crypt|Worm|Spy|Ransom|Trojan|RAT))\b"
    ).unwrap()
}

/// Check if claim references specific names/CVEs absent from evidence.
fn has_ungrounded_specifics(claim: &str, evidence: &str) -> bool {
    let pat = specific_pattern();
    let claim_specifics: HashSet<String> = pat
        .find_iter(claim)
        .map(|m| m.as_str().to_uppercase())
        .collect();

    if claim_specifics.is_empty() {
        return false;
    }

    let evidence_upper = evidence.to_uppercase();
    claim_specifics.iter().any(|spec| !evidence_upper.contains(spec.as_str()))
}

/// Lightweight gate that classifies candidate data before storage.
pub struct NoveltyGate {
    /// Whether the gate is enabled.
    enabled: bool,
}

impl NoveltyGate {
    /// Create a new novelty gate.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Check novelty of a candidate against recent items (heuristic only).
    ///
    /// Three-tier pipeline:
    ///   Tier 0 — Jaccard > 0.95 → DUPLICATE
    ///   Tier 1 — Cosine vector filter (if embeddings available)
    ///   Tier 2 — Would use LLM (returns NOVEL as fallback)
    pub fn check_novelty_heuristic(
        &self,
        candidate: &str,
        recent_items: &[String],
    ) -> NoveltyVerdict {
        if !self.enabled {
            return NoveltyVerdict::Novel;
        }

        // Tier 0: Exact/near-duplicate (Jaccard > 0.95)
        for item in recent_items {
            if jaccard_tokens(candidate, item) > 0.95 {
                return NoveltyVerdict::Duplicate;
            }
        }

        // Tier 1: Would use vector embeddings if available
        // For now, use a looser Jaccard check for the danger zone
        let mut max_similarity = 0.0;
        for item in recent_items {
            let sim = jaccard_tokens(candidate, item);
            if sim > max_similarity {
                max_similarity = sim;
            }
        }

        if max_similarity > 0.80 {
            // Danger zone — conservative: treat as KNOWN
            return NoveltyVerdict::Known;
        }

        NoveltyVerdict::Novel
    }

    /// Check novelty with embedding vectors (Tier 1).
    pub fn check_novelty_with_embeddings(
        &self,
        candidate: &str,
        recent_items: &[String],
        candidate_embedding: &[f64],
        item_embeddings: &[Vec<f64>],
    ) -> NoveltyVerdict {
        if !self.enabled {
            return NoveltyVerdict::Novel;
        }

        // Tier 0: Jaccard check
        for item in recent_items {
            if jaccard_tokens(candidate, item) > 0.95 {
                return NoveltyVerdict::Duplicate;
            }
        }

        // Tier 1: Cosine similarity
        let mut max_cosine = 0.0;
        for emb in item_embeddings {
            let sim = cosine_sim(candidate_embedding, emb);
            if sim > max_cosine {
                max_cosine = sim;
            }
        }

        if max_cosine > 0.95 {
            return NoveltyVerdict::Duplicate;
        }
        if max_cosine < 0.70 {
            return NoveltyVerdict::Novel;
        }

        // Danger zone 0.70–0.95: would use LLM, default to KNOWN
        NoveltyVerdict::Known
    }

    /// Check if a claim is grounded in evidence (heuristic).
    pub fn check_hallucination_heuristic(
        &self,
        claim: &str,
        evidence: &str,
    ) -> HallucinationVerdict {
        if !self.enabled {
            return HallucinationVerdict::Grounded;
        }

        if has_ungrounded_specifics(claim, evidence) {
            return HallucinationVerdict::Hallucinated;
        }

        HallucinationVerdict::Grounded
    }

    /// Epistemic triage — heuristic check for parametric redundancy.
    pub fn check_epistemic_heuristic(&self, candidate: &str) -> EpistemicVerdict {
        if !self.enabled {
            return EpistemicVerdict::Retain;
        }

        // Heuristic: very short or very generic statements are likely parametric
        let word_count = candidate.split_whitespace().count();
        if word_count < 5 {
            return EpistemicVerdict::Reject;
        }

        // Check for common textbook patterns
        let textbook_patterns = [
            "is a programming language",
            "is an operating system",
            "was invented by",
            "is defined as",
            "refers to",
            "is commonly used for",
        ];

        for pattern in &textbook_patterns {
            if candidate.to_lowercase().contains(pattern) {
                return EpistemicVerdict::Reject;
            }
        }

        EpistemicVerdict::Retain
    }

    /// Whether the gate is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Default for NoveltyGate {
    fn default() -> Self {
        Self::new(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaccard_identical() {
        assert!((jaccard_tokens("hello world", "hello world") - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_disjoint() {
        assert!((jaccard_tokens("hello world", "foo bar")).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_partial() {
        let sim = jaccard_tokens("hello world foo", "hello world bar");
        assert!(sim > 0.3 && sim < 0.8);
    }

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_sim(&v, &v) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_sim(&a, &b).abs() < 0.001);
    }

    #[test]
    fn test_duplicate_detection() {
        let gate = NoveltyGate::default();
        let recent = vec!["the cpu usage is at 45 percent".to_string()];
        let verdict = gate.check_novelty_heuristic(
            "the cpu usage is at 45 percent",
            &recent,
        );
        assert_eq!(verdict, NoveltyVerdict::Duplicate);
    }

    #[test]
    fn test_novel_detection() {
        let gate = NoveltyGate::default();
        let recent = vec!["the cpu usage is at 45 percent".to_string()];
        let verdict = gate.check_novelty_heuristic(
            "a new process called malware.exe was detected on port 4444",
            &recent,
        );
        assert_eq!(verdict, NoveltyVerdict::Novel);
    }

    #[test]
    fn test_disabled_gate() {
        let gate = NoveltyGate::new(false);
        let verdict = gate.check_novelty_heuristic("anything", &["anything".to_string()]);
        assert_eq!(verdict, NoveltyVerdict::Novel);
    }

    #[test]
    fn test_hallucination_ungrounded_cve() {
        let gate = NoveltyGate::default();
        let verdict = gate.check_hallucination_heuristic(
            "This system is vulnerable to CVE-2024-1234",
            "System scan complete. No vulnerabilities found.",
        );
        assert_eq!(verdict, HallucinationVerdict::Hallucinated);
    }

    #[test]
    fn test_hallucination_grounded() {
        let gate = NoveltyGate::default();
        let verdict = gate.check_hallucination_heuristic(
            "CPU usage is high",
            "CPU: 95% utilization detected",
        );
        assert_eq!(verdict, HallucinationVerdict::Grounded);
    }

    #[test]
    fn test_epistemic_reject_textbook() {
        let gate = NoveltyGate::default();
        let verdict = gate.check_epistemic_heuristic("Python is a programming language used for web development");
        assert_eq!(verdict, EpistemicVerdict::Reject);
    }

    #[test]
    fn test_epistemic_retain_specific() {
        let gate = NoveltyGate::default();
        let verdict = gate.check_epistemic_heuristic(
            "The user's AMD RX 7900 XT has 20GB VRAM and runs at gfx1100 architecture with ROCm 7.1"
        );
        assert_eq!(verdict, EpistemicVerdict::Retain);
    }

    #[test]
    fn test_epistemic_reject_short() {
        let gate = NoveltyGate::default();
        assert_eq!(gate.check_epistemic_heuristic("hi"), EpistemicVerdict::Reject);
    }

    #[test]
    fn test_with_embeddings_novel() {
        let gate = NoveltyGate::default();
        let cand_emb = vec![1.0, 0.0, 0.0];
        let item_embs = vec![vec![0.0, 1.0, 0.0]];
        let verdict = gate.check_novelty_with_embeddings(
            "completely different",
            &["something else".to_string()],
            &cand_emb,
            &item_embs,
        );
        assert_eq!(verdict, NoveltyVerdict::Novel);
    }
}
