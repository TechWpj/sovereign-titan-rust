//! Source Override Rules — determines when to skip academic sources or apply
//! user-directed site overrides.
//!
//! Ported from `sovereign_titan/sources/overrides.py`. Uses regex patterns to
//! detect music playback requests, extract `site:` directives, and gate
//! academic source routing.

use regex::Regex;
use std::sync::OnceLock;

/// Compiled regex for music playback detection.
fn music_playback_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(play|listen\s+to|queue|shuffle|skip|pause|resume|next\s+track|previous\s+track)\b.*\b(song|music|track|album|playlist|artist|band|spotify|youtube\s+music|soundcloud)\b"
        ).unwrap()
    })
}

/// Compiled regex for reverse music pattern (noun first, verb second).
fn music_playback_reverse_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(song|music|track|album|playlist)\b.*\b(play|listen|queue|shuffle)\b"
        ).unwrap()
    })
}

/// Compiled regex for research-about-music detection (should NOT be treated as playback).
fn music_research_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(research|study|paper|analysis|history\s+of|theory|impact|effect|influence)\b.*\b(music|song|album)\b"
        ).unwrap()
    })
}

/// Regex to extract `site:domain.com` directives.
fn site_directive_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bsite:([a-zA-Z0-9._-]+\.[a-zA-Z]{2,})").unwrap()
    })
}

/// Regex to extract `from domain.com` directives.
fn from_directive_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bfrom\s+([a-zA-Z0-9._-]+\.[a-zA-Z]{2,})").unwrap()
    })
}

/// Returns `true` if the query appears to be a music playback request
/// (e.g. "play some jazz", "listen to Radiohead") rather than a research query.
///
/// Queries about the *study* of music (e.g. "research on music therapy") are
/// excluded.
pub fn is_music_playback_request(query: &str) -> bool {
    // If it looks like research about music, it is NOT a playback request.
    if music_research_regex().is_match(query) {
        return false;
    }
    music_playback_regex().is_match(query) || music_playback_reverse_regex().is_match(query)
}

/// Returns `true` if academic sources should be skipped for this query.
///
/// Currently wraps [`is_music_playback_request`]; additional override rules
/// can be added here in the future.
pub fn should_skip_academic_sources(query: &str) -> bool {
    is_music_playback_request(query)
}

/// Extract a user-directed site override from the query.
///
/// Recognizes two patterns:
/// - `site:domain.com` (search-engine style)
/// - `from domain.com`
///
/// Returns the first matching domain, if any.
pub fn get_user_site_override(query: &str) -> Option<String> {
    if let Some(caps) = site_directive_regex().captures(query) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    if let Some(caps) = from_directive_regex().captures(query) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_music_playback_play_song() {
        assert!(is_music_playback_request("play some jazz music"));
    }

    #[test]
    fn test_music_playback_listen_to() {
        assert!(is_music_playback_request("listen to Radiohead on Spotify"));
    }

    #[test]
    fn test_music_playback_reverse_order() {
        assert!(is_music_playback_request("this playlist shuffle please"));
    }

    #[test]
    fn test_music_research_not_playback() {
        assert!(!is_music_playback_request("research on music therapy effects"));
    }

    #[test]
    fn test_non_music_query() {
        assert!(!is_music_playback_request("what is quantum computing"));
    }

    #[test]
    fn test_should_skip_for_playback() {
        assert!(should_skip_academic_sources("play my workout playlist"));
    }

    #[test]
    fn test_should_not_skip_for_research() {
        assert!(!should_skip_academic_sources("find papers on neural networks"));
    }

    #[test]
    fn test_site_directive() {
        let result = get_user_site_override("search neural networks site:arxiv.org");
        assert_eq!(result, Some("arxiv.org".to_string()));
    }

    #[test]
    fn test_from_directive() {
        let result = get_user_site_override("get papers from pubmed.ncbi.nlm.nih.gov");
        assert_eq!(result, Some("pubmed.ncbi.nlm.nih.gov".to_string()));
    }

    #[test]
    fn test_no_override() {
        let result = get_user_site_override("what is machine learning");
        assert!(result.is_none());
    }

    #[test]
    fn test_site_directive_case_insensitive() {
        let result = get_user_site_override("results SITE:Reddit.com about Rust");
        assert_eq!(result, Some("Reddit.com".to_string()));
    }
}
