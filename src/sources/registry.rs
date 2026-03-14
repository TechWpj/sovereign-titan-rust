//! Academic Source Registry — declarative catalog of curated academic sources.
//!
//! Ported from `sovereign_titan/sources/registry.py`. Provides a static catalog
//! of academic and news sources with domain-based routing for research queries.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// A curated academic or news source with API metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcademicSource {
    /// Internal identifier (e.g. `"semantic_scholar"`).
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Base website URL.
    pub base_url: String,
    /// API endpoint URL (empty if no public API).
    pub api_url: String,
    /// Relevant knowledge domains (e.g. `["ai", "cs"]`).
    pub domains: Vec<String>,
    /// Priority ranking (lower = higher priority).
    pub priority: u8,
    /// Whether the source exposes a public API.
    pub has_api: bool,
}

/// Static catalog of all known academic sources.
fn academic_sources_map() -> &'static HashMap<String, AcademicSource> {
    static SOURCES: OnceLock<HashMap<String, AcademicSource>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let entries: Vec<AcademicSource> = vec![
            AcademicSource {
                name: "semantic_scholar".into(),
                display_name: "Semantic Scholar".into(),
                base_url: "https://www.semanticscholar.org".into(),
                api_url: "https://api.semanticscholar.org/graph/v1".into(),
                domains: vec!["ai".into(), "cs".into(), "general".into()],
                priority: 1,
                has_api: true,
            },
            AcademicSource {
                name: "arxiv".into(),
                display_name: "arXiv".into(),
                base_url: "https://arxiv.org".into(),
                api_url: "http://export.arxiv.org/api/query".into(),
                domains: vec!["ai".into(), "cs".into(), "physics".into(), "math".into()],
                priority: 2,
                has_api: true,
            },
            AcademicSource {
                name: "pubmed".into(),
                display_name: "PubMed".into(),
                base_url: "https://pubmed.ncbi.nlm.nih.gov".into(),
                api_url: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils".into(),
                domains: vec!["medical".into(), "biology".into()],
                priority: 1,
                has_api: true,
            },
            AcademicSource {
                name: "doaj".into(),
                display_name: "DOAJ".into(),
                base_url: "https://doaj.org".into(),
                api_url: "https://doaj.org/api/search/articles".into(),
                domains: vec!["general".into()],
                priority: 5,
                has_api: true,
            },
            AcademicSource {
                name: "hackernews".into(),
                display_name: "Hacker News".into(),
                base_url: "https://news.ycombinator.com".into(),
                api_url: "https://hn.algolia.com/api/v1".into(),
                domains: vec!["tech".into(), "cs".into()],
                priority: 4,
                has_api: true,
            },
            AcademicSource {
                name: "plos".into(),
                display_name: "PLOS ONE".into(),
                base_url: "https://journals.plos.org/plosone".into(),
                api_url: "https://api.plos.org/search".into(),
                domains: vec!["biology".into(), "medical".into(), "general".into()],
                priority: 3,
                has_api: true,
            },
            AcademicSource {
                name: "ssrn".into(),
                display_name: "SSRN".into(),
                base_url: "https://www.ssrn.com".into(),
                api_url: String::new(),
                domains: vec!["economics".into(), "finance".into(), "law".into(), "social_science".into()],
                priority: 3,
                has_api: false,
            },
            AcademicSource {
                name: "biorxiv".into(),
                display_name: "bioRxiv".into(),
                base_url: "https://www.biorxiv.org".into(),
                api_url: "https://api.biorxiv.org".into(),
                domains: vec!["biology".into()],
                priority: 2,
                has_api: true,
            },
            AcademicSource {
                name: "medrxiv".into(),
                display_name: "medRxiv".into(),
                base_url: "https://www.medrxiv.org".into(),
                api_url: "https://api.medrxiv.org".into(),
                domains: vec!["medical".into()],
                priority: 2,
                has_api: true,
            },
            AcademicSource {
                name: "bmc".into(),
                display_name: "BioMed Central".into(),
                base_url: "https://www.biomedcentral.com".into(),
                api_url: String::new(),
                domains: vec!["medical".into(), "biology".into()],
                priority: 4,
                has_api: false,
            },
            AcademicSource {
                name: "core".into(),
                display_name: "CORE".into(),
                base_url: "https://core.ac.uk".into(),
                api_url: "https://api.core.ac.uk/v3".into(),
                domains: vec!["general".into()],
                priority: 5,
                has_api: true,
            },
            AcademicSource {
                name: "mit_ocw".into(),
                display_name: "MIT OpenCourseWare".into(),
                base_url: "https://ocw.mit.edu".into(),
                api_url: String::new(),
                domains: vec!["education".into(), "engineering".into(), "cs".into()],
                priority: 6,
                has_api: false,
            },
            AcademicSource {
                name: "propublica".into(),
                display_name: "ProPublica".into(),
                base_url: "https://www.propublica.org".into(),
                api_url: "https://api.propublica.org".into(),
                domains: vec!["journalism".into(), "law".into()],
                priority: 4,
                has_api: true,
            },
            AcademicSource {
                name: "ap_news".into(),
                display_name: "Associated Press".into(),
                base_url: "https://apnews.com".into(),
                api_url: String::new(),
                domains: vec!["journalism".into()],
                priority: 3,
                has_api: false,
            },
            AcademicSource {
                name: "reuters".into(),
                display_name: "Reuters".into(),
                base_url: "https://www.reuters.com".into(),
                api_url: String::new(),
                domains: vec!["journalism".into(), "finance".into()],
                priority: 3,
                has_api: false,
            },
            AcademicSource {
                name: "the_conversation".into(),
                display_name: "The Conversation".into(),
                base_url: "https://theconversation.com".into(),
                api_url: String::new(),
                domains: vec!["education".into(), "general".into()],
                priority: 5,
                has_api: false,
            },
            AcademicSource {
                name: "npr".into(),
                display_name: "NPR".into(),
                base_url: "https://www.npr.org".into(),
                api_url: String::new(),
                domains: vec!["journalism".into()],
                priority: 4,
                has_api: false,
            },
            AcademicSource {
                name: "ieee".into(),
                display_name: "IEEE Xplore".into(),
                base_url: "https://ieeexplore.ieee.org".into(),
                api_url: String::new(),
                domains: vec!["engineering".into(), "cs".into(), "physics".into()],
                priority: 2,
                has_api: false,
            },
        ];
        let mut map = HashMap::with_capacity(entries.len());
        for src in entries {
            map.insert(src.name.clone(), src);
        }
        map
    })
}

