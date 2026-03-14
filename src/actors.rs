//! Cognitive Actors — process-isolated actor loops for each brain in the swarm.
//!
//! **Prime** runs in-process as a `tokio::spawn` task (GPU-accelerated 14B).
//! **Subconscious** and **Warden** run in isolated OS processes via
//! `titan_worker` binaries, communicating over JSON-over-stdio IPC.
//! This prevents GGML C-state conflicts between concurrent model contexts.
//!
//! Fallback: if the worker binary isn't available, the legacy in-process
//! actors (`subconscious_actor`, `warden_actor`) are used instead.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use tokio::sync::broadcast;

use crate::agent::react::ReActAgent;
use crate::agent::verbosity::VerbosityMode;
use crate::event_bus::CognitiveEvent;
use crate::ipc::{WorkerHandle, WorkerRequest, WorkerResponse};
use crate::messages::{CognitiveMessage, SubconsciousCommand, WardenCommand};
use crate::nexus::{ModelNexus, ModelTarget};
use crate::system::app_discovery::AppDiscovery;
use crate::tools::ToolRegistry;

// ─────────────────────────────────────────────────────────────────────────────
// Tauri event payloads for actor events
// ─────────────────────────────────────────────────────────────────────────────

/// Emitted when the subconscious produces an insight.
#[derive(Debug, Clone, Serialize)]
pub struct SubconsciousEvent {
    pub insight: String,
    pub timestamp: u64,
}

