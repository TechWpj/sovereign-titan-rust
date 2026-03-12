//! ReAct Agent — Thought → Action → Observation loop.
//!
//! Implements the ReAct pattern (Yao et al. 2022) as a native Rust loop.
//! The agent generates structured text with `THOUGHT:`, `ACTION:`, and
//! `ACTION_INPUT:` fields, which are parsed via regex and dispatched to
//! registered tools. Tool results are appended as `OBSERVATION:` blocks,
//! and the loop continues until a `FINAL_ANSWER:` is produced or the
//! maximum number of steps is reached.

use std::sync::Arc;

use anyhow::Result;
use regex::Regex;
use tracing::{info, warn};

use crate::nexus::{ModelNexus, ModelTarget};
use crate::tools::ToolRegistry;

/// Maximum number of ReAct steps before forcing a final answer.
const MAX_STEPS: usize = 10;

/// A parsed ReAct step from model output.
#[derive(Debug, Clone)]
pub enum ReActStep {
    /// The agent wants to call a tool.
    Action {
        thought: String,
        action: String,
        action_input: String,
    },
    /// The agent is ready to answer.
    FinalAnswer {
        thought: String,
        answer: String,
    },
}

/// The ReAct agent — drives the THOUGHT → ACTION → OBSERVATION loop.
pub struct ReActAgent {
    nexus: Arc<ModelNexus>,
    registry: ToolRegistry,
    system_prompt: String,
    max_tokens: u32,
    temperature: f32,
}

impl ReActAgent {
    /// Create a new ReAct agent.
    pub fn new(nexus: Arc<ModelNexus>, registry: ToolRegistry) -> Self {
        let tool_block = registry.describe_all();
        let system_prompt = Self::build_system_prompt(&tool_block);

        Self {
            nexus,
            registry,
            system_prompt,
            max_tokens: 1024,
            temperature: 0.7,
        }
    }

    /// Build the system prompt with tool descriptions injected.
    fn build_system_prompt(tool_descriptions: &str) -> String {
        format!(
            "You are Titan, a sovereign AI assistant with full control over this Windows system.\n\
             You use the ReAct framework to reason and act.\n\
             \n\
             You have access to the following tools:\n\
             {tool_descriptions}\n\
             \n\
             Tool usage guide:\n\
             - Use **file_search** to find files by name across Desktop, Documents, OneDrive.\n\
             - Use **shell** to run any system command (dir, echo, pip, git, etc.).\n\
             - Use **system_control** to launch programs, kill processes, manage services, or lock/sleep.\n\
             \n\
             For each step, you MUST respond in EXACTLY this format:\n\
             \n\
             THOUGHT: <your reasoning about what to do next>\n\
             ACTION: <tool name>\n\
             ACTION_INPUT: <JSON input for the tool>\n\
             \n\
             After receiving an OBSERVATION, continue reasoning.\n\
             When you have the final answer, respond with:\n\
             \n\
             THOUGHT: <your final reasoning>\n\
             FINAL_ANSWER: <your answer to the user>\n\
             \n\
             Always think step-by-step. Use tools when needed. Prefer system_control for \
             launching apps and managing processes. Use shell for general commands."
        )
    }

    /// Parse a model response into a [`ReActStep`].
    pub fn parse_response(text: &str) -> Option<ReActStep> {
        // Try FINAL_ANSWER first.
        let final_re =
            Regex::new(r"(?si)THOUGHT:\s*(.+?)FINAL_ANSWER:\s*(.+?)$").unwrap();
        if let Some(caps) = final_re.captures(text) {
            return Some(ReActStep::FinalAnswer {
                thought: caps[1].trim().to_string(),
                answer: caps[2].trim().to_string(),
            });
        }

        // Try ACTION pattern.
        let action_re = Regex::new(
            r"(?si)THOUGHT:\s*(.+?)ACTION:\s*(\S+)\s*\nACTION_INPUT:\s*(.+?)$",
        )
        .unwrap();
        if let Some(caps) = action_re.captures(text) {
            return Some(ReActStep::Action {
                thought: caps[1].trim().to_string(),
                action: caps[2].trim().to_string(),
                action_input: caps[3].trim().to_string(),
            });
        }

        None
    }

