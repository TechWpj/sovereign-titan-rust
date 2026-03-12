//! ReAct Agent — Thought → Action → Observation loop.
//!
//! Implements the ReAct pattern (Yao et al. 2022) as a native Rust loop.
//! The agent generates structured text with `THOUGHT:`, `ACTION:`, and
//! `ACTION_INPUT:` fields, which are parsed via regex and dispatched to
//! registered tools. Tool results are appended as `OBSERVATION:` blocks,
//! and the loop continues until a `FINAL_ANSWER:` is produced or the
//! maximum number of steps is reached.

use std::collections::{HashMap, HashSet};
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

/// Dedicated thought event for the `agent-thought` channel.
#[derive(Debug, Clone, Serialize)]
pub struct AgentThoughtEvent {
    pub step: usize,
    pub thought: String,
}

/// Dedicated action event for the `agent-action` channel.
#[derive(Debug, Clone, Serialize)]
pub struct AgentActionEvent {
    pub step: usize,
    pub tool: String,
    pub input: String,
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
// Fallback cascade — tool health tracking and error recovery
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks tool failure counts and marks tools as offline after repeated failures.
///
/// Ported from Python `fallback_cascades.py`.
pub struct ToolHealthTracker {
    failure_counts: HashMap<String, usize>,
    offline_tools: HashSet<String>,
    reroute_map: HashMap<&'static str, &'static str>,
}

/// Number of consecutive failures before a tool is marked offline.
const OFFLINE_THRESHOLD: usize = 3;

impl ToolHealthTracker {
    /// Create a new health tracker with default reroute mappings.
    pub fn new() -> Self {
        let mut reroute_map = HashMap::new();
        reroute_map.insert("web_search", "api_search");
        reroute_map.insert("api_search", "web_search");
        reroute_map.insert("open_browser", "system_control");

        Self {
            failure_counts: HashMap::new(),
            offline_tools: HashSet::new(),
            reroute_map,
        }
    }

    /// Record a tool failure. Returns true if the tool just went offline.
    pub fn record_failure(&mut self, tool_name: &str) -> bool {
        let count = self.failure_counts.entry(tool_name.to_string()).or_insert(0);
        *count += 1;
        if *count >= OFFLINE_THRESHOLD && !self.offline_tools.contains(tool_name) {
            self.offline_tools.insert(tool_name.to_string());
            warn!(
                "FALLBACK: {} has failed {} times — marked OFFLINE",
                tool_name, count
            );
            true
        } else {
            false
        }
    }

    /// Record a tool success — resets failure count.
    pub fn record_success(&mut self, tool_name: &str) {
        self.failure_counts.insert(tool_name.to_string(), 0);
    }

    /// Check if a tool is offline.
    pub fn is_offline(&self, tool_name: &str) -> bool {
        self.offline_tools.contains(tool_name)
    }

    /// Get an alternative tool when the requested tool is offline.
    pub fn get_reroute(&self, tool_name: &str) -> Option<&str> {
        self.reroute_map
            .get(tool_name)
            .copied()
            .filter(|alt| !self.offline_tools.contains(*alt))
    }

    /// Get the failure count for a tool.
    pub fn failure_count(&self, tool_name: &str) -> usize {
        self.failure_counts.get(tool_name).copied().unwrap_or(0)
    }

    /// Get the set of offline tools.
    pub fn offline_tools(&self) -> &HashSet<String> {
        &self.offline_tools
    }

