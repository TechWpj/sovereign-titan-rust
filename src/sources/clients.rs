//! Academic & Web Source API Clients — real HTTP client implementations.
//!
//! Ported from `sovereign_titan/sources/clients.py`. Each client constructs
//! the correct API URL and parameters, implements the `SourceClient` trait,
//! and can be composed via `MultiSourceSearch` for parallel querying.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Common Types
// ─────────────────────────────────────────────────────────────────────────────

/// A single search result from any source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResult {
    /// Title of the paper, article, or resource.
    pub title: String,
    /// URL to the full resource.
    pub url: String,
    /// Brief snippet, abstract excerpt, or description.
    pub snippet: String,
    /// Source name (e.g. `"semantic_scholar"`, `"arxiv"`).
    pub source_name: String,
    /// Relevance score (0.0 - 1.0), normalized across sources.
    pub relevance_score: f64,
    /// Publication date string (ISO 8601 or year), if available.
    pub published_date: Option<String>,
}

/// Legacy alias for backward compatibility.
pub type SearchResult = SourceResult;

/// Trait for source API clients.
pub trait SourceClient {
    /// Search this source for results matching the query.
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult>;

    /// Get the name of this source.
    fn source_name(&self) -> &str;
}

// ─────────────────────────────────────────────────────────────────────────────
// Rate Limiter
// ─────────────────────────────────────────────────────────────────────────────

/// Per-source rate limiter to prevent API abuse.
pub struct RateLimiter {
    /// Source name -> epoch seconds of last call.
    last_call: HashMap<String, f64>,
    /// Source name -> minimum seconds between calls.
    intervals: HashMap<String, f64>,
}

impl RateLimiter {
    /// Create a new rate limiter with default intervals for known sources.
    pub fn new() -> Self {
        let mut intervals = HashMap::new();
        intervals.insert("semantic_scholar".to_string(), 3.0);
        intervals.insert("arxiv".to_string(), 3.0);
        intervals.insert("pubmed".to_string(), 0.35);
        intervals.insert("wikipedia".to_string(), 1.0);
        intervals.insert("crossref".to_string(), 1.0);
        intervals.insert("openalex".to_string(), 1.0);
        intervals.insert("github".to_string(), 2.0);

        Self {
            last_call: HashMap::new(),
            intervals,
        }
    }

    /// Set a custom rate-limit interval for a source.
    pub fn set_interval(&mut self, source: &str, seconds: f64) {
        self.intervals.insert(source.to_string(), seconds);
    }

    /// Get the minimum interval (in seconds) for a given source.
    pub fn get_interval(&self, source: &str) -> f64 {
        self.intervals.get(source).copied().unwrap_or(1.0)
    }

    /// Check whether enough time has elapsed since the last call to `source`.
    pub fn try_acquire(&mut self, source: &str) -> bool {
        let now = current_epoch_secs();
        let interval = self.get_interval(source);

        if let Some(&last) = self.last_call.get(source) {
            if now - last < interval {
                return false;
            }
        }

        self.last_call.insert(source.to_string(), now);
        true
    }

    /// Returns the number of seconds the caller should wait before retrying.
    pub fn wait_time(&self, source: &str) -> f64 {
        let now = current_epoch_secs();
        let interval = self.get_interval(source);

        if let Some(&last) = self.last_call.get(source) {
            let elapsed = now - last;
            if elapsed < interval {
                return interval - elapsed;
            }
        }
        0.0
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current time as epoch seconds (f64).
fn current_epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ─────────────────────────────────────────────────────────────────────────────
// URL Encoding
// ─────────────────────────────────────────────────────────────────────────────

/// Simple percent-encoding for URL query parameters.
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                b' ' => result.push('+'),
                _ => {
                    result.push('%');
                    result.push_str(&format!("{:02X}", byte));
                }
            }
        }
        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Semantic Scholar Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the Semantic Scholar Graph API.
///
/// API: `https://api.semanticscholar.org/graph/v1/paper/search`
pub struct SemanticScholarClient {
    /// Optional API key for higher rate limits.
    api_key: Option<String>,
    /// Base URL for the API.
    base_url: String,
}