    /// Run the full ReAct loop for a user query, returning the final answer.
    pub async fn run(&self, user_query: &str) -> Result<String> {
        let mut history = format!(
            "{}\n\nUser: {}\n",
            self.system_prompt, user_query
        );

        for step in 0..MAX_STEPS {
            info!("ReAct step {}/{MAX_STEPS}", step + 1);

            // Generate from the model.
            let response = self
                .nexus
                .generate(&history, ModelTarget::Prime, self.max_tokens, self.temperature)
                .await?;

            info!("Model output:\n{response}");

            // Parse the response.
            let parsed = match Self::parse_response(&response) {
                Some(p) => p,
                None => {
                    warn!("ReAct: could not parse model output, treating as final answer");
                    return Ok(response.trim().to_string());
                }
            };

            match parsed {
                ReActStep::FinalAnswer { thought, answer } => {
                    info!("ReAct final thought: {thought}");
                    return Ok(answer);
                }
                ReActStep::Action {
                    thought,
                    action,
                    action_input,
                } => {
                    info!("ReAct thought: {thought}");
                    info!("ReAct action: {action}({action_input})");

                    // Look up the tool.
                    let observation = if let Some(tool) = self.registry.get(&action) {
                        // Parse the action input as JSON.
                        let input: serde_json::Value =
                            serde_json::from_str(&action_input).unwrap_or_else(|_| {
                                serde_json::json!({"query": action_input})
                            });

                        match tool.execute(input).await {
                            Ok(result) => result,
                            Err(e) => format!("Tool error: {e}"),
                        }
                    } else {
                        format!("Unknown tool: \"{action}\". Available tools: {:?}", self.registry.names())
                    };

                    // Append the step to history.
                    history.push_str(&response);
                    history.push_str(&format!("\nOBSERVATION: {observation}\n"));
                }
            }
        }

        warn!("ReAct: max steps reached, forcing final answer");
        // One final generation to force an answer.
        history.push_str(
            "\nYou have reached the maximum number of steps. \
             You MUST now provide a FINAL_ANSWER based on what you know.\n\
             THOUGHT: I need to provide my best answer now.\n\
             FINAL_ANSWER: ",
        );

        let final_response = self
            .nexus
            .generate(&history, ModelTarget::Prime, self.max_tokens, self.temperature)
            .await?;

        Ok(final_response.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_final_answer() {
        let text = "THOUGHT: I know the answer.\nFINAL_ANSWER: The capital of France is Paris.";
        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::FinalAnswer { thought, answer } => {
                assert_eq!(thought, "I know the answer.");
                assert_eq!(answer, "The capital of France is Paris.");
            }
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_parse_action() {
        let text = "THOUGHT: I need to search for this file.\nACTION: file_search\nACTION_INPUT: {\"query\": \"readme\"}";
        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::Action {
                thought,
                action,
                action_input,
            } => {
                assert_eq!(thought, "I need to search for this file.");
                assert_eq!(action, "file_search");
                assert_eq!(action_input, "{\"query\": \"readme\"}");
            }
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn test_parse_garbage_returns_none() {
        let text = "Hello world, this is not a ReAct response.";
        let step = ReActAgent::parse_response(text);
        assert!(step.is_none());
    }

    #[test]
    fn test_parse_multiline_thought() {
        let text = "THOUGHT: First I need to think.\nThen I realize I should search.\nACTION: file_search\nACTION_INPUT: {\"query\": \"test\"}";
        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::Action { action, .. } => {
                assert_eq!(action, "file_search");
            }
            _ => panic!("expected Action"),
        }
    }
}
