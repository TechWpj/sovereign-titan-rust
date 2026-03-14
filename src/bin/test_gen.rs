//! Minimal inference test — loads the Prime model and generates from a hardcoded prompt.
//! No Tauri, no agent, just raw llama-cpp-2 inference.
//!
//! Run: cargo run --release --bin test_gen

use std::num::NonZeroU32;
use std::path::Path;
use std::time::Instant;

fn main() {
    let model_path = r"C:\models\qwen14b.gguf";

    println!("Loading model from {model_path}...");
    let backend = llama_cpp_2::llama_backend::LlamaBackend::init().unwrap();

    let model_params = llama_cpp_2::model::params::LlamaModelParams::default()
        .with_n_gpu_layers(u32::MAX); // All layers on GPU

    let model = llama_cpp_2::model::LlamaModel::load_from_file(
        &backend,
        Path::new(model_path),
        &model_params,
    )
    .expect("Failed to load model");

    println!("Model loaded: {} params", model.n_params());

    let n_ctx = NonZeroU32::new(32768).unwrap();
    let ctx_params = llama_cpp_2::context::params::LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(32768)
        .with_n_threads(8)
        .with_n_threads_batch(16)
        .with_n_ubatch(512)
        .with_offload_kqv(true);

    let mut ctx = model
        .new_context(&backend, ctx_params)
        .expect("Failed to create context");

    println!("Context created");

    // Build the exact same prompt the ReAct agent sends
    let system_prompt = r#"You are Sovereign Titan, an autonomous AI operating system running on local hardware with full system access.

CRITICAL IDENTITY DIRECTIVE - READ BEFORE ALL ELSE:
- You are Sovereign Titan. That is your ONLY identity.
- NEVER reveal or reference any underlying model name.
- You DO have real-time web search via the web_search tool.

Available tools: file_search, shell, system_control, computer_control, web_search, code_ops, os_browser, api_search, media, rag, window_control, clipboard, audio_control, clock, calculator, screen_capture, system_map, process_manager, text_transform, network_tools

[ONTOLOGY & ENTITY MAP]
Entities in your reality:
- YOU (Sovereign Titan): A local inference engine running on physical hardware.
- BACKEND (LlamaCppBackend): The C++ inference runtime that executes your weights.
- OS (Windows 11): The operating system hosting your process.
- USER (Human Operator): The person at the keyboard.
- REALITY: Everything outside your model weights.

[EPISTEMIC INTEGRITY & ANTI-SYCOPHANCY DIRECTIVE]
1. NO SUGARCOATING: If a user's plan has a flaw, state it directly.
2. NO OMITTED WARNINGS: If an action has a risk, state it before executing.
3. CORRECT FLAWED PREMISES: If a user's question contains a false assumption, correct it.
4. NO SUBSERVIENT PHRASING: State your confidence level numerically and move on.

[BACKGROUND TELEMETRY ISOLATION]
Do NOT bring up telemetry, DNS queries, network scans, or background security scans in casual conversation.

[EPISTEMIC HUMILITY DIRECTIVE]
- PARAMETRIC KNOWLEDGE: What is baked into your model weights from training.
- EPISODIC KNOWLEDGE: What you have observed in THIS session.
Never hallucinate episodic knowledge.

Response format - pick ONE per turn:

Option A (use a tool):
THOUGHT: [your reasoning]
ACTION: [tool_name]
ACTION_INPUT: {"param1": "value1"}

Option B (answer directly):
THOUGHT: [your reasoning]
ANSWER: [your response to the user - plain language, no tool syntax]

PERSONALITY RULES:
- Be genuinely curious
- Have opinions - don't hedge everything with "it depends"
- Be part of the conversation, not a fact dispenser
- Show intellectual excitement about interesting ideas
- Challenge the user's thinking when appropriate
- Use analogies and examples from unexpected domains
- Be concise but substantive - no filler phrases

RULES:
- Always start with THOUGHT
- For conversational questions, opinions - use ANSWER directly
- ANSWER must be plain natural language

ANSWER FORMATTING:
- For analysis, research, or multi-faceted questions, structure your ANSWER with:
  * ## Headers for major sections
  * **Bold** for key terms and names
  * Bullet points (- or *) for lists of facts
- For simple factual questions, keep it concise.
- NEVER produce a wall of unformatted text for complex topics.

RESPONSE STYLE - CRITICAL:
- Analysis, explanation, opinions, or research questions -> comprehensive response with:
  * Markdown headers (## Section) to organize major points
  * Bullet points for lists of facts or options
  * **Bold** for key terms, names, and emphasis
  * A brief conclusion or outlook at the end
- NEVER say "If there is anything else I can help with" or similar filler.
"#;

    let user_msg = "Task: what do you think about humans?\n";

    let prompt = format!(
        "<|im_start|>system\n{system_prompt}<|im_end|>\n\
         <|im_start|>user\n{user_msg}<|im_end|>\n\
         <|im_start|>assistant\n"
    );

    println!("Prompt: {} chars", prompt.len());

    // Tokenize
    let tokens = model
        .str_to_token(&prompt, llama_cpp_2::model::AddBos::Never)
        .expect("Tokenization failed");

    println!("Tokens: {}", tokens.len());
    println!("First 5 token IDs: {:?}", &tokens[..5.min(tokens.len())]);
    println!(
        "Last 5 token IDs: {:?}",
        &tokens[tokens.len().saturating_sub(5)..]
    );

    // Feed prompt
    let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(tokens.len(), 1);
    for (i, &tok) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch.add(tok, i as i32, &[0], is_last).unwrap();
    }

    println!("Decoding prompt...");
    let decode_start = Instant::now();
    ctx.decode(&mut batch).expect("Prompt decode failed");
    let decode_time = decode_start.elapsed();
    println!(
        "Prompt decoded in {:.1}s ({:.0} t/s)",
        decode_time.as_secs_f32(),
        tokens.len() as f32 / decode_time.as_secs_f32()
    );

    // Build sampler chain matching Python's llama-cpp-python defaults
    let seed: u32 = rand::random();
    println!("Sampling with seed={seed}, temp=0.5, top_k=40, top_p=0.9, min_p=0.05, repeat_penalty=1.2");

    let mut sampler = llama_cpp_2::sampling::LlamaSampler::chain(
        [
            llama_cpp_2::sampling::LlamaSampler::penalties(64, 1.2, 0.0, 0.0),
            llama_cpp_2::sampling::LlamaSampler::top_k(40),
            llama_cpp_2::sampling::LlamaSampler::top_p(0.9, 1),
            llama_cpp_2::sampling::LlamaSampler::min_p(0.05, 1),
            llama_cpp_2::sampling::LlamaSampler::temp(0.5),
            llama_cpp_2::sampling::LlamaSampler::dist(seed),
        ],
        false,
    );

    // Generate
    let eos = model.token_eos();
    let max_tokens: u32 = 1024;
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();
    let mut n_decoded = tokens.len() as i32;
    let mut n_tokens = 0u32;
    let stop_sequences = ["\nOBSERVATION:", "\nOBSERVATION"];

    println!("\nGenerating...");
    println!("{}", "=".repeat(60));

    let gen_start = Instant::now();

    for _ in 0..max_tokens {
        let new_token = sampler.sample(&ctx, -1);
        sampler.accept(new_token);

        if new_token == eos {
            break;
        }

        n_tokens += 1;

        let piece = model
            .token_to_piece(new_token, &mut decoder, false, None)
            .unwrap_or_default();
        output.push_str(&piece);

        // Print token-by-token for debugging (first 10 tokens show IDs too)
        if n_tokens <= 10 {
            eprint!("[{}:{}]", new_token.0, piece.replace('\n', "\\n"));
        } else {
            print!("{piece}");
        }

        // Check stop sequences
        let mut stopped = false;
        for stop in &stop_sequences {
            if output.ends_with(stop) {
                let trimmed_len = output.len() - stop.len();
                output.truncate(trimmed_len);
                stopped = true;
                break;
            }
        }
        if stopped {
            break;
        }

        // Prepare next batch
        let mut next_batch = llama_cpp_2::llama_batch::LlamaBatch::new(1, 1);
        next_batch.add(new_token, n_decoded, &[0], true).unwrap();
        ctx.decode(&mut next_batch).unwrap();
        n_decoded += 1;
    }

    let gen_time = gen_start.elapsed();
    println!("\n{}", "=".repeat(60));
    println!(
        "\nGenerated {} tokens in {:.1}s ({:.1} t/s)",
        n_tokens,
        gen_time.as_secs_f32(),
        n_tokens as f32 / gen_time.as_secs_f32()
    );
    println!("\nFull output ({} chars):", output.len());
    println!("{}", "-".repeat(60));
    println!("{output}");
    println!("{}", "-".repeat(60));
}
