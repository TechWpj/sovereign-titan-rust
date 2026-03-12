//! Settings Manager — mirrors `sovereign_titan/settings.py`.
//!
//! Loads configuration from environment variables (with `.env` file support via dotenvy)
//! and exposes a [`TitanConfig`] struct with all model paths, GPU settings, and feature flags.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Helper: read an env var or return a default.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Helper: read an env var as bool ("true"/"1" = true).
fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"),
        Err(_) => default,
    }
}

/// Helper: read an env var as i32.
fn env_i32(key: &str, default: i32) -> i32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Helper: read an env var as u32.
fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ─────────────────────────────────────────────────────────────────────────────
// Model descriptor
// ─────────────────────────────────────────────────────────────────────────────

/// Describes a single GGUF model to load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// Explicit local path (overrides HF download when set).
    pub path: Option<String>,
    /// HuggingFace repo id for auto-download.
    pub repo_id: String,
    /// GGUF filename within the repo.
    pub filename: String,
    /// Number of layers to offload to GPU (-1 = all, 0 = CPU only).
    pub gpu_layers: i32,
    /// Context window size.
    pub context_length: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Top-level config
// ─────────────────────────────────────────────────────────────────────────────

/// Central configuration for the Titan engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitanConfig {
    // ── Models ───────────────────────────────────────────────────────────
    /// 14B Prime model (GPU-accelerated, main inference).
    pub prime: ModelDescriptor,
    /// 0.5B Worker model (CPU-only, speculative decoding drafts).
    pub worker: ModelDescriptor,
    /// 3B Subconscious model (CPU-only, consciousness / inner monologue).
    pub subconscious: ModelDescriptor,
    /// 3B Warden model (CPU-only, security scanning).
    pub warden: ModelDescriptor,

    // ── GPU / Inference ──────────────────────────────────────────────────
    /// Number of CPU threads for inference.
    pub threads: u32,
    /// Batch threads (parallel prompt eval).
    pub threads_batch: u32,
    /// Enable flash attention.
    pub flash_attn: bool,
    /// KV cache type: "f16", "q8_0", "q4_0".
    pub kv_cache_type: String,
    /// Micro-batch size for prompt evaluation.
    pub n_ubatch: u32,
    /// Lock model weights in RAM (mlock).
    pub use_mlock: bool,

    // ── Feature flags ────────────────────────────────────────────────────
    pub swarm_enabled: bool,
    pub consciousness_enabled: bool,
    pub subconscious_enabled: bool,
    pub warden_enabled: bool,
    pub dag_enabled: bool,
    pub mcts_enabled: bool,
    pub vlm_enabled: bool,
    pub quantum_enabled: bool,
    pub voice_enabled: bool,

    // ── Wave 4 feature flags ──────────────────────────────────────────
    pub app_discovery_enabled: bool,
    pub execution_paths_enabled: bool,
    pub observation_distiller_enabled: bool,
    pub tool_memory_enabled: bool,
    pub fast_paths_enabled: bool,
    pub verbosity_mode: String,

    // ── Server ───────────────────────────────────────────────────────────
    pub api_host: String,
    pub api_port: u16,
}

