//! Multi-Model Nexus — concurrent model management via `llama-cpp-2`.
//!
//! Manages multiple GGUF model instances behind `Arc<RwLock<_>>` so that
//! inference requests can be dispatched to the correct model concurrently.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::config::{ModelDescriptor, TitanConfig};

// ─────────────────────────────────────────────────────────────────────────────
// Per-model handle
// ─────────────────────────────────────────────────────────────────────────────

/// A loaded model + its context, protected for concurrent access.
struct ModelSlot {
    model: LlamaModel,
    ctx: LlamaContext<'static>,
}

// SAFETY: LlamaModel and LlamaContext are Send+Sync per llama-cpp-2 docs.
unsafe impl Send for ModelSlot {}
unsafe impl Sync for ModelSlot {}

/// Which model to route a generation request to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTarget {
    Prime,
    Worker,
    Subconscious,
    Warden,
}

// ─────────────────────────────────────────────────────────────────────────────
// ModelNexus
// ─────────────────────────────────────────────────────────────────────────────

/// Manages the full model fleet: Prime (14B GPU), Worker (0.5B CPU),
/// Subconscious (3B CPU), and Warden (3B CPU).
pub struct ModelNexus {
    backend: Arc<LlamaBackend>,
    prime: Option<Arc<RwLock<ModelSlot>>>,
    worker: Option<Arc<RwLock<ModelSlot>>>,
    subconscious: Option<Arc<RwLock<ModelSlot>>>,
    warden: Option<Arc<RwLock<ModelSlot>>>,
    config: TitanConfig,
}

impl ModelNexus {
    /// Create an empty nexus (no models loaded yet).
    pub fn new(config: TitanConfig) -> Result<Self> {
        let backend = Arc::new(LlamaBackend::init()?);
        Ok(Self {
            backend,
            prime: None,
            worker: None,
            subconscious: None,
            warden: None,
            config,
        })
    }

    /// Resolve a [`ModelDescriptor`] to an on-disk GGUF path.
    ///
    /// If `descriptor.path` is set, uses that directly.
    /// Otherwise returns the repo_id/filename for manual download
    /// (full HF download integration is deferred to a later phase).
    fn resolve_model_path(descriptor: &ModelDescriptor) -> Result<PathBuf> {
        if let Some(ref explicit) = descriptor.path {
            let p = PathBuf::from(explicit);
            anyhow::ensure!(p.exists(), "Model path does not exist: {}", p.display());
            return Ok(p);
        }

        // Fall back to finetuned_models/ directory for local GGUF files.
        let local = PathBuf::from("finetuned_models").join(&descriptor.filename);
        if local.exists() {
            return Ok(local);
        }

        anyhow::bail!(
            "Model not found locally. Set explicit path or download: {}/{}",
            descriptor.repo_id,
            descriptor.filename
        );
    }

