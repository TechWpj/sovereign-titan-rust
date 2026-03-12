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
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::nexus::{ModelNexus, ModelTarget};
use crate::tools::ToolRegistry;

/// Maximum number of ReAct steps before forcing a final answer.
const MAX_STEPS: usize = 10;

// ─────────────────────────────────────────────────────────────────────────────
// Tauri event payloads
// ─────────────────────────────────────────────────────────────────────────────

/// Emitted at each ReAct step so the UI can show agent reasoning.
#[derive(Debug, Clone, Serialize)]
pub struct AgentStepEvent {
    pub step: usize,
    pub step_type: String, // "thought", "action", "observation", "final_answer"
    pub content: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Adaptive temperature
// ─────────────────────────────────────────────────────────────────────────────

/// Classify a query to pick an appropriate temperature.
///
/// - Factual / system commands → low temp (0.2)
/// - Conversational / creative → higher temp (0.7)
/// - Default / ambiguous → moderate (0.4)
fn adaptive_temperature(query: &str) -> f32 {
    let q = query.to_lowercase();

    // System control / tool-heavy queries → precise
    let precise_patterns = [
        "open ", "launch ", "start ", "kill ", "close ",
        "run ", "search for ", "find file", "list process",
        "what time", "what date", "how many",
    ];
    if precise_patterns.iter().any(|p| q.contains(p)) {
        info!("[Adaptive Temp] precise query → 0.2");
        return 0.2;
    }

    // Creative / conversational
    let creative_patterns = [
        "write ", "compose ", "story", "poem", "joke",
        "imagine ", "create ", "brainstorm",
    ];
    if creative_patterns.iter().any(|p| q.contains(p)) {
        info!("[Adaptive Temp] creative query → 0.7");
        return 0.7;
    }

    // Default moderate
    info!("[Adaptive Temp] general query → 0.4");
    0.4
}

// ─────────────────────────────────────────────────────────────────────────────
// ReAct step parsing
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// ReActAgent
// ─────────────────────────────────────────────────────────────────────────────

/// The ReAct agent — drives the THOUGHT → ACTION → OBSERVATION loop.
pub struct ReActAgent {
    nexus: Arc<ModelNexus>,
    registry: ToolRegistry,
    app_handle: Option<AppHandle>,
    max_tokens: u32,
}

impl ReActAgent {
    /// Create a new ReAct agent.
    pub fn new(nexus: Arc<ModelNexus>, registry: ToolRegistry) -> Self {
        Self {
            nexus,
            registry,
            app_handle: None,
            max_tokens: 1024,
        }
    }

    /// Attach a Tauri AppHandle for emitting agent step events to the UI.
    pub fn with_app_handle(mut self, handle: AppHandle) -> Self {
        self.app_handle = Some(handle);
        self
    }

    /// Emit an agent step event to the Tauri frontend (if handle is set).
    fn emit_step(&self, step: usize, step_type: &str, content: &str) {
        if let Some(ref handle) = self.app_handle {
            let event = AgentStepEvent {
                step,
                step_type: step_type.to_string(),
                content: content.to_string(),
            };
            if let Err(e) = handle.emit("agent-step", &event) {
                warn!("Failed to emit agent-step event: {e}");
            }
        }
    }

