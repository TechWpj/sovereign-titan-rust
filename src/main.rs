#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actors;
mod agent;
mod cognitive;
mod config;
mod knowledge;
mod memory;
mod messages;
mod nexus;
mod routing;
mod security;
mod tools;
mod warden;

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::actors::{prime_actor, subconscious_actor, warden_actor};
use crate::config::TitanConfig;
use crate::knowledge::graph::KnowledgeGraph;
use crate::memory::ledger::Ledger;
use crate::messages::{CognitiveMessage, SubconsciousCommand, WardenCommand};
use crate::nexus::ModelNexus;

// ─────────────────────────────────────────────────────────────────────────────
// Shared application state for Tauri IPC commands
// ─────────────────────────────────────────────────────────────────────────────

struct AppState {
    prime_tx: mpsc::Sender<CognitiveMessage>,
    nexus: Arc<ModelNexus>,
    // Keep command senders alive so actor channels don't close.
    _sub_cmd_tx: Option<mpsc::Sender<SubconsciousCommand>>,
    _warden_cmd_tx: Option<mpsc::Sender<WardenCommand>>,
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
    let mut nexus = ModelNexus::new(config.clone())?;
    nexus.load_all()?;

    for (name, loaded) in nexus.status() {
        info!("{name}: {}", if loaded { "loaded" } else { "not loaded" });
    }

    let nexus = Arc::new(nexus);

    // ── Channels ───────────────────────────────────────────────────────────
    let (prime_tx, prime_rx) = mpsc::channel::<CognitiveMessage>(64);
    let (sub_cmd_tx, sub_cmd_rx) = mpsc::channel::<SubconsciousCommand>(16);
    let (warden_cmd_tx, warden_cmd_rx) = mpsc::channel::<WardenCommand>(16);

    // ── Memory & Knowledge ─────────────────────────────────────────────────
    let ledger = Arc::new(
        Ledger::new(Some("workspace/ledger.enc"), Some("workspace/.ledger_key"))
            .unwrap_or_else(|e| {
                warn!("Ledger init failed, using default: {e}");
                Ledger::new(None, None).expect("default ledger must succeed")
            }),
    );

    let knowledge = Arc::new(tokio::sync::Mutex::new(
        KnowledgeGraph::new(Some("workspace/knowledge.json"))
            .unwrap_or_else(|e| {
                warn!("KnowledgeGraph init failed, using empty: {e}");
                KnowledgeGraph::new(None).expect("empty graph must succeed")
            }),
    ));

    info!("Memory systems initialized (ledger + knowledge graph)");

    // ── Spawn Actors ───────────────────────────────────────────────────────
    tauri::async_runtime::spawn(prime_actor(
        Arc::clone(&nexus),
        prime_rx,
        Arc::clone(&ledger),
        Arc::clone(&knowledge),
    ));

    let mut keep_sub_tx: Option<mpsc::Sender<SubconsciousCommand>> = None;
    let mut keep_warden_tx: Option<mpsc::Sender<WardenCommand>> = None;

    if config.subconscious_enabled {
        tauri::async_runtime::spawn(subconscious_actor(
            Arc::clone(&nexus),
            prime_tx.clone(),
            sub_cmd_rx,
            Duration::from_secs(30),
        ));
        keep_sub_tx = Some(sub_cmd_tx);
    } else {
        drop(sub_cmd_rx);
        drop(sub_cmd_tx);
    }

    if config.warden_enabled {
        // Security alert bridge — forwards alerts to the UI.
        let (alert_bridge_tx, mut alert_bridge_rx) = mpsc::channel::<String>(32);
        let alert_handle = handle.clone();

        tauri::async_runtime::spawn(async move {
            while let Some(alert) = alert_bridge_rx.recv().await {
                if let Err(e) = alert_handle.emit("security-alert", &alert) {
                    warn!("Failed to emit security alert to UI: {e}");
                }
            }
        });

        tauri::async_runtime::spawn(warden_actor(
            Arc::clone(&nexus),
            prime_tx.clone(),
            warden_cmd_rx,
            Duration::from_secs(60),
        ));

        // Keep alert bridge sender alive.
        let _alert_tx = alert_bridge_tx;
        keep_warden_tx = Some(warden_cmd_tx);
    } else {
        drop(warden_cmd_rx);
        drop(warden_cmd_tx);
    }

    // ── Register shared state for IPC commands ─────────────────────────────
    // Senders stored in AppState keep channels alive for actors.
    app.manage(AppState {
        prime_tx,
        nexus: Arc::clone(&nexus),
        _sub_cmd_tx: keep_sub_tx,
        _warden_cmd_tx: keep_warden_tx,
    });

    info!("Swarm online — all actors spawned (Tauri mode)");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

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
        .invoke_handler(tauri::generate_handler![send_chat, get_status])
        .run(tauri::generate_context!())
        .expect("error while running Sovereign Titan");
}
