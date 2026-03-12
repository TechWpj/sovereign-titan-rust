//! Execution Paths — deterministic multi-step workflows that bypass the LLM.
//!
//! Ported from `sovereign_titan/agents/execution_paths.py`. Each path is a
//! sequence of steps with parameter extraction, tool dispatch, and result
//! chaining. Patterns are matched via regex with priority ordering.

use std::collections::HashMap;

use regex::Regex;
use tracing::info;

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// A single step in an execution path.
#[derive(Debug, Clone)]
pub struct ExecutionStep {
    /// Tool to call.
    pub tool: &'static str,
    /// Function to build parameters from extracted params and previous results.
    pub build_params: fn(&HashMap<String, String>, &HashMap<String, String>) -> serde_json::Value,
    /// Key to extract from the result for the next step.
    pub extract_key: Option<&'static str>,
    /// Delay before this step (ms).
    pub delay_ms: u64,
    /// What to do on failure.
    pub on_fail: OnFail,
    /// Max retries for this step.
    pub max_retries: u32,
}

/// Failure handling strategy.
#[derive(Debug, Clone)]
pub enum OnFail {
    /// Stop the path and return the error.
    Stop,
    /// Skip this step and continue.
    Skip,
    /// Retry up to max_retries times.
    Retry,
}

/// A complete execution path.
#[derive(Debug)]
pub struct ExecutionPath {
    /// Name of this path.
    pub name: &'static str,
    /// Regex pattern to match against user query.
    pub pattern: Regex,
    /// Priority (higher = checked first).
    pub priority: u32,
    /// Function to extract named parameters from regex captures.
    pub extract_params: fn(&regex::Captures) -> HashMap<String, String>,
    /// Steps to execute in order.
    pub steps: Vec<ExecutionStep>,
}

/// Result of matching a path.
pub struct PathMatch {
    pub path_name: &'static str,
    pub params: HashMap<String, String>,
    pub steps: Vec<ExecutionStep>,
}

// ─────────────────────────────────────────────────────────────────────────────
// ExecutionPathRouter
// ─────────────────────────────────────────────────────────────────────────────

/// Routes queries to deterministic execution paths.
pub struct ExecutionPathRouter {
    paths: Vec<ExecutionPath>,
}

impl ExecutionPathRouter {
    /// Create a router with all default paths.
    pub fn new() -> Self {
        let mut paths = Self::default_paths();
        paths.sort_by(|a, b| b.priority.cmp(&a.priority));
        Self { paths }
    }

    /// Try to match a query against registered paths.
    pub fn match_path(&self, query: &str) -> Option<PathMatch> {
        let trimmed = query.trim();

        // Skip very long queries
        if trimmed.len() > 200 {
            return None;
        }

        for path in &self.paths {
            if let Some(caps) = path.pattern.captures(trimmed) {
                let params = (path.extract_params)(&caps);
                info!(
                    "ExecutionPath matched: '{}' (params: {:?})",
                    path.name, params
                );
                return Some(PathMatch {
                    path_name: path.name,
                    params,
                    steps: path.steps.clone(),
                });
            }
        }

        None
    }

