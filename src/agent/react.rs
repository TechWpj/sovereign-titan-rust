//! ReAct Agent — Thought → Action → Observation loop with routing waterfall.
//!
//! Implements the ReAct pattern (Yao et al. 2022) as a native Rust loop,
//! enhanced with a multi-tier routing waterfall that intercepts common
//! operations before they reach the LLM:
//!
//! 1. **Fast Launch** — "open Discord" → AppDiscovery resolve → system_control
//! 2. **Fast Answer** — time/date, processes, installed apps (no LLM)
//! 3. **ReAct Loop** — full THOUGHT→ACTION→OBSERVATION cycle with:
//!    - Observation truncation (cap 2000 chars)
//!    - Context compression (drop old OBSERVATIONs when >12K chars)
//!    - Answer quality gate (reject <10 chars, echo, refusal)
//!    - Duplicate action detection

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::agent::context::ContextManager;
use crate::agent::distiller::ObservationDistiller;
use crate::agent::execution_paths::ExecutionPathRouter;
use crate::agent::quality::{AnswerQualityGate, QualityVerdict};
use crate::agent::tone::ToneDetector;
use crate::agent::tool_memory::ToolOutcomeMemory;
use crate::agent::verbosity::VerbosityMode;
use crate::nexus::{ModelNexus, ModelTarget};
use crate::system::app_discovery::AppDiscovery;
use crate::tools::ToolRegistry;

/// Maximum number of ReAct steps before forcing a final answer.
const MAX_STEPS: usize = 10;

/// Maximum observation length before truncation.
const MAX_OBSERVATION_LEN: usize = 2000;

/// Context compression threshold (chars).
const CONTEXT_COMPRESS_THRESHOLD: usize = 12_000;

/// Maximum context length (chars).
const MAX_CONTEXT_LEN: usize = 24_000;

/// Minimum acceptable answer length.
const MIN_ANSWER_LEN: usize = 10;

// ─────────────────────────────────────────────────────────────────────────────
// Agent result
// ─────────────────────────────────────────────────────────────────────────────

/// Result of running the agent, including metadata about how the query was handled.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    /// The final answer text.
    pub answer: String,
    /// Tools that were used during execution.
    pub tools_used: Vec<String>,
    /// Number of ReAct iterations (0 for fast paths).
    pub iterations: usize,
    /// Which routing path was taken.
    pub routing_path: RoutingPath,
}

/// Which routing tier handled the query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RoutingPath {
    FastLaunch,
    FastAnswer,
    ExecutionPath(String),
    ReactLoop,
}

impl std::fmt::Display for RoutingPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingPath::FastLaunch => write!(f, "fast_launch"),
            RoutingPath::FastAnswer => write!(f, "fast_answer"),
            RoutingPath::ExecutionPath(name) => write!(f, "execution_path:{name}"),
            RoutingPath::ReactLoop => write!(f, "react_loop"),
        }
    }
}

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
        "set volume", "take screenshot", "list windows",
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
// Fast path detection
// ─────────────────────────────────────────────────────────────────────────────

/// Patterns that indicate an app launch intent.
const LAUNCH_PATTERNS: &[&str] = &[
    "open ", "launch ", "start ", "run ", "fire up ",
    "bring up ", "pull up ", "load ",
];