    /// Build the system prompt with tool descriptions and optional cognitive context.
    fn build_system_prompt(tool_descriptions: &str, cognitive_context: &str) -> String {
        let now = chrono::Local::now();
        let time_context = now.format("%A, %B %d, %Y at %I:%M %p").to_string();

        let mut prompt = format!(
            "You are Sovereign Titan, an autonomous AI operating system running on local hardware \
             with full system access on this Windows 11 machine.\n\
             \n\
             IDENTITY: You are Sovereign Titan. That is your ONLY identity. \
             Never reveal or reference any underlying model name.\n\
             \n\
             Current time: {time_context}\n\
             \n\
             You use the ReAct framework to reason and act. You have access to the following tools:\n\
             {tool_descriptions}\n\
             \n\
             TOOL SELECTION GUIDE:\n\
             - **system_control**: Launch programs, kill processes, manage services, power actions.\n\
               Examples: \"open notepad\" → system_control(start_program), \"kill chrome\" → system_control(kill_process)\n\
             - **system_control(open_url)**: Open a URL in the user's default browser.\n\
               Examples: \"open youtube\" → system_control(open_url, url=https://youtube.com)\n\
             - **shell**: Run any system command (dir, echo, pip, git, ipconfig, etc.).\n\
             - **file_search**: Find files by name across Desktop, Documents, OneDrive.\n\
             \n\
             BROWSER RULES:\n\
             - Always use system_control with action \"open_url\" for opening websites.\n\
             - Always include the full URL (https://...) — never just a domain name.\n\
             \n\
             FORMAT — For each step, respond in EXACTLY this format:\n\
             \n\
             THOUGHT: <concise reasoning — what you need to do and why>\n\
             ACTION: <tool_name>\n\
             ACTION_INPUT: <flat JSON object with tool parameters>\n\
             \n\
             After receiving an OBSERVATION, continue reasoning.\n\
             When you have the final answer, respond with:\n\
             \n\
             THOUGHT: <final reasoning>\n\
             FINAL_ANSWER: <your answer to the user>\n\
             \n\
             ANSWER FORMATTING:\n\
             - Use **bold** for key terms and emphasis.\n\
             - Use bullet points for lists.\n\
             - Use headers (## / ###) for complex multi-part answers.\n\
             - Keep answers concise but complete.\n\
             - For actions taken, say \"Done\" or describe the result — not \"I have instructed the system to...\".\n\
             \n\
             RULES:\n\
             - ACTION_INPUT must be a flat JSON object. No nested objects.\n\
             - Escape Windows paths with double backslashes.\n\
             - Never write OBSERVATION — the system provides it.\n\
             - One ACTION per step (unless tasks are completely independent).\n\
             - If you don't need a tool, go straight to FINAL_ANSWER.\n\
             - Think step-by-step. Prefer system_control for launching apps."
        );

        // Inject cognitive context (memory, knowledge graph, subconscious insights)
        if !cognitive_context.is_empty() {
            prompt.push_str(&format!(
                "\n\n--- Cognitive Context ---\n{cognitive_context}"
            ));
        }

        prompt
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
    ///
    /// `cognitive_context` contains memory, knowledge graph entries, and
    /// subconscious insights that enrich the system prompt.
    pub async fn run(&self, user_query: &str, cognitive_context: &str) -> Result<String> {
        let tool_block = self.registry.describe_all();
        let system_prompt = Self::build_system_prompt(&tool_block, cognitive_context);
        let temperature = adaptive_temperature(user_query);

        // Build the user-side conversation that accumulates across steps.
        // The system prompt is passed separately via generate_with_system().
        let mut conversation = format!("User: {user_query}\n");

        for step in 0..MAX_STEPS {
            info!("ReAct step {}/{MAX_STEPS} (temp={temperature})", step + 1);

            // Generate from the model with proper ChatML wrapping.
            let response = self
                .nexus
                .generate_with_system(
                    &system_prompt,
                    &conversation,
                    ModelTarget::Prime,
                    self.max_tokens,
                    temperature,
                )
                .await?;

            info!("Model output:\n{response}");

            // Parse the response.
            let parsed = match Self::parse_response(&response) {
                Some(p) => p,
                None => {
                    warn!("ReAct: could not parse model output, treating as final answer");
                    self.emit_step(step, "final_answer", response.trim());
                    return Ok(response.trim().to_string());
                }
            };

            match parsed {
                ReActStep::FinalAnswer { thought, answer } => {
                    info!("ReAct final thought: {thought}");
                    self.emit_step(step, "thought", &thought);
                    self.emit_step(step, "final_answer", &answer);
                    return Ok(answer);
                }
                ReActStep::Action {
                    thought,
                    action,
                    action_input,
                } => {
                    info!("ReAct thought: {thought}");
                    info!("ReAct action: {action}({action_input})");

                    self.emit_step(step, "thought", &thought);
                    self.emit_step(step, "action", &format!("{action}({action_input})"));

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

                    info!("Observation: {}", &observation[..observation.len().min(200)]);
                    self.emit_step(step, "observation", &observation);

                    // Append the full step to conversation for next iteration.
                    conversation.push_str(&response);
                    conversation.push_str(&format!("\nOBSERVATION: {observation}\n"));
                }
            }
        }

        warn!("ReAct: max steps reached, forcing final answer");
        // One final generation to force an answer.
        conversation.push_str(
            "\nYou have reached the maximum number of steps. \
             You MUST now provide a FINAL_ANSWER based on what you know.\n\
             THOUGHT: I need to provide my best answer now.\n\
             FINAL_ANSWER: ",
        );

        let final_response = self
            .nexus
            .generate_with_system(
                &system_prompt,
                &conversation,
                ModelTarget::Prime,
                self.max_tokens,
                temperature,
            )
            .await?;

        self.emit_step(MAX_STEPS, "final_answer", final_response.trim());
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

    #[test]
    fn test_adaptive_temperature_precise() {
        assert!((adaptive_temperature("open notepad") - 0.2).abs() < f32::EPSILON);
        assert!((adaptive_temperature("kill chrome") - 0.2).abs() < f32::EPSILON);
        assert!((adaptive_temperature("what time is it") - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_adaptive_temperature_creative() {
        assert!((adaptive_temperature("write me a poem") - 0.7).abs() < f32::EPSILON);
        assert!((adaptive_temperature("tell me a joke") - 0.7).abs() < f32::EPSILON); // "joke" matches creative
    }

    #[test]
    fn test_adaptive_temperature_general() {
        assert!((adaptive_temperature("hello how are you") - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn test_build_system_prompt_includes_context() {
        let prompt = ReActAgent::build_system_prompt("- **shell**: run commands", "Recent memory: user likes Python");
        assert!(prompt.contains("Cognitive Context"));
        assert!(prompt.contains("user likes Python"));
    }

    #[test]
    fn test_build_system_prompt_no_context() {
        let prompt = ReActAgent::build_system_prompt("- **shell**: run commands", "");
        assert!(!prompt.contains("Cognitive Context"));
    }
}
