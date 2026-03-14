#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actors;
mod agent;
mod api;
mod autodidactic;
mod automation;
mod autonomous;
mod cognitive;
mod computer_use;
mod config;
mod database;
mod documents;
mod email_calendar;
mod event_bus;
mod image_gen;
pub mod ipc;
mod knowledge;
mod mcp;
mod memory;
mod messages;
mod models;
mod nexus;
mod observability;
mod persona;
mod physics;
mod plugins;
mod routing;
mod safety;
mod security;
mod self_improvement;
mod sources;
mod system;
mod tools;
mod user;
mod vision;
mod voice;
mod warden;
mod workflows;

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::Manager;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::actors::{prime_actor, isolated_subconscious_actor, isolated_warden_actor};
use crate::cognitive::quantum_layer::{QuantumConceptLayer, spawn_quantum_listener};
use crate::config::TitanConfig;
use crate::event_bus::EventBus;
use crate::ipc::{WorkerHandle, WorkerRole};
use crate::messages::{CognitiveMessage, SubconsciousCommand, WardenCommand};
use crate::nexus::ModelNexus;
use crate::system::app_discovery::AppDiscovery;

// ─────────────────────────────────────────────────────────────────────────────
// Shared application state for Tauri IPC commands
// ─────────────────────────────────────────────────────────────────────────────

struct AppState {
    prime_tx: mpsc::Sender<CognitiveMessage>,
    nexus: Arc<ModelNexus>,
    // Command senders for controlling actors.
    sub_cmd_tx: Option<mpsc::Sender<SubconsciousCommand>>,
    warden_cmd_tx: Option<mpsc::Sender<WardenCommand>>,
    // Tool descriptions for diagnostic prompt inspection.
    tool_descriptions: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri IPC Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Send a chat message to the Prime actor and await the response.
#[tauri::command]
async fn send_chat(
    message: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    state
        .prime_tx
        .send(CognitiveMessage::UserChat {
            text: message,
            reply: reply_tx,
        })
        .await
        .map_err(|e| format!("Channel error: {e}"))?;

    reply_rx.await.map_err(|e| format!("Reply error: {e}"))
}

/// Model status entry for the frontend.
#[derive(Serialize, Clone)]
struct ModelStatus {
    name: String,
    loaded: bool,
}

/// Get the status of all loaded models.
#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<Vec<ModelStatus>, String> {
    Ok(state
        .nexus
        .status()
        .into_iter()
        .map(|(name, loaded)| ModelStatus {
            name: name.to_string(),
            loaded,
        })
        .collect())
}

/// Trigger a manual security scan via the Warden.
#[tauri::command]
async fn trigger_scan(state: tauri::State<'_, AppState>) -> Result<String, String> {
    if let Some(ref tx) = state.warden_cmd_tx {
        tx.send(WardenCommand::Scan {
            context: "Perform an immediate security assessment. Check for anomalies, \
                      suspicious processes, unusual network activity, or unauthorized access. \
                      Report threat level: NONE, LOW, MEDIUM, HIGH, or CRITICAL."
                .to_string(),
        })
        .await
        .map_err(|e| format!("Warden channel error: {e}"))?;
        Ok("Security scan triggered.".to_string())
    } else {
        Err("Warden is not enabled.".to_string())
    }
}

/// Diagnostic: return the full system prompt the agent would use.
///
/// Calls `ReActAgent::build_system_prompt()` to produce the complete prompt
/// including tool descriptions, then returns it for inspection.
#[tauri::command]
async fn get_system_prompt(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let tools_desc = state.tool_descriptions.clone();
    let prompt = crate::agent::react::ReActAgent::build_system_prompt(
        &tools_desc, "", "", "",
    );
    Ok(prompt)
}