impl TitanConfig {
    /// Load configuration from environment variables (+ optional `.env` file).
    pub fn from_env() -> Result<Self> {
        // Best-effort load of .env — silently ignore if missing.
        let _ = dotenvy::dotenv();

        Ok(Self {
            // ── Prime (14B GPU) ──────────────────────────────────────────
            prime: ModelDescriptor {
                path: std::env::var("TITAN_MODEL_PATH").ok(),
                repo_id: env_or("TITAN_MODEL_NAME", "bartowski/Qwen2.5-14B-Instruct-GGUF"),
                filename: env_or("TITAN_MODEL_FILE", "Qwen2.5-14B-Instruct-Q4_K_M.gguf"),
                gpu_layers: env_i32("TITAN_GPU_LAYERS", -1),
                context_length: env_u32("TITAN_CONTEXT_LENGTH", 32768),
            },

            // ── Worker (0.5B CPU) ────────────────────────────────────────
            worker: ModelDescriptor {
                path: std::env::var("TITAN_WORKER_MODEL_PATH").ok(),
                repo_id: env_or("TITAN_WORKER_MODEL_NAME", "Qwen/Qwen2.5-0.5B-Instruct-GGUF"),
                filename: env_or("TITAN_WORKER_MODEL_FILE", "qwen2.5-0.5b-instruct-q8_0.gguf"),
                gpu_layers: 0,
                context_length: env_u32("TITAN_WORKER_CONTEXT_LENGTH", 32768),
            },

            // ── Subconscious (3B CPU) ────────────────────────────────────
            subconscious: ModelDescriptor {
                path: std::env::var("TITAN_SUBCONSCIOUS_MODEL_PATH").ok(),
                repo_id: env_or(
                    "TITAN_SUBCONSCIOUS_MODEL_NAME",
                    "bartowski/Qwen2.5-3B-Instruct-GGUF",
                ),
                filename: env_or(
                    "TITAN_SUBCONSCIOUS_MODEL_FILE",
                    "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
                ),
                gpu_layers: 0,
                context_length: 8192,
            },

            // ── Warden (3B CPU) ──────────────────────────────────────────
            warden: ModelDescriptor {
                path: std::env::var("TITAN_WARDEN_MODEL_PATH").ok(),
                repo_id: env_or(
                    "TITAN_WARDEN_MODEL_NAME",
                    "bartowski/Qwen2.5-3B-Instruct-GGUF",
                ),
                filename: env_or("TITAN_WARDEN_MODEL_FILE", "Qwen2.5-3B-Instruct-Q4_K_M.gguf"),
                gpu_layers: 0,
                context_length: 8192,
            },

            // ── GPU / Inference ──────────────────────────────────────────
            threads: env_u32("TITAN_THREADS", 12),
            threads_batch: env_u32("TITAN_THREADS_BATCH", 24),
            flash_attn: env_bool("TITAN_FLASH_ATTN", true),
            kv_cache_type: env_or("TITAN_KV_CACHE_TYPE", "f16"),
            n_ubatch: env_u32("TITAN_N_UBATCH", 512),
            use_mlock: env_bool("TITAN_USE_MLOCK", true),

            // ── Feature flags ────────────────────────────────────────────
            swarm_enabled: env_bool("TITAN_SWARM_ENABLED", true),
            consciousness_enabled: env_bool("TITAN_CONSCIOUSNESS_ENABLED", true),
            subconscious_enabled: env_bool("TITAN_SUBCONSCIOUS_ENABLED", true),
            warden_enabled: env_bool("TITAN_WARDEN_ENABLED", true),
            dag_enabled: env_bool("TITAN_DAG_ENABLED", true),
            mcts_enabled: env_bool("TITAN_MCTS_ENABLED", true),
            vlm_enabled: env_bool("TITAN_VLM_ENABLED", true),
            quantum_enabled: env_bool("TITAN_QUANTUM_ENABLED", true),
            voice_enabled: env_bool("TITAN_VOICE_ENABLED", true),

            // ── Wave 4 feature flags ────────────────────────────────────
            app_discovery_enabled: env_bool("TITAN_APP_DISCOVERY_ENABLED", true),
            execution_paths_enabled: env_bool("TITAN_EXECUTION_PATHS_ENABLED", true),
            observation_distiller_enabled: env_bool("TITAN_OBSERVATION_DISTILLER_ENABLED", true),
            tool_memory_enabled: env_bool("TITAN_TOOL_MEMORY_ENABLED", true),
            fast_paths_enabled: env_bool("TITAN_FAST_PATHS_ENABLED", true),
            verbosity_mode: env_or("TITAN_VERBOSITY_MODE", "assistant"),

            // ── Server ──────────────────────────────────────────────────
            api_host: env_or("TITAN_API_HOST", "127.0.0.1"),
            api_port: env_u32("TITAN_API_PORT", 8000) as u16,
        })
    }
}