/// Static mapping of knowledge domains to ranked source names.
fn domain_sources_map() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static DOMAINS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    DOMAINS.get_or_init(|| {
        let mut m: HashMap<&str, Vec<&str>> = HashMap::new();
        m.insert("medical",     vec!["pubmed", "medrxiv", "bmc", "plos"]);
        m.insert("biology",     vec!["biorxiv", "pubmed", "plos", "bmc"]);
        m.insert("ai",          vec!["semantic_scholar", "arxiv", "hackernews"]);
        m.insert("cs",          vec!["semantic_scholar", "arxiv", "hackernews", "ieee", "mit_ocw"]);
        m.insert("tech",        vec!["hackernews", "semantic_scholar", "arxiv"]);
        m.insert("physics",     vec!["arxiv", "ieee", "semantic_scholar"]);
        m.insert("math",        vec!["arxiv", "semantic_scholar"]);
        m.insert("economics",   vec!["ssrn", "semantic_scholar", "reuters"]);
        m.insert("finance",     vec!["ssrn", "reuters", "semantic_scholar"]);
        m.insert("law",         vec!["ssrn", "propublica"]);
        m.insert("journalism",  vec!["ap_news", "reuters", "npr", "propublica"]);
        m.insert("education",   vec!["mit_ocw", "the_conversation", "doaj"]);
        m.insert("engineering", vec!["ieee", "mit_ocw", "semantic_scholar"]);
        m.insert("social_science", vec!["ssrn", "doaj", "the_conversation"]);
        m.insert("general",     vec!["semantic_scholar", "doaj", "core", "the_conversation"]);
        m
    })
}