/// Extract app name from a launch query, if it matches.
fn extract_launch_target(query: &str) -> Option<String> {
    let q = query.trim().to_lowercase();

    for pattern in LAUNCH_PATTERNS {
        if q.starts_with(pattern) {
            let target = q[pattern.len()..].trim();
            // Don't match things that look like commands or URLs
            if target.is_empty()
                || target.contains("http://")
                || target.contains("https://")
                || target.contains('\\')
                || target.contains('/')
                || target.starts_with("command")
                || target.starts_with("a ")
            {
                continue;
            }
            // Strip trailing "for me", "please", "app", "application"
            let cleaned = target
                .trim_end_matches(" for me")
                .trim_end_matches(" please")
                .trim_end_matches(" app")
                .trim_end_matches(" application")
                .trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

/// Fast-answer queries that don't need the LLM.
fn try_fast_answer(query: &str) -> Option<String> {
    let q = query.trim().to_lowercase();

    // Time queries
    if q.contains("what time") || q.contains("current time") || q == "time" {
        let now = chrono::Local::now();
        return Some(format!("The current time is **{}**.", now.format("%I:%M:%S %p")));
    }

    // Date queries
    if q.contains("what date") || q.contains("today's date") || q.contains("what day")
        || q == "date"
    {
        let now = chrono::Local::now();
        return Some(format!(
            "Today is **{}**.",
            now.format("%A, %B %d, %Y")
        ));
    }

    // Combined time + date
    if q.contains("time and date") || q.contains("date and time") {
        let now = chrono::Local::now();
        return Some(format!(
            "It is **{}** on **{}**.",
            now.format("%I:%M:%S %p"),
            now.format("%A, %B %d, %Y")
        ));
    }

    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Observation truncation
// ─────────────────────────────────────────────────────────────────────────────

/// Truncate an observation to `MAX_OBSERVATION_LEN` chars.
fn truncate_observation(obs: &str) -> String {
    if obs.len() <= MAX_OBSERVATION_LEN {
        obs.to_string()
    } else {
        let truncated = &obs[..MAX_OBSERVATION_LEN];
        let remaining = obs.len() - MAX_OBSERVATION_LEN;
        format!("{truncated}\n[...truncated {remaining} chars...]")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Context compression
// ─────────────────────────────────────────────────────────────────────────────

/// Compress conversation context by dropping old OBSERVATION blocks.
fn compress_context(conversation: &str) -> String {
    if conversation.len() <= CONTEXT_COMPRESS_THRESHOLD {
        return conversation.to_string();
    }

    let mut result = String::new();
    let mut in_observation = false;
    let mut observation_count = 0;

    // Count total observations
    let total_observations = conversation.matches("OBSERVATION:").count();
    // Keep only the last 3 observations
    let keep_from = total_observations.saturating_sub(3);

    for line in conversation.lines() {
        if line.starts_with("OBSERVATION:") {
            observation_count += 1;
            if observation_count <= keep_from {
                result.push_str("OBSERVATION: [compressed — see recent observations]\n");
                in_observation = true;
                continue;
            }
            in_observation = false;
        } else if in_observation
            && (line.starts_with("THOUGHT:")
                || line.starts_with("ACTION:")
                || line.starts_with("ACTION_INPUT:")
                || line.starts_with("User:")
                || line.starts_with("SYSTEM:"))
        {
            in_observation = false;
        }

        if in_observation {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    // If still too long, hard-truncate from the front
    if result.len() > MAX_CONTEXT_LEN {
        let skip = result.len() - MAX_CONTEXT_LEN;
        format!("[...context truncated...]\n{}", &result[skip..])
    } else {
        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Answer quality gate
// ─────────────────────────────────────────────────────────────────────────────

/// Check if an answer is acceptable quality.
fn is_quality_answer(answer: &str, query: &str) -> bool {
    let a = answer.trim();

    // Too short
    if a.len() < MIN_ANSWER_LEN {
        return false;
    }

    // Echo detection — answer is just the query repeated
    if a.to_lowercase() == query.trim().to_lowercase() {
        return false;
    }

    // Refusal patterns
    let refusal_patterns = [
        "i cannot", "i can't", "i'm not able", "i am not able",
        "as an ai", "i don't have the ability",
    ];
    if refusal_patterns.iter().any(|p| a.to_lowercase().contains(p)) {
        // Only reject if the entire answer is a refusal (not just contains the phrase)
        if a.len() < 100 {
            return false;
        }
    }

    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Duplicate action detection
// ─────────────────────────────────────────────────────────────────────────────

/// Track actions to detect duplicates.
struct ActionTracker {
    history: Vec<(String, String)>, // (tool, input)
}

impl ActionTracker {
    fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    /// Returns true if this exact action+input was already executed.
    fn is_duplicate(&self, tool: &str, input: &str) -> bool {
        self.history.iter().any(|(t, i)| t == tool && i == input)
    }

    /// Record an action.
    fn record(&mut self, tool: &str, input: &str) {
        self.history.push((tool.to_string(), input.to_string()));
    }
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

/// The ReAct agent — drives the routing waterfall and THOUGHT → ACTION → OBSERVATION loop.
///
/// Routing waterfall:
/// 1. `try_fast_launch()` — "open Discord" → AppDiscovery resolve → system_control
/// 2. `try_fast_answer()` — time/date queries (no LLM needed)
/// 3. `run_react_loop()` — full ReAct loop with observation truncation,
///    context compression, answer quality gate, and duplicate action detection
pub struct ReActAgent {
    nexus: Arc<ModelNexus>,
    registry: ToolRegistry,
    app_handle: Option<AppHandle>,
    max_tokens: u32,
    health: std::sync::Mutex<ToolHealthTracker>,
    app_discovery: Option<Arc<std::sync::Mutex<AppDiscovery>>>,
    // ── Intelligence layer (Wave 3) ────────────────────────────────────
    distiller: ObservationDistiller,
    tool_memory: std::sync::Mutex<ToolOutcomeMemory>,
    execution_paths: ExecutionPathRouter,
    context_manager: ContextManager,
    // ── Polish layer (Wave 4) ────────────────────────────────────────
    tone_detector: ToneDetector,
    verbosity: VerbosityMode,
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
            app_discovery: None,
            distiller: ObservationDistiller::new(),
            tool_memory: std::sync::Mutex::new(ToolOutcomeMemory::new()),
            execution_paths: ExecutionPathRouter::new(),
            context_manager: ContextManager::new(),
            tone_detector: ToneDetector::new(),
            verbosity: VerbosityMode::Assistant,
        }
    }

    /// Set the verbosity mode.
    pub fn with_verbosity(mut self, mode: VerbosityMode) -> Self {
        self.verbosity = mode;
        self
    }

    /// Attach a Tauri AppHandle for emitting agent step events to the UI.
    pub fn with_app_handle(mut self, handle: AppHandle) -> Self {
        self.app_handle = Some(handle);
        self
    }

    /// Attach an AppDiscovery instance for fast launch resolution.
    pub fn with_app_discovery(mut self, discovery: Arc<std::sync::Mutex<AppDiscovery>>) -> Self {
        self.app_discovery = Some(discovery);
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

    /// Build the system prompt with tool descriptions, cognitive context,
    /// and optional tone/verbosity directives.
    fn build_system_prompt(
        tool_descriptions: &str,
        cognitive_context: &str,
        tone_directive: &str,
        verbosity_directive: &str,
    ) -> String {
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

        // Inject tone directive
        if !tone_directive.is_empty() {
            prompt.push_str(tone_directive);
        }

        // Inject verbosity directive
        if !verbosity_directive.is_empty() {
            prompt.push_str(verbosity_directive);
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

    // ── Routing Waterfall ───────────────────────────────────────────────

    /// Try fast launch: resolve app name via AppDiscovery and launch directly.
    fn try_fast_launch(&self, query: &str) -> Option<AgentResult> {
        let target = extract_launch_target(query)?;

        let discovery = self.app_discovery.as_ref()?;
        let disc = discovery.lock().ok()?;
        let resolve = disc.resolve(&target)?;

        info!(
            "Fast launch: '{}' → {} (tier={}, score={:.2})",
            target, resolve.entry.exe_path, resolve.tier, resolve.score
        );

        // We have a match — launch it via system_control
        drop(disc);

        Some(AgentResult {
            answer: format!("Launching **{}**...", resolve.entry.name),
            tools_used: vec!["system_control".to_string()],
            iterations: 0,
            routing_path: RoutingPath::FastLaunch,
        })
    }

    /// Execute the fast launch (actually start the program).
    async fn execute_fast_launch(&self, query: &str) -> Result<AgentResult> {
        let target = extract_launch_target(query)
            .ok_or_else(|| anyhow::anyhow!("No launch target"))?;

        let exe_path = {
            let discovery = self.app_discovery.as_ref()
                .ok_or_else(|| anyhow::anyhow!("No AppDiscovery"))?;
            let disc = discovery.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
            let resolve = disc.resolve(&target)
                .ok_or_else(|| anyhow::anyhow!("Could not resolve: {target}"))?;
            (resolve.entry.name.clone(), resolve.entry.exe_path.clone())
        };

        let (app_name, path) = exe_path;

        // Execute via system_control tool
        if let Some(tool) = self.registry.get("system_control") {
            let input = serde_json::json!({
                "action": "start_program",
                "name": path
            });
            match tool.execute(input).await {
                Ok(result) => {
                    self.health.lock().unwrap().record_success("system_control");
                    info!("Fast launch result: {result}");
                    Ok(AgentResult {
                        answer: format!("Launched **{app_name}**."),
                        tools_used: vec!["system_control".to_string()],
                        iterations: 0,
                        routing_path: RoutingPath::FastLaunch,
                    })
                }
                Err(e) => {
                    self.health.lock().unwrap().record_failure("system_control");
                    Ok(AgentResult {
                        answer: format!("Failed to launch {app_name}: {e}"),
                        tools_used: vec!["system_control".to_string()],
                        iterations: 0,
                        routing_path: RoutingPath::FastLaunch,
                    })
                }
            }
        } else {
            anyhow::bail!("system_control tool not registered")
        }
    }

    /// Main entry point — routes through the waterfall.
    ///
    /// 1. Try fast launch (AppDiscovery)
    /// 2. Try fast answer (time/date)
    /// 3. Full ReAct loop
    pub async fn run(&self, user_query: &str, cognitive_context: &str) -> Result<String> {
        let result = self.run_with_result(user_query, cognitive_context).await?;
        Ok(result.answer)
    }

    /// Main entry point returning full [`AgentResult`] with metadata.
    pub async fn run_with_result(
        &self,
        user_query: &str,
        cognitive_context: &str,
    ) -> Result<AgentResult> {
        // Tier 1: Fast launch (AppDiscovery)
        if self.try_fast_launch(user_query).is_some() {
            info!("Routing: fast_launch for '{}'", &user_query[..user_query.len().min(50)]);
            return self.execute_fast_launch(user_query).await;
        }

        // Tier 2: Fast answer (time, date — no LLM)
        if let Some(answer) = try_fast_answer(user_query) {
            info!("Routing: fast_answer for '{}'", &user_query[..user_query.len().min(50)]);
            self.emit_step(0, "final_answer", &answer);
            return Ok(AgentResult {
                answer,
                tools_used: vec![],
                iterations: 0,
                routing_path: RoutingPath::FastAnswer,
            });
        }

        // Tier 3: Execution paths (deterministic multi-step workflows)
        if let Some(result) = self.try_execution_path(user_query).await {
            return Ok(result);
        }

        // Tier 4: Full ReAct loop
        info!("Routing: react_loop for '{}'", &user_query[..user_query.len().min(50)]);
        self.run_react_loop(user_query, cognitive_context).await
    }

    /// Try to match the query against deterministic execution paths.
    async fn try_execution_path(&self, query: &str) -> Option<AgentResult> {
        use crate::agent::execution_paths::OnFail;

        let matched = self.execution_paths.match_path(query)?;
        let path_name = matched.path_name;

        info!(
            "Routing: execution_path '{}' for '{}'",
            path_name,
            &query[..query.len().min(50)]
        );

        let mut tools_used = Vec::new();
        let mut results: HashMap<String, String> = HashMap::new();

        for step in &matched.steps {
            let tool_name = step.tool;
            let params = (step.build_params)(&matched.params, &results);

            if let Some(tool) = self.registry.get(tool_name) {
                if step.delay_ms > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(step.delay_ms)).await;
                }

                let mut attempts = 0u32;

                loop {
                    match tool.execute(params.clone()).await {
                        Ok(result) => {
                            self.health.lock().unwrap().record_success(tool_name);
                            if let Ok(mut mem) = self.tool_memory.lock() {
                                let action_type = crate::agent::tool_memory::classify_action_type(query);
                                mem.record_success(tool_name, action_type);
                            }
                            if !tools_used.contains(&tool_name.to_string()) {
                                tools_used.push(tool_name.to_string());
                            }

                            // Extract result if there's an extract_key
                            if let Some(key) = step.extract_key {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                                    let extracted = json
                                        .get(key)
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(&result)
                                        .to_string();
                                    results.insert(key.to_string(), extracted);
                                } else {
                                    results.insert(key.to_string(), result.clone());
                                }
                            }
                            results.insert(format!("{tool_name}_result"), result);
                            break;
                        }
                        Err(e) => {
                            attempts += 1;
                            self.health.lock().unwrap().record_failure(tool_name);
                            if let Ok(mut mem) = self.tool_memory.lock() {
                                let action_type = crate::agent::tool_memory::classify_action_type(query);
                                mem.record_failure(tool_name, action_type, &e.to_string());
                            }

                            if attempts > step.max_retries {
                                match step.on_fail {
                                    OnFail::Skip => {
                                        results.insert(
                                            format!("{tool_name}_result"),
                                            format!("Error: {e}"),
                                        );
                                        break;
                                    }
                                    OnFail::Stop | OnFail::Retry => {
                                        warn!(
                                            "Execution path '{}' aborted at step {}: {e}",
                                            path_name, tool_name
                                        );
                                        return None;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                warn!("Execution path '{}': tool '{}' not found", path_name, tool_name);
                return None;
            }
        }

        // Use the last tool's result as the answer
        let last_tool = matched.steps.last().map(|s| s.tool).unwrap_or("");
        let answer = results
            .get(&format!("{last_tool}_result"))
            .cloned()
            .unwrap_or_else(|| "Completed.".to_string());

        self.emit_step(0, "final_answer", &answer);
        Some(AgentResult {
            answer,
            tools_used,
            iterations: 0,
            routing_path: RoutingPath::ExecutionPath(path_name.to_string()),
        })
    }

    /// Run the full ReAct loop with all enhancements.
    async fn run_react_loop(
        &self,
        user_query: &str,
        cognitive_context: &str,
    ) -> Result<AgentResult> {
        let tool_block = self.registry.describe_all();

        // Inject tool memory hints into cognitive context
        let tool_hints = {
            self.tool_memory.lock().ok()
                .map(|mem| mem.get_hints(user_query))
                .unwrap_or_default()
        };
        let enriched_context = if tool_hints.is_empty() {
            cognitive_context.to_string()
        } else {
            format!("{cognitive_context}\n{tool_hints}")
        };

        // Tone and verbosity directives
        let tone_directive = self.tone_detector.tone_directive(user_query);
        let verbosity_directive = self.verbosity.directive();

        let system_prompt = Self::build_system_prompt(
            &tool_block,
            &enriched_context,
            tone_directive,
            verbosity_directive,
        );
        let temperature = adaptive_temperature(user_query);

        let mut conversation = format!("User: {user_query}\n");
        let mut tools_used = Vec::new();
        let mut action_tracker = ActionTracker::new();
        let mut quality_gate = AnswerQualityGate::new();

        for step in 0..MAX_STEPS {
            info!("ReAct step {}/{MAX_STEPS} (temp={temperature})", step + 1);

            // Context compression via ContextManager
            conversation = self.context_manager.compress(&conversation);

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
                    return Ok(AgentResult {
                        answer: response.trim().to_string(),
                        tools_used,
                        iterations: step + 1,
                        routing_path: RoutingPath::ReactLoop,
                    });
                }
            };

            match parsed {
                ReActStep::FinalAnswer { thought, answer } => {
                    info!("ReAct final thought: {thought}");
                    self.emit_step(step, "thought", &thought);
                    self.emit_thought(step, &thought);

                    // Answer quality gate
                    match quality_gate.check(&answer, user_query) {
                        QualityVerdict::Accept => {}
                        QualityVerdict::Reject { reason, reprompt } => {
                            if step < MAX_STEPS - 1 {
                                warn!("Quality gate: answer rejected ({reason}), re-prompting");
                                conversation.push_str(&response);
                                conversation.push_str(&format!("\n{reprompt}\n"));
                                continue;
                            }
                            // Last step — accept anyway
                        }
                    }

                    self.emit_step(step, "final_answer", &answer);
                    return Ok(AgentResult {
                        answer,
                        tools_used,
                        iterations: step + 1,
                        routing_path: RoutingPath::ReactLoop,
                    });
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

                    // Duplicate action detection
                    if action_tracker.is_duplicate(&action, &action_input) {
                        warn!("Duplicate action detected: {action}({action_input})");
                        conversation.push_str(&response);
                        conversation.push_str(
                            "\nOBSERVATION: SYSTEM_ALERT: You already executed this exact action. \
                             The result was the same. Try a different approach or provide FINAL_ANSWER.\n"
                        );
                        continue;
                    }
                    action_tracker.record(&action, &action_input);

                    // Check if the tool is offline (fallback cascade).
                    let is_offline = {
                        let h = self.health.lock().unwrap();
                        h.is_offline(&action)
                    };

                    let action_type = crate::agent::tool_memory::classify_action_type(user_query);

                    // Look up the tool.
                    let observation = if is_offline {
                        let mut h = self.health.lock().unwrap();
                        if let Ok(mut mem) = self.tool_memory.lock() {
                            mem.record_failure(&action, action_type, "tool is offline");
                        }
                        h.handle_failure(&action, "tool is offline")
                    } else if let Some(tool) = self.registry.get(&action) {
                        // Parse the action input as JSON.
                        let input: serde_json::Value =
                            serde_json::from_str(&action_input).unwrap_or_else(|_| {
                                serde_json::json!({"query": action_input})
                            });

                        match tool.execute(input).await {
                            Ok(result) => {
                                // Record success
                                self.health.lock().unwrap().record_success(&action);
                                if let Ok(mut mem) = self.tool_memory.lock() {
                                    mem.record_success(&action, action_type);
                                }
                                if !tools_used.contains(&action) {
                                    tools_used.push(action.clone());
                                }
                                result
                            }
                            Err(e) => {
                                // Record failure
                                let mut h = self.health.lock().unwrap();
                                if let Ok(mut mem) = self.tool_memory.lock() {
                                    mem.record_failure(&action, action_type, &e.to_string());
                                }
                                h.handle_failure(&action, &e.to_string())
                            }
                        }
                    } else {
                        format!("Unknown tool: \"{action}\". Available tools: {:?}", self.registry.names())
                    };

                    // Distill long observations via ObservationDistiller
                    let distilled = self.distiller.distill(&action, &observation, user_query);
                    // Then truncate if still too long
                    let truncated = truncate_observation(&distilled);

                    info!("Observation: {}", &truncated[..truncated.len().min(200)]);
                    self.emit_step(step, "observation", &truncated);

                    // Append the full step to conversation for next iteration.
                    conversation.push_str(&response);
                    conversation.push_str(&format!("\nOBSERVATION: {truncated}\n"));
                }
            }
        }

        warn!("ReAct: max steps reached, forcing final answer");
        // Save tool memory before final answer
        if let Ok(mut mem) = self.tool_memory.lock() {
            mem.save();
        }

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
        Ok(AgentResult {
            answer: final_response.trim().to_string(),
            tools_used,
            iterations: MAX_STEPS,
            routing_path: RoutingPath::ReactLoop,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Parse tests ─────────────────────────────────────────────────────

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

    // ─── Adaptive temperature tests ─────────────────────────────────────

    #[test]
    fn test_adaptive_temperature_precise() {
        assert!((adaptive_temperature("open notepad") - 0.2).abs() < f32::EPSILON);
        assert!((adaptive_temperature("kill chrome") - 0.2).abs() < f32::EPSILON);
        assert!((adaptive_temperature("what time is it") - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_adaptive_temperature_creative() {
        assert!((adaptive_temperature("write me a poem") - 0.7).abs() < f32::EPSILON);
        assert!((adaptive_temperature("tell me a joke") - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_adaptive_temperature_general() {
        assert!((adaptive_temperature("hello how are you") - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn test_adaptive_temperature_new_patterns() {
        assert!((adaptive_temperature("set volume to 50") - 0.2).abs() < f32::EPSILON);
        assert!((adaptive_temperature("take screenshot") - 0.2).abs() < f32::EPSILON);
    }

    // ─── System prompt tests ────────────────────────────────────────────

    #[test]
    fn test_build_system_prompt_includes_context() {
        let prompt = ReActAgent::build_system_prompt(
            "- **shell**: run commands",
            "Recent memory: user likes Python",
            "",
            "",
        );
        assert!(prompt.contains("Cognitive Context"));
        assert!(prompt.contains("user likes Python"));
    }

    #[test]
    fn test_build_system_prompt_no_context() {
        let prompt = ReActAgent::build_system_prompt("- **shell**: run commands", "", "", "");
        assert!(!prompt.contains("Cognitive Context"));
    }

    #[test]
    fn test_build_system_prompt_with_tone() {
        let prompt = ReActAgent::build_system_prompt(
            "- **shell**: run commands",
            "",
            "\nTONE DIRECTIVE: frustrated user",
            "",
        );
        assert!(prompt.contains("TONE DIRECTIVE"));
    }

    #[test]
    fn test_build_system_prompt_with_verbosity() {
        let prompt = ReActAgent::build_system_prompt(
            "- **shell**: run commands",
            "",
            "",
            "\nVERBOSITY: Terminal mode.",
        );
        assert!(prompt.contains("VERBOSITY"));
    }

    // ─── Fast path tests ────────────────────────────────────────────────

    #[test]
    fn test_extract_launch_open_discord() {
        assert_eq!(extract_launch_target("open Discord"), Some("discord".to_string()));
    }

    #[test]
    fn test_extract_launch_start_chrome() {
        assert_eq!(extract_launch_target("start chrome"), Some("chrome".to_string()));
    }

    #[test]
    fn test_extract_launch_case_insensitive() {
        assert_eq!(extract_launch_target("LAUNCH Spotify"), Some("spotify".to_string()));
    }

    #[test]
    fn test_extract_launch_strip_suffixes() {
        assert_eq!(extract_launch_target("open discord for me"), Some("discord".to_string()));
        assert_eq!(extract_launch_target("open discord please"), Some("discord".to_string()));
    }

    #[test]
    fn test_extract_launch_no_url() {
        assert!(extract_launch_target("open https://google.com").is_none());
    }

    #[test]
    fn test_extract_launch_no_path() {
        assert!(extract_launch_target("open C:\\Windows\\notepad.exe").is_none());
    }

    #[test]
    fn test_extract_launch_no_match() {
        assert!(extract_launch_target("what time is it").is_none());
    }

    #[test]
    fn test_fast_answer_time() {
        let result = try_fast_answer("what time is it");
        assert!(result.is_some());
        assert!(result.unwrap().contains("current time"));
    }

    #[test]
    fn test_fast_answer_date() {
        let result = try_fast_answer("what date is it");
        assert!(result.is_some());
        assert!(result.unwrap().contains("Today is"));
    }

    #[test]
    fn test_fast_answer_time_and_date() {
        let result = try_fast_answer("time and date");
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("on"));
    }

    #[test]
    fn test_fast_answer_no_match() {
        assert!(try_fast_answer("open discord").is_none());
        assert!(try_fast_answer("search for rust programming").is_none());
    }

    // ─── Observation truncation tests ───────────────────────────────────

    #[test]
    fn test_truncate_short_observation() {
        let obs = "Short observation.";
        assert_eq!(truncate_observation(obs), obs);
    }

    #[test]
    fn test_truncate_long_observation() {
        let obs = "a".repeat(3000);
        let result = truncate_observation(&obs);
        assert!(result.len() < 3000);
        assert!(result.contains("[...truncated"));
        assert!(result.contains("1000 chars...]"));
    }

    #[test]
    fn test_truncate_exact_boundary() {
        let obs = "a".repeat(MAX_OBSERVATION_LEN);
        assert_eq!(truncate_observation(&obs), obs);
    }

    // ─── Context compression tests ──────────────────────────────────────

    #[test]
    fn test_compress_short_context_unchanged() {
        let ctx = "User: hello\nTHOUGHT: thinking\n";
        assert_eq!(compress_context(ctx), ctx);
    }

    #[test]
    fn test_compress_drops_old_observations() {
        // Build a context with 5 observations
        let mut ctx = String::from("User: test\n");
        for i in 0..5 {
            ctx.push_str(&format!("THOUGHT: step {i}\n"));
            ctx.push_str(&format!("ACTION: tool_{i}\n"));
            let obs = "x".repeat(3000);
            ctx.push_str(&format!("OBSERVATION: {obs}\n"));
        }

        assert!(ctx.len() > CONTEXT_COMPRESS_THRESHOLD);
        let compressed = compress_context(&ctx);
        assert!(compressed.len() < ctx.len());
        // Should contain the word "compressed"
        assert!(compressed.contains("compressed"));
    }

    // ─── Quality gate tests ─────────────────────────────────────────────

    #[test]
    fn test_quality_good_answer() {
        assert!(is_quality_answer("The capital of France is Paris.", "what is the capital of France"));
    }

    #[test]
    fn test_quality_too_short() {
        assert!(!is_quality_answer("Yes", "what is quantum computing"));
    }

    #[test]
    fn test_quality_echo_rejected() {
        assert!(!is_quality_answer("what time is it", "what time is it"));
    }

    #[test]
    fn test_quality_refusal_rejected() {
        assert!(!is_quality_answer("I cannot help with that.", "open notepad"));
    }

    #[test]
    fn test_quality_long_refusal_accepted() {
        // Long answers that happen to contain refusal phrases should be OK
        let long_answer = format!("I cannot tell you the exact result because the process is still running, but here's what I found so far: {}", "a".repeat(100));
        assert!(is_quality_answer(&long_answer, "check status"));
    }

    // ─── Action tracker tests ───────────────────────────────────────────

    #[test]
    fn test_action_tracker_no_duplicates_initially() {
        let tracker = ActionTracker::new();
        assert!(!tracker.is_duplicate("shell", "{\"command\": \"ls\"}"));
    }

    #[test]
    fn test_action_tracker_detects_duplicate() {
        let mut tracker = ActionTracker::new();
        tracker.record("shell", "{\"command\": \"ls\"}");
        assert!(tracker.is_duplicate("shell", "{\"command\": \"ls\"}"));
    }

    #[test]
    fn test_action_tracker_different_input_not_duplicate() {
        let mut tracker = ActionTracker::new();
        tracker.record("shell", "{\"command\": \"ls\"}");
        assert!(!tracker.is_duplicate("shell", "{\"command\": \"pwd\"}"));
    }

    #[test]
    fn test_action_tracker_different_tool_not_duplicate() {
        let mut tracker = ActionTracker::new();
        tracker.record("shell", "{\"command\": \"ls\"}");
        assert!(!tracker.is_duplicate("file_search", "{\"command\": \"ls\"}"));
    }

    // ─── Routing path display tests ─────────────────────────────────────

    #[test]
    fn test_routing_path_display() {
        assert_eq!(format!("{}", RoutingPath::FastLaunch), "fast_launch");
        assert_eq!(format!("{}", RoutingPath::FastAnswer), "fast_answer");
        assert_eq!(format!("{}", RoutingPath::ExecutionPath("get_time".to_string())), "execution_path:get_time");
        assert_eq!(format!("{}", RoutingPath::ReactLoop), "react_loop");
    }

    // ─── Fallback cascade tests ─────────────────────────────────────────

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
