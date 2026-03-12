//! Semantic Router — fast-path routing for prompts that can skip the LLM.
//!
//! Ported from `sovereign_titan/routing/router.py`.
//! Uses regex heuristics and keyword matching to instantly determine if a
//! user prompt should bypass inference and directly trigger a tool action.

use regex::Regex;

/// The result of routing a user prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteDecision {
    /// Skip the LLM — directly invoke this tool with this input.
    DirectTool {
        tool: String,
        action: String,
        args: serde_json::Value,
    },
    /// No fast path matched — send to the LLM for full reasoning.
    Inference,
}

/// A single routing rule with a regex pattern and target tool.
struct Route {
    pattern: Regex,
    tool: &'static str,
    action: &'static str,
    /// Function to extract args from the regex captures.
    extract: fn(&regex::Captures) -> serde_json::Value,
}

/// Semantic router that checks incoming prompts against fast-path rules.
pub struct SemanticRouter {
    routes: Vec<Route>,
    route_count: usize,
}

impl SemanticRouter {
    /// Create a router with the default set of heuristic rules.
    pub fn new() -> Self {
        let routes = vec![
            // ── Shell Commands (before app launch to avoid "run command" conflict)
            Route {
                pattern: Regex::new(r"(?i)^(?:run command|execute|shell)\s*[:\s]+(.+)$").unwrap(),
                tool: "shell",
                action: "execute",
                extract: |caps| serde_json::json!({
                    "command": caps[1].trim()
                }),
            },
            // ── App Launch ──────────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^(?:open|launch|start|run)\s+(.+)$").unwrap(),
                tool: "system_control",
                action: "start_program",
                extract: |caps| serde_json::json!({
                    "action": "start_program",
                    "name": caps[1].trim()
                }),
            },
            // ── Close / Kill ────────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^(?:close|kill|stop|end)\s+(.+)$").unwrap(),
                tool: "system_control",
                action: "kill_process",
                extract: |caps| serde_json::json!({
                    "action": "kill_process",
                    "name": caps[1].trim()
                }),
            },
            // ── Web Navigation ──────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^(?:go to|navigate to|visit|browse)\s+(https?://\S+)$").unwrap(),
                tool: "web_search",
                action: "fetch",
                extract: |caps| serde_json::json!({
                    "url": caps[1].trim()
                }),
            },
            // ── File Read ───────────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^(?:read|show|cat|display)\s+(?:file\s+)?(.+\.\w{1,5})$").unwrap(),
                tool: "code_ops",
                action: "read",
                extract: |caps| serde_json::json!({
                    "action": "read",
                    "path": caps[1].trim()
                }),
            },
            // ── File Search ─────────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^(?:find|search for|locate)\s+(?:file\s+)?(.+)$").unwrap(),
                tool: "file_search",
                action: "search",
                extract: |caps| serde_json::json!({
                    "query": caps[1].trim()
                }),
            },
            // ── List Directory ──────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^(?:list|ls|dir)\s+(.+)$").unwrap(),
                tool: "code_ops",
                action: "list",
                extract: |caps| serde_json::json!({
                    "action": "list",
                    "path": caps[1].trim()
                }),
            },
            // ── Lock / Sleep ────────────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^lock\s+(?:the\s+)?(?:computer|pc|screen|system)$").unwrap(),
                tool: "system_control",
                action: "lock",
                extract: |_| serde_json::json!({"action": "lock"}),
            },
            Route {
                pattern: Regex::new(r"(?i)^(?:sleep|suspend)\s+(?:the\s+)?(?:computer|pc|system)$").unwrap(),
                tool: "system_control",
                action: "sleep",
                extract: |_| serde_json::json!({"action": "sleep"}),
            },
            // ── Type Text (computer control) ────────────────────────────
            Route {
                pattern: Regex::new(r#"(?i)^type\s+"([^"]+)"$"#).unwrap(),
                tool: "computer_control",
                action: "type_text",
                extract: |caps| serde_json::json!({
                    "action": "type_text",
                    "text": &caps[1]
                }),
            },
            // ── Click At Coordinates ────────────────────────────────────
            Route {
                pattern: Regex::new(r"(?i)^click\s+(?:at\s+)?(\d+)\s*,\s*(\d+)$").unwrap(),
                tool: "computer_control",
                action: "click",
                extract: |caps| serde_json::json!({
                    "action": "click",
                    "x": caps[1].parse::<i64>().unwrap_or(0),
                    "y": caps[2].parse::<i64>().unwrap_or(0)
                }),
            },
        ];

        let route_count = routes.len();
        Self {
            routes,
            route_count,
        }
    }

    /// Route a user prompt. Returns `DirectTool` if a fast path matches,
    /// or `Inference` if the prompt needs the LLM.
    pub fn route(&self, prompt: &str) -> RouteDecision {
        let trimmed = prompt.trim();

        // Skip very long prompts — they're unlikely to be simple commands.
        if trimmed.len() > 200 {
            return RouteDecision::Inference;
        }

        for route in &self.routes {
            if let Some(caps) = route.pattern.captures(trimmed) {
                let args = (route.extract)(&caps);
                return RouteDecision::DirectTool {
                    tool: route.tool.to_string(),
                    action: route.action.to_string(),
                    args,
                };
            }
        }

        RouteDecision::Inference
    }

    /// Number of registered routes.
    pub fn route_count(&self) -> usize {
        self.route_count
    }
}

impl Default for SemanticRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> SemanticRouter {
        SemanticRouter::new()
    }

    #[test]
    fn test_open_app() {
        let r = router();
        match r.route("open Discord") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "system_control");
                assert_eq!(args["name"], "Discord");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_launch_case_insensitive() {
        let r = router();
        match r.route("LAUNCH chrome") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "system_control");
                assert_eq!(args["name"], "chrome");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_kill_process() {
        let r = router();
        match r.route("kill notepad") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "system_control");
                assert_eq!(args["name"], "notepad");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_navigate_url() {
        let r = router();
        match r.route("go to https://example.com") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "web_search");
                assert_eq!(args["url"], "https://example.com");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_read_file() {
        let r = router();
        match r.route("read file config.toml") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "code_ops");
                assert_eq!(args["action"], "read");
                assert_eq!(args["path"], "config.toml");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_find_file() {
        let r = router();
        match r.route("find readme.md") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "file_search");
                assert_eq!(args["query"], "readme.md");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_lock_computer() {
        let r = router();
        match r.route("lock the computer") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "system_control");
                assert_eq!(args["action"], "lock");
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_complex_prompt_goes_to_inference() {
        let r = router();
        assert_eq!(
            r.route("Can you explain how async/await works in Rust?"),
            RouteDecision::Inference
        );
    }

    #[test]
    fn test_long_prompt_goes_to_inference() {
        let r = router();
        let long = "a ".repeat(150);
        assert_eq!(r.route(&long), RouteDecision::Inference);
    }

    #[test]
    fn test_click_coordinates() {
        let r = router();
        match r.route("click at 500, 300") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "computer_control");
                assert_eq!(args["x"], 500);
                assert_eq!(args["y"], 300);
            }
            _ => panic!("expected DirectTool"),
        }
    }

    #[test]
    fn test_shell_command() {
        let r = router();
        match r.route("run command: git status") {
            RouteDecision::DirectTool { tool, args, .. } => {
                assert_eq!(tool, "shell");
                assert_eq!(args["command"], "git status");
            }
            _ => panic!("expected DirectTool"),
        }
    }
}
