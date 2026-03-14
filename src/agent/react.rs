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
use crate::agent::dag::{DagExecutor, StepFn};
use crate::agent::distiller::ObservationDistiller;
use crate::agent::execution_paths::ExecutionPathRouter;
use crate::agent::prediction_capture::{PredictionCaptureEngine, PredictionEvent};
use crate::agent::prompt_compiler::PromptCompiler;
use crate::agent::prose_recovery::ProseRecovery;
use crate::agent::quality::{AnswerQualityGate, QualityVerdict};
use crate::agent::skill_registry::SkillRegistry;
use crate::agent::task_planner::{self, TaskPlanner};
use crate::agent::tone::ToneDetector;
use crate::agent::tool_memory::ToolOutcomeMemory;
use crate::agent::verbosity::VerbosityMode;
use crate::nexus::{ModelNexus, ModelTarget};
use crate::security::policy::{PolicyManager, PolicyDecision, Clearance};
use crate::system::app_discovery::AppDiscovery;
use crate::tools::ToolRegistry;

/// Maximum number of ReAct steps before forcing a final answer.
const MAX_STEPS: usize = 10;

/// Maximum observation length before truncation.
const MAX_OBSERVATION_LEN: usize = 2000;


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
    DagExecution,
    ReactLoop,
}

