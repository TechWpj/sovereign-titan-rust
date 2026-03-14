//! Isolated model worker — runs a single 3B model in its own OS process.
//!
//! Usage:
//!   titan_worker subconscious   # Load the 3B subconscious model
//!   titan_worker warden         # Load the 3B warden model
//!
//! Communication: JSON-over-stdio (newline-delimited).
//! The parent process sends `WorkerRequest` objects via stdin,
//! and the worker replies with `WorkerResponse` objects on stdout.

use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;

use anyhow::{Context, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

// Import the shared IPC types from the main crate.
// Since we're a binary in the same package, we use the crate path.
#[path = "../ipc.rs"]
mod ipc;

#[path = "../config.rs"]
mod config;

use ipc::{WorkerRequest, WorkerResponse, WorkerRole};

fn main() -> Result<()> {
    // Parse role from CLI args.
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: titan_worker <subconscious|warden>");
        std::process::exit(1);
    }

    let role = WorkerRole::from_str(&args[1]).unwrap_or_else(|| {
        eprintln!("Unknown role '{}'. Use 'subconscious' or 'warden'.", args[1]);
        std::process::exit(1);
    });

    eprintln!("[titan_worker] Starting as {} worker", role.as_str());

    // Load config to get model paths.
    let _ = dotenvy::dotenv();
    let config = config::TitanConfig::from_env()
        .context("Failed to load config")?;

    let descriptor = match role {
        WorkerRole::Subconscious => &config.subconscious,
        WorkerRole::Warden => &config.warden,
    };

    // Resolve model path.
    let model_path = if let Some(ref explicit) = descriptor.path {
        std::path::PathBuf::from(explicit)
    } else {
        // Try finetuned_models/ directory or HuggingFace cache.
        let local = std::path::PathBuf::from("finetuned_models").join(&descriptor.filename);
        if local.exists() {
            local
        } else {
            // Try HuggingFace cache.
            let cache_dir = dirs_next::cache_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("huggingface/hub");
            let repo_dir = cache_dir.join(format!(
                "models--{}",
                descriptor.repo_id.replace('/', "--")
            ));
            // Search for the GGUF in blobs.
            let blobs_dir = repo_dir.join("blobs");
            if blobs_dir.exists() {
                // Find any blob file (they're named by hash).
                let mut found = None;
                if let Ok(entries) = std::fs::read_dir(&blobs_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            found = Some(path);
                            break;
                        }
                    }
                }
                found.unwrap_or_else(|| {
                    eprintln!("[titan_worker] Model not found for {}", role.as_str());
                    std::process::exit(1);
                })
            } else {
                eprintln!(
                    "[titan_worker] Model not found: {}/{}",
                    descriptor.repo_id, descriptor.filename
                );
                std::process::exit(1);
            }
        }
    };

    eprintln!(
        "[titan_worker] Loading model: {} (gpu_layers={}, ctx={})",
        model_path.display(),
        descriptor.gpu_layers,
        descriptor.context_length
    );

    // Initialize llama backend and load the model.
    let backend = LlamaBackend::init()?;

    let gpu_layers = if descriptor.gpu_layers < 0 {
        u32::MAX
    } else {
        descriptor.gpu_layers as u32
    };

    let model_params = LlamaModelParams::default()
        .with_n_gpu_layers(gpu_layers)
        .with_use_mlock(config.use_mlock);

    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .map_err(|e| anyhow::anyhow!("Failed to load model: {e:?}"))?;

    let n_ctx = NonZeroU32::new(descriptor.context_length)
        .unwrap_or(NonZeroU32::new(8192).unwrap());

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(descriptor.context_length)
        .with_n_threads(config.threads as i32)
        .with_n_threads_batch(config.threads_batch as i32)
        .with_n_ubatch(config.n_ubatch)
        .with_offload_kqv(descriptor.gpu_layers != 0);

    let mut ctx = model
        .new_context(&backend, ctx_params)
        .map_err(|e| anyhow::anyhow!("Failed to create context: {e:?}"))?;

    eprintln!(
        "[titan_worker] {} model loaded ({} params)",
        role.as_str(),
        model.n_params()
    );

    // ── Main loop: read JSON requests from stdin, respond on stdout ──
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut writer = stdout.lock();

    for line in reader.lines() {
        let line = line.context("Failed to read from stdin")?;
        if line.trim().is_empty() {
            continue;
        }

        let request: WorkerRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = WorkerResponse::Error {
                    message: format!("Invalid JSON: {e}"),
                };
                writeln!(writer, "{}", serde_json::to_string(&resp)?)?;
                writer.flush()?;
                continue;
            }
        };

        let response = match request {
            WorkerRequest::Ping => WorkerResponse::Ok,

            WorkerRequest::Shutdown => {
                eprintln!("[titan_worker] Shutdown requested, exiting");
                let resp = WorkerResponse::Ok;
                writeln!(writer, "{}", serde_json::to_string(&resp)?)?;
                writer.flush()?;
                break;
            }

            WorkerRequest::Generate {
                prompt,
                max_tokens,
                temperature,
            } => {
                match generate(&model, &mut ctx, &prompt, max_tokens, temperature) {
                    Ok((text, tokens)) => WorkerResponse::Generated { text, tokens },
                    Err(e) => WorkerResponse::Error {
                        message: format!("{e:#}"),
                    },
                }
            }
        };

        writeln!(writer, "{}", serde_json::to_string(&response)?)?;
        writer.flush()?;
    }

    eprintln!("[titan_worker] {} worker exiting", role.as_str());
    Ok(())
}

/// Generate text from the loaded model.
fn generate(
    model: &LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
) -> Result<(String, u32)> {
    // Clear KV cache.
    ctx.clear_kv_cache();

    // Tokenize.
    let tokens = model
        .str_to_token(prompt, AddBos::Never)
        .map_err(|e| anyhow::anyhow!("Tokenization failed: {e:?}"))?;

    if tokens.is_empty() {
        anyhow::bail!("Tokenization produced 0 tokens");
    }

    // Feed prompt.
    let mut batch = LlamaBatch::new(tokens.len(), 1);
    for (i, &tok) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(tok, i as i32, &[0], is_last)
            .map_err(|e| anyhow::anyhow!("Batch add failed: {e:?}"))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| anyhow::anyhow!("Prompt decode failed: {e:?}"))?;

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

    // Generate.
    let eos = model.token_eos();
    let mut n_decoded = tokens.len() as i32;
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();
    let mut token_count: u32 = 0;

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
        token_count += 1;

        let mut next_batch = LlamaBatch::new(1, 1);
        next_batch
            .add(new_token, n_decoded, &[0], true)
            .map_err(|e| anyhow::anyhow!("Next batch failed: {e:?}"))?;
        ctx.decode(&mut next_batch)
            .map_err(|e| anyhow::anyhow!("Decode step failed: {e:?}"))?;
        n_decoded += 1;
    }

    Ok((output, token_count))
}