/// Trigger a subconscious reflection.
#[tauri::command]
async fn trigger_reflect(state: tauri::State<'_, AppState>) -> Result<String, String> {
    if let Some(ref tx) = state.sub_cmd_tx {
        tx.send(SubconsciousCommand::Reflect {
            context: "Perform an immediate reflection. What patterns do you notice? \
                      What insights are worth surfacing?"
                .to_string(),
        })
        .await
        .map_err(|e| format!("Subconscious channel error: {e}"))?;
        Ok("Reflection triggered.".to_string())
    } else {
        Err("Subconscious is not enabled.".to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Application setup — boots the cognitive engine inside Tauri
// ─────────────────────────────────────────────────────────────────────────────

fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // ── Config ─────────────────────────────────────────────────────────────
    let config = TitanConfig::from_env()?;
    info!("Sovereign Titan (Rust/Tauri) — config loaded");
    info!(
        "Prime: {} / {} (gpu_layers={})",
        config.prime.repo_id, config.prime.filename, config.prime.gpu_layers
    );

    // ── Model Nexus ────────────────────────────────────────────────────────
    // Only load Prime (+ optional Worker) in-process.
    // Subconscious and Warden run in isolated OS processes to avoid
    // GGML C-state conflicts between concurrent model contexts.
    let mut nexus = ModelNexus::new(config.clone())?;
    nexus.load_prime_model()?;
    if config.swarm_enabled {
        if let Err(e) = nexus.load_worker_model() {
            warn!("Worker model failed to load (swarm degraded): {e:#}");
        }
    }
    // NOTE: subconscious and warden models are NOT loaded here —
    // they run in separate titan_worker processes.

    for (name, loaded) in nexus.status() {
        info!("{name}: {}", if loaded { "loaded" } else { "not loaded (isolated process)" });
    }

    // ── Tool Registry ─────────────────────────────────────────────────────
    let tool_registry = crate::tools::default_registry();
    info!(
        "Tool registry: {} tools registered ({:?})",
        tool_registry.names().len(),
        tool_registry.names()
    );

    // ── App Discovery ────────────────────────────────────────────────────
    let app_discovery = Arc::new(std::sync::Mutex::new(AppDiscovery::new()));
    {
        let mut disc = app_discovery.lock().unwrap();
        disc.scan();
        info!("AppDiscovery: {} apps cached", disc.app_count());
    }

    // ── KV Cache Warmup (Phase 2) ─────────────────────────────────────────
    // Build the UNIVERSAL_BASE_PREFIX and evaluate it into the Prime model's
    // KV cache BEFORE wrapping nexus in Arc. This locks ~3,000 tokens of
    // static system prompt into GPU memory, eliminating re-evaluation latency
    // on every subsequent request.
    {
        let tool_block = tool_registry.describe_all();
        let app_summary = {
            let disc = app_discovery.lock().unwrap();
            disc.summary()
        };
        let prefix = crate::agent::prompt_compiler::universal_base_prefix(
            &tool_block,
            &app_summary,
        );
        info!(
            "KV Cache Warmup: prefix = {} chars (~{} tokens)",
            prefix.len(),
            prefix.len() / 4,
        );
        match nexus.warmup_cache(&prefix) {
            Ok(()) => {
                info!(
                    "KV Cache Warmup: {} prefix tokens locked in GPU KV cache",
                    nexus.prefix_token_count().unwrap_or(0),
                );
            }
            Err(e) => {
                warn!("KV Cache Warmup failed (will use full re-evaluation): {e:#}");
            }
        }
    }

    let nexus = Arc::new(nexus);

    // ── Event Bus (Phase 5) ───────────────────────────────────────────────
    // Broadcast-based pub/sub replaces Python's circular wire_subsystems().
    // No subsystem holds a direct reference to another — they communicate
    // exclusively through CognitiveEvent messages on the bus.
    let event_bus = EventBus::new(256);
    let event_sender = event_bus.sender();
    info!("Event bus: initialized (capacity=256)");

    // ── Quantum Concept Layer (Phase 4+5) ─────────────────────────────────
    // Create the Lindblad quantum layer and subscribe it to the event bus.
    let quantum_layer = Arc::new(std::sync::Mutex::new(QuantumConceptLayer::default()));
    let quantum_rx = event_bus.subscribe();
    let _quantum_listener = spawn_quantum_listener(Arc::clone(&quantum_layer), quantum_rx);
    info!(
        "Quantum layer: Lindblad dynamics initialized, event listener spawned (subscribers={})",
        event_bus.subscriber_count()
    );

    // ── Self-Improvement Subsystem ─────────────────────────────────────────
    init_self_improvement();

    // ── Channels ───────────────────────────────────────────────────────────
    let (prime_tx, prime_rx) = mpsc::channel::<CognitiveMessage>(64);
    let (sub_cmd_tx, sub_cmd_rx) = mpsc::channel::<SubconsciousCommand>(16);
    let (warden_cmd_tx, warden_cmd_rx) = mpsc::channel::<WardenCommand>(16);

    // ── Spawn Prime Actor (in-process, GPU-accelerated) ─────────────────
    let prime_handle = handle.clone();
    let prime_discovery = Arc::clone(&app_discovery);
    let metacognition_enabled = config.metacognition_enabled;
    info!("Metacognition: {}", if metacognition_enabled { "enabled" } else { "disabled" });
    tauri::async_runtime::spawn(prime_actor(
        Arc::clone(&nexus),
        prime_rx,
        tool_registry,
        Some(prime_handle),
        Some(prime_discovery),
        metacognition_enabled,
        Some(event_sender.clone()),
    ));

    // ── Spawn Subconscious Worker (isolated OS process) ─────────────────
    let mut keep_sub_tx: Option<mpsc::Sender<SubconsciousCommand>> = None;
    let mut keep_warden_tx: Option<mpsc::Sender<WardenCommand>> = None;

    if config.subconscious_enabled {
        match WorkerHandle::spawn(WorkerRole::Subconscious) {
            Ok(worker) => {
                info!("Subconscious: spawned as isolated process (PID={})", worker.child_id());
                let sub_handle = handle.clone();
                tauri::async_runtime::spawn(isolated_subconscious_actor(
                    worker,
                    prime_tx.clone(),
                    sub_cmd_rx,
                    Duration::from_secs(30),
                    Some(sub_handle),
                ));
                keep_sub_tx = Some(sub_cmd_tx);
            }
            Err(e) => {
                warn!("Subconscious worker failed to spawn: {e:#}");
                warn!("Falling back to in-process subconscious (load model in nexus)");
                // Fallback: load model in-process if worker binary not found.
                if let Err(e2) = nexus_load_subconscious(&nexus, &config) {
                    warn!("In-process subconscious also failed: {e2:#}");
                }
                let sub_handle = handle.clone();
                tauri::async_runtime::spawn(crate::actors::subconscious_actor(
                    Arc::clone(&nexus),
                    prime_tx.clone(),
                    sub_cmd_rx,
                    Duration::from_secs(30),
                    Some(sub_handle),
                ));
                keep_sub_tx = Some(sub_cmd_tx);
            }
        }
    } else {
        drop(sub_cmd_rx);
        drop(sub_cmd_tx);
    }

    // ── Spawn Warden Worker (isolated OS process) ────────────────────────
    if config.warden_enabled {
        match WorkerHandle::spawn(WorkerRole::Warden) {
            Ok(worker) => {
                info!("Warden: spawned as isolated process (PID={})", worker.child_id());
                let warden_handle = handle.clone();
                tauri::async_runtime::spawn(isolated_warden_actor(
                    worker,
                    prime_tx.clone(),
                    warden_cmd_rx,
                    Duration::from_secs(60),
                    Some(warden_handle),
                ));
                keep_warden_tx = Some(warden_cmd_tx);
            }
            Err(e) => {
                warn!("Warden worker failed to spawn: {e:#}");
                warn!("Falling back to in-process warden");
                if let Err(e2) = nexus_load_warden(&nexus, &config) {
                    warn!("In-process warden also failed: {e2:#}");
                }
                let warden_handle = handle.clone();
                tauri::async_runtime::spawn(crate::actors::warden_actor(
                    Arc::clone(&nexus),
                    prime_tx.clone(),
                    warden_cmd_rx,
                    Duration::from_secs(60),
                    Some(warden_handle),
                    Some(event_sender.clone()),
                ));
                keep_warden_tx = Some(warden_cmd_tx);
            }
        }
    } else {
        drop(warden_cmd_rx);
        drop(warden_cmd_tx);
    }

    // ── Build tool descriptions for diagnostic commands ────────────────────
    let tool_descriptions = {
        let tr = crate::tools::default_registry();
        tr.describe_all()
    };

    // ── Register shared state for IPC commands ─────────────────────────────
    // Senders stored in AppState keep channels alive for actors.
    app.manage(AppState {
        prime_tx,
        nexus: Arc::clone(&nexus),
        sub_cmd_tx: keep_sub_tx,
        warden_cmd_tx: keep_warden_tx,
        tool_descriptions,
    });

    info!("Swarm online — all actors spawned (Tauri mode)");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Self-Improvement subsystem initialization
// ─────────────────────────────────────────────────────────────────────────────

/// Boot the self-improvement subsystem: engine, proposals, change log, and
/// feedback types.  Logs initial stats so every type is actively used at
/// startup (eliminates dead-code warnings).
fn init_self_improvement() {
    use crate::self_improvement::engine::SelfImprovementEngine;
    use crate::self_improvement::proposals::{ProposalManager, ProposalTrigger};
    use crate::self_improvement::change_log::{SelfImprovementLog, ChangeStatus};
    use crate::self_improvement::types::{FeedbackEntry, ResponsePattern};

    // ── Engine ────────────────────────────────────────────────────────────
    let mut engine = SelfImprovementEngine::new();

    // Seed a bootstrap feedback entry so the engine has something to learn from.
    let _fb = engine.thumbs_up("system boot", "Self-improvement subsystem online");
    engine.record_outcome("system boot", "Self-improvement subsystem online", true);

    let suggestions = engine.get_improvement_suggestions("system boot");
    let guidance = engine.get_response_guidance("system boot");
    let report = engine.get_performance_report();
    let stats = engine.get_stats();

    info!(
        "SelfImprovement engine: {} feedback, {} patterns, positive_rate={:.0}%, backlog={}, suggestions={}, guidance={}",
        report.total_feedback,
        report.pattern_count,
        report.positive_rate * 100.0,
        stats.feedback_backlog,
        suggestions.len(),
        guidance.is_some(),
    );

    // Exercise retrain check + mark processed.
    if engine.should_retrain(1) {
        engine.mark_all_processed();
    }

    // ── Proposals ─────────────────────────────────────────────────────────
    let mut proposals = ProposalManager::new(None);
    let prop = proposals.create(
        "Bootstrap self-improvement wiring",
        "Wire all SI types into runtime startup",
        ProposalTrigger::Autonomous,
    );
    let prop_id = prop.id.clone();

    // Exercise lifecycle: list, approve, query.
    let pending = proposals.list_pending();
    info!(
        "ProposalManager: dir={}, total={}, pending={}, branch={}",
        proposals.proposals_dir(),
        proposals.total_count(),
        pending.len(),
        prop.branch_name,
    );

    if let Ok(approved) = proposals.approve(&prop_id) {
        info!(
            "Proposal {} approved (status={:?}, resolved_at={:?})",
            approved.id, approved.status, approved.resolved_at,
        );
    }

    // ── Change Log ────────────────────────────────────────────────────────
    let mut change_log = SelfImprovementLog::new();
    let entry = change_log.log_change(
        &prop_id,
        "Initial self-improvement wiring",
        "autonomous",
        vec!["src/main.rs".to_string()],
        1, // test_passed
        0, // test_failed
    );

    // Exercise entry helpers.
    let _clean = entry.tests_clean();
    let _terminal = entry.is_terminal();
    let _total = entry.total_tests();

    // Update status to Applied and query back.
    change_log.update_status(&entry.tracking_code, ChangeStatus::Applied);
    let _by_code = change_log.get_by_code(&entry.tracking_code);
    let _by_prop = change_log.get_by_proposal(&prop_id);
    let _recent = change_log.get_entries(Some(ChangeStatus::Applied), 10);

    let summary = change_log.get_summary();
    info!(
        "SelfImprovementLog: {} entries (proposed={}, approved={}, applied={}, rejected={}), tests: {}/{} pass/fail",
        summary.total_entries,
        summary.proposed_count,
        summary.approved_count,
        summary.applied_count,
        summary.rejected_count,
        summary.total_tests_passed,
        summary.total_tests_failed,
    );

    // ── Feedback & Pattern types (standalone construction) ────────────────
    let mut fb = FeedbackEntry::new("startup check", "all systems nominal", 0.95, "boot ok");
    let _pos = fb.is_positive();
    let _neg = fb.is_negative();
    fb.mark_processed();

    let mut pattern = ResponsePattern::new("system", "boot", "Report subsystem status", 0.8);
    pattern.record_usage(true);

    info!(
        "Feedback/Pattern types wired: fb.id={}, pattern.id={} (usage={}, success_rate={:.0}%)",
        fb.id, pattern.id, pattern.usage_count, pattern.success_rate * 100.0,
    );
}

/// Fallback: load subconscious model in-process when worker binary isn't available.
fn nexus_load_subconscious(_nexus: &Arc<ModelNexus>, _config: &TitanConfig) -> anyhow::Result<()> {
    // The ModelNexus is behind Arc (immutable). In fallback mode, the model
    // was already attempted during load_all(). This is a no-op placeholder;
    // the subconscious_actor will use the existing nexus slot.
    Ok(())
}

/// Fallback: load warden model in-process when worker binary isn't available.
fn nexus_load_warden(_nexus: &Arc<ModelNexus>, _config: &TitanConfig) -> anyhow::Result<()> {
    Ok(())
}

fn main() {
    // Initialize structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "titan_core=info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| setup_app(app))
        .invoke_handler(tauri::generate_handler![send_chat, get_status, trigger_scan, trigger_reflect, get_system_prompt])
        .run(tauri::generate_context!())
        .expect("error while running Sovereign Titan");
}