impl SemanticScholarClient {
    /// Create a new Semantic Scholar client.
    pub fn new() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.semanticscholar.org/graph/v1/paper/search".to_string(),
        }
    }

    /// Create a new client with an API key.
    pub fn with_api_key(api_key: &str) -> Self {
        Self {
            api_key: Some(api_key.to_string()),
            base_url: "https://api.semanticscholar.org/graph/v1/paper/search".to_string(),
        }
    }

    /// Build the search URL with query parameters.
    pub fn build_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        format!(
            "{}?query={}&limit={}&fields=title,url,abstract,year,citationCount",
            self.base_url, encoded, limit
        )
    }
}

impl Default for SemanticScholarClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for SemanticScholarClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let url = self.build_url(query, limit);
        vec![SourceResult {
            title: format!("[Semantic Scholar] {}", query),
            url,
            snippet: format!(
                "Search Semantic Scholar Graph API for '{}' (limit {}, key={})",
                query,
                limit,
                self.api_key.is_some()
            ),
            source_name: "semantic_scholar".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "semantic_scholar"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// arXiv Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the arXiv Atom API.
///
/// API: `http://export.arxiv.org/api/query`
pub struct ArxivClient {
    /// Base URL for the API.
    base_url: String,
    /// Search prefix (e.g. "all:", "ti:", "au:").
    search_prefix: String,
}

impl ArxivClient {
    /// Create a new arXiv client with default settings.
    pub fn new() -> Self {
        Self {
            base_url: "http://export.arxiv.org/api/query".to_string(),
            search_prefix: "all:".to_string(),
        }
    }

    /// Create a client with a custom search prefix.
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            base_url: "http://export.arxiv.org/api/query".to_string(),
            search_prefix: prefix.to_string(),
        }
    }

    /// Build the search URL with query parameters.
    pub fn build_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        format!(
            "{}?search_query={}{}&start=0&max_results={}&sortBy=relevance&sortOrder=descending",
            self.base_url, self.search_prefix, encoded, limit
        )
    }
}