    /// Generate a SYSTEM_ALERT observation for a failed tool.
    ///
    /// Handles offline detection, rerouting, and error-specific recovery
    /// guidance (deterministic fallback cascades).
    pub fn handle_failure(&mut self, tool_name: &str, error: &str) -> String {
        // Check if already offline.
        if self.is_offline(tool_name) {
            if let Some(alt) = self.get_reroute(tool_name) {
                return format!(
                    "SYSTEM_ALERT: {tool_name} is OFFLINE. Use '{alt}' instead."
                );
            }
            return format!(
                "SYSTEM_ALERT: {tool_name} is OFFLINE and no alternative available. \
                 Try a different approach."
            );
        }

        // Record this failure.
        let went_offline = self.record_failure(tool_name);

        if went_offline {
            let msg = format!(
                "SYSTEM_ALERT: {tool_name} has failed {OFFLINE_THRESHOLD} times \
                 and is now OFFLINE."
            );
            if let Some(alt) = self.get_reroute(tool_name) {
                return format!("{msg} Routing to '{alt}' as fallback.");
            }
            return msg;
        }

        // Error-specific recovery guidance.
        let error_lower = error.to_lowercase();
        let count = self.failure_count(tool_name);

        if error_lower.contains("timeout") {
            format!(
                "SYSTEM_ALERT: {tool_name} timed out (attempt {count}/{OFFLINE_THRESHOLD}). \
                 Retry or try an alternative tool."
            )
        } else if error_lower.contains("permission") || error_lower.contains("denied") {
            format!(
                "SYSTEM_ALERT: {tool_name} — permission denied. \
                 The resource may be locked or protected."
            )
        } else if error_lower.contains("not found") {
            format!(
                "SYSTEM_ALERT: {tool_name} — resource not found. \
                 Check the input and try again."
            )
        } else if error_lower.contains("connection") || error_lower.contains("network") {
            format!(
                "SYSTEM_ALERT: {tool_name} — connection error (attempt {count}/{OFFLINE_THRESHOLD}). \
                 Check network and retry."
            )
        } else {
            format!(
                "Tool error (attempt {count}/{OFFLINE_THRESHOLD}): {error}"
            )
        }
    }