    /// Number of registered paths.
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// List all path names.
    pub fn path_names(&self) -> Vec<&'static str> {
        self.paths.iter().map(|p| p.name).collect()
    }

    // ── Default Paths ───────────────────────────────────────────────────

    fn default_paths() -> Vec<ExecutionPath> {
        vec![
            // 1. Search and open URL
            ExecutionPath {
                name: "search_and_open_url",
                pattern: Regex::new(r"(?i)^(?:search\s+(?:for\s+)?(.+?)\s+and\s+open(?:\s+(?:the\s+)?(?:first|top)\s+result)?)$").unwrap(),
                priority: 90,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("query".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "web_search",
                        build_params: |params, _| serde_json::json!({
                            "query": params.get("query").unwrap_or(&String::new())
                        }),
                        extract_key: Some("first_url"),
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 1,
                    },
                    ExecutionStep {
                        tool: "system_control",
                        build_params: |_, results| {
                            let url = results.get("first_url").cloned().unwrap_or_default();
                            serde_json::json!({
                                "action": "open_url",
                                "url": url
                            })
                        },
                        extract_key: None,
                        delay_ms: 500,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 2. Play media on YouTube
            ExecutionPath {
                name: "play_media_youtube",
                pattern: Regex::new(r"(?i)^play\s+(.+?)(?:\s+on\s+youtube)?$").unwrap(),
                priority: 85,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("query".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "media",
                        build_params: |params, _| serde_json::json!({
                            "action": "play",
                            "query": params.get("query").unwrap_or(&String::new())
                        }),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 1,
                    },
                ],
            },

            // 3. Open website
            ExecutionPath {
                name: "open_website",
                pattern: Regex::new(r"(?i)^(?:open|go\s+to|navigate\s+to|visit)\s+((?:https?://)?[\w][\w.-]+\.\w{2,}(?:/\S*)?)$").unwrap(),
                priority: 80,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    let mut url = caps[1].trim().to_string();
                    if !url.starts_with("http://") && !url.starts_with("https://") {
                        url = format!("https://{url}");
                    }
                    params.insert("url".to_string(), url);
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "system_control",
                        build_params: |params, _| serde_json::json!({
                            "action": "open_url",
                            "url": params.get("url").unwrap_or(&String::new())
                        }),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 4. Search only
            ExecutionPath {
                name: "search_only",
                pattern: Regex::new(r"(?i)^(?:search|google|look\s+up|search\s+for)\s+(.+)$").unwrap(),
                priority: 70,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("query".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "web_search",
                        build_params: |params, _| serde_json::json!({
                            "query": params.get("query").unwrap_or(&String::new())
                        }),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Retry,
                        max_retries: 1,
                    },
                ],
            },

            // 5. File search and open
            ExecutionPath {
                name: "file_search_and_open",
                pattern: Regex::new(r"(?i)^find\s+(?:and\s+open\s+)?(?:file\s+)?(.+?\.\w{1,5})$").unwrap(),
                priority: 75,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("filename".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "file_search",
                        build_params: |params, _| serde_json::json!({
                            "query": params.get("filename").unwrap_or(&String::new())
                        }),
                        extract_key: Some("first_path"),
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 6. Calculate
            ExecutionPath {
                name: "calculate",
                pattern: Regex::new(r"(?i)^(?:calculate|calc|compute|what\s+is)\s+(\d[\d\s+\-*/^().%]+)$").unwrap(),
                priority: 85,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("expression".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "calculator",
                        build_params: |params, _| serde_json::json!({
                            "action": "evaluate",
                            "expression": params.get("expression").unwrap_or(&String::new())
                        }),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 7. Get time
            ExecutionPath {
                name: "get_time",
                pattern: Regex::new(r"(?i)^(?:what(?:'s|\s+is)\s+the\s+)?(?:current\s+)?time\??$").unwrap(),
                priority: 95,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "clock",
                        build_params: |_, _| serde_json::json!({"action": "get_time"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 8. Screenshot
            ExecutionPath {
                name: "screenshot",
                pattern: Regex::new(r"(?i)^(?:take\s+(?:a\s+)?)?screenshot$").unwrap(),
                priority: 85,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "screen_capture",
                        build_params: |_, _| serde_json::json!({"action": "capture"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 9. Volume control
            ExecutionPath {
                name: "volume_control",
                pattern: Regex::new(r"(?i)^(?:set\s+)?volume\s+(?:to\s+)?(\d+)(?:\s*%)?$").unwrap(),
                priority: 85,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("level".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "audio_control",
                        build_params: |params, _| {
                            let level: u64 = params.get("level").and_then(|l| l.parse().ok()).unwrap_or(50);
                            serde_json::json!({
                                "action": "set_volume",
                                "level": level
                            })
                        },
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 10. List windows
            ExecutionPath {
                name: "list_windows",
                pattern: Regex::new(r"(?i)^(?:list|show)\s+(?:all\s+)?(?:open\s+)?windows$").unwrap(),
                priority: 80,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "window_control",
                        build_params: |_, _| serde_json::json!({"action": "list"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 11. Clipboard read
            ExecutionPath {
                name: "clipboard_read",
                pattern: Regex::new(r"(?i)^(?:what(?:'s|\s+is)\s+(?:in|on)\s+(?:the|my)\s+)?clipboard\??$").unwrap(),
                priority: 80,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "clipboard",
                        build_params: |_, _| serde_json::json!({"action": "read"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 12. System info
            ExecutionPath {
                name: "system_info",
                pattern: Regex::new(r"(?i)^(?:show\s+)?(?:system\s+)?(?:info|information|specs|hardware)$").unwrap(),
                priority: 75,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "system_map",
                        build_params: |_, _| serde_json::json!({"action": "all"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 13. Process list
            ExecutionPath {
                name: "process_list",
                pattern: Regex::new(r"(?i)^(?:list|show)\s+(?:all\s+)?(?:running\s+)?processes$").unwrap(),
                priority: 75,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "process_manager",
                        build_params: |_, _| serde_json::json!({"action": "list"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 14. Ping
            ExecutionPath {
                name: "ping",
                pattern: Regex::new(r"(?i)^ping\s+(\S+)$").unwrap(),
                priority: 80,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    params.insert("host".to_string(), caps[1].trim().to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "network_tools",
                        build_params: |params, _| serde_json::json!({
                            "action": "ping",
                            "host": params.get("host").unwrap_or(&String::new())
                        }),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 15. My IP
            ExecutionPath {
                name: "my_ip",
                pattern: Regex::new(r"(?i)^(?:what(?:'s|\s+is)\s+)?my\s+(?:public\s+)?ip(?:\s+address)?\??$").unwrap(),
                priority: 85,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "network_tools",
                        build_params: |_, _| serde_json::json!({"action": "my_ip"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 1,
                    },
                ],
            },

            // 16. Mute / Unmute
            ExecutionPath {
                name: "mute_toggle",
                pattern: Regex::new(r"(?i)^(mute|unmute)(?:\s+(?:the\s+)?(?:volume|audio|sound|speakers?))?$").unwrap(),
                priority: 85,
                extract_params: |caps| {
                    let mut params = HashMap::new();
                    let action = if caps[1].to_lowercase() == "unmute" { "unmute" } else { "mute" };
                    params.insert("action".to_string(), action.to_string());
                    params
                },
                steps: vec![
                    ExecutionStep {
                        tool: "audio_control",
                        build_params: |params, _| serde_json::json!({
                            "action": params.get("action").unwrap_or(&"mute".to_string())
                        }),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },

            // 17. Get date
            ExecutionPath {
                name: "get_date",
                pattern: Regex::new(r"(?i)^(?:what(?:'s|\s+is)\s+)?(?:today(?:'s)?\s+)?date\??$").unwrap(),
                priority: 95,
                extract_params: |_| HashMap::new(),
                steps: vec![
                    ExecutionStep {
                        tool: "clock",
                        build_params: |_, _| serde_json::json!({"action": "get_date"}),
                        extract_key: None,
                        delay_ms: 0,
                        on_fail: OnFail::Stop,
                        max_retries: 0,
                    },
                ],
            },
        ]
    }
}

impl Default for ExecutionPathRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> ExecutionPathRouter {
        ExecutionPathRouter::new()
    }

    #[test]
    fn test_path_count() {
        let r = router();
        assert!(r.path_count() >= 17, "Expected at least 17 paths, got {}", r.path_count());
    }

    #[test]
    fn test_match_search_and_open() {
        let r = router();
        let m = r.match_path("search for rust programming and open the first result");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.path_name, "search_and_open_url");
        assert_eq!(m.params.get("query").unwrap(), "rust programming");
    }

    #[test]
    fn test_match_play_media() {
        let r = router();
        let m = r.match_path("play never gonna give you up");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "play_media_youtube");
    }

    #[test]
    fn test_match_open_website() {
        let r = router();
        let m = r.match_path("open google.com");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.path_name, "open_website");
        assert_eq!(m.params.get("url").unwrap(), "https://google.com");
    }

    #[test]
    fn test_match_open_website_with_https() {
        let r = router();
        let m = r.match_path("go to https://github.com");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.params.get("url").unwrap(), "https://github.com");
    }

    #[test]
    fn test_match_search_only() {
        let r = router();
        let m = r.match_path("search for quantum computing");
        assert!(m.is_some());
        // Could match search_and_open or search_only depending on priority
        assert!(m.unwrap().params.contains_key("query"));
    }

    #[test]
    fn test_match_calculate() {
        let r = router();
        let m = r.match_path("calculate 2 + 3 * 4");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "calculate");
    }

    #[test]
    fn test_match_screenshot() {
        let r = router();
        let m = r.match_path("take a screenshot");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "screenshot");
    }

    #[test]
    fn test_match_volume() {
        let r = router();
        let m = r.match_path("volume to 75");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "volume_control");
    }

    #[test]
    fn test_match_list_windows() {
        let r = router();
        let m = r.match_path("list open windows");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "list_windows");
    }

    #[test]
    fn test_match_mute() {
        let r = router();
        let m = r.match_path("mute");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.path_name, "mute_toggle");
        assert_eq!(m.params.get("action").unwrap(), "mute");
    }

    #[test]
    fn test_match_unmute() {
        let r = router();
        let m = r.match_path("unmute the volume");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.params.get("action").unwrap(), "unmute");
    }

    #[test]
    fn test_match_ping() {
        let r = router();
        let m = r.match_path("ping google.com");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "ping");
    }

    #[test]
    fn test_match_my_ip() {
        let r = router();
        let m = r.match_path("what is my ip");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "my_ip");
    }

    #[test]
    fn test_match_system_info() {
        let r = router();
        let m = r.match_path("system info");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "system_info");
    }

    #[test]
    fn test_match_process_list() {
        let r = router();
        let m = r.match_path("list running processes");
        assert!(m.is_some());
        assert_eq!(m.unwrap().path_name, "process_list");
    }

    #[test]
    fn test_no_match_complex() {
        let r = router();
        assert!(r.match_path("Can you explain how async/await works in Rust?").is_none());
    }

    #[test]
    fn test_no_match_long() {
        let r = router();
        let long = "a ".repeat(150);
        assert!(r.match_path(&long).is_none());
    }

    #[test]
    fn test_path_names() {
        let r = router();
        let names = r.path_names();
        assert!(names.contains(&"search_and_open_url"));
        assert!(names.contains(&"play_media_youtube"));
        assert!(names.contains(&"screenshot"));
    }

    #[test]
    fn test_paths_sorted_by_priority() {
        let r = router();
        for i in 1..r.paths.len() {
            assert!(
                r.paths[i - 1].priority >= r.paths[i].priority,
                "Paths should be sorted by priority descending"
            );
        }
    }
}
