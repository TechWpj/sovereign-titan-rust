//! Prose Error Recovery — intercepts conversational tool descriptions.
//!
//! When the LLM generates prose like "I will search the web for..." instead of
//! the required `THOUGHT: / ACTION: / ACTION_INPUT:` format, this module
//! detects the intent, attempts a correction prompt, and as a last resort
//! auto-converts the prose into a valid tool call.
//!
//! Three-tier recovery (matching Python's `_detect_prose_tool_call`):
//! 1. First detection → build correction prompt, re-prompt the model
//! 2. Second detection → auto-convert prose to action, execute the tool
//! 3. Auto-convert fails → bail with raw response as answer

use std::collections::HashMap;

use fancy_regex::Regex;
use tracing::info;

// ─────────────────────────────────────────────────────────────────────────────
// Prose detection result
// ─────────────────────────────────────────────────────────────────────────────

/// Detected prose tool call info.
#[derive(Debug, Clone)]
pub struct ProseToolCall {
    /// The tool name that was detected.
    pub tool: String,
    /// The matched prose fragment (e.g. "I'll use web_search").
    pub intent: String,
}

/// Auto-converted action from prose.
#[derive(Debug, Clone)]
pub struct AutoConvertedAction {
    pub tool: String,
    pub action_input: serde_json::Value,
    pub thought: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// ProseRecovery engine
// ─────────────────────────────────────────────────────────────────────────────

/// Detects and recovers from prose tool call descriptions.
pub struct ProseRecovery {
    /// Compiled regex patterns (lazy-compiled once).
    react_marker_re: Regex,
    prose_intent_re: Regex,
    prose_let_me_re: Regex,
    prose_using_tool_re: Regex,
    /// Known tool names (populated from ToolRegistry).
    known_tools: Vec<String>,
    /// Verb → tool name mapping.
    verb_to_tool: HashMap<String, String>,
    /// Number of format failures in the current conversation.
    pub format_failures: u32,
}

impl ProseRecovery {
    /// Create a new prose recovery engine with the given known tool names.
    pub fn new(known_tools: Vec<String>) -> Self {
        // Compile fancy-regex patterns (supports lookahead/lookbehind).
        let react_marker_re = Regex::new(r"(?mi)^(THOUGHT|ACTION|ANSWER):").unwrap();

        let prose_intent_re = Regex::new(
            r"(?i)I\s*(?:'ll|will|should|need to|am going to)\s+(?:use|call|run|execute|try)\s+(?:the\s+)?(\w+)"
        ).unwrap();

        let prose_let_me_re = Regex::new(
            r"(?i)(?:Let me|I(?:'ll| will))\s+(search|look up|find|calculate|open|launch|browse|fetch|check)"
        ).unwrap();

        let prose_using_tool_re = Regex::new(
            r"(?i)(?:using|with)\s+(?:the\s+)?(\w+)\s+tool"
        ).unwrap();

        // Verb → tool mapping (matching Python's _VERB_TO_TOOL).
        let verb_to_tool: HashMap<String, String> = [
            ("search", "web_search"),
            ("look up", "web_search"),
            ("find", "web_search"),
            ("browse", "web_search"),
            ("fetch", "web_search"),
            ("calculate", "calculator"),
            ("compute", "calculator"),
            ("open", "system_control"),
            ("launch", "system_control"),
            ("start", "system_control"),
            ("run", "shell"),
            ("check", "web_search"),
            ("read", "file_search"),
            ("write", "file_search"),
            ("screenshot", "screen_capture"),
            ("look", "computer_control"),
            ("click", "computer_control"),
            ("type", "computer_control"),
            ("scroll", "computer_control"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        Self {
            react_marker_re,
            prose_intent_re,
            prose_let_me_re,
            prose_using_tool_re,
            known_tools,
            verb_to_tool,
            format_failures: 0,
        }
    }

    /// Reset the format failure counter (call at start of each conversation).
    pub fn reset(&mut self) {
        self.format_failures = 0;
    }

    /// Detect prose descriptions of tool usage in a model response.
    ///
    /// Returns `Some(ProseToolCall)` if prose is detected (and no ReAct markers
    /// are present), `None` otherwise.
    pub fn detect_prose_tool_call(&self, response: &str) -> Option<ProseToolCall> {
        // If the response already has THOUGHT/ACTION/ANSWER markers, skip.
        if self.react_marker_re.is_match(response).unwrap_or(false) {
            return None;
        }

        // Pattern 1: "I'll use/call/run <tool_name>"
        if let Ok(Some(m)) = self.prose_intent_re.captures(response) {
            if let Some(cap) = m.get(1) {
                let candidate = cap.as_str().to_lowercase();
                // Check if candidate is an actual tool name.
                if self.known_tools.iter().any(|t| t == &candidate) {
                    return Some(ProseToolCall {
                        tool: candidate,
                        intent: m.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    });
                }
                // Check if it's a verb we can map.
                if let Some(tool) = self.verb_to_tool.get(&candidate) {
                    return Some(ProseToolCall {
                        tool: tool.clone(),
                        intent: m.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    });
                }
            }
        }

        // Pattern 2: "Let me search/open/calculate..." (verb-based)
        if let Ok(Some(m)) = self.prose_let_me_re.captures(response) {
            if let Some(cap) = m.get(1) {
                let verb = cap.as_str().to_lowercase();
                if let Some(tool) = self.verb_to_tool.get(&verb) {
                    return Some(ProseToolCall {
                        tool: tool.clone(),
                        intent: m.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    });
                }
            }
        }

        // Pattern 3: "using the web_search tool"
        if let Ok(Some(m)) = self.prose_using_tool_re.captures(response) {
            if let Some(cap) = m.get(1) {
                let candidate = cap.as_str().to_lowercase();
                if self.known_tools.iter().any(|t| t == &candidate) {
                    return Some(ProseToolCall {
                        tool: candidate,
                        intent: m.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    });
                }
            }
        }

        None
    }

    /// Build a correction prompt when prose tool description is detected.
    ///
    /// This is tier 1 of the 3-tier recovery: re-prompt the model with
    /// explicit format instructions.
    pub fn build_correction_prompt(
        &self,
        prose_info: &ProseToolCall,
        task: &str,
    ) -> String {
        let tool = &prose_info.tool;
        let example_input = self.infer_example_input(tool, task);

        format!(
            "FORMAT ERROR: You described using {tool} in plain text.\n\
             Do NOT describe tools — USE them with the exact format:\n\
             THOUGHT: <your reasoning>\n\
             ACTION: {tool}\n\
             ACTION_INPUT: {example_input}\n\n\
             Now reformulate your response for the task: {task}"
        )
    }

    /// Auto-convert prose to a valid action (tier 2: last-resort fallback).
    ///
    /// Called after 2+ failed re-prompts. Attempts to construct a valid tool
    /// call from the detected tool name and the original task text.
    pub fn auto_convert_prose_to_action(
        &self,
        prose_info: &ProseToolCall,
        task: &str,
        raw_response: &str,
    ) -> Option<AutoConvertedAction> {
        let tool = &prose_info.tool;

        // Only convert if the tool is actually known.
        if !self.known_tools.iter().any(|t| t == tool) {
            return None;
        }

        // Try to infer params from the task text.
        let params = self.infer_params(tool, task, raw_response);

        info!(
            "Auto-converting prose to action: tool={tool}, params={}",
            params
        );

        Some(AutoConvertedAction {
            tool: tool.clone(),
            action_input: params,
            thought: format!("[auto-converted from prose] {}", prose_info.intent),
        })
    }

    /// Infer example ACTION_INPUT for a given tool (for correction prompts).
    fn infer_example_input(&self, tool: &str, task: &str) -> String {
        match tool {
            "web_search" => {
                // Extract a reasonable search query from the task.
                let query = task.replace('"', "");
                format!(r#"{{"query": "{query}"}}"#)
            }
            "system_control" => {
                format!(r#"{{"action": "start_program", "name": "notepad"}}"#)
            }
            "shell" => {
                format!(r#"{{"command": "echo hello"}}"#)
            }
            "file_search" => {
                format!(r#"{{"query": "readme"}}"#)
            }
            "calculator" => {
                format!(r#"{{"expression": "2 + 2"}}"#)
            }
            "computer_control" => {
                format!(r#"{{"action": "screenshot"}}"#)
            }
            "screen_capture" => {
                format!(r#"{{"action": "capture"}}"#)
            }
            _ => {
                format!(r#"{{"query": "{task}"}}"#)
            }
        }
    }

    /// Infer action parameters from the task text and raw response.
    fn infer_params(
        &self,
        tool: &str,
        task: &str,
        raw_response: &str,
    ) -> serde_json::Value {
        match tool {
            "web_search" => {
                serde_json::json!({"query": task})
            }
            "system_control" => {
                // Try to extract an app name from the task.
                let app = extract_quoted_or_last_word(task);
                serde_json::json!({"action": "start_program", "name": app})
            }
            "shell" => {
                let cmd = extract_quoted_or_last_word(raw_response);
                serde_json::json!({"command": cmd})
            }
            "calculator" => {
                // Try to find a math expression in the task.
                let expr = extract_math_expression(task);
                serde_json::json!({"expression": expr})
            }
            "file_search" => {
                serde_json::json!({"query": task})
            }
            _ => {
                serde_json::json!({"query": task})
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract a quoted string or the last word from text.
fn extract_quoted_or_last_word(text: &str) -> String {
    // Try quoted strings first.
    let quote_re = regex::Regex::new(r#""([^"]+)""#).unwrap();
    if let Some(m) = quote_re.captures(text) {
        return m[1].to_string();
    }
    // Fall back to last significant word.
    text.split_whitespace()
        .last()
        .unwrap_or("unknown")
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .to_string()
}

/// Try to extract a math expression from text.
fn extract_math_expression(text: &str) -> String {
    // Look for patterns like "2 + 2", "sqrt(16)", etc.
    let math_re = regex::Regex::new(r"[\d.]+\s*[+\-*/^%]\s*[\d.]+").unwrap();
    if let Some(m) = math_re.find(text) {
        return m.as_str().to_string();
    }
    // Fall back to the whole text.
    text.to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_recovery() -> ProseRecovery {
        ProseRecovery::new(vec![
            "web_search".to_string(),
            "shell".to_string(),
            "system_control".to_string(),
            "file_search".to_string(),
            "calculator".to_string(),
            "computer_control".to_string(),
        ])
    }

    #[test]
    fn test_no_detection_on_proper_react() {
        let r = make_recovery();
        let response = "THOUGHT: I need to search.\nACTION: web_search\nACTION_INPUT: {\"query\": \"rust\"}";
        assert!(r.detect_prose_tool_call(response).is_none());
    }

    #[test]
    fn test_detect_ill_use() {
        let r = make_recovery();
        let response = "I'll use web_search to find the answer.";
        let result = r.detect_prose_tool_call(response);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool, "web_search");
    }

    #[test]
    fn test_detect_will_use() {
        let r = make_recovery();
        let response = "I will use the shell to run a command.";
        let result = r.detect_prose_tool_call(response);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool, "shell");
    }

    #[test]
    fn test_detect_let_me_search() {
        let r = make_recovery();
        let response = "Let me search for information about Rust.";
        let result = r.detect_prose_tool_call(response);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool, "web_search");
    }

    #[test]
    fn test_detect_let_me_open() {
        let r = make_recovery();
        let response = "Let me open Notepad for you.";
        let result = r.detect_prose_tool_call(response);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool, "system_control");
    }

    #[test]
    fn test_detect_using_tool() {
        let r = make_recovery();
        let response = "I can help by using the web_search tool.";
        let result = r.detect_prose_tool_call(response);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool, "web_search");
    }

    #[test]
    fn test_detect_verb_mapping() {
        let r = make_recovery();
        let response = "I'll calculate the result.";
        let result = r.detect_prose_tool_call(response);
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool, "calculator");
    }

    #[test]
    fn test_no_detection_on_plain_text() {
        let r = make_recovery();
        let response = "The capital of France is Paris.";
        assert!(r.detect_prose_tool_call(response).is_none());
    }

    #[test]
    fn test_correction_prompt() {
        let r = make_recovery();
        let info = ProseToolCall {
            tool: "web_search".to_string(),
            intent: "I'll use web_search".to_string(),
        };
        let prompt = r.build_correction_prompt(&info, "search for rust programming");
        assert!(prompt.contains("FORMAT ERROR"));
        assert!(prompt.contains("ACTION: web_search"));
        assert!(prompt.contains("search for rust programming"));
    }

    #[test]
    fn test_auto_convert() {
        let r = make_recovery();
        let info = ProseToolCall {
            tool: "web_search".to_string(),
            intent: "I'll use web_search".to_string(),
        };
        let result = r.auto_convert_prose_to_action(&info, "rust programming", "I'll search");
        assert!(result.is_some());
        let action = result.unwrap();
        assert_eq!(action.tool, "web_search");
    }

    #[test]
    fn test_auto_convert_unknown_tool() {
        let r = make_recovery();
        let info = ProseToolCall {
            tool: "unknown_tool".to_string(),
            intent: "I'll use unknown_tool".to_string(),
        };
        let result = r.auto_convert_prose_to_action(&info, "task", "response");
        assert!(result.is_none());
    }
}