    /// Load a single GGUF model into a [`ModelSlot`].
    fn load_slot(
        backend: &LlamaBackend,
        descriptor: &ModelDescriptor,
        config: &TitanConfig,
        label: &str,
    ) -> Result<ModelSlot> {
        let path = Self::resolve_model_path(descriptor)
            .with_context(|| format!("resolving {label} model path"))?;

        info!(
            "{label}: loading {} (gpu_layers={}, ctx={})",
            path.display(),
            descriptor.gpu_layers,
            descriptor.context_length,
        );

        // Model params
        let gpu_layers = if descriptor.gpu_layers < 0 {
            u32::MAX // offload all layers
        } else {
            descriptor.gpu_layers as u32
        };

        let model_params = LlamaModelParams::default()
            .with_n_gpu_layers(gpu_layers)
            .with_use_mlock(config.use_mlock);

        let model = LlamaModel::load_from_file(backend, &path, &model_params)
            .map_err(|e| anyhow::anyhow!("{label}: failed to load model: {e:?}"))?;

        // Context params
        let n_ctx =
            NonZeroU32::new(descriptor.context_length).unwrap_or(NonZeroU32::new(32768).unwrap());

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx))
            .with_n_threads(config.threads as i32)
            .with_n_threads_batch(config.threads_batch as i32)
            .with_n_ubatch(config.n_ubatch)
            .with_offload_kqv(descriptor.gpu_layers != 0);

        // SAFETY: we store model and ctx together in ModelSlot and never
        // separate them — the model outlives the context.
        let ctx: LlamaContext<'static> = unsafe {
            std::mem::transmute(
                model
                    .new_context(backend, ctx_params)
                    .map_err(|e| anyhow::anyhow!("{label}: failed to create context: {e:?}"))?,
            )
        };

        info!("{label}: loaded successfully ({} params)", model.n_params());

        Ok(ModelSlot { model, ctx })
    }

    // ── Public loaders ───────────────────────────────────────────────────

    /// Load the 14B Prime model (GPU-accelerated).
    pub fn load_prime_model(&mut self) -> Result<()> {
        let slot = Self::load_slot(
            &self.backend,
            &self.config.prime,
            &self.config,
            "Titan-Prime",
        )?;
        self.prime = Some(Arc::new(RwLock::new(slot)));
        Ok(())
    }

    /// Load the 0.5B Worker model (CPU-only, speculative decoding).
    pub fn load_worker_model(&mut self) -> Result<()> {
        let slot = Self::load_slot(
            &self.backend,
            &self.config.worker,
            &self.config,
            "Titan-Worker",
        )?;
        self.worker = Some(Arc::new(RwLock::new(slot)));
        Ok(())
    }

    /// Load the 3B Subconscious model (CPU-only, consciousness).
    pub fn load_subconscious_model(&mut self) -> Result<()> {
        let slot = Self::load_slot(
            &self.backend,
            &self.config.subconscious,
            &self.config,
            "Titan-Subconscious",
        )?;
        self.subconscious = Some(Arc::new(RwLock::new(slot)));
        Ok(())
    }

    /// Load the 3B Warden model (CPU-only, security).
    pub fn load_warden_model(&mut self) -> Result<()> {
        let slot = Self::load_slot(
            &self.backend,
            &self.config.warden,
            &self.config,
            "Titan-Warden",
        )?;
        self.warden = Some(Arc::new(RwLock::new(slot)));
        Ok(())
    }

    /// Load all models that are enabled in the config.
    pub fn load_all(&mut self) -> Result<()> {
        self.load_prime_model()?;

        if self.config.swarm_enabled {
            if let Err(e) = self.load_worker_model() {
                warn!("Worker model failed to load (swarm degraded): {e:#}");
            }
        }

        if self.config.subconscious_enabled {
            if let Err(e) = self.load_subconscious_model() {
                warn!("Subconscious model failed to load: {e:#}");
            }
        }

        if self.config.warden_enabled {
            if let Err(e) = self.load_warden_model() {
                warn!("Warden model failed to load: {e:#}");
            }
        }

        Ok(())
    }

    // ── Generation ───────────────────────────────────────────────────────

    /// Select the slot for a given target.
    fn slot_for(&self, target: ModelTarget) -> Result<Arc<RwLock<ModelSlot>>> {
        let slot = match target {
            ModelTarget::Prime => &self.prime,
            ModelTarget::Worker => &self.worker,
            ModelTarget::Subconscious => &self.subconscious,
            ModelTarget::Warden => &self.warden,
        };
        slot.as_ref()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{target:?} model is not loaded"))
    }

    /// Generate a text completion from the given prompt.
    ///
    /// Routes to the specified model, tokenizes the prompt, runs autoregressive
    /// decoding with greedy sampling, and returns the generated text.
    pub async fn generate(
        &self,
        prompt: &str,
        target: ModelTarget,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String> {
        // Role-specific system prompts per model target.
        let system = match target {
            ModelTarget::Prime => {
                "You are Sovereign Titan, an autonomous AI operating system running on local \
                 hardware with full system access. You are direct, precise, and helpful. \
                 Format responses with markdown when appropriate."
            }
            ModelTarget::Worker => "You are a fast, concise AI assistant.",
            ModelTarget::Subconscious => {
                "You are the Subconscious — the inner monologue of Sovereign Titan. \
                 Your role is background awareness: pattern recognition, anomaly detection, \
                 and surfacing insights the conscious mind may have missed. \
                 You reflect on system state, recent interactions, and ambient signals. \
                 Keep insights concise (2-3 sentences). Focus on actionable observations."
            }
            ModelTarget::Warden => {
                "You are the Warden — the security subsystem of Sovereign Titan. \
                 Your sole purpose is threat assessment and system protection. \
                 Analyze the current security posture and report a threat level: \
                 NONE, LOW, MEDIUM, HIGH, or CRITICAL. \
                 Be specific about what you observe. Keep reports brief and structured."
            }
        };

        let chatml = format!(
            "<|im_start|>system\n{system}<|im_end|>\n\
             <|im_start|>user\n{prompt}<|im_end|>\n\
             <|im_start|>assistant\n"
        );
        self.generate_raw(&chatml, target, max_tokens, temperature).await
    }

    /// Low-level generation: takes an already-formatted prompt (e.g. ChatML).
    ///
    /// Clears the KV cache before each call to avoid stale state, then runs
    /// autoregressive decoding.
    async fn generate_raw(
        &self,
        prompt: &str,
        target: ModelTarget,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String> {
        let slot = self.slot_for(target)?;

        // Move into a blocking task — llama.cpp inference is CPU/GPU-bound.
        let prompt = prompt.to_string();
        tokio::task::spawn_blocking(move || {
            let mut guard = slot.blocking_write();
            let ModelSlot {
                ref model,
                ref mut ctx,
            } = *guard;

            // Clear KV cache to avoid stale state from previous generations.
            ctx.clear_kv_cache();

            // Tokenize — no BOS since ChatML provides its own framing.
            let tokens = model
                .str_to_token(&prompt, AddBos::Never)
                .map_err(|e| anyhow::anyhow!("tokenization failed: {e:?}"))?;

            anyhow::ensure!(!tokens.is_empty(), "tokenization produced 0 tokens");

            // Feed prompt tokens via batch
            let mut batch = LlamaBatch::new(tokens.len(), 1);
            for (i, &tok) in tokens.iter().enumerate() {
                let is_last = i == tokens.len() - 1;
                batch
                    .add(tok, i as i32, &[0], is_last)
                    .map_err(|e| anyhow::anyhow!("batch add failed: {e:?}"))?;
            }

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("prompt decode failed: {e:?}"))?;

            // Build sampler chain
            let sampler = if temperature <= 0.0 {
                LlamaSampler::greedy()
            } else {
                LlamaSampler::chain(
                    [LlamaSampler::temp(temperature), LlamaSampler::dist(42)],
                    false,
                )
            };
            let mut sampler = sampler;

            // Autoregressive generation
            let eos = model.token_eos();
            let mut output_tokens: Vec<LlamaToken> = Vec::new();
            let mut n_decoded = tokens.len() as i32;

            for _ in 0..max_tokens {
                let new_token = sampler.sample(ctx, -1);
                sampler.accept(new_token);

                if new_token == eos {
                    break;
                }

                output_tokens.push(new_token);

                // Prepare next batch (single token)
                let mut next_batch = LlamaBatch::new(1, 1);
                next_batch
                    .add(new_token, n_decoded, &[0], true)
                    .map_err(|e| anyhow::anyhow!("next batch failed: {e:?}"))?;
                ctx.decode(&mut next_batch)
                    .map_err(|e| anyhow::anyhow!("decode step failed: {e:?}"))?;
                n_decoded += 1;
            }

            // Detokenize
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let mut output = String::new();
            for tok in &output_tokens {
                let piece = model
                    .token_to_piece(*tok, &mut decoder, false, None)
                    .unwrap_or_default();
                output.push_str(&piece);
            }

            Ok(output)
        })
        .await?
    }

    /// Generate a text completion with explicit system/user ChatML wrapping.
    ///
    /// Unlike [`generate()`] which takes a raw prompt, this method wraps the
    /// system prompt and user message in proper ChatML format and supports
    /// stop sequences for the ReAct loop (stops on `OBSERVATION:` so the
    /// agent can inject tool output).
    pub async fn generate_with_system(
        &self,
        system_prompt: &str,
        user_message: &str,
        target: ModelTarget,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String> {
        let stop_sequences: Vec<String> = vec![
            "\nOBSERVATION:".to_string(),
            "\nOBSERVATION :".to_string(),
            "<|im_end|>".to_string(),
        ];

        let prompt = format!(
            "<|im_start|>system\n{system_prompt}<|im_end|>\n\
             <|im_start|>user\n{user_message}<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        self.generate_with_stops(&prompt, target, max_tokens, temperature, &stop_sequences)
            .await
    }

    /// Generate a text completion with optional stop sequences.
    ///
    /// The autoregressive loop checks after each token whether the accumulated
    /// output ends with any stop sequence. If so, generation stops early and
    /// the stop sequence is trimmed from the output.
    async fn generate_with_stops(
        &self,
        prompt: &str,
        target: ModelTarget,
        max_tokens: u32,
        temperature: f32,
        stop_sequences: &[String],
    ) -> Result<String> {
        let slot = self.slot_for(target)?;
        let prompt = prompt.to_string();
        let stops = stop_sequences.to_vec();

        tokio::task::spawn_blocking(move || {
            let mut guard = slot.blocking_write();
            let ModelSlot {
                ref model,
                ref mut ctx,
            } = *guard;

            // Clear KV cache to avoid stale state from previous generations.
            ctx.clear_kv_cache();

            // Tokenize — no BOS since ChatML provides its own framing.
            let tokens = model
                .str_to_token(&prompt, AddBos::Never)
                .map_err(|e| anyhow::anyhow!("tokenization failed: {e:?}"))?;

            anyhow::ensure!(!tokens.is_empty(), "tokenization produced 0 tokens");

            // Feed prompt tokens via batch
            let mut batch = LlamaBatch::new(tokens.len(), 1);
            for (i, &tok) in tokens.iter().enumerate() {
                let is_last = i == tokens.len() - 1;
                batch
                    .add(tok, i as i32, &[0], is_last)
                    .map_err(|e| anyhow::anyhow!("batch add failed: {e:?}"))?;
            }

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("prompt decode failed: {e:?}"))?;

            // Build sampler chain
            let sampler = if temperature <= 0.0 {
                LlamaSampler::greedy()
            } else {
                LlamaSampler::chain(
                    [LlamaSampler::temp(temperature), LlamaSampler::dist(42)],
                    false,
                )
            };
            let mut sampler = sampler;

            // Autoregressive generation with stop sequence detection
            let eos = model.token_eos();
            let mut output_tokens: Vec<LlamaToken> = Vec::new();
            let mut n_decoded = tokens.len() as i32;
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let mut output = String::new();
            let mut stopped = false;

            for _ in 0..max_tokens {
                let new_token = sampler.sample(ctx, -1);
                sampler.accept(new_token);

                if new_token == eos {
                    break;
                }

                output_tokens.push(new_token);

                // Decode incrementally for stop sequence checking
                let piece = model
                    .token_to_piece(new_token, &mut decoder, false, None)
                    .unwrap_or_default();
                output.push_str(&piece);

                // Check stop sequences against the accumulated output
                for stop in &stops {
                    if output.ends_with(stop.as_str()) {
                        // Trim the stop sequence from output
                        let trimmed_len = output.len() - stop.len();
                        output.truncate(trimmed_len);
                        stopped = true;
                        break;
                    }
                }
                if stopped {
                    break;
                }

                // Prepare next batch (single token)
                let mut next_batch = LlamaBatch::new(1, 1);
                next_batch
                    .add(new_token, n_decoded, &[0], true)
                    .map_err(|e| anyhow::anyhow!("next batch failed: {e:?}"))?;
                ctx.decode(&mut next_batch)
                    .map_err(|e| anyhow::anyhow!("decode step failed: {e:?}"))?;
                n_decoded += 1;
            }

            Ok(output)
        })
        .await?
    }

    /// Check which models are currently loaded.
    pub fn status(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("prime", self.prime.is_some()),
            ("worker", self.worker.is_some()),
            ("subconscious", self.subconscious.is_some()),
            ("warden", self.warden.is_some()),
        ]
    }
}