    /// Reset all counters (new session).
    pub fn reset(&mut self) {
        self.failure_counts.clear();
        self.offline_tools.clear();
    }
}

impl Default for ToolHealthTracker {
    fn default() -> Self {
        Self::new()
    }
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
///
/// Includes a [`ToolHealthTracker`] for fallback cascade support:
/// if a tool fails 3 consecutive times it is marked offline and the
/// agent is guided to use an alternative.
pub struct ReActAgent {
    nexus: Arc<ModelNexus>,
    registry: ToolRegistry,
    app_handle: Option<AppHandle>,
    max_tokens: u32,
    health: std::sync::Mutex<ToolHealthTracker>,
}

impl ReActAgent {
    /// Create a new ReAct agent.
    pub fn new(nexus: Arc<ModelNexus>, registry: ToolRegistry) -> Self {
        Self {
            nexus,
            registry,
            app_handle: None,
            max_tokens: 1024,
            health: std::sync::Mutex::new(ToolHealthTracker::new()),
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

    /// Emit a dedicated `agent-thought` event for THOUGHT: blocks.
    fn emit_thought(&self, step: usize, thought: &str) {
        if let Some(ref handle) = self.app_handle {
            let event = AgentThoughtEvent {
                step,
                thought: thought.to_string(),
            };
            if let Err(e) = handle.emit("agent-thought", &event) {
                warn!("Failed to emit agent-thought event: {e}");
            }
        }
    }

    /// Emit a dedicated `agent-action` event for ACTION: blocks.
    fn emit_action(&self, step: usize, tool: &str, input: &str) {
        if let Some(ref handle) = self.app_handle {
            let event = AgentActionEvent {
                step,
                tool: tool.to_string(),
                input: input.to_string(),
            };
            if let Err(e) = handle.emit("agent-action", &event) {
                warn!("Failed to emit agent-action event: {e}");
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
                    self.emit_thought(step, &thought);
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
                    self.emit_thought(step, &thought);
                    self.emit_step(step, "action", &format!("{action}({action_input})"));
                    self.emit_action(step, &action, &action_input);

                    // Check if the tool is offline (fallback cascade).
                    let is_offline = {
                        let h = self.health.lock().unwrap();
                        h.is_offline(&action)
                    };

                    // Look up the tool.
                    let observation = if is_offline {
                        let mut h = self.health.lock().unwrap();
                        h.handle_failure(&action, "tool is offline")
                    } else if let Some(tool) = self.registry.get(&action) {
                        // Parse the action input as JSON.
                        let input: serde_json::Value =
                            serde_json::from_str(&action_input).unwrap_or_else(|_| {
                                serde_json::json!({"query": action_input})
                            });

                        match tool.execute(input).await {
                            Ok(result) => {
                                // Record success — resets failure count.
                                self.health.lock().unwrap().record_success(&action);
                                result
                            }
                            Err(e) => {
                                // Record failure — may trigger offline.
                                let mut h = self.health.lock().unwrap();
                                h.handle_failure(&action, &e.to_string())
                            }
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

    // ─── Fallback cascade tests ──────────────────────────────────────────

    #[test]
    fn test_health_tracker_record_failure() {
        let mut tracker = ToolHealthTracker::new();
        assert_eq!(tracker.failure_count("web_search"), 0);

        tracker.record_failure("web_search");
        assert_eq!(tracker.failure_count("web_search"), 1);
        assert!(!tracker.is_offline("web_search"));

        tracker.record_failure("web_search");
        assert_eq!(tracker.failure_count("web_search"), 2);
        assert!(!tracker.is_offline("web_search"));

        // Third failure → offline.
        tracker.record_failure("web_search");
        assert!(tracker.is_offline("web_search"));
    }

    #[test]
    fn test_health_tracker_success_resets() {
        let mut tracker = ToolHealthTracker::new();
        tracker.record_failure("shell");
        tracker.record_failure("shell");
        assert_eq!(tracker.failure_count("shell"), 2);

        tracker.record_success("shell");
        assert_eq!(tracker.failure_count("shell"), 0);
        assert!(!tracker.is_offline("shell"));
    }

    #[test]
    fn test_health_tracker_reroute() {
        let mut tracker = ToolHealthTracker::new();

        // Mark web_search offline.
        for _ in 0..3 {
            tracker.record_failure("web_search");
        }
        assert!(tracker.is_offline("web_search"));

        // Should reroute to api_search.
        assert_eq!(tracker.get_reroute("web_search"), Some("api_search"));

        // If api_search is also offline, no reroute.
        for _ in 0..3 {
            tracker.record_failure("api_search");
        }
        assert_eq!(tracker.get_reroute("web_search"), None);
    }

    #[test]
    fn test_health_tracker_handle_failure_timeout() {
        let mut tracker = ToolHealthTracker::new();
        let msg = tracker.handle_failure("shell", "Connection timeout occurred");
        assert!(msg.contains("timed out"));
        assert!(msg.contains("attempt 1/3"));
    }

    #[test]
    fn test_health_tracker_handle_failure_permission() {
        let mut tracker = ToolHealthTracker::new();
        let msg = tracker.handle_failure("file_search", "Permission denied");
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn test_health_tracker_handle_failure_not_found() {
        let mut tracker = ToolHealthTracker::new();
        let msg = tracker.handle_failure("system_control", "Program not found");
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_health_tracker_handle_failure_goes_offline() {
        let mut tracker = ToolHealthTracker::new();
        tracker.handle_failure("web_search", "error 1");
        tracker.handle_failure("web_search", "error 2");
        let msg = tracker.handle_failure("web_search", "error 3");
        assert!(msg.contains("OFFLINE"));
        assert!(msg.contains("api_search"));
    }

    #[test]
    fn test_health_tracker_already_offline() {
        let mut tracker = ToolHealthTracker::new();
        for _ in 0..3 {
            tracker.record_failure("web_search");
        }
        let msg = tracker.handle_failure("web_search", "still broken");
        assert!(msg.contains("OFFLINE"));
        assert!(msg.contains("api_search"));
    }

    #[test]
    fn test_health_tracker_reset() {
        let mut tracker = ToolHealthTracker::new();
        for _ in 0..3 {
            tracker.record_failure("shell");
        }
        assert!(tracker.is_offline("shell"));

        tracker.reset();
        assert!(!tracker.is_offline("shell"));
        assert_eq!(tracker.failure_count("shell"), 0);
    }

    #[test]
    fn test_health_tracker_offline_tools_set() {
        let mut tracker = ToolHealthTracker::new();
        assert!(tracker.offline_tools().is_empty());
        for _ in 0..3 {
            tracker.record_failure("web_search");
        }
        assert!(tracker.offline_tools().contains("web_search"));
    }
}