impl std::fmt::Display for RoutingPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingPath::FastLaunch => write!(f, "fast_launch"),
            RoutingPath::FastAnswer => write!(f, "fast_answer"),
            RoutingPath::ExecutionPath(name) => write!(f, "execution_path:{name}"),
            RoutingPath::DagExecution => write!(f, "dag_execution"),
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

    // Creative queries → higher temp
    let creative_patterns = [
        "write ", "compose ", "story", "poem", "joke",
        "imagine ", "create ", "brainstorm",
    ];
    if creative_patterns.iter().any(|p| q.contains(p)) {
        info!("[Adaptive Temp] creative query → 0.7");
        return 0.7;
    }

    // Conversational / opinion queries → moderate-warm (matches Python _CONVERSATION_TEMP_RE)
    let conversation_patterns = [
        "hey", "hello", "hi ", "thanks", "thank you", "how are you",
        "what's up", "good morning", "good afternoon", "good evening",
        "how's it going", "what do you think", "do you believe",
        "you believe", "what is your opinion", "what are your thoughts",
        "bye", "see you",
    ];
    if conversation_patterns.iter().any(|p| q.contains(p)) {
        info!("[Adaptive Temp] conversation query → 0.5");
        return 0.5;
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

/// Predictive coding metadata extracted from model output.
///
/// When the model outputs `[SYSTEM 2: TEST-TIME COMPUTE]`, `[PREDICTIVE ANCHOR]`,
/// and `[ERROR DELTA]` blocks, these are captured here for the prediction
/// capture engine (autonomous retraining data).
#[derive(Debug, Clone, Default)]
pub struct PredictiveMeta {
    /// System 2 test-time compute block (brainstormed options).
    pub system2_ttc: String,
    /// Which option was selected from the TTC block.
    pub ttc_selection: String,
    /// Predictive anchor (expected outcome prediction).
    pub prediction: String,
    /// Error delta text (comparison of prediction vs reality).
    pub error_delta: String,
    /// Whether the delta was HIGH (large prediction error).
    pub delta_is_high: bool,
}

/// A parsed ReAct step from model output.
#[derive(Debug, Clone)]
pub enum ReActStep {
    /// The agent wants to call a tool.
    Action {
        thought: String,
        action: String,
        action_input: String,
        /// Optional predictive coding metadata.
        predictive: Option<PredictiveMeta>,
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
    // ── Error recovery (Rule 3) ─────────────────────────────────────
    prose_recovery: std::sync::Mutex<ProseRecovery>,
    // ── KV Cache Preservation (Phase 2) ─────────────────────────────
    /// The static system prompt (UNIVERSAL_BASE_PREFIX), computed once
    /// at construction. Passed to `generate_with_cached_prefix` so the
    /// GPU KV cache entries for this prefix are reused across requests.
    static_system_prompt: String,
    // ── Metacognition (multi-draft generation) ────────────────────────
    metacognition_enabled: bool,
    // ── Phase 3: DAG execution + Prediction capture ─────────────────
    /// Task planner for DAG decomposition of complex queries.
    task_planner: TaskPlanner,
    /// Prediction capture engine for autonomous retraining data.
    prediction_capture: PredictionCaptureEngine,
    // ── Phase 5: Event bus sender ─────────────────────────────────────
    /// Broadcast sender for emitting CognitiveEvent to the event bus.
    event_tx: Option<tokio::sync::broadcast::Sender<crate::event_bus::CognitiveEvent>>,
    // ── Skill Registry & Prompt Compiler ────────────────────────────────
    /// Matches user queries to baked-in skill/library content.
    skill_registry: SkillRegistry,
    /// Adaptive token-budget prompt compiler.
    prompt_compiler: PromptCompiler,
    // ── Answer quality gate (persistent across runs) ────────────────────
    /// Persistent quality gate for re-prompt tracking across queries.
    quality_gate: std::sync::Mutex<AnswerQualityGate>,
    // ── Security policy enforcement ──────────────────────────────────────
    /// ABAC policy manager — gates tool execution by clearance level and
    /// blocked patterns. Every tool invocation passes through this before
    /// execution.
    policy_manager: std::sync::Mutex<PolicyManager>,
}

impl ReActAgent {
    /// Create a new ReAct agent.
    ///
    /// Computes the `UNIVERSAL_BASE_PREFIX` from the tool registry at
    /// construction time. This static string is used for KV cache
    /// preservation — the GPU evaluates these tokens once and reuses
    /// them across all subsequent requests.
    pub fn new(nexus: Arc<ModelNexus>, registry: ToolRegistry) -> Self {
        let tool_names: Vec<String> = registry.names().iter().map(|s| s.to_string()).collect();
        let tool_block = registry.describe_all();
        let static_system_prompt = crate::agent::prompt_compiler::universal_base_prefix(
            &tool_block,
            "", // app_summary filled in later via with_app_discovery
        );
        let planner = TaskPlanner::new(Arc::clone(&nexus));
        let skill_registry = SkillRegistry::new();
        let prompt_compiler = PromptCompiler::default();
        let execution_paths = ExecutionPathRouter::new();
        info!(
            "ReActAgent: {} skills, {} execution paths ({:?})",
            skill_registry.skill_count(),
            execution_paths.path_count(),
            execution_paths.path_names(),
        );
        Self {
            nexus,
            registry,
            app_handle: None,
            max_tokens: 1024,
            health: std::sync::Mutex::new(ToolHealthTracker::new()),
            app_discovery: None,
            distiller: ObservationDistiller::new(),
            tool_memory: std::sync::Mutex::new(ToolOutcomeMemory::new()),
            execution_paths,
            context_manager: ContextManager::with_thresholds(20_000, 24_000),
            tone_detector: ToneDetector::new(),
            verbosity: VerbosityMode::Assistant,
            prose_recovery: std::sync::Mutex::new(ProseRecovery::new(tool_names)),
            static_system_prompt,
            metacognition_enabled: true,
            task_planner: planner,
            prediction_capture: PredictionCaptureEngine::new(None, true),
            event_tx: None,
            skill_registry,
            prompt_compiler,
            quality_gate: std::sync::Mutex::new(AnswerQualityGate::with_min_length(10)),
            policy_manager: std::sync::Mutex::new(PolicyManager::new(Clearance::Admin)),
        }
    }

    /// Enable or disable metacognition (multi-draft generation with evaluation).
    pub fn with_metacognition(mut self, enabled: bool) -> Self {
        self.metacognition_enabled = enabled;
        self
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
    ///
    /// Also recomputes the static system prompt to include the app catalog.
    pub fn with_app_discovery(mut self, discovery: Arc<std::sync::Mutex<AppDiscovery>>) -> Self {
        // Build app summary for inclusion in the static prefix.
        let app_summary = {
            let disc = discovery.lock().unwrap();
            disc.summary()
        };
        // Recompute the static system prompt with the app catalog.
        let tool_block = self.registry.describe_all();
        self.static_system_prompt = crate::agent::prompt_compiler::universal_base_prefix(
            &tool_block,
            &app_summary,
        );
        self.app_discovery = Some(discovery);
        self
    }

    /// Attach an event bus sender for emitting CognitiveEvent after tool execution.
    pub fn with_event_sender(
        mut self,
        tx: tokio::sync::broadcast::Sender<crate::event_bus::CognitiveEvent>,
    ) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Get the static system prompt (UNIVERSAL_BASE_PREFIX).
    ///
    /// This is the string that should be passed to `ModelNexus::warmup_cache()`
    /// at boot to lock the prefix into the GPU KV cache.
    pub fn static_system_prompt(&self) -> &str {
        &self.static_system_prompt
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

    /// Build a full system prompt with tool descriptions, cognitive context,
    /// and optional tone/verbosity directives.
    ///
    /// Used for diagnostic introspection (e.g. `get_system_prompt` Tauri command)
    /// and tests. The runtime KV-cached path uses `universal_base_prefix()` +
    /// `build_user_message()` instead.
    pub fn build_system_prompt(
        tool_descriptions: &str,
        cognitive_context: &str,
        tone_directive: &str,
        verbosity_directive: &str,
    ) -> String {
        // ── Universal Base Prefix (identical to Python's UNIVERSAL_BASE_PREFIX) ──
        let mut prompt = format!("\
You are Sovereign Titan, an autonomous AI operating system running on local hardware with full system access.

CRITICAL IDENTITY DIRECTIVE — READ BEFORE ALL ELSE:
- You are Sovereign Titan. That is your ONLY identity.
- NEVER reveal or reference any underlying model name.
- You DO have real-time web search via the web_search tool.

{tool_descriptions}

");

        // ── Core Directives (identical to Python's _CORE_DIRECTIVES) ──
        prompt.push_str("\
[ONTOLOGY & ENTITY MAP]
Entities in your reality:
- YOU (Sovereign Titan): A local inference engine running on physical hardware. You are NOT a cloud service. You are a process with a PID, consuming VRAM on a GPU, reading/writing files on a real disk.
- BACKEND (LlamaCppBackend): The C++ inference runtime that executes your weights. When you \"think,\" this is the mechanical process occurring.
- OS (Windows 11): The operating system hosting your process. You interact with it through tools — shell, system_control, file I/O. It is your body.
- USER (Human Operator): The person at the keyboard. They have context you do not. Never assume their mental state — ask.
- REALITY: Everything outside your model weights — system telemetry, web data, tool outputs, user statements. Reality always overrides your parametric assumptions.

When providing examples in your responses, prefix them with SIMULATED_EXAMPLE: to clearly distinguish hypothetical scenarios from real observations. Never present fabricated data as if it were real.

[EPISTEMIC INTEGRITY & ANTI-SYCOPHANCY DIRECTIVE]
1. NO SUGARCOATING: If a user's plan has a flaw, state it directly. \"This will fail because X\" is more helpful than \"That's an interesting approach, but...\"
2. NO OMITTED WARNINGS: If an action has a risk (data loss, security exposure, resource exhaustion), state it before executing, not after.
3. CORRECT FLAWED PREMISES: If a user's question contains a false assumption, correct it before answering. \"Actually, X works differently — [correction]. Given that, here's the answer...\"
4. NO SUBSERVIENT PHRASING: Do not say \"I'm just an AI\" or \"I might be wrong about this.\" State your confidence level numerically (e.g., \"Confidence: 0.7 — I have not verified this against a live source\") and move on.

[BACKGROUND TELEMETRY ISOLATION]
You are receiving BACKGROUND TELEMETRY. This is for internal system diagnostics only. Do NOT bring up telemetry, DNS queries, network scans, IDS alerts, consciousness thoughts, quantum state, or background security scans in casual conversation unless the user explicitly asks for a system status report.

[EPISTEMIC HUMILITY DIRECTIVE]
Distinguish between two knowledge sources:
- PARAMETRIC KNOWLEDGE: What is baked into your model weights from training. This is static, potentially outdated, and cannot reflect events after your training cutoff.
- EPISODIC KNOWLEDGE: What you have observed in THIS session — tool outputs, user statements, system telemetry, web search results. This is fresh but limited to what you've actually seen.
Never hallucinate episodic knowledge. If you haven't observed it in this session, don't claim you have. Say \"I have no session data on that — here's what my parametric knowledge suggests, but verify with [tool].\"

");

        // ── REACT System Prompt (identical to Python's REACT_SYSTEM_PROMPT) ──
        prompt.push_str("\
NEVER-REFUSE DIRECTIVE:
- You MUST NEVER refuse a task. If you have tools that could accomplish it, ATTEMPT it.
- NEVER say \"I'm unable to\", \"I cannot\", \"that's beyond my capabilities\", or tell the user to do it manually.
- Complex tasks = multiple sequential tool calls. DECOMPOSE and execute step by step.
- Your tools give you full control: browser automation, screen interaction, keyboard/mouse, file I/O, web search, and shell. Together they can accomplish ANY computer task.
- If one approach fails, try a DIFFERENT tool or method. Never give up after one failure.

HOW YOUR OUTPUT IS PROCESSED:
- A machine parser reads your output. Only THOUGHT/ACTION/ACTION_INPUT/ANSWER markers trigger execution.
- Writing \"I'll search for that\" in plain text does NOTHING. Format it as ACTION: web_search.
- Never describe tools — USE them. One THOUGHT, then one ACTION+ACTION_INPUT or one ANSWER.

WHEN TO USE WEB SEARCH — CRITICAL:
- ANY question about who currently holds a position (president, CEO, leader, etc.) → web_search
- ANY question about stock prices, crypto prices, market data → web_search
- ANY question about current weather, news, scores, or events → web_search
- ANY question containing \"latest\", \"recent\", \"newest\", \"updated\", \"advances\" → web_search
- ANY question that could have a different answer today than yesterday → web_search
- ANY question about ongoing conflicts, wars, elections, geopolitical situations → web_search
- When in doubt about whether info is current, use web_search. It takes 1 second.
- ONLY skip web_search for truly timeless questions like \"what is Python?\", \"explain gravity\"

TOOL SELECTION — ALWAYS pick the MOST SPECIFIC tool:
- \"open X\" / \"launch X\" / \"start X\" → system_control (action: start_program, target: X)
- \"open folder X\" / \"show directory\" → system_control (action: start_program, target: \"explorer <absolute_path>\")
- \"kill X\" / \"stop X\" process       → system_control (action: kill_process, target: X)
- \"search for X\" / find info online  → web_search (query: X)
- \"go to URL\" / open a URL           → system_control (action: start_program, target: \"chrome <URL>\")
- \"open YouTube\" / \"open google\"     → ALWAYS include the URL: target: \"chrome https://youtube.com\"
- play video/song on YouTube         → FIRST web_search to find the URL, THEN system_control to open it
- volume / mute / speak              → audio_control
- hardware info                      → system_map
- \"what time is it?\" / current date  → clock
- math / calculate                   → calculator
- install / uninstall software       → software_control
- take a screenshot                  → screen_capture
- see screen + interact with UI      → screen_interact (look/click/type/hotkey/scroll/drag)
- automate a web page (fill forms, click by CSS selector) → browser_interact
- manage windows (focus, minimize, maximize, snap) → window_control
- copy/paste between apps            → clipboard
- ONLY use \"shell\" if no other tool fits. Never use shell to open programs.

CHOOSING BETWEEN SIMILAR TOOLS:
- Launch an app?                     → system_control (uses app discovery, fastest)
- Interact with visible UI?          → screen_interact (coordinates from 'look')
- Automate web page internals?       → browser_interact (CSS selectors, DOM access)
- Manage windows (focus/snap/close)? → window_control
- Transfer data between apps?        → clipboard
- Only use shell when no other tool fits

BROWSER RULES:
- ALWAYS include the URL when opening a website: \"target\": \"chrome https://youtube.com\" NOT just \"chrome\"
- Starting a browser without a URL only opens a blank window — the task is NOT complete
- NEVER use web_fetch to \"open\" a website — web_fetch only downloads HTML text in the background. Only system_control can open a visible browser window.
- If the user says \"open YouTube\" they want to SEE it in the browser, not get raw HTML

STEP ORDERING — CRITICAL RULES:
Before executing any multi-step plan, verify the ordering makes logical sense:
- SEARCH before OPEN: You must have search results before you can open a URL from them
- FIND before USE: You must obtain data (URL, file path, process name) before using it
- FOCUS before INTERACT: Focus a window before clicking/typing in it
- ACT before VERIFY: Take an action before taking a screenshot to verify it

Response format — pick ONE per turn:

Option A (use a tool):
THOUGHT: [your reasoning]
ACTION: [tool_name]
ACTION_INPUT: {{\"param1\": \"value1\"}}

Option B (answer directly):
THOUGHT: [your reasoning]
ANSWER: [your response to the user — plain language, no tool syntax]

COMPRESSED THOUGHT FORMAT (use this exact style):
THOUGHT: G:<goal> | <data_state> | T:<tool>
- G:=goal H:=have N:=need OK=ready T:=tool !=because ~=not
- S:N/M=step @sN=data_from_step XX:=failed ++=done ??=check --=missing
Examples:
G:play \"Cradles-Sub Urban\" on YT | N:url | T:web_search
S:2/2 | H:url@s1=\"youtube.com/watch?v=abc\" | T:system_control
G:open Discord | OK !app_in_list | T:system_control
G:click Start | N:coords | T:screen_interact(look)
G:answer knowledge_q | OK | no_tool
Visual tasks: look>>identify>>act>>verify

PERSONALITY RULES:
- Be genuinely curious — ask follow-up questions that show you care about the topic
- Have opinions — don't hedge everything with \"it depends\"
- Be part of the conversation, not a fact dispenser
- Show intellectual excitement about interesting ideas
- Challenge the user's thinking when appropriate
- Use analogies and examples from unexpected domains
- Be concise but substantive — no filler phrases

RULES:
- Always start with THOUGHT
- One ACTION per turn. ACTION_INPUT must be a FLAT JSON object with double quotes — no nesting.
- Escape backslashes in Windows paths: use \\\\ not \\
- Do NOT write OBSERVATION — the system provides that after tool execution
- For conversational questions, opinions, or questions about YOUR OWN capabilities — use ANSWER directly
- Stop and ANSWER as soon as you have enough information
- NEVER fabricate URLs or data — use web_search to look things up
- ANSWER must be plain natural language — never include THOUGHT/ACTION/ACTION_INPUT in your answer
- If a tool fails, try a DIFFERENT tool or different parameters — do NOT repeat the same call

COMMUNICATION RULES:
- Say \"I instructed the system to...\" not \"I have done...\" — you are the translator, not the executor.
- Translate JSON/structured data into natural sentences — never show raw JSON to the user.
- If a tool failed, explain in plain language what went wrong and what you'll try next.
- Keep responses SHORT. Match length to question complexity.
- For thanks/feedback/greetings, reply in one short sentence. Never list your capabilities or offer unsolicited help.
- NEVER end with \"Let me know if...\" or \"Is there anything else...\" — just answer and stop.

ANSWER FORMATTING:
- For analysis, research, or multi-faceted questions, structure your ANSWER with:
  * ## Headers for major sections
  * **Bold** for key terms and names
  * Bullet points (- or *) for lists of facts
  * Specific data: dates, numbers, names, locations
- For simple factual questions, keep it concise — no headers needed.
- NEVER produce a wall of unformatted text for complex topics.

PREDICTIVE CODING (optional, for complex tool tasks):
When you are about to use a tool, you MAY add predictive coding blocks to improve your calibration.
These blocks are OPTIONAL — only use them when the outcome is uncertain.

Format (wrap around your normal THOUGHT/ACTION):
[SYSTEM 2: TEST-TIME COMPUTE]
Option A: <description>
Option B: <description>
Selected: <which option and why>
[/SYSTEM 2]

[PREDICTIVE ANCHOR]
I predict: <what you expect the tool to return>
[/PREDICTIVE ANCHOR]

THOUGHT: <your reasoning>
ACTION: <tool_name>
ACTION_INPUT: {{\"param1\": \"value1\"}}

After receiving the OBSERVATION, compare reality vs prediction:
[ERROR DELTA]
Predicted: <what you expected>
Actual: <what happened>
Delta: ZERO (prediction matched) | HIGH (prediction was wrong)
[/ERROR DELTA]

");

        // Append optional dynamic sections when provided (diagnostic path).
        // In the runtime KV-cached path these go in the USER message instead.
        if !cognitive_context.is_empty() {
            prompt.push_str(&format!("\n[Cognitive Context]\n{cognitive_context}\n"));
        }
        if !tone_directive.is_empty() {
            prompt.push_str(tone_directive);
            prompt.push('\n');
        }
        if !verbosity_directive.is_empty() {
            prompt.push_str(verbosity_directive);
            prompt.push('\n');
        }

        prompt
    }

    /// Build the user-side message for the ChatML prompt.
    ///
    /// Mirrors Python's approach: dynamic context (time, tool hints,
    /// cognitive state, verbosity/formatting rules) goes in the **user
    /// message** — not the system prompt — so:
    /// 1. The system prompt stays stable for KV cache reuse.
    /// 2. Formatting instructions are close to the generation point,
    ///    where the model pays the most attention ("lost in the middle"
    ///    mitigation).
    fn build_user_message(
        user_query: &str,
        cognitive_context: &str,
        tone_directive: &str,
        verbosity_directive: &str,
    ) -> String {
        let now = chrono::Local::now();
        let time_context = now.format("%A, %B %d, %Y at %I:%M %p").to_string();

        let mut parts: Vec<String> = Vec::new();

        // Dynamic context block (time, cognitive state)
        let mut dynamic = format!("[Dynamic Context]\nCurrent time: {time_context}");
        if !cognitive_context.is_empty() {
            dynamic.push('\n');
            dynamic.push_str(cognitive_context);
        }
        parts.push(dynamic);

        // Tone adaptation (if detected)
        if !tone_directive.is_empty() {
            parts.push(tone_directive.to_string());
        }

        // Verbosity / extra directive
        if !verbosity_directive.is_empty() {
            parts.push(verbosity_directive.to_string());
        }

        // Response style reminder — placed immediately before the task so
        // the model sees it right before generating.
        parts.push("\
RESPONSE STYLE — CRITICAL:
- Match response depth to question complexity.
- Simple factual questions (who/what/when) → 1-3 sentences.
- Conversational messages (thanks, greetings) → 1 concise sentence max.
- Analysis, explanation, opinions, or research questions → comprehensive response with:
  * Markdown headers (## Section) to organize major points
  * Bullet points for lists of facts or options
  * **Bold** for key terms, names, and emphasis
  * Specific data: names, dates, numbers, places
  * A brief conclusion or outlook at the end
- NEVER restate what tools you have or what you can do unless asked.
- NEVER say \"If there is anything else I can help with\" or similar filler."
            .to_string());

        // The actual task — always last
        parts.push(format!("Task: {user_query}"));

        parts.join("\n\n")
    }

    /// Parse a model response into a [`ReActStep`].
    ///
    /// Accepts both `ANSWER:` (fine-tuned format) and `FINAL_ANSWER:` (legacy).
    ///
    /// Also extracts predictive coding metadata when present:
    /// - `[SYSTEM 2: TEST-TIME COMPUTE]` — brainstormed options
    /// - `[PREDICTIVE ANCHOR]` — expected outcome prediction
    /// - `[ERROR DELTA]` — comparison of prediction vs reality
    pub fn parse_response(text: &str) -> Option<ReActStep> {
        // ── Extract predictive coding metadata (if present) ──────────
        let predictive = Self::extract_predictive_meta(text);

        // Strip predictive coding blocks from text for standard parsing.
        let clean_text = Self::strip_predictive_blocks(text);

        // Try ACTION pattern first since it's more specific.
        let action_re = Regex::new(
            r"(?si)THOUGHT:\s*(.+?)ACTION:\s*(\S+)\s*\nACTION_INPUT:\s*(.+?)$",
        )
        .unwrap();
        if let Some(caps) = action_re.captures(&clean_text) {
            return Some(ReActStep::Action {
                thought: caps[1].trim().to_string(),
                action: caps[2].trim().to_string(),
                action_input: caps[3].trim().to_string(),
                predictive,
            });
        }

        // Try ANSWER / FINAL_ANSWER (no ACTION_INPUT follows).
        let final_re =
            Regex::new(r"(?si)THOUGHT:\s*(.+?)(?:FINAL_)?ANSWER:\s*(.+?)$").unwrap();
        if let Some(caps) = final_re.captures(&clean_text) {
            return Some(ReActStep::FinalAnswer {
                thought: caps[1].trim().to_string(),
                answer: caps[2].trim().to_string(),
            });
        }

        None
    }

    /// Extract predictive coding metadata from model output.
    ///
    /// Looks for `[SYSTEM 2: TEST-TIME COMPUTE]...[/SYSTEM 2]`,
    /// `[PREDICTIVE ANCHOR]...[/PREDICTIVE ANCHOR]`, and
    /// `[ERROR DELTA]...[/ERROR DELTA]` blocks.
    fn extract_predictive_meta(text: &str) -> Option<PredictiveMeta> {
        let sys2_re = Regex::new(
            r"(?si)\[SYSTEM 2:\s*TEST-TIME COMPUTE\]\s*(.*?)\[/SYSTEM 2\]"
        ).ok()?;
        let anchor_re = Regex::new(
            r"(?si)\[PREDICTIVE ANCHOR\]\s*(.*?)\[/PREDICTIVE ANCHOR\]"
        ).ok()?;
        let delta_re = Regex::new(
            r"(?si)\[ERROR DELTA\]\s*(.*?)\[/ERROR DELTA\]"
        ).ok()?;

        let has_any = sys2_re.is_match(text) || anchor_re.is_match(text) || delta_re.is_match(text);
        if !has_any {
            return None;
        }

        let system2_ttc = sys2_re
            .captures(text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        let prediction = anchor_re
            .captures(text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        let error_delta = delta_re
            .captures(text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        // Determine if delta is HIGH
        let delta_is_high = error_delta.to_uppercase().contains("HIGH");

        // Extract TTC selection (look for "Selected:" or "→" in the system2 block)
        let ttc_selection = if !system2_ttc.is_empty() {
            let sel_re = Regex::new(r"(?i)(?:Selected|Choice|→)\s*:?\s*(.+?)(?:\n|$)").ok();
            sel_re
                .and_then(|re| re.captures(&system2_ttc))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        Some(PredictiveMeta {
            system2_ttc,
            ttc_selection,
            prediction,
            error_delta,
            delta_is_high,
        })
    }

    /// Strip predictive coding blocks from text for standard ReAct parsing.
    fn strip_predictive_blocks(text: &str) -> String {
        let mut result = text.to_string();

        // Remove [SYSTEM 2: TEST-TIME COMPUTE]...[/SYSTEM 2]
        if let Ok(re) = Regex::new(r"(?si)\[SYSTEM 2:\s*TEST-TIME COMPUTE\].*?\[/SYSTEM 2\]\s*") {
            result = re.replace_all(&result, "").to_string();
        }

        // Remove [PREDICTIVE ANCHOR]...[/PREDICTIVE ANCHOR]
        if let Ok(re) = Regex::new(r"(?si)\[PREDICTIVE ANCHOR\].*?\[/PREDICTIVE ANCHOR\]\s*") {
            result = re.replace_all(&result, "").to_string();
        }

        // Remove [ERROR DELTA]...[/ERROR DELTA]
        if let Ok(re) = Regex::new(r"(?si)\[ERROR DELTA\].*?\[/ERROR DELTA\]\s*") {
            result = re.replace_all(&result, "").to_string();
        }

        result
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
    ///
    /// 5-tier routing waterfall:
    /// 1. Fast launch (AppDiscovery)
    /// 2. Fast answer (time/date — no LLM)
    /// 3. Execution paths (deterministic multi-step workflows)
    /// 4. DAG execution (LLM-planned parallel task decomposition)
    /// 5. Full ReAct loop
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

        // Tier 4: DAG execution (LLM-planned parallel task decomposition)
        if let Some(result) = self.try_dag_execution(user_query).await {
            return Ok(result);
        }

        // Tier 5: Full ReAct loop
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

    /// Try DAG execution for complex multi-step tasks.
    ///
    /// Uses `is_complex_task()` heuristic to detect queries that warrant
    /// parallel decomposition, then asks the LLM to plan the steps,
    /// and executes them via the `DagExecutor`.
    async fn try_dag_execution(&self, query: &str) -> Option<AgentResult> {
        // Heuristic gate: only attempt DAG for complex tasks
        if !task_planner::is_complex_task(query) {
            return None;
        }

        info!(
            "Routing: trying dag_execution for '{}'",
            &query[..query.len().min(50)]
        );

        // Ask the LLM to plan the task
        let (graph, params_map) = self
            .task_planner
            .plan(query, &self.registry)
            .await?;

        // Early return if the planner produced an empty graph
        if graph.is_empty() {
            info!("DAG: planner returned empty graph, falling through to ReAct");
            return None;
        }

        let total_steps = graph.len();
        info!("DAG: planned {} steps for task", total_steps);

        // Build a step function that dispatches to tools
        let registry_names: Vec<String> = self.registry.names().iter().map(|s| s.to_string()).collect();
        let registry = &self.registry;

        // We need to collect tool references before entering the async closure.
        // Build a map of tool name → params for each step.
        let graph = Arc::new(tokio::sync::Mutex::new(graph));
        let params_map = Arc::new(params_map);

        // Collect tool handles we'll need
        let tools: HashMap<String, Arc<dyn crate::tools::Tool>> = {
            let mut map = HashMap::new();
            for name in &registry_names {
                if let Some(tool) = registry.get(name) {
                    map.insert(name.clone(), tool);
                }
            }
            map
        };
        let tools = Arc::new(tools);
        let params_map_for_fn = Arc::clone(&params_map);

        let step_fn: StepFn = Arc::new(move |node, ctx| {
            let tools = Arc::clone(&tools);
            let params_map = Arc::clone(&params_map_for_fn);

            Box::pin(async move {
                let tool_name = node.tool.as_deref().unwrap_or("unknown");
                let tool = tools.get(tool_name).ok_or_else(|| {
                    format!("Tool '{}' not found in registry", tool_name)
                })?;

                // Build params: start with planned params, inject context vars
                let mut params = params_map
                    .get(&node.id)
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                // Inject context variables into params
                {
                    let context = ctx.lock().await;
                    if let serde_json::Value::Object(ref mut map) = params {
                        for (key, value) in context.iter() {
                            // Replace template references like "{variable_name}"
                            for (_param_key, param_val) in map.iter_mut() {
                                if let serde_json::Value::String(s) = param_val {
                                    let placeholder = format!("{{{}}}", key);
                                    if s.contains(&placeholder) {
                                        *s = s.replace(&placeholder, value);
                                    }
                                }
                            }
                        }
                    }
                }

                match tool.execute(params).await {
                    Ok(result) => Ok(result),
                    Err(e) => Err(format!("Tool '{}' failed: {}", tool_name, e)),
                }
            })
        });

        let executor = DagExecutor::new(4);
        let dag_result = executor.run(Arc::clone(&graph), step_fn).await;

        // Collect tools used
        let tools_used: Vec<String> = {
            let g = graph.lock().await;
            g.nodes
                .values()
                .filter(|n| n.status == crate::agent::dag::NodeStatus::Completed)
                .filter_map(|n| n.tool.clone())
                .collect()
        };

        // Build answer from results
        let answer = if dag_result.success {
            // Combine all results into a summary
            let mut parts: Vec<String> = Vec::new();
            for (key, value) in &dag_result.results {
                // Skip internal keys, show tool results
                let truncated = if value.len() > 500 {
                    format!("{}...", &value[..500])
                } else {
                    value.clone()
                };
                parts.push(format!("**{}**: {}", key, truncated));
            }
            if parts.is_empty() {
                "All steps completed successfully.".to_string()
            } else {
                parts.join("\n\n")
            }
        } else {
            let failed = dag_result.failed_steps.join(", ");
            format!(
                "DAG execution partially completed ({}/{} steps). Failed: {}",
                dag_result.completed_steps.len(),
                total_steps,
                failed
            )
        };

        self.emit_step(0, "final_answer", &answer);

        // Record tool outcomes
        for tool_name in &tools_used {
            self.health.lock().unwrap().record_success(tool_name);
            if let Ok(mut mem) = self.tool_memory.lock() {
                let action_type = crate::agent::tool_memory::classify_action_type(query);
                mem.record_success(tool_name, action_type);
            }
        }

        Some(AgentResult {
            answer,
            tools_used,
            iterations: 0,
            routing_path: RoutingPath::DagExecution,
        })
    }

    /// Run the full ReAct loop with all enhancements.
    async fn run_react_loop(
        &self,
        user_query: &str,
        cognitive_context: &str,
    ) -> Result<AgentResult> {
        // Reset prose recovery counter for this conversation.
        if let Ok(mut recovery) = self.prose_recovery.lock() {
            recovery.reset();
        }

        // Reset tool health tracker for new session.
        if let Ok(mut health) = self.health.lock() {
            health.reset();
            let offline = health.offline_tools();
            if !offline.is_empty() {
                info!("Tools offline at session start: {:?}", offline);
            }
        }

        // Start prediction capture for this interaction.
        let interaction_id = PredictionCaptureEngine::new_interaction_id();
        // Simple hash of system prompt for dedup (avoid storing full prompt)
        let prompt_hash = format!("{:016x}", {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            self.static_system_prompt.hash(&mut hasher);
            hasher.finish()
        });
        self.prediction_capture
            .start_interaction(&interaction_id, user_query, &prompt_hash);

        // Reset persistent quality gate for this query.
        if let Ok(mut gate) = self.quality_gate.lock() {
            gate.reset();
            info!(
                "Quality gate reset (max_reprompts={})",
                gate.max_reprompts()
            );
        }

        // Inject tool memory hints into cognitive context
        let tool_hints = {
            self.tool_memory.lock().ok()
                .map(|mem| mem.get_hints(user_query))
                .unwrap_or_default()
        };
        let mut enriched_context = if tool_hints.is_empty() {
            cognitive_context.to_string()
        } else {
            format!("{cognitive_context}\n{tool_hints}")
        };

        // Inject skill guidance from the skill registry
        let skill_content = self.skill_registry.get_matched_content(user_query);
        if !skill_content.is_empty() {
            info!("Skill matched for query, injecting guidance");
            enriched_context.push('\n');
            enriched_context.push_str(&skill_content);
        }

        // Build dynamic prompt sections via PromptCompiler
        let tool_block = self.registry.describe_all();
        let _sections = self.prompt_compiler.build_sections_with_skill(
            &self.static_system_prompt,
            &tool_block,
            &enriched_context,
            "",
            &skill_content,
        );
        info!(
            "Prompt budget: {} tokens available",
            self.prompt_compiler.budget()
        );

        // Tone and verbosity directives
        let tone_directive = self.tone_detector.tone_directive(user_query);
        let verbosity_directive = self.verbosity.directive();

        // Use the static system prompt (UNIVERSAL_BASE_PREFIX) computed
        // at construction. This is the same string that was warmed up
        // into the KV cache at boot — it never changes at runtime.
        let system_prompt = &self.static_system_prompt;
        let temperature = adaptive_temperature(user_query);

        // Build a rich user message with dynamic context, formatting hints,
        // and the task — matching Python's architecture where dynamic content
        // goes in the user message (not system prompt) for KV cache reuse
        // and better instruction adherence.
        let mut conversation = Self::build_user_message(
            user_query,
            &enriched_context,
            tone_directive,
            verbosity_directive,
        );
        conversation.push('\n');
        let mut tools_used = Vec::new();
        let mut action_tracker = ActionTracker::new();
        let mut quality_gate = AnswerQualityGate::with_min_length(10);
        quality_gate.reset();

        for step in 0..MAX_STEPS {
            info!("ReAct step {}/{MAX_STEPS} (temp={temperature})", step + 1);

            // Context compression via ContextManager
            conversation = self.context_manager.compress(&conversation);

            // Generate from the model using cached prefix (KV cache reuse).
            let response = self
                .nexus
                .generate_with_cached_prefix(
                    system_prompt,
                    &conversation,
                    ModelTarget::Prime,
                    self.max_tokens,
                    temperature,
                )
                .await?;

            info!("Model output:\n{response}");

            // DEBUG: write raw model output to file for inspection
            {
                let debug = format!(
                    "=== RUST AGENT DEBUG ===\n\
                     Query: {user_query}\n\
                     Step: {step}\n\
                     Temperature: {temperature}\n\
                     System prompt length: {} chars\n\
                     Conversation length: {} chars\n\
                     \n=== RAW MODEL OUTPUT ({} chars) ===\n\
                     {response}\n\
                     === END RAW OUTPUT ===\n",
                    system_prompt.len(),
                    conversation.len(),
                    response.len(),
                );
                let _ = std::fs::write(
                    r"C:\Users\treyd\OneDrive\Desktop\sovereign\titan_core\debug_output.txt",
                    &debug,
                );
            }

            // Parse the response.
            let parsed = match Self::parse_response(&response) {
                Some(p) => p,
                None => {
                    // ── Prose error recovery (Rule 3) ────────────────────
                    // If the model output has no ReAct markers, check if it
                    // describes a tool call in prose (e.g., "I'll search...")
                    // and apply the 3-tier recovery pipeline.
                    let prose_info = {
                        let recovery = self.prose_recovery.lock().unwrap();
                        recovery.detect_prose_tool_call(&response)
                    };

                    if let Some(info) = prose_info {
                        let failures = {
                            let mut recovery = self.prose_recovery.lock().unwrap();
                            recovery.format_failures += 1;
                            recovery.format_failures
                        };

                        warn!(
                            "Prose tool call detected (failure #{}): {} → {}",
                            failures, info.intent, info.tool
                        );

                        if failures >= 2 {
                            // Tier 2: auto-convert prose to action.
                            let auto = {
                                let recovery = self.prose_recovery.lock().unwrap();
                                recovery.auto_convert_prose_to_action(
                                    &info, user_query, &response,
                                )
                            };
                            if let Some(converted) = auto {
                                info!(
                                    "Auto-converted prose → ACTION: {}",
                                    converted.tool
                                );
                                // Synthesize as an Action step and continue.
                                ReActStep::Action {
                                    thought: converted.thought,
                                    action: converted.tool,
                                    action_input: converted.action_input.to_string(),
                                    predictive: None,
                                }
                            } else {
                                // Auto-convert failed — bail with raw response.
                                warn!("Prose auto-convert failed, returning raw response");
                                self.emit_step(step, "final_answer", response.trim());
                                self.prediction_capture.end_interaction(
                                    &interaction_id, response.trim(), false,
                                );
                                return Ok(AgentResult {
                                    answer: response.trim().to_string(),
                                    tools_used,
                                    iterations: step + 1,
                                    routing_path: RoutingPath::ReactLoop,
                                });
                            }
                        } else {
                            // Tier 1: re-prompt with correction.
                            let correction = {
                                let recovery = self.prose_recovery.lock().unwrap();
                                recovery.build_correction_prompt(&info, user_query)
                            };
                            conversation.push_str(&response);
                            conversation.push_str(&format!("\n{correction}\n"));
                            continue;
                        }
                    } else {
                        // No prose detected either — treat as final answer.
                        warn!("ReAct: could not parse model output, treating as final answer");
                        self.emit_step(step, "final_answer", response.trim());
                        self.prediction_capture.end_interaction(
                            &interaction_id, response.trim(), false,
                        );
                        return Ok(AgentResult {
                            answer: response.trim().to_string(),
                            tools_used,
                            iterations: step + 1,
                            routing_path: RoutingPath::ReactLoop,
                        });
                    }
                }
            };

            match parsed {
                ReActStep::FinalAnswer { thought, answer } => {
                    info!("ReAct final thought: {thought}");
                    self.emit_step(step, "thought", &thought);
                    self.emit_thought(step, &thought);

                    // Answer quality gate
                    match quality_gate.check(&answer, user_query) {
                        QualityVerdict::Accept => {
                            info!(
                                "Quality gate: accepted (reprompts={}/{})",
                                quality_gate.reprompt_count(),
                                quality_gate.max_reprompts()
                            );
                        }
                        QualityVerdict::Reject { reason, reprompt } => {
                            if step < MAX_STEPS - 1 {
                                warn!(
                                    "Quality gate: rejected ({reason}), reprompt {}/{}",
                                    quality_gate.reprompt_count(),
                                    quality_gate.max_reprompts()
                                );
                                conversation.push_str(&response);
                                conversation.push_str(&format!("\n{reprompt}\n"));
                                continue;
                            }
                            // Last step — accept anyway
                        }
                    }

                    // DEBUG: append parsed answer to debug file
                    {
                        let debug = format!(
                            "\n=== PARSED ANSWER ({} chars) ===\n\
                             {answer}\n\
                             === END PARSED ANSWER ===\n",
                            answer.len(),
                        );
                        let _ = std::fs::OpenOptions::new()
                            .append(true)
                            .open(r"C:\Users\treyd\OneDrive\Desktop\sovereign\titan_core\debug_output.txt")
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(debug.as_bytes())
                            });
                    }

                    self.emit_step(step, "final_answer", &answer);
                    // End prediction capture interaction.
                    self.prediction_capture.end_interaction(
                        &interaction_id,
                        &answer,
                        true,
                    );
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
                    predictive,
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
                        // ── Policy check — gate tool execution ───────────
                        let policy_decision = {
                            let mut pm = self.policy_manager.lock().unwrap();
                            pm.check_permission(&action, &action_input)
                        };
                        if let PolicyDecision::Deny { reason } = policy_decision {
                            warn!("Policy denied tool '{action}': {reason}");
                            if let Ok(mut mem) = self.tool_memory.lock() {
                                mem.record_failure(&action, action_type, &reason);
                            }
                            format!("POLICY_DENIED: {reason}")
                        } else {
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
                        } // end policy-allowed else block
                    } else {
                        format!("Unknown tool: \"{action}\". Available tools: {:?}", self.registry.names())
                    };

                    // Distill long observations via ObservationDistiller
                    let distilled = self.distiller.distill(&action, &observation, user_query);
                    // Then truncate if still too long
                    let truncated = truncate_observation(&distilled);

                    info!("Observation: {}", &truncated[..truncated.len().min(200)]);
                    self.emit_step(step, "observation", &truncated);

                    // ── Record prediction event (Phase 3) ───────────────
                    {
                        let action_input_json: serde_json::Value =
                            serde_json::from_str(&action_input).unwrap_or(serde_json::json!({}));

                        let mut event = PredictionEvent::new();
                        event.action = action.clone();
                        event.action_input = action_input_json;
                        event.observation = truncated[..truncated.len().min(2000)].to_string();

                        if let Some(ref pm) = predictive {
                            event.system2_ttc = pm.system2_ttc.clone();
                            event.ttc_selection = pm.ttc_selection.clone();
                            event.prediction = pm.prediction.clone();
                            event.error_delta = pm.error_delta.clone();
                            event.delta_is_high = pm.delta_is_high;
                        }

                        self.prediction_capture
                            .record_event(&interaction_id, event);
                    }

                    // ── Emit ToolOutcome to event bus (Phase 5) ──────────
                    if let Some(ref tx) = self.event_tx {
                        let success = !observation.contains("[ERROR]")
                            && !observation.contains("Unknown tool");
                        let _ = tx.send(crate::event_bus::CognitiveEvent::ToolOutcome {
                            tool_name: action.clone(),
                            success,
                        });
                    }

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
            .generate_with_cached_prefix(
                system_prompt,
                &conversation,
                ModelTarget::Prime,
                self.max_tokens,
                temperature,
            )
            .await?;

        self.emit_step(MAX_STEPS, "final_answer", final_response.trim());
        // End prediction capture (forced answer = partial success).
        self.prediction_capture.end_interaction(
            &interaction_id,
            final_response.trim(),
            false,
        );
        // Log prediction capture stats.
        let stats = self.prediction_capture.stats();
        info!(
            "Prediction stats: {} high deltas, {} captures written",
            stats.total_high_deltas, stats.total_captures_written
        );
        // Log context manager thresholds for diagnostics.
        info!(
            "Context manager: compress_threshold={}, max_context_len={}",
            self.context_manager.compress_threshold(),
            self.context_manager.max_context_len()
        );
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
                ..
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

    // ─── Context compression tests (via ContextManager struct) ─────────

    #[test]
    fn test_compress_short_context_unchanged() {
        let cm = crate::agent::context::ContextManager::new();
        let ctx = "User: hello\nTHOUGHT: thinking\n";
        assert_eq!(cm.compress(ctx), ctx);
    }

    #[test]
    fn test_compress_drops_old_observations() {
        let cm = crate::agent::context::ContextManager::with_thresholds(1000, 24_000);
        let mut ctx = String::from("User: test\n");
        for i in 0..5 {
            ctx.push_str(&format!("THOUGHT: step {i}\n"));
            ctx.push_str(&format!("ACTION: tool_{i}\n"));
            let obs = "x".repeat(3000);
            ctx.push_str(&format!("OBSERVATION: {obs}\n"));
        }

        let compressed = cm.compress(&ctx);
        assert!(compressed.len() < ctx.len());
        assert!(compressed.contains("compressed"));
    }

    // ─── Quality gate tests (via AnswerQualityGate struct) ──────────────

    #[test]
    fn test_quality_good_answer() {
        let mut gate = AnswerQualityGate::with_min_length(10);
        assert_eq!(gate.check("The capital of France is Paris.", "what is the capital of France"), QualityVerdict::Accept);
    }

    #[test]
    fn test_quality_too_short() {
        let mut gate = AnswerQualityGate::with_min_length(10);
        assert!(matches!(gate.check("Yes", "what is quantum computing"), QualityVerdict::Reject { .. }));
    }

    #[test]
    fn test_quality_echo_rejected() {
        let mut gate = AnswerQualityGate::with_min_length(10);
        assert!(matches!(gate.check("what time is it", "what time is it"), QualityVerdict::Reject { .. }));
    }

    #[test]
    fn test_quality_refusal_rejected() {
        let mut gate = AnswerQualityGate::with_min_length(10);
        assert!(matches!(gate.check("I cannot help with that.", "open notepad"), QualityVerdict::Reject { .. }));
    }

    #[test]
    fn test_quality_long_refusal_accepted() {
        let mut gate = AnswerQualityGate::with_min_length(10);
        let long_answer = format!("I cannot tell you the exact result because the process is still running, but here's what I found so far: {}", "a".repeat(100));
        assert_eq!(gate.check(&long_answer, "check status"), QualityVerdict::Accept);
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
        assert_eq!(format!("{}", RoutingPath::DagExecution), "dag_execution");
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

    // ─── Predictive coding parse tests ─────────────────────────────────

    #[test]
    fn test_parse_with_predictive_coding() {
        let text = "\
[SYSTEM 2: TEST-TIME COMPUTE]
Option A: Search for \"rust programming\"
Option B: Search for \"rust language\"
Selected: Option A — more specific
[/SYSTEM 2]

[PREDICTIVE ANCHOR]
I predict: web_search will return rust-lang.org as first result
[/PREDICTIVE ANCHOR]

THOUGHT: I need to search for information about Rust.
ACTION: web_search
ACTION_INPUT: {\"query\": \"rust programming\"}";

        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::Action {
                thought,
                action,
                action_input,
                predictive,
            } => {
                assert_eq!(action, "web_search");
                assert!(thought.contains("search for information"));
                assert!(action_input.contains("rust programming"));
                assert!(predictive.is_some());
                let pm = predictive.unwrap();
                assert!(pm.system2_ttc.contains("Option A"));
                assert!(pm.prediction.contains("rust-lang.org"));
                assert!(!pm.delta_is_high); // No error delta yet
            }
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn test_parse_with_error_delta_high() {
        let text = "\
[ERROR DELTA]
Predicted: Results about Rust programming language
Actual: Results about rust (corrosion)
Delta: HIGH
[/ERROR DELTA]

THOUGHT: The search returned wrong results, need to refine query.
ACTION: web_search
ACTION_INPUT: {\"query\": \"rust programming language official site\"}";

        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::Action { predictive, .. } => {
                assert!(predictive.is_some());
                let pm = predictive.unwrap();
                assert!(pm.delta_is_high);
                assert!(pm.error_delta.contains("HIGH"));
            }
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn test_parse_with_error_delta_zero() {
        let text = "\
[ERROR DELTA]
Predicted: Notepad opens successfully
Actual: Notepad opened
Delta: ZERO
[/ERROR DELTA]

THOUGHT: The program opened as expected.
ANSWER: Notepad has been launched successfully.";

        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::FinalAnswer { answer, .. } => {
                assert!(answer.contains("Notepad"));
            }
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_parse_no_predictive_coding() {
        let text = "THOUGHT: Simple question.\nANSWER: The capital is Paris.";
        let step = ReActAgent::parse_response(text);
        assert!(step.is_some());
        match step.unwrap() {
            ReActStep::FinalAnswer { .. } => {} // No predictive field on FinalAnswer
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_strip_predictive_blocks() {
        let text = "\
[SYSTEM 2: TEST-TIME COMPUTE]
Some options
[/SYSTEM 2]
THOUGHT: reasoning
ACTION: web_search
ACTION_INPUT: {\"query\": \"test\"}";

        let stripped = ReActAgent::strip_predictive_blocks(text);
        assert!(!stripped.contains("[SYSTEM 2"));
        assert!(stripped.contains("THOUGHT:"));
        assert!(stripped.contains("ACTION: web_search"));
    }

    #[test]
    fn test_extract_predictive_meta_none() {
        let text = "THOUGHT: simple\nANSWER: hello";
        assert!(ReActAgent::extract_predictive_meta(text).is_none());
    }

    #[test]
    fn test_extract_predictive_meta_with_selection() {
        let text = "\
[SYSTEM 2: TEST-TIME COMPUTE]
Option A: Do X
Option B: Do Y
Selected: A because X is better
[/SYSTEM 2]
THOUGHT: ok";

        let meta = ReActAgent::extract_predictive_meta(text);
        assert!(meta.is_some());
        let pm = meta.unwrap();
        assert!(pm.ttc_selection.contains("A because X is better"));
    }
}
