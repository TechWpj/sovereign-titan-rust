//! Verbosity Mode — controls response detail level.
//!
//! Three modes:
//! - **Terminal**: Ultra-concise (2 sentences max). For power users.
//! - **Assistant**: Adaptive detail level (default). Matches query complexity.
//! - **Debug**: Full routing info, tool calls, timings. For development.

use serde::{Deserialize, Serialize};

/// Verbosity levels for agent responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerbosityMode {
    /// Ultra-concise: 2 sentences max, no explanation.
    Terminal,
    /// Adaptive: matches response detail to query complexity (default).
    Assistant,
    /// Full debug: routing path, tool calls, timings, internal state.
    Debug,
}

impl VerbosityMode {
    /// Parse from a string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "terminal" | "terse" | "short" | "brief" => VerbosityMode::Terminal,
            "debug" | "verbose" | "full" => VerbosityMode::Debug,
            _ => VerbosityMode::Assistant,
        }
    }

    /// Get a system prompt directive for this verbosity level.
    pub fn directive(&self) -> &'static str {
        match self {
            VerbosityMode::Terminal => {
                "\nVERBOSITY: Terminal mode. Keep responses to 2 sentences maximum. \
                 No explanations, no bullet points — just the direct answer or confirmation."
            }
            VerbosityMode::Assistant => {
                // Default — no extra directive needed
                ""
            }
            VerbosityMode::Debug => {
                "\nVERBOSITY: Debug mode. Include all details: tool selection reasoning, \
                 parameter choices, intermediate results, and timing information."
            }
        }
    }
}

impl Default for VerbosityMode {
    fn default() -> Self {
        VerbosityMode::Assistant
    }
}

impl std::fmt::Display for VerbosityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerbosityMode::Terminal => write!(f, "terminal"),
            VerbosityMode::Assistant => write!(f, "assistant"),
            VerbosityMode::Debug => write!(f, "debug"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_assistant() {
        assert_eq!(VerbosityMode::default(), VerbosityMode::Assistant);
    }

    #[test]
    fn test_parse_terminal() {
        assert_eq!(VerbosityMode::from_str_loose("terminal"), VerbosityMode::Terminal);
        assert_eq!(VerbosityMode::from_str_loose("terse"), VerbosityMode::Terminal);
        assert_eq!(VerbosityMode::from_str_loose("SHORT"), VerbosityMode::Terminal);
        assert_eq!(VerbosityMode::from_str_loose("brief"), VerbosityMode::Terminal);
    }

    #[test]
    fn test_parse_debug() {
        assert_eq!(VerbosityMode::from_str_loose("debug"), VerbosityMode::Debug);
        assert_eq!(VerbosityMode::from_str_loose("verbose"), VerbosityMode::Debug);
        assert_eq!(VerbosityMode::from_str_loose("FULL"), VerbosityMode::Debug);
    }

    #[test]
    fn test_parse_assistant() {
        assert_eq!(VerbosityMode::from_str_loose("assistant"), VerbosityMode::Assistant);
        assert_eq!(VerbosityMode::from_str_loose("normal"), VerbosityMode::Assistant);
        assert_eq!(VerbosityMode::from_str_loose("anything"), VerbosityMode::Assistant);
    }

    #[test]
    fn test_directive_terminal() {
        let d = VerbosityMode::Terminal.directive();
        assert!(d.contains("2 sentences"));
    }

    #[test]
    fn test_directive_assistant_empty() {
        assert!(VerbosityMode::Assistant.directive().is_empty());
    }

    #[test]
    fn test_directive_debug() {
        let d = VerbosityMode::Debug.directive();
        assert!(d.contains("Debug mode"));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", VerbosityMode::Terminal), "terminal");
        assert_eq!(format!("{}", VerbosityMode::Assistant), "assistant");
        assert_eq!(format!("{}", VerbosityMode::Debug), "debug");
    }

    #[test]
    fn test_serialize_deserialize() {
        let json = serde_json::to_string(&VerbosityMode::Debug).unwrap();
        let parsed: VerbosityMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, VerbosityMode::Debug);
    }
}