/// Emitted when the warden completes a security scan.
#[derive(Debug, Clone, Serialize)]
pub struct WardenEvent {
    pub threat_level: String,
    pub details: String,
    pub timestamp: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Prime Actor (14B GPU)
// ─────────────────────────────────────────────────────────────────────────────

/// The Prime actor — main user-facing inference brain.
///
/// Creates a [`ReActAgent`] at startup and routes all `UserChat` messages
/// through the full ReAct loop (THOUGHT → ACTION → OBSERVATION) with tool
/// dispatch, adaptive temperature, and Tauri event emission.
///
/// `SecurityAlert` and `SubconsciousInsight` messages are handled inline
/// without the ReAct agent.
pub async fn prime_actor(
    nexus: Arc<ModelNexus>,
    mut rx: mpsc::Receiver<CognitiveMessage>,
    tool_registry: ToolRegistry,
    app_handle: Option<AppHandle>,
    app_discovery: Option<Arc<std::sync::Mutex<AppDiscovery>>>,
    metacognition_enabled: bool,
    event_tx: Option<broadcast::Sender<CognitiveEvent>>,
) {
    info!("Prime actor started (ReAct-enabled, metacognition={}, event_bus={})",
        metacognition_enabled, event_tx.is_some());

    // Build the ReAct agent with tools, optional Tauri handle, AppDiscovery,
    // and event bus sender.
    let agent = {
        // Parse verbosity from environment or default to "assistant"
        let verbosity = VerbosityMode::from_str_loose(
            &std::env::var("TITAN_VERBOSITY").unwrap_or_default(),
        );
        info!("Prime agent verbosity: {verbosity}");
        let mut a = ReActAgent::new(Arc::clone(&nexus), tool_registry)
            .with_metacognition(metacognition_enabled)
            .with_verbosity(verbosity);
        if let Some(ref handle) = app_handle {
            a = a.with_app_handle(handle.clone());
        }
        if let Some(discovery) = app_discovery {
            a = a.with_app_discovery(discovery);
        }
        if let Some(tx) = event_tx {
            a = a.with_event_sender(tx);
        }
        a
    };

    // Rolling buffer of subconscious insights for cognitive context.
    let mut insights: Vec<String> = Vec::new();
    const MAX_INSIGHTS: usize = 10;

    while let Some(msg) = rx.recv().await {
        match msg {
            CognitiveMessage::UserChat { text, reply } => {
                info!("Prime: received user chat ({} chars)", text.len());

                // Build cognitive context from accumulated insights.
                let cognitive_context = if insights.is_empty() {
                    String::new()
                } else {
                    let joined = insights.join("\n");
                    format!("[Background Awareness / Subconscious Insights]\n{joined}")
                };

                // Route through the ReAct agent (full prompt system).
                match agent.run(&text, &cognitive_context).await {
                    Ok(response) => {
                        let _ = reply.send(response);
                    }
                    Err(e) => {
                        error!("Prime ReAct generation failed: {e:#}");
                        let _ = reply.send(format!("[ERROR] Generation failed: {e}"));
                    }
                }
            }

            CognitiveMessage::SecurityAlert {
                threat_level,
                details,
            } => {
                warn!("Prime: SECURITY INTERRUPT — level={threat_level}, details={details}");

                // Security alerts bypass the ReAct agent — direct generation.
                let prompt = format!(
                    "[SECURITY ALERT — PRIORITY INTERRUPT]\n\
                     Threat Level: {threat_level}\n\
                     Details: {details}\n\n\
                     Analyze this threat and recommend immediate action."
                );

                match nexus.generate(&prompt, ModelTarget::Prime, 256, 0.3).await {
                    Ok(response) => {
                        info!("Prime security response: {response}");
                    }
                    Err(e) => {
                        error!("Prime security response failed: {e:#}");
                    }
                }
            }

            CognitiveMessage::SubconsciousInsight { memory_summary } => {
                info!(
                    "Prime: received subconscious insight ({} chars)",
                    memory_summary.len()
                );
                insights.push(memory_summary);
                if insights.len() > MAX_INSIGHTS {
                    insights.remove(0);
                }
            }
        }
    }

    info!("Prime actor stopped (channel closed)");
}

// ─────────────────────────────────────────────────────────────────────────────
// Subconscious Actor (3B CPU)
// ─────────────────────────────────────────────────────────────────────────────

/// The Subconscious actor — background inner monologue brain.
///
/// Runs an endless loop: sleep → reflect → send insight to Prime.
/// Can also be triggered on-demand via `SubconsciousCommand::Reflect`.
pub async fn subconscious_actor(
    nexus: Arc<ModelNexus>,
    prime_tx: mpsc::Sender<CognitiveMessage>,
    mut cmd_rx: mpsc::Receiver<SubconsciousCommand>,
    interval: Duration,
    app_handle: Option<AppHandle>,
) {
    info!(
        "Subconscious actor started (interval={}s)",
        interval.as_secs()
    );

    loop {
        // Wait for either the interval to elapse or an explicit command.
        let context = tokio::select! {
            _ = tokio::time::sleep(interval) => {
                // Periodic autonomous reflection.
                String::from(
                    "Reflect on your current state. What patterns do you notice? \
                     What should you be aware of? Summarize any important insights."
                )
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SubconsciousCommand::Reflect { context }) => context,
                    Some(SubconsciousCommand::Shutdown) | None => {
                        info!("Subconscious actor shutting down");
                        return;
                    }
                }
            }
        };

