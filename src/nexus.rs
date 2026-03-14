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
///
/// IMPORTANT: `ctx` is declared before `model` so it drops first.
/// `LlamaContext` holds an internal pointer to `LlamaModel`'s C struct,
/// so the context must be freed before the model.
struct ModelSlot {
    ctx: LlamaContext<'static>,
    model: LlamaModel,
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

/// Manages the full model fleet: Prime (14B GPU), Worker (0.5B GPU draft),
/// Subconscious (3B CPU, isolated process), and Warden (3B CPU, isolated process).
pub struct ModelNexus {
    backend: Arc<LlamaBackend>,
    prime: Option<Arc<RwLock<ModelSlot>>>,
    worker: Option<Arc<RwLock<ModelSlot>>>,
    subconscious: Option<Arc<RwLock<ModelSlot>>>,
    warden: Option<Arc<RwLock<ModelSlot>>>,
    config: TitanConfig,
    /// Number of tokens in the warmed-up system prefix (Prime model).
    /// When set, `generate_with_cached_prefix` preserves this many KV
    /// entries instead of clearing the full cache.
    prefix_token_count: Option<usize>,
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
            prefix_token_count: None,
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
            .with_n_batch(descriptor.context_length)
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

        Ok(ModelSlot { ctx, model })
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

    /// Load the 0.5B Worker model (GPU, speculative decoding draft for Prime).
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

    // ── KV Cache Warmup ────────────────────────────────────────────────

    /// Warm up the KV cache for the Prime model with the static system prefix.
    ///
    /// Tokenizes the `UNIVERSAL_BASE_PREFIX` (wrapped in ChatML system tags),
    /// evaluates all prefix tokens through the transformer to populate the KV
    /// cache, then records the token count. Subsequent calls to
    /// [`generate_with_cached_prefix`] will preserve these KV entries and only
    /// clear/re-evaluate the dynamic user turn.
    ///
    /// This eliminates the ~3,000-token re-evaluation penalty on every request.
    pub fn warmup_cache(&mut self, system_prefix: &str) -> Result<()> {
        let slot = self.prime.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Prime model not loaded — cannot warmup cache"))?
            .clone();

        let prefix_chatml = format!(
            "<|im_start|>system\n{system_prefix}<|im_end|>\n"
        );

        let prefix_str = prefix_chatml.clone();
        let n_tokens = {
            let mut guard = slot.blocking_write();
            let ModelSlot { ref mut ctx, ref model } = *guard;

            // Clear any existing KV state.
            ctx.clear_kv_cache();

            // Tokenize the static prefix.
            let tokens = model
                .str_to_token(&prefix_str, AddBos::Never)
                .map_err(|e| anyhow::anyhow!("warmup tokenization failed: {e:?}"))?;

            anyhow::ensure!(!tokens.is_empty(), "warmup produced 0 tokens");

            let n = tokens.len();
            info!(
                "Warmup: evaluating {} prefix tokens ({} chars) into KV cache",
                n, prefix_str.len()
            );

            // Feed prefix tokens into the context to populate the KV cache.
            let mut batch = LlamaBatch::new(n, 1);
            for (i, &tok) in tokens.iter().enumerate() {
                let is_last = i == n - 1;
                batch
                    .add(tok, i as i32, &[0], is_last)
                    .map_err(|e| anyhow::anyhow!("warmup batch add failed: {e:?}"))?;
            }

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("warmup decode failed: {e:?}"))?;

            info!("Warmup complete: {} prefix tokens locked in KV cache", n);
            n
        };

        self.prefix_token_count = Some(n_tokens);
        Ok(())
    }