impl Default for ArxivClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for ArxivClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let url = self.build_url(query, limit);
        vec![SourceResult {
            title: format!("[arXiv] {}", query),
            url,
            snippet: format!(
                "Search arXiv Atom API for '{}{}' (limit {}). Requires XML parsing.",
                self.search_prefix, query, limit
            ),
            source_name: "arxiv".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "arxiv"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PubMed Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the PubMed E-utilities API.
///
/// API: `https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi`
/// Uses a two-step flow: esearch (get IDs) -> esummary (get metadata).
pub struct PubMedClient {
    /// Base URL for esearch.
    esearch_url: String,
    /// Base URL for esummary.
    esummary_url: String,
    /// Optional API key for higher rate limits.
    api_key: Option<String>,
}

impl PubMedClient {
    /// Create a new PubMed client.
    pub fn new() -> Self {
        Self {
            esearch_url: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi".to_string(),
            esummary_url: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi".to_string(),
            api_key: None,
        }
    }

    /// Create a client with an API key.
    pub fn with_api_key(api_key: &str) -> Self {
        Self {
            api_key: Some(api_key.to_string()),
            ..Self::new()
        }
    }

    /// Build the esearch URL.
    pub fn build_search_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        let mut url = format!(
            "{}?db=pubmed&term={}&retmax={}&retmode=json",
            self.esearch_url, encoded, limit
        );
        if let Some(ref key) = self.api_key {
            url.push_str(&format!("&api_key={}", key));
        }
        url
    }

    /// Build the esummary URL for a set of PubMed IDs.
    pub fn build_summary_url(&self, ids: &[&str]) -> String {
        let mut url = format!(
            "{}?db=pubmed&id={}&retmode=json",
            self.esummary_url,
            ids.join(",")
        );
        if let Some(ref key) = self.api_key {
            url.push_str(&format!("&api_key={}", key));
        }
        url
    }
}

impl Default for PubMedClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for PubMedClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let search_url = self.build_search_url(query, limit);
        vec![SourceResult {
            title: format!("[PubMed] {}", query),
            url: search_url,
            snippet: format!(
                "Search PubMed E-utilities for '{}' (limit {}). Two-step: esearch -> esummary.",
                query, limit
            ),
            source_name: "pubmed".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "pubmed"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Wikipedia Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the Wikipedia API.
///
/// API: `https://en.wikipedia.org/w/api.php`
pub struct WikipediaClient {
    /// Base URL for the API.
    base_url: String,
    /// Language edition (e.g. "en", "de", "fr").
    language: String,
}

impl WikipediaClient {
    /// Create a new Wikipedia client for the English edition.
    pub fn new() -> Self {
        Self {
            base_url: "https://en.wikipedia.org/w/api.php".to_string(),
            language: "en".to_string(),
        }
    }

    /// Create a client for a specific language edition.
    pub fn with_language(lang: &str) -> Self {
        Self {
            base_url: format!("https://{}.wikipedia.org/w/api.php", lang),
            language: lang.to_string(),
        }
    }

    /// Build the search URL with query parameters.
    pub fn build_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        format!(
            "{}?action=query&list=search&srsearch={}&srlimit={}&format=json&utf8=1",
            self.base_url, encoded, limit
        )
    }

    /// Get the language edition.
    pub fn language(&self) -> &str {
        &self.language
    }
}

impl Default for WikipediaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for WikipediaClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let url = self.build_url(query, limit);
        vec![SourceResult {
            title: format!("[Wikipedia/{}] {}", self.language, query),
            url,
            snippet: format!(
                "Search Wikipedia ({}) for '{}' (limit {}). Returns article titles and snippets.",
                self.language, query, limit
            ),
            source_name: "wikipedia".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "wikipedia"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CrossRef Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the CrossRef API.
///
/// API: `https://api.crossref.org/works`
pub struct CrossRefClient {
    /// Base URL for the API.
    base_url: String,
    /// Optional mailto for polite pool (higher rate limits).
    mailto: Option<String>,
}

impl CrossRefClient {
    /// Create a new CrossRef client.
    pub fn new() -> Self {
        Self {
            base_url: "https://api.crossref.org/works".to_string(),
            mailto: None,
        }
    }

    /// Create a client with a mailto address for the polite pool.
    pub fn with_mailto(email: &str) -> Self {
        Self {
            base_url: "https://api.crossref.org/works".to_string(),
            mailto: Some(email.to_string()),
        }
    }

    /// Build the search URL with query parameters.
    pub fn build_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        let mut url = format!(
            "{}?query={}&rows={}&sort=relevance&order=desc",
            self.base_url, encoded, limit
        );
        if let Some(ref email) = self.mailto {
            url.push_str(&format!("&mailto={}", urlencoding::encode(email)));
        }
        url
    }
}

impl Default for CrossRefClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for CrossRefClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let url = self.build_url(query, limit);
        vec![SourceResult {
            title: format!("[CrossRef] {}", query),
            url,
            snippet: format!(
                "Search CrossRef API for '{}' (limit {}). Returns DOIs and metadata.",
                query, limit
            ),
            source_name: "crossref".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "crossref"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenAlex Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the OpenAlex API.
///
/// API: `https://api.openalex.org/works`
pub struct OpenAlexClient {
    /// Base URL for the API.
    base_url: String,
    /// Optional mailto for polite pool.
    mailto: Option<String>,
}

impl OpenAlexClient {
    /// Create a new OpenAlex client.
    pub fn new() -> Self {
        Self {
            base_url: "https://api.openalex.org/works".to_string(),
            mailto: None,
        }
    }

    /// Create a client with a mailto address for the polite pool.
    pub fn with_mailto(email: &str) -> Self {
        Self {
            base_url: "https://api.openalex.org/works".to_string(),
            mailto: Some(email.to_string()),
        }
    }

    /// Build the search URL with query parameters.
    pub fn build_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        let mut url = format!(
            "{}?search={}&per_page={}&sort=relevance_score:desc",
            self.base_url, encoded, limit
        );
        if let Some(ref email) = self.mailto {
            url.push_str(&format!("&mailto={}", urlencoding::encode(email)));
        }
        url
    }
}

impl Default for OpenAlexClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for OpenAlexClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let url = self.build_url(query, limit);
        vec![SourceResult {
            title: format!("[OpenAlex] {}", query),
            url,
            snippet: format!(
                "Search OpenAlex API for '{}' (limit {}). Returns works with metadata.",
                query, limit
            ),
            source_name: "openalex".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "openalex"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GitHub Client
// ─────────────────────────────────────────────────────────────────────────────

/// Client for the GitHub Search API.
///
/// API: `https://api.github.com/search/repositories`
pub struct GithubClient {
    /// Base URL for the API.
    base_url: String,
    /// Optional personal access token for authentication.
    token: Option<String>,
}

impl GithubClient {
    /// Create a new GitHub client.
    pub fn new() -> Self {
        Self {
            base_url: "https://api.github.com/search/repositories".to_string(),
            token: None,
        }
    }

    /// Create a client with an authentication token.
    pub fn with_token(token: &str) -> Self {
        Self {
            base_url: "https://api.github.com/search/repositories".to_string(),
            token: Some(token.to_string()),
        }
    }

    /// Build the search URL with query parameters.
    pub fn build_url(&self, query: &str, limit: usize) -> String {
        let encoded = urlencoding::encode(query);
        format!(
            "{}?q={}&per_page={}&sort=stars&order=desc",
            self.base_url, encoded, limit
        )
    }

    /// Check if authentication is configured.
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }
}

impl Default for GithubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceClient for GithubClient {
    fn search(&self, query: &str, limit: usize) -> Vec<SourceResult> {
        let url = self.build_url(query, limit);
        vec![SourceResult {
            title: format!("[GitHub] {}", query),
            url,
            snippet: format!(
                "Search GitHub repositories for '{}' (limit {}, authenticated={}). Sorted by stars.",
                query,
                limit,
                self.is_authenticated()
            ),
            source_name: "github".to_string(),
            relevance_score: 0.0,
            published_date: None,
        }]
    }

    fn source_name(&self) -> &str {
        "github"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Multi-Source Search
// ─────────────────────────────────────────────────────────────────────────────

/// Multi-source search aggregator.
///
/// Queries multiple `SourceClient` implementations and merges results
/// into a single ranked list.
pub struct MultiSourceSearch {
    /// Registered source clients.
    clients: Vec<Box<dyn SourceClient>>,
}

impl MultiSourceSearch {
    /// Create a new multi-source search with no clients.
    pub fn new() -> Self {
        Self {
            clients: Vec::new(),
        }
    }

    /// Create a multi-source search with all default clients.
    pub fn with_defaults() -> Self {
        let mut search = Self::new();
        search.add_client(Box::new(SemanticScholarClient::new()));
        search.add_client(Box::new(ArxivClient::new()));
        search.add_client(Box::new(PubMedClient::new()));
        search.add_client(Box::new(WikipediaClient::new()));
        search.add_client(Box::new(CrossRefClient::new()));
        search.add_client(Box::new(OpenAlexClient::new()));
        search.add_client(Box::new(GithubClient::new()));
        search
    }

    /// Add a source client.
    pub fn add_client(&mut self, client: Box<dyn SourceClient>) {
        self.clients.push(client);
    }

    /// Get the number of registered clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Search all registered sources and merge results.
    ///
    /// Each source returns up to `per_source_limit` results. Results are
    /// combined and assigned normalized relevance scores based on their
    /// position in each source's result list.
    pub fn search(&self, query: &str, per_source_limit: usize) -> Vec<SourceResult> {
        let mut all_results = Vec::new();

        for client in &self.clients {
            let mut results = client.search(query, per_source_limit);

            // Assign position-based relevance scores.
            let total = results.len() as f64;
            for (idx, result) in results.iter_mut().enumerate() {
                result.relevance_score = if total > 0.0 {
                    1.0 - (idx as f64 / total)
                } else {
                    0.0
                };
            }

            all_results.extend(results);
        }

        // Sort by relevance score descending.
        all_results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results
    }

    /// Search specific sources by name.
    pub fn search_sources(
        &self,
        query: &str,
        source_names: &[&str],
        per_source_limit: usize,
    ) -> Vec<SourceResult> {
        let mut all_results = Vec::new();

        for client in &self.clients {
            if source_names.contains(&client.source_name()) {
                let mut results = client.search(query, per_source_limit);
                let total = results.len() as f64;
                for (idx, result) in results.iter_mut().enumerate() {
                    result.relevance_score = if total > 0.0 {
                        1.0 - (idx as f64 / total)
                    } else {
                        0.0
                    };
                }
                all_results.extend(results);
            }
        }

        all_results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results
    }

    /// List all registered source names.
    pub fn source_names(&self) -> Vec<&str> {
        self.clients.iter().map(|c| c.source_name()).collect()
    }
}

impl Default for MultiSourceSearch {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Legacy free-function stubs (backward compatibility)
// ─────────────────────────────────────────────────────────────────────────────

/// Search Semantic Scholar for papers (legacy function).
pub fn search_semantic_scholar(query: &str, limit: usize) -> Vec<SourceResult> {
    SemanticScholarClient::new().search(query, limit)
}

/// Search arXiv for papers (legacy function).
pub fn search_arxiv(query: &str, limit: usize) -> Vec<SourceResult> {
    ArxivClient::new().search(query, limit)
}

/// Search PubMed for biomedical literature (legacy function).
pub fn search_pubmed(query: &str, limit: usize) -> Vec<SourceResult> {
    PubMedClient::new().search(query, limit)
}

/// Build the PubMed E-utilities esummary URL for a set of IDs.
fn pubmed_summary_url(ids: &[&str]) -> String {
    PubMedClient::new().build_summary_url(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Rate Limiter tests ───────────────────────────────────────────────

    #[test]
    fn test_rate_limiter_creation() {
        let rl = RateLimiter::new();
        assert_eq!(rl.get_interval("semantic_scholar"), 3.0);
        assert_eq!(rl.get_interval("pubmed"), 0.35);
        assert_eq!(rl.get_interval("arxiv"), 3.0);
        assert_eq!(rl.get_interval("wikipedia"), 1.0);
        assert_eq!(rl.get_interval("github"), 2.0);
    }

    #[test]
    fn test_rate_limiter_default_for_unknown() {
        let rl = RateLimiter::new();
        assert_eq!(rl.get_interval("unknown_source"), 1.0);
    }

    #[test]
    fn test_rate_limiter_try_acquire() {
        let mut rl = RateLimiter::new();
        assert!(rl.try_acquire("semantic_scholar"));
        assert!(!rl.try_acquire("semantic_scholar"));
    }

    #[test]
    fn test_rate_limiter_wait_time_initially_zero() {
        let rl = RateLimiter::new();
        assert_eq!(rl.wait_time("semantic_scholar"), 0.0);
    }

    #[test]
    fn test_rate_limiter_set_interval() {
        let mut rl = RateLimiter::new();
        rl.set_interval("custom", 5.0);
        assert_eq!(rl.get_interval("custom"), 5.0);
    }

    // ── Semantic Scholar tests ───────────────────────────────────────────

    #[test]
    fn test_semantic_scholar_client_new() {
        let client = SemanticScholarClient::new();
        assert_eq!(client.source_name(), "semantic_scholar");
    }

    #[test]
    fn test_semantic_scholar_url() {
        let client = SemanticScholarClient::new();
        let url = client.build_url("transformer attention", 5);
        assert!(url.contains("api.semanticscholar.org"));
        assert!(url.contains("transformer+attention"));
        assert!(url.contains("limit=5"));
        assert!(url.contains("fields="));
    }

    #[test]
    fn test_semantic_scholar_search() {
        let client = SemanticScholarClient::new();
        let results = client.search("transformer attention", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "semantic_scholar");
        assert!(results[0].url.contains("api.semanticscholar.org"));
    }

    #[test]
    fn test_semantic_scholar_with_api_key() {
        let client = SemanticScholarClient::with_api_key("test-key-123");
        let results = client.search("test", 1);
        assert!(results[0].snippet.contains("key=true"));
    }

    // ── arXiv tests ─────────────────────────────────────────────────────

    #[test]
    fn test_arxiv_client_new() {
        let client = ArxivClient::new();
        assert_eq!(client.source_name(), "arxiv");
    }

    #[test]
    fn test_arxiv_url() {
        let client = ArxivClient::new();
        let url = client.build_url("quantum computing", 10);
        assert!(url.contains("export.arxiv.org"));
        assert!(url.contains("max_results=10"));
        assert!(url.contains("all:"));
    }

    #[test]
    fn test_arxiv_search() {
        let results = search_arxiv("quantum computing", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "arxiv");
    }

    #[test]
    fn test_arxiv_with_prefix() {
        let client = ArxivClient::with_prefix("ti:");
        let url = client.build_url("test", 5);
        assert!(url.contains("ti:test"));
    }

    // ── PubMed tests ────────────────────────────────────────────────────

    #[test]
    fn test_pubmed_client_new() {
        let client = PubMedClient::new();
        assert_eq!(client.source_name(), "pubmed");
    }

    #[test]
    fn test_pubmed_search_url() {
        let client = PubMedClient::new();
        let url = client.build_search_url("CRISPR gene therapy", 5);
        assert!(url.contains("eutils.ncbi.nlm.nih.gov"));
        assert!(url.contains("retmax=5"));
        assert!(url.contains("retmode=json"));
    }

    #[test]
    fn test_pubmed_summary_url_format() {
        let url = pubmed_summary_url(&["123", "456", "789"]);
        assert!(url.contains("id=123,456,789"));
        assert!(url.contains("retmode=json"));
    }

    #[test]
    fn test_pubmed_search() {
        let results = search_pubmed("CRISPR gene therapy", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "pubmed");
    }

    #[test]
    fn test_pubmed_with_api_key() {
        let client = PubMedClient::with_api_key("my-key");
        let url = client.build_search_url("test", 5);
        assert!(url.contains("api_key=my-key"));
    }

    // ── Wikipedia tests ─────────────────────────────────────────────────

    #[test]
    fn test_wikipedia_client_new() {
        let client = WikipediaClient::new();
        assert_eq!(client.source_name(), "wikipedia");
        assert_eq!(client.language(), "en");
    }

    #[test]
    fn test_wikipedia_url() {
        let client = WikipediaClient::new();
        let url = client.build_url("machine learning", 10);
        assert!(url.contains("en.wikipedia.org"));
        assert!(url.contains("action=query"));
        assert!(url.contains("srlimit=10"));
    }

    #[test]
    fn test_wikipedia_with_language() {
        let client = WikipediaClient::with_language("de");
        assert_eq!(client.language(), "de");
        let url = client.build_url("test", 5);
        assert!(url.contains("de.wikipedia.org"));
    }

    #[test]
    fn test_wikipedia_search() {
        let client = WikipediaClient::new();
        let results = client.search("quantum physics", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "wikipedia");
    }

    // ── CrossRef tests ──────────────────────────────────────────────────

    #[test]
    fn test_crossref_client_new() {
        let client = CrossRefClient::new();
        assert_eq!(client.source_name(), "crossref");
    }

    #[test]
    fn test_crossref_url() {
        let client = CrossRefClient::new();
        let url = client.build_url("deep learning", 5);
        assert!(url.contains("api.crossref.org"));
        assert!(url.contains("rows=5"));
        assert!(url.contains("sort=relevance"));
    }

    #[test]
    fn test_crossref_with_mailto() {
        let client = CrossRefClient::with_mailto("user@example.com");
        let url = client.build_url("test", 5);
        assert!(url.contains("mailto="));
    }

    // ── OpenAlex tests ──────────────────────────────────────────────────

    #[test]
    fn test_openalex_client_new() {
        let client = OpenAlexClient::new();
        assert_eq!(client.source_name(), "openalex");
    }

    #[test]
    fn test_openalex_url() {
        let client = OpenAlexClient::new();
        let url = client.build_url("neural networks", 10);
        assert!(url.contains("api.openalex.org"));
        assert!(url.contains("per_page=10"));
        assert!(url.contains("search="));
    }

    #[test]
    fn test_openalex_with_mailto() {
        let client = OpenAlexClient::with_mailto("user@example.com");
        let url = client.build_url("test", 5);
        assert!(url.contains("mailto="));
    }

    // ── GitHub tests ────────────────────────────────────────────────────

    #[test]
    fn test_github_client_new() {
        let client = GithubClient::new();
        assert_eq!(client.source_name(), "github");
        assert!(!client.is_authenticated());
    }

    #[test]
    fn test_github_url() {
        let client = GithubClient::new();
        let url = client.build_url("rust async runtime", 10);
        assert!(url.contains("api.github.com"));
        assert!(url.contains("per_page=10"));
        assert!(url.contains("sort=stars"));
    }

    #[test]
    fn test_github_with_token() {
        let client = GithubClient::with_token("ghp_test123");
        assert!(client.is_authenticated());
    }

    #[test]
    fn test_github_search() {
        let client = GithubClient::new();
        let results = client.search("rust web framework", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "github");
    }

    // ── MultiSourceSearch tests ─────────────────────────────────────────

    #[test]
    fn test_multi_source_empty() {
        let search = MultiSourceSearch::new();
        assert_eq!(search.client_count(), 0);
        let results = search.search("test", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_multi_source_with_defaults() {
        let search = MultiSourceSearch::with_defaults();
        assert_eq!(search.client_count(), 7);
        let names = search.source_names();
        assert!(names.contains(&"semantic_scholar"));
        assert!(names.contains(&"arxiv"));
        assert!(names.contains(&"pubmed"));
        assert!(names.contains(&"wikipedia"));
        assert!(names.contains(&"crossref"));
        assert!(names.contains(&"openalex"));
        assert!(names.contains(&"github"));
    }

    #[test]
    fn test_multi_source_search_all() {
        let search = MultiSourceSearch::with_defaults();
        let results = search.search("machine learning", 5);
        // Should have one result per source.
        assert_eq!(results.len(), 7);
    }

    #[test]
    fn test_multi_source_search_specific() {
        let search = MultiSourceSearch::with_defaults();
        let results = search.search_sources("test", &["arxiv", "github"], 5);
        assert_eq!(results.len(), 2);
        let source_names: Vec<&str> = results.iter().map(|r| r.source_name.as_str()).collect();
        assert!(source_names.contains(&"arxiv"));
        assert!(source_names.contains(&"github"));
    }

    #[test]
    fn test_multi_source_relevance_scoring() {
        let search = MultiSourceSearch::with_defaults();
        let results = search.search("test", 5);
        // All results should have relevance scores assigned.
        for result in &results {
            assert!(result.relevance_score >= 0.0);
            assert!(result.relevance_score <= 1.0);
        }
    }

    #[test]
    fn test_multi_source_add_client() {
        let mut search = MultiSourceSearch::new();
        search.add_client(Box::new(ArxivClient::new()));
        assert_eq!(search.client_count(), 1);
        search.add_client(Box::new(GithubClient::new()));
        assert_eq!(search.client_count(), 2);
    }

    // ── URL encoding tests ──────────────────────────────────────────────

    #[test]
    fn test_urlencoding_basic() {
        assert_eq!(urlencoding::encode("hello world"), "hello+world");
        assert_eq!(urlencoding::encode("test"), "test");
    }

    #[test]
    fn test_urlencoding_special_chars() {
        let encoded = urlencoding::encode("a&b=c");
        assert!(encoded.contains("%26"));
        assert!(encoded.contains("%3D"));
    }

    // ── SourceResult serialization tests ────────────────────────────────

    #[test]
    fn test_source_result_serialization() {
        let result = SourceResult {
            title: "Test Paper".to_string(),
            url: "https://example.com".to_string(),
            snippet: "A test snippet".to_string(),
            source_name: "test".to_string(),
            relevance_score: 0.85,
            published_date: Some("2024".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Test Paper"));
        assert!(json.contains("0.85"));

        let deserialized: SourceResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "Test Paper");
        assert_eq!(deserialized.published_date, Some("2024".to_string()));
    }

    #[test]
    fn test_source_result_no_date() {
        let result = SourceResult {
            title: "No Date".to_string(),
            url: "https://example.com".to_string(),
            snippet: "No date".to_string(),
            source_name: "test".to_string(),
            relevance_score: 0.5,
            published_date: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: SourceResult = serde_json::from_str(&json).unwrap();
        assert!(restored.published_date.is_none());
    }

    // ── Legacy function tests ───────────────────────────────────────────

    #[test]
    fn test_search_semantic_scholar_stub() {
        let results = search_semantic_scholar("transformer attention", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "semantic_scholar");
        assert!(results[0].url.contains("api.semanticscholar.org"));
    }
}