/// Look up sources relevant to a set of keywords by mapping them to domains.
///
/// Keywords are matched against the domain source map. Results are deduplicated
/// and sorted by priority (lowest priority value first).
pub fn get_sources_for_topic(keywords: &[&str]) -> Vec<AcademicSource> {
    let sources = academic_sources_map();
    let domains = domain_sources_map();

    let mut seen = std::collections::HashSet::new();
    let mut results: Vec<AcademicSource> = Vec::new();

    for &keyword in keywords {
        let lower = keyword.to_lowercase();
        // Try the keyword as a domain directly.
        if let Some(source_names) = domains.get(lower.as_str()) {
            for &name in source_names {
                if seen.insert(name.to_string()) {
                    if let Some(src) = sources.get(name) {
                        results.push(src.clone());
                    }
                }
            }
        }
        // Also try matching keyword against source domains.
        for src in sources.values() {
            if src.domains.iter().any(|d| d == &lower) && seen.insert(src.name.clone()) {
                results.push(src.clone());
            }
        }
    }

    // If no matches, fall back to the general domain.
    if results.is_empty() {
        if let Some(source_names) = domains.get("general") {
            for &name in source_names {
                if let Some(src) = sources.get(name) {
                    results.push(src.clone());
                }
            }
        }
    }

    results.sort_by_key(|s| s.priority);
    results
}

/// Get a single source by its internal name.
pub fn get_source(name: &str) -> Option<AcademicSource> {
    academic_sources_map().get(name).cloned()
}

/// Return all registered academic sources.
pub fn all_sources() -> Vec<AcademicSource> {
    let mut sources: Vec<AcademicSource> = academic_sources_map().values().cloned().collect();
    sources.sort_by_key(|s| s.priority);
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_sources_returns_18() {
        let sources = all_sources();
        assert_eq!(sources.len(), 18);
    }

    #[test]
    fn test_get_source_by_name() {
        let src = get_source("semantic_scholar").expect("semantic_scholar should exist");
        assert_eq!(src.display_name, "Semantic Scholar");
        assert!(src.has_api);
    }

    #[test]
    fn test_get_source_not_found() {
        assert!(get_source("nonexistent_source").is_none());
    }

    #[test]
    fn test_sources_for_ai_topic() {
        let results = get_sources_for_topic(&["ai"]);
        assert!(!results.is_empty());
        let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"semantic_scholar"));
        assert!(names.contains(&"arxiv"));
    }

    #[test]
    fn test_sources_for_medical_topic() {
        let results = get_sources_for_topic(&["medical"]);
        let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"pubmed"));
        assert!(names.contains(&"medrxiv"));
    }

    #[test]
    fn test_sources_for_unknown_topic_falls_back_to_general() {
        let results = get_sources_for_topic(&["underwater_basket_weaving"]);
        assert!(!results.is_empty());
        // Should contain general sources
        let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"semantic_scholar") || names.contains(&"doaj"));
    }

    #[test]
    fn test_sources_sorted_by_priority() {
        let results = get_sources_for_topic(&["cs"]);
        for window in results.windows(2) {
            assert!(window[0].priority <= window[1].priority);
        }
    }

    #[test]
    fn test_sources_deduplication() {
        // Both "ai" and "cs" include semantic_scholar — it should appear only once.
        let results = get_sources_for_topic(&["ai", "cs"]);
        let ss_count = results.iter().filter(|s| s.name == "semantic_scholar").count();
        assert_eq!(ss_count, 1);
    }

    #[test]
    fn test_all_sources_have_valid_fields() {
        for src in all_sources() {
            assert!(!src.name.is_empty(), "Source name must not be empty");
            assert!(!src.display_name.is_empty(), "Display name must not be empty");
            assert!(!src.base_url.is_empty(), "Base URL must not be empty");
            assert!(!src.domains.is_empty(), "Domains must not be empty");
            if src.has_api {
                assert!(!src.api_url.is_empty(), "{} has_api=true but api_url is empty", src.name);
            }
        }
    }

    #[test]
    fn test_domain_sources_map_coverage() {
        let domains = domain_sources_map();
        assert!(domains.contains_key("medical"));
        assert!(domains.contains_key("ai"));
        assert!(domains.contains_key("general"));
        assert!(domains.contains_key("journalism"));
        assert!(domains.len() >= 14);
    }
}