    /// Get the number of prefix tokens locked in the KV cache.
    pub fn prefix_token_count(&self) -> Option<usize> {
        self.prefix_token_count
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
            // Catch any panics from llama-cpp (debug assertions, etc.)
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut guard = slot.blocking_write();
                let ModelSlot {
                    ref mut ctx,
                    ref model,
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

                // Build sampler chain — match Python's llama-cpp-python defaults:
                // penalties(repeat=1.2) → top_k(40) → top_p(0.9) → min_p(0.05) → temp → dist
                let seed: u32 = rand::random();
                let mut sampler = if temperature <= 0.0 {
                    LlamaSampler::greedy()
                } else {
                    LlamaSampler::chain(
                        [
                            LlamaSampler::penalties(64, 1.2, 0.0, 0.0),
                            LlamaSampler::top_k(40),
                            LlamaSampler::top_p(0.9, 1),
                            LlamaSampler::min_p(0.05, 1),
                            LlamaSampler::temp(temperature),
                            LlamaSampler::dist(seed),
                        ],
                        false,
                    )
                };

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
            }));

            match result {
                Ok(inner) => inner,
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "unknown panic in llama-cpp inference".to_string()
                    };
                    Err(anyhow::anyhow!("Inference panic caught: {msg}"))
                }
            }
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
            "\nOBSERVATION".to_string(),
        ];

        let prompt = format!(
            "<|im_start|>system\n{system_prompt}<|im_end|>\n\
             <|im_start|>user\n{user_message}<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        info!(
            "[generate_with_system] system_prompt={} chars, user_message={} chars, \
             total_chatml={} chars, temp={}, max_tokens={}",
            system_prompt.len(),
            user_message.len(),
            prompt.len(),
            temperature,
            max_tokens,
        );
        info!(
            "[generate_with_system] user_message:\n{}",
            user_message,
        );

        let result = self
            .generate_with_stops(&prompt, target, max_tokens, temperature, &stop_sequences)
            .await?;

        info!(
            "[generate_with_system] output={} chars:\n{}",
            result.len(),
            &result,
        );
        Ok(result)
    }

    /// Generate text with the KV-cached system prefix preserved.
    ///
    /// Instead of clearing the entire KV cache and re-evaluating the full
    /// ChatML prompt (system + user), this method:
    /// 1. Keeps the prefix tokens (evaluated during `warmup_cache()`) in the KV cache
    /// 2. Clears only the KV entries after the prefix
    /// 3. Tokenizes and evaluates only the dynamic user turn
    /// 4. Generates output tokens
    ///
    /// This saves ~3,000 tokens of re-evaluation on every request.
    ///
    /// Falls back to `generate_with_system` if the KV cache hasn't been
    /// warmed up (i.e., `prefix_token_count` is None).
    pub async fn generate_with_cached_prefix(
        &self,
        system_prompt: &str,
        user_message: &str,
        target: ModelTarget,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String> {
        // Fall back if no prefix is cached.
        let prefix_len = match self.prefix_token_count {
            Some(n) => n,
            None => {
                info!("[cached_prefix] No prefix cached, falling back to full generation");
                return self
                    .generate_with_system(system_prompt, user_message, target, max_tokens, temperature)
                    .await;
            }
        };

        let stop_sequences: Vec<String> = vec![
            "\nOBSERVATION:".to_string(),
            "\nOBSERVATION".to_string(),
        ];

        // Only the user turn needs to be tokenized and evaluated fresh.
        // The system prefix is already in the KV cache from warmup.
        let user_turn = format!(
            "<|im_start|>user\n{user_message}<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        info!(
            "[cached_prefix] prefix={} tokens (cached), user_turn={} chars, temp={}, max_tokens={}",
            prefix_len, user_turn.len(), temperature, max_tokens,
        );

        let slot = self.slot_for(target)?;
        let stops = stop_sequences;

        tokio::task::spawn_blocking(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut guard = slot.blocking_write();
                let ModelSlot { ref mut ctx, ref model } = *guard;

                // Clear only KV entries AFTER the prefix.
                // This preserves the warmed-up system prompt tokens.
                let _ = ctx.clear_kv_cache_seq(
                    Some(0),                     // seq_id 0
                    Some(prefix_len as u32),      // from prefix_len
                    None,                         // to end
                );

                // Tokenize only the dynamic user turn.
                let user_tokens = model
                    .str_to_token(&user_turn, AddBos::Never)
                    .map_err(|e| anyhow::anyhow!("user turn tokenization failed: {e:?}"))?;

                if user_tokens.is_empty() {
                    anyhow::bail!("user turn tokenization produced 0 tokens");
                }

                info!(
                    "[cached_prefix] user turn: {} tokens (prefix already cached: {} tokens)",
                    user_tokens.len(), prefix_len,
                );

                // Feed user tokens starting at position prefix_len.
                let mut batch = LlamaBatch::new(user_tokens.len(), 1);
                for (i, &tok) in user_tokens.iter().enumerate() {
                    let pos = (prefix_len + i) as i32;
                    let is_last = i == user_tokens.len() - 1;
                    batch
                        .add(tok, pos, &[0], is_last)
                        .map_err(|e| anyhow::anyhow!("user batch add failed: {e:?}"))?;
                }

                ctx.decode(&mut batch)
                    .map_err(|e| anyhow::anyhow!("user turn decode failed: {e:?}"))?;

                // Build sampler chain.
                let seed: u32 = rand::random();
                let mut sampler = if temperature <= 0.0 {
                    LlamaSampler::greedy()
                } else {
                    LlamaSampler::chain(
                        [
                            LlamaSampler::penalties(64, 1.2, 0.0, 0.0),
                            LlamaSampler::top_k(40),
                            LlamaSampler::top_p(0.9, 1),
                            LlamaSampler::min_p(0.05, 1),
                            LlamaSampler::temp(temperature),
                            LlamaSampler::dist(seed),
                        ],
                        false,
                    )
                };

                // Autoregressive generation with stop sequence detection.
                let eos = model.token_eos();
                let mut n_decoded = (prefix_len + user_tokens.len()) as i32;
                let mut decoder = encoding_rs::UTF_8.new_decoder();
                let mut output = String::new();
                let mut stopped = false;

                for _ in 0..max_tokens {
                    let new_token = sampler.sample(ctx, -1);
                    sampler.accept(new_token);

                    if new_token == eos {
                        break;
                    }

                    let piece = model
                        .token_to_piece(new_token, &mut decoder, false, None)
                        .unwrap_or_default();
                    output.push_str(&piece);

                    // Check stop sequences.
                    for stop in &stops {
                        if output.ends_with(stop.as_str()) {
                            let trimmed_len = output.len() - stop.len();
                            output.truncate(trimmed_len);
                            stopped = true;
                            break;
                        }
                    }
                    if stopped {
                        break;
                    }

                    let mut next_batch = LlamaBatch::new(1, 1);
                    next_batch
                        .add(new_token, n_decoded, &[0], true)
                        .map_err(|e| anyhow::anyhow!("next batch failed: {e:?}"))?;
                    ctx.decode(&mut next_batch)
                        .map_err(|e| anyhow::anyhow!("decode step failed: {e:?}"))?;
                    n_decoded += 1;
                }

                Ok(output)
            }));

            match result {
                Ok(inner) => inner,
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "unknown panic in llama-cpp inference".to_string()
                    };
                    Err(anyhow::anyhow!("Inference panic caught: {msg}"))
                }
            }
        })
        .await?
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
            // Catch any panics from llama-cpp (debug assertions, etc.)
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut guard = slot.blocking_write();
                let ModelSlot {
                    ref mut ctx,
                    ref model,
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

                // Build sampler chain — match Python's llama-cpp-python defaults:
                // penalties(repeat=1.2) → top_k(40) → top_p(0.9) → min_p(0.05) → temp → dist
                let seed: u32 = rand::random();
                let mut sampler = if temperature <= 0.0 {
                    LlamaSampler::greedy()
                } else {
                    LlamaSampler::chain(
                        [
                            LlamaSampler::penalties(64, 1.2, 0.0, 0.0),
                            LlamaSampler::top_k(40),
                            LlamaSampler::top_p(0.9, 1),
                            LlamaSampler::min_p(0.05, 1),
                            LlamaSampler::temp(temperature),
                            LlamaSampler::dist(seed),
                        ],
                        false,
                    )
                };

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
            }));

            match result {
                Ok(inner) => inner,
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "unknown panic in llama-cpp inference".to_string()
                    };
                    Err(anyhow::anyhow!("Inference panic caught: {msg}"))
                }
            }
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
