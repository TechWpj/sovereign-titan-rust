//! Topic Detector — keyword-based domain detection for routing queries to
//! the appropriate academic sources.
//!
//! Ported from `sovereign_titan/sources/topic_detector.py`. Matches queries
//! against domain-specific regex patterns and returns a list of domain tags.

use std::sync::OnceLock;

use regex::Regex;

/// A compiled domain detection pattern.
struct DomainPattern {
    domain: &'static str,
    regex: Regex,
}

/// Build and cache the domain pattern table.
fn domain_patterns() -> &'static Vec<DomainPattern> {
    static PATTERNS: OnceLock<Vec<DomainPattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            DomainPattern {
                domain: "medical",
                regex: Regex::new(
                    r"(?i)\b(medic(?:al|ine)|clinic(?:al)?|disease|symptom|treatment|therapy|diagnosis|patient|doctor|hospital|pharma(?:ceutical|cology)?|drug|vaccine|pathology|oncology|cardiology|neurology|surgery|epidemiology|health\s*care)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "biology",
                regex: Regex::new(
                    r"(?i)\b(biolog(?:y|ical)|gene(?:tic)?|genom(?:e|ics)|protein|cell(?:ular)?|organism|evolution|ecology|microbiology|biochem(?:istry)?|molecular|dna|rna|species|biodiversity|neuroscience)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "ai",
                regex: Regex::new(
                    r"(?i)\b(artificial\s*intelligence|machine\s*learning|deep\s*learning|neural\s*net(?:work)?|transformer|llm|large\s*language\s*model|reinforcement\s*learning|generative\s*ai|diffusion\s*model|gpt|bert|attention\s*mechanism|nlp|natural\s*language\s*processing|computer\s*vision)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "cs",
                regex: Regex::new(
                    r"(?i)\b(computer\s*science|algorithm|data\s*structure|compiler|operating\s*system|distributed\s*system|database|software\s*engineering|programming\s*language|cryptography|cyber\s*security|network(?:ing)?|parallel\s*computing|computation)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "tech",
                regex: Regex::new(
                    r"(?i)\b(technolog(?:y|ies)|startup|silicon\s*valley|software|hardware|cloud\s*computing|devops|kubernetes|docker|microservices|api|saas|open\s*source|github|linux)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "physics",
                regex: Regex::new(
                    r"(?i)\b(physics|quantum\s*(?:mechanics|computing|field)|relativity|thermodynamics|electromagnetism|particle\s*physics|cosmology|astrophysics|condensed\s*matter|optics|higgs|gravitational)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "math",
                regex: Regex::new(
                    r"(?i)\b(mathemat(?:ics|ical)|theorem|proof|algebra|topology|calculus|differential\s*equation|statistics|probability|number\s*theory|combinatorics|geometry|linear\s*algebra|stochastic)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "economics",
                regex: Regex::new(
                    r"(?i)\b(econom(?:ics|y|ic)|macroeconom|microeconom|fiscal|monetary\s*policy|gdp|inflation|unemployment|trade|market\s*(?:failure|structure)|supply\s*(?:and\s*)?demand|keynesian|recession)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "finance",
                regex: Regex::new(
                    r"(?i)\b(financ(?:e|ial)|stock\s*market|invest(?:ment|ing)|portfolio|bond|equity|derivative|hedge\s*fund|banking|cryptocurrency|bitcoin|blockchain|forex|trading|valuation)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "law",
                regex: Regex::new(
                    r"(?i)\b(law|legal|legislation|regulat(?:ion|ory)|constitution|court|judicial|statute|attorney|litigation|patent|copyright|compliance|jurisdiction|precedent)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "journalism",
                regex: Regex::new(
                    r"(?i)\b(journalism|news|reporter|investigat(?:ive|ion)\s*(?:journalism|report)|media|press|editorial|headline|breaking\s*news|current\s*events|fact[\s-]*check)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "education",
                regex: Regex::new(
                    r"(?i)\b(education|pedagog(?:y|ical)|curriculum|teaching|learning\s*(?:theory|outcome)|student|university|academic|lecture|course(?:ware)?|mooc|textbook)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "engineering",
                regex: Regex::new(
                    r"(?i)\b(engineer(?:ing)?|mechanical|electrical|civil\s*engineering|aerospace|robotics|embedded\s*system|signal\s*processing|control\s*system|structural|materials\s*science|cad|manufacturing)\b"
                ).unwrap(),
            },
            DomainPattern {
                domain: "social_science",
                regex: Regex::new(
                    r"(?i)\b(social\s*science|sociology|psychology|anthropology|political\s*science|demograph(?:y|ics)|behavioral|cognitive\s*science|linguistics|cultural\s*studies|public\s*policy|survey\s*research)\b"
                ).unwrap(),
            },
        ]
    })
}

/// Detect which knowledge domains are relevant to the given query.
///
/// Returns a list of domain tag strings (e.g. `["ai", "cs"]`). If no
/// domain-specific patterns match, returns `["general"]`.
pub fn detect_domains(query: &str) -> Vec<String> {
    let patterns = domain_patterns();
    let mut matched: Vec<String> = Vec::new();

    for dp in patterns.iter() {
        if dp.regex.is_match(query) {
            matched.push(dp.domain.to_string());
        }
    }

    if matched.is_empty() {
        matched.push("general".to_string());
    }

    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_ai_domain() {
        let domains = detect_domains("transformer architecture for large language models");
        assert!(domains.contains(&"ai".to_string()));
    }

    #[test]
    fn test_detect_medical_domain() {
        let domains = detect_domains("clinical treatment for cardiovascular disease");
        assert!(domains.contains(&"medical".to_string()));
    }

    #[test]
    fn test_detect_biology_domain() {
        let domains = detect_domains("CRISPR gene editing in cellular organisms");
        assert!(domains.contains(&"biology".to_string()));
    }

    #[test]
    fn test_detect_physics_domain() {
        let domains = detect_domains("quantum mechanics and particle physics");
        assert!(domains.contains(&"physics".to_string()));
    }

    #[test]
    fn test_detect_finance_domain() {
        let domains = detect_domains("stock market investment portfolio management");
        assert!(domains.contains(&"finance".to_string()));
    }

    #[test]
    fn test_detect_multiple_domains() {
        let domains = detect_domains("machine learning algorithms for computer science");
        assert!(domains.contains(&"ai".to_string()));
        assert!(domains.contains(&"cs".to_string()));
    }

    #[test]
    fn test_fallback_to_general() {
        let domains = detect_domains("what is the meaning of life");
        assert_eq!(domains, vec!["general".to_string()]);
    }

    #[test]
    fn test_detect_law_domain() {
        let domains = detect_domains("constitutional law and judicial precedent");
        assert!(domains.contains(&"law".to_string()));
    }

    #[test]
    fn test_detect_engineering_domain() {
        let domains = detect_domains("embedded system design in robotics");
        assert!(domains.contains(&"engineering".to_string()));
    }

    #[test]
    fn test_case_insensitivity() {
        let domains = detect_domains("DEEP LEARNING NEURAL NETWORK");
        assert!(domains.contains(&"ai".to_string()));
    }
}