        // Generate an insight using the 3B subconscious model.
        match nexus
            .generate(&context, ModelTarget::Subconscious, 256, 0.6)
            .await
        {
            Ok(insight) => {
                if !insight.trim().is_empty() {
                    info!(
                        "Subconscious insight: {}...",
                        &insight[..insight.len().min(80)]
                    );

                    // Emit to UI so the consciousness panel can display it.
                    if let Some(ref handle) = app_handle {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let event = SubconsciousEvent {
                            insight: insight.clone(),
                            timestamp: now,
                        };
                        if let Err(e) = handle.emit("subconscious-insight", &event) {
                            warn!("Failed to emit subconscious-insight event: {e}");
                        }
                    }

                    let msg = CognitiveMessage::SubconsciousInsight {
                        memory_summary: insight,
                    };
                    if prime_tx.send(msg).await.is_err() {
                        warn!("Subconscious: Prime channel closed, stopping");
                        return;
                    }
                } else {
                    info!("Subconscious: generated empty insight, skipping");
                }
            }
            Err(e) => {
                error!("Subconscious generation failed: {e:#}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Warden Actor (3B CPU)
// ─────────────────────────────────────────────────────────────────────────────

/// The Warden actor — autonomous security scanning brain.
///
/// Runs periodic security scans using IDS, traffic, and DNS sensors,
/// then feeds sensor data to the LLM for threat assessment.
pub async fn warden_actor(
    nexus: Arc<ModelNexus>,
    prime_tx: mpsc::Sender<CognitiveMessage>,
    mut cmd_rx: mpsc::Receiver<WardenCommand>,
    interval: Duration,
    app_handle: Option<AppHandle>,
    event_tx: Option<broadcast::Sender<CognitiveEvent>>,
) {
    info!("Warden actor started (interval={}s)", interval.as_secs());

    // Initialize security sensors (IDS + Traffic + DNS).
    let mut sensors = crate::warden::sensors::SecuritySensors::new();
    info!("Warden: security sensors initialized (IDS, Traffic, DNS)");

    loop {
        // Wait for either the scan interval or an explicit command.
        let context = tokio::select! {
            _ = tokio::time::sleep(interval) => {
                String::from(
                    "Perform a security assessment. Check for anomalies, \
                     suspicious processes, unusual network activity, or \
                     unauthorized access patterns. Report threat level: \
                     NONE, LOW, MEDIUM, HIGH, or CRITICAL."
                )
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WardenCommand::Scan { context }) => context,
                    Some(WardenCommand::Shutdown) | None => {
                        info!("Warden actor shutting down");
                        return;
                    }
                }
            }
        };

        // Poll security sensors for real data.
        let sensor_data = sensors.scan();

        // Combine sensor data with the scan context for the LLM.
        let augmented_context = format!(
            "{context}\n\n\
             The following sensor data was collected. Analyze it and determine \
             the threat level:\n\n{sensor_data}"
        );

        // Run the security scan via the 3B warden model.
        match nexus
            .generate(&augmented_context, ModelTarget::Warden, 256, 0.3)
            .await
        {
            Ok(assessment) => {
                // Parse threat level from response — look for keywords.
                let threat_level = if assessment.contains("CRITICAL") {
                    "CRITICAL"
                } else if assessment.contains("HIGH") {
                    "HIGH"
                } else if assessment.contains("MEDIUM") {
                    "MEDIUM"
                } else if assessment.contains("LOW") {
                    "LOW"
                } else {
                    "NONE"
                };

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Always emit scan result to UI (even NONE for status display).
                if let Some(ref handle) = app_handle {
                    let event = WardenEvent {
                        threat_level: threat_level.to_string(),
                        details: assessment.clone(),
                        timestamp: now,
                    };
                    if let Err(e) = handle.emit("security-alert", &event) {
                        warn!("Failed to emit security-alert event: {e}");
                    }
                }

                if threat_level != "NONE" {
                    warn!("Warden: threat detected — level={threat_level}");

                    // Emit SecurityAnomaly to event bus (Phase 5)
                    if let Some(ref tx) = event_tx {
                        let severity = match threat_level {
                            "CRITICAL" => 1.0,
                            "HIGH" => 0.8,
                            "MEDIUM" => 0.5,
                            "LOW" => 0.3,
                            _ => 0.1,
                        };
                        let _ = tx.send(CognitiveEvent::SecurityAnomaly {
                            severity,
                            description: assessment.clone(),
                        });
                    }

                    let msg = CognitiveMessage::SecurityAlert {
                        threat_level: threat_level.to_string(),
                        details: assessment,
                    };
                    if prime_tx.send(msg).await.is_err() {
                        warn!("Warden: Prime channel closed, stopping");
                        return;
                    }
                } else {
                    info!("Warden: perimeter nominal — scan complete");
                }
            }
            Err(e) => {
                error!("Warden scan failed: {e:#}");
                // Emit error to UI so user knows warden is struggling.
                if let Some(ref handle) = app_handle {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let event = WardenEvent {
                        threat_level: "ERROR".to_string(),
                        details: format!("Warden scan failed: {e}"),
                        timestamp: now,
                    };
                    let _ = handle.emit("security-alert", &event);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Isolated Subconscious Actor (OS process via WorkerHandle)
// ─────────────────────────────────────────────────────────────────────────────

/// Isolated Subconscious actor — runs in a separate OS process.
///
/// Communicates with the `titan_worker` subprocess via JSON-over-stdio IPC
/// instead of calling `nexus.generate()` directly. This prevents GGML C-state
/// conflicts between concurrent model contexts.
pub async fn isolated_subconscious_actor(
    worker: WorkerHandle,
    prime_tx: mpsc::Sender<CognitiveMessage>,
    mut cmd_rx: mpsc::Receiver<SubconsciousCommand>,
    interval: Duration,
    app_handle: Option<AppHandle>,
) {
    info!(
        "Isolated subconscious actor started (interval={}s, PID={})",
        interval.as_secs(),
        worker.child_id(),
    );

    let worker = Arc::new(std::sync::Mutex::new(worker));

    loop {
        // Wait for either the interval to elapse or an explicit command.
        let context = tokio::select! {
            _ = tokio::time::sleep(interval) => {
                // Periodic autonomous reflection.
                String::from(
                    "Reflect on your current state. What patterns do you notice? \
                     What should you be aware of? Summarize any important insights."
                )
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SubconsciousCommand::Reflect { context }) => context,
                    Some(SubconsciousCommand::Shutdown) | None => {
                        info!("Isolated subconscious actor shutting down");
                        if let Ok(mut w) = worker.lock() {
                            w.shutdown();
                        }
                        return;
                    }
                }
            }
        };

        // Send Generate request to worker subprocess via IPC.
        let worker_clone = Arc::clone(&worker);
        let result = tokio::task::spawn_blocking(move || {
            let mut w = worker_clone.lock().unwrap();
            w.send(&WorkerRequest::Generate {
                prompt: context,
                max_tokens: 256,
                temperature: 0.6,
            })
        })
        .await;

        match result {
            Ok(Ok(WorkerResponse::Generated { text, tokens })) => {
                if !text.trim().is_empty() {
                    info!(
                        "Isolated subconscious insight ({} tokens): {}...",
                        tokens,
                        &text[..text.len().min(80)]
                    );

                    // Emit to UI.
                    if let Some(ref handle) = app_handle {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let event = SubconsciousEvent {
                            insight: text.clone(),
                            timestamp: now,
                        };
                        if let Err(e) = handle.emit("subconscious-insight", &event) {
                            warn!("Failed to emit subconscious-insight event: {e}");
                        }
                    }

                    let msg = CognitiveMessage::SubconsciousInsight {
                        memory_summary: text,
                    };
                    if prime_tx.send(msg).await.is_err() {
                        warn!("Isolated subconscious: Prime channel closed, stopping");
                        if let Ok(mut w) = worker.lock() {
                            w.shutdown();
                        }
                        return;
                    }
                } else {
                    info!("Isolated subconscious: generated empty insight, skipping");
                }
            }
            Ok(Ok(WorkerResponse::Error { message })) => {
                error!("Isolated subconscious worker error: {message}");
            }
            Ok(Ok(WorkerResponse::Ok)) => {
                warn!("Isolated subconscious: unexpected Ok response to Generate");
            }
            Ok(Err(e)) => {
                error!("Isolated subconscious IPC error: {e:#}");
            }
            Err(e) => {
                error!("Isolated subconscious spawn_blocking failed: {e:#}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Isolated Warden Actor (OS process via WorkerHandle)
// ─────────────────────────────────────────────────────────────────────────────

/// Isolated Warden actor — runs security scans in a separate OS process.
///
/// Communicates with the `titan_worker` subprocess via JSON-over-stdio IPC.
/// Sensor data is collected in the main process and forwarded as part of the
/// prompt to the worker for LLM-based threat assessment.
pub async fn isolated_warden_actor(
    worker: WorkerHandle,
    prime_tx: mpsc::Sender<CognitiveMessage>,
    mut cmd_rx: mpsc::Receiver<WardenCommand>,
    interval: Duration,
    app_handle: Option<AppHandle>,
) {
    info!(
        "Isolated warden actor started (interval={}s, PID={})",
        interval.as_secs(),
        worker.child_id(),
    );

    let worker = Arc::new(std::sync::Mutex::new(worker));

    // Initialize security sensors (IDS + Traffic + DNS).
    let mut sensors = crate::warden::sensors::SecuritySensors::new();
    info!("Isolated warden: security sensors initialized");

    loop {
        // Wait for either the scan interval or an explicit command.
        let context = tokio::select! {
            _ = tokio::time::sleep(interval) => {
                String::from(
                    "Perform a security assessment. Check for anomalies, \
                     suspicious processes, unusual network activity, or \
                     unauthorized access patterns. Report threat level: \
                     NONE, LOW, MEDIUM, HIGH, or CRITICAL."
                )
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WardenCommand::Scan { context }) => context,
                    Some(WardenCommand::Shutdown) | None => {
                        info!("Isolated warden actor shutting down");
                        if let Ok(mut w) = worker.lock() {
                            w.shutdown();
                        }
                        return;
                    }
                }
            }
        };

        // Poll security sensors for real data.
        let sensor_data = sensors.scan();

        // Combine sensor data with the scan context.
        let augmented_context = format!(
            "{context}\n\n\
             The following sensor data was collected. Analyze it and determine \
             the threat level:\n\n{sensor_data}"
        );

        // Send Generate request to worker subprocess via IPC.
        let worker_clone = Arc::clone(&worker);
        let result = tokio::task::spawn_blocking(move || {
            let mut w = worker_clone.lock().unwrap();
            w.send(&WorkerRequest::Generate {
                prompt: augmented_context,
                max_tokens: 256,
                temperature: 0.3,
            })
        })
        .await;

        match result {
            Ok(Ok(WorkerResponse::Generated { text: assessment, .. })) => {
                // Parse threat level from response.
                let threat_level = if assessment.contains("CRITICAL") {
                    "CRITICAL"
                } else if assessment.contains("HIGH") {
                    "HIGH"
                } else if assessment.contains("MEDIUM") {
                    "MEDIUM"
                } else if assessment.contains("LOW") {
                    "LOW"
                } else {
                    "NONE"
                };

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Emit scan result to UI.
                if let Some(ref handle) = app_handle {
                    let event = WardenEvent {
                        threat_level: threat_level.to_string(),
                        details: assessment.clone(),
                        timestamp: now,
                    };
                    if let Err(e) = handle.emit("security-alert", &event) {
                        warn!("Failed to emit security-alert event: {e}");
                    }
                }

                if threat_level != "NONE" {
                    warn!("Isolated warden: threat detected — level={threat_level}");
                    let msg = CognitiveMessage::SecurityAlert {
                        threat_level: threat_level.to_string(),
                        details: assessment,
                    };
                    if prime_tx.send(msg).await.is_err() {
                        warn!("Isolated warden: Prime channel closed, stopping");
                        if let Ok(mut w) = worker.lock() {
                            w.shutdown();
                        }
                        return;
                    }
                } else {
                    info!("Isolated warden: perimeter nominal — scan complete");
                }
            }
            Ok(Ok(WorkerResponse::Error { message })) => {
                error!("Isolated warden worker error: {message}");
                if let Some(ref handle) = app_handle {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let event = WardenEvent {
                        threat_level: "ERROR".to_string(),
                        details: format!("Worker error: {message}"),
                        timestamp: now,
                    };
                    let _ = handle.emit("security-alert", &event);
                }
            }
            Ok(Ok(WorkerResponse::Ok)) => {
                warn!("Isolated warden: unexpected Ok response to Generate");
            }
            Ok(Err(e)) => {
                error!("Isolated warden IPC error: {e:#}");
                if let Some(ref handle) = app_handle {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let event = WardenEvent {
                        threat_level: "ERROR".to_string(),
                        details: format!("IPC error: {e}"),
                        timestamp: now,
                    };
                    let _ = handle.emit("security-alert", &event);
                }
            }
            Err(e) => {
                error!("Isolated warden spawn_blocking failed: {e:#}");
            }
        }
    }
}
