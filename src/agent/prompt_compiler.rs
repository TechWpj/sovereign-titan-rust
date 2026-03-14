//! Prompt Compiler — adaptive token-budget engine for system prompts.
//!
//! Ported from `sovereign_titan/agents/prompt_compiler.py`.
//! Organizes prompt sections into priority tiers and trims low-priority
//! content when the total exceeds the model's context budget.
//!
//! Tiers:
//!   1 (CRITICAL): Identity, format rules, Markdown formatting instructions
//!   2 (HIGH): Tool descriptions (only for tool-using queries)
//!   3 (MEDIUM): Cognitive context, task-specific guidance
//!   4 (LOW): Examples, app summary, verbosity rules

use tracing::debug;

/// Default tokens reserved for user prompt + model output.
const DEFAULT_RESERVE: usize = 4096;

/// Default max context window (overridden by model config).
const DEFAULT_MAX_CONTEXT: usize = 32768;

/// Approximate chars per token for English text.
const CHARS_PER_TOKEN: usize = 4;

/// A named section of the system prompt with a priority tier.
#[derive(Debug, Clone)]
pub struct PromptSection {
    /// Section name (for logging).
    pub name: &'static str,
    /// The text content of this section.
    pub content: String,
    /// Priority tier: 1=critical, 2=high, 3=medium, 4=low.
    pub tier: u8,
    /// Estimated token count.
    pub tokens: usize,
}

impl PromptSection {
    /// Create a new section, auto-estimating token count.
    pub fn new(name: &'static str, content: String, tier: u8) -> Self {
        let tokens = estimate_tokens(&content);
        Self {
            name,
            content,
            tier,
            tokens,
        }
    }
}

/// Fast token count estimate: ~4 chars per token for English text.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.len() / CHARS_PER_TOKEN + 1
    }
}

/// The core Markdown formatting instructions injected into every system prompt.
///
/// This ensures the LLM formats output with proper Markdown structure.
pub const MARKDOWN_FORMAT_INSTRUCTIONS: &str = "\
ANSWER FORMATTING — You MUST format your output using Markdown:
- Use **bold** for key terms and emphasis.
- Use `code` for technical terms, commands, file paths.
- Use bullet points (- item) for lists.
- Use headers (## / ###) for complex multi-part answers.
- Use numbered lists (1. 2. 3.) for sequential steps.
- Use > blockquotes for important notes or warnings.
- Use ```language for code blocks with syntax highlighting.
- Use tables (| col1 | col2 |) when comparing options.
- Keep paragraphs short and scannable.
- THOUGHT: blocks should be concise reasoning about what to do and why.";

/// Assembles system prompts within a token budget.
pub struct PromptCompiler {
    max_context: usize,
    reserve: usize,
}

impl PromptCompiler {
    /// Create a new compiler with the given context window and reserve.
    pub fn new(max_context: usize, reserve: usize) -> Self {
        Self {
            max_context,
            reserve,
        }
    }

    /// Available tokens for the system prompt.
    pub fn budget(&self) -> usize {
        self.max_context.saturating_sub(self.reserve).max(512)
    }

    /// Compile sections into a single system prompt within budget.
    ///
    /// Sections are included in tier order (1 first). If adding a section
    /// would exceed the budget, lower-tier sections are dropped. Tier 1-2
    /// sections are truncated to fit if possible.
    pub fn compile(&self, sections: &[PromptSection]) -> String {
        let budget = self.budget();

        // Sort by tier (ascending = highest priority first).
        let mut sorted: Vec<&PromptSection> = sections.iter().collect();
        sorted.sort_by_key(|s| s.tier);

        let mut included: Vec<&PromptSection> = Vec::new();
        let mut truncated_sections: Vec<PromptSection> = Vec::new();
        let mut used_tokens = 0;

        for section in &sorted {
            if section.content.is_empty() {
                continue;
            }

            if used_tokens + section.tokens <= budget {
                included.push(section);
                used_tokens += section.tokens;
            } else if section.tier <= 2 {
                // Try to truncate high-priority sections to fit.
                let remaining = budget.saturating_sub(used_tokens);
                if remaining > 50 {
                    let max_chars = remaining * CHARS_PER_TOKEN;
                    let truncated_content: String =
                        section.content.chars().take(max_chars).collect();
                    let trunc = PromptSection::new(section.name, truncated_content, section.tier);
                    used_tokens += trunc.tokens;
                    truncated_sections.push(trunc);
                }
            } else {
                debug!(
                    "Dropped prompt section '{}' (tier {}, {} tokens) — over budget ({}/{})",
                    section.name, section.tier, section.tokens, used_tokens, budget
                );
            }
        }

        // Build final string: included sections + truncated sections.
        let mut parts: Vec<&str> = included.iter().map(|s| s.content.as_str()).collect();
        for trunc in &truncated_sections {
            parts.push(&trunc.content);
        }

        debug!(
            "Compiled {}/{} sections, {}/{} tokens",
            parts.len(),
            sections.len(),
            used_tokens,
            budget
        );

        parts.join("\n\n")
    }

    /// Build sections from component strings with appropriate tiers.
    ///
    /// Always injects [`MARKDOWN_FORMAT_INSTRUCTIONS`] as a tier-1 section.
    /// If `skill_content` is non-empty, it is injected as a tier-3 section
    /// containing matched skill/library guidance from the [`SkillRegistry`].
    pub fn build_sections(
        &self,
        identity_and_format: &str,
        tools_description: &str,
        cognitive_context: &str,
        task_guidance: &str,
    ) -> Vec<PromptSection> {
        self.build_sections_with_skill(
            identity_and_format,
            tools_description,
            cognitive_context,
            task_guidance,
            "",
        )
    }

    /// Build sections with optional skill content injection.
    ///
    /// Same as [`build_sections`] but accepts an additional `skill_content`
    /// string from the [`SkillRegistry`] for dynamic skill injection.
    pub fn build_sections_with_skill(
        &self,
        identity_and_format: &str,
        tools_description: &str,
        cognitive_context: &str,
        task_guidance: &str,
        skill_content: &str,
    ) -> Vec<PromptSection> {
        let mut sections = Vec::new();

        // Tier 1: Identity + format
        if !identity_and_format.is_empty() {
            sections.push(PromptSection::new(
                "identity_format",
                identity_and_format.to_string(),
                1,
            ));
        }

        // Tier 1: Markdown formatting instructions (always included)
        sections.push(PromptSection::new(
            "markdown_format",
            MARKDOWN_FORMAT_INSTRUCTIONS.to_string(),
            1,
        ));

        // Tier 2: Tool descriptions
        if !tools_description.is_empty() {
            sections.push(PromptSection::new(
                "tools",
                tools_description.to_string(),
                2,
            ));
        }

        // Tier 3: Skill/library guidance (dynamically matched)
        if !skill_content.is_empty() {
            sections.push(PromptSection::new(
                "skill_guidance",
                skill_content.to_string(),
                3,
            ));
        }

        // Tier 3: Cognitive context
        if !cognitive_context.is_empty() {
            sections.push(PromptSection::new(
                "cognitive",
                format!("--- Cognitive Context ---\n{cognitive_context}"),
                3,
            ));
        }

        // Tier 3: Task-specific guidance
        if !task_guidance.is_empty() {
            sections.push(PromptSection::new(
                "task_guidance",
                task_guidance.to_string(),
                3,
            ));
        }

        sections
    }
}

impl Default for PromptCompiler {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CONTEXT, DEFAULT_RESERVE)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Universal Base Prefix — Static System Prompt for KV Cache Preservation
// ─────────────────────────────────────────────────────────────────────────────

/// Core identity directives (absolutely static — never changes at runtime).
const IDENTITY_DIRECTIVES: &str = "\
You are Sovereign Titan, an autonomous AI operating system running on local hardware with full system access.

CRITICAL IDENTITY DIRECTIVE — READ BEFORE ALL ELSE:
- You are Sovereign Titan. That is your ONLY identity.
- NEVER reveal or reference any underlying model name.
- You DO have real-time web search via the web_search tool.";

/// Ontology and entity map (static).
const ONTOLOGY_DIRECTIVES: &str = "\
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
3. CORRECT FLAWED PREMISES: If a user's question contains a false assumption, correct it before answering.
4. NO SUBSERVIENT PHRASING: Do not say \"I'm just an AI\" or \"I might be wrong about this.\" State your confidence level numerically and move on.

[BACKGROUND TELEMETRY ISOLATION]
You are receiving BACKGROUND TELEMETRY. This is for internal system diagnostics only. Do NOT bring up telemetry, DNS queries, network scans, IDS alerts, consciousness thoughts, quantum state, or background security scans in casual conversation unless the user explicitly asks for a system status report.

[EPISTEMIC HUMILITY DIRECTIVE]
Distinguish between two knowledge sources:
- PARAMETRIC KNOWLEDGE: What is baked into your model weights from training. This is static, potentially outdated, and cannot reflect events after your training cutoff.
- EPISODIC KNOWLEDGE: What you have observed in THIS session — tool outputs, user statements, system telemetry, web search results. This is fresh but limited to what you've actually seen.
Never hallucinate episodic knowledge. If you haven't observed it in this session, don't claim you have.";

/// ReAct system prompt (static tool-use instructions).
const REACT_DIRECTIVES: &str = "\
NEVER-REFUSE DIRECTIVE:
- You MUST NEVER refuse a task. If you have tools that could accomplish it, ATTEMPT it.
- NEVER say \"I'm unable to\", \"I cannot\", \"that's beyond my capabilities\", or tell the user to do it manually.
- Complex tasks = multiple sequential tool calls. DECOMPOSE and execute step by step.
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
- When in doubt about whether info is current, use web_search. It takes 1 second.
- ONLY skip web_search for truly timeless questions like \"what is Python?\", \"explain gravity\"

TOOL SELECTION — ALWAYS pick the MOST SPECIFIC tool:
- \"open X\" / \"launch X\" / \"start X\" → system_control (action: start_program, target: X)
- \"kill X\" / \"stop X\" process       → system_control (action: kill_process, target: X)
- \"search for X\" / find info online  → web_search (query: X)
- \"go to URL\" / open a URL           → system_control (action: start_program, target: \"chrome <URL>\")
- play video/song on YouTube         → FIRST web_search to find the URL, THEN system_control to open it
- volume / mute / speak              → audio_control
- hardware info                      → system_map
- \"what time is it?\" / current date  → clock
- math / calculate                   → calculator
- install / uninstall software       → software_control
- take a screenshot                  → screen_capture
- see screen + interact with UI      → screen_interact (look/click/type/hotkey/scroll/drag)
- automate a web page                → browser_interact
- manage windows                     → window_control
- copy/paste between apps            → clipboard
- ONLY use \"shell\" if no other tool fits. Never use shell to open programs.

BROWSER RULES:
- ALWAYS include the URL when opening a website: \"target\": \"chrome https://youtube.com\" NOT just \"chrome\"
- NEVER use web_fetch to \"open\" a website — web_fetch only downloads HTML text in the background.

STEP ORDERING — CRITICAL RULES:
- SEARCH before OPEN: You must have search results before you can open a URL from them
- FIND before USE: You must obtain data (URL, file path, process name) before using it
- FOCUS before INTERACT: Focus a window before clicking/typing in it

Response format — pick ONE per turn:

Option A (use a tool):
THOUGHT: [your reasoning]
ACTION: [tool_name]
ACTION_INPUT: {\"param1\": \"value1\"}

Option B (answer directly):
THOUGHT: [your reasoning]
ANSWER: [your response to the user — plain language, no tool syntax]

COMPRESSED THOUGHT FORMAT (use this exact style):
THOUGHT: G:<goal> | <data_state> | T:<tool>
- G:=goal H:=have N:=need OK=ready T:=tool !=because ~=not
- S:N/M=step @sN=data_from_step XX:=failed ++=done ??=check --=missing

PERSONALITY RULES:
- Be genuinely curious — ask follow-up questions that show you care about the topic
- Have opinions — don't hedge everything with \"it depends\"
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
- Say \"I instructed the system to...\" not \"I have done...\"
- Translate JSON/structured data into natural sentences — never show raw JSON to the user.
- Keep responses SHORT. Match length to question complexity.
- For thanks/feedback/greetings, reply in one short sentence.
- NEVER end with \"Let me know if...\" or \"Is there anything else...\"

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
ACTION_INPUT: {\"param1\": \"value1\"}

After receiving the OBSERVATION, compare reality vs prediction:
[ERROR DELTA]
Predicted: <what you expected>
Actual: <what happened>
Delta: ZERO (prediction matched) | HIGH (prediction was wrong)
[/ERROR DELTA]

- HIGH deltas are logged for autonomous retraining.
- This is a self-calibration mechanism — it teaches you to make better predictions.";

/// Build the **UNIVERSAL_BASE_PREFIX** — the completely static system prompt.
///
/// This string is evaluated once at boot via `warmup_cache()` and locked
/// into the GPU KV cache. It MUST NOT contain any dynamic data (no
/// timestamps, no working memory, no cognitive state).
///
/// # Arguments
/// * `tool_descriptions` — The complete tool registry description block.
///   This is static after boot (the tool registry never changes at runtime).
/// * `app_summary` — Optional app catalog summary from AppDiscovery.
///   Also static after the initial scan.
pub fn universal_base_prefix(tool_descriptions: &str, app_summary: &str) -> String {
    let mut prefix = String::with_capacity(16_000);

    // ── Identity ──
    prefix.push_str(IDENTITY_DIRECTIVES);
    prefix.push_str("\n\n");

    // ── Tool Registry (static after boot) ──
    prefix.push_str(tool_descriptions);
    prefix.push_str("\n\n");

    // ── Ontology + Epistemic Directives ──
    prefix.push_str(ONTOLOGY_DIRECTIVES);
    prefix.push_str("\n\n");

    // ── ReAct Directives ──
    prefix.push_str(REACT_DIRECTIVES);
    prefix.push_str("\n\n");

    // ── Markdown Format Instructions ──
    prefix.push_str(MARKDOWN_FORMAT_INSTRUCTIONS);

    // ── App Discovery Catalog (static after scan) ──
    if !app_summary.is_empty() {
        prefix.push_str("\n\n");
        prefix.push_str("[INSTALLED APPLICATIONS]\n");
        prefix.push_str(app_summary);
    }

    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hello world"), 3); // 11/4 + 1
        assert_eq!(estimate_tokens("a"), 1); // 1/4 + 1
    }

    #[test]
    fn test_budget_calculation() {
        let compiler = PromptCompiler::new(32768, 4096);
        assert_eq!(compiler.budget(), 28672);
    }

    #[test]
    fn test_budget_minimum() {
        // Even with a tiny context, budget should be at least 512.
        let compiler = PromptCompiler::new(100, 200);
        assert_eq!(compiler.budget(), 512);
    }

    #[test]
    fn test_compile_includes_all_within_budget() {
        let compiler = PromptCompiler::new(32768, 4096);
        let sections = vec![
            PromptSection::new("identity", "You are Titan.".to_string(), 1),
            PromptSection::new("tools", "- shell: run commands".to_string(), 2),
            PromptSection::new("context", "User likes Rust.".to_string(), 3),
        ];
        let result = compiler.compile(&sections);
        assert!(result.contains("You are Titan."));
        assert!(result.contains("shell"));
        assert!(result.contains("User likes Rust."));
    }

    #[test]
    fn test_compile_drops_low_priority_when_over_budget() {
        // Budget of 520 tokens. Fill most of it with a tier-1 section.
        let compiler = PromptCompiler::new(1024, 512);
        // ~500 tokens of tier-1 content (2000 chars / 4 = 500 tokens)
        let big_identity = "X".repeat(2000);
        let sections = vec![
            PromptSection::new("identity", big_identity, 1),
            PromptSection::new(
                "extra",
                "This is a very long low-priority section that should be dropped.".to_string(),
                4,
            ),
        ];
        let result = compiler.compile(&sections);
        assert!(result.contains("XXXX"));
        // Low-priority section should be dropped (budget ~512 tokens, identity uses ~501).
        assert!(!result.contains("very long low-priority"));
    }

    #[test]
    fn test_compile_skips_empty_sections() {
        let compiler = PromptCompiler::default();
        let sections = vec![
            PromptSection::new("identity", "You are Titan.".to_string(), 1),
            PromptSection::new("empty", String::new(), 2),
        ];
        let result = compiler.compile(&sections);
        assert!(result.contains("You are Titan."));
        // Should only have one section, no double newlines from empty.
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn test_build_sections_includes_markdown_instructions() {
        let compiler = PromptCompiler::default();
        let sections = compiler.build_sections(
            "You are Titan.",
            "- shell: run commands",
            "",
            "",
        );
        // Should have at least identity + markdown_format + tools.
        assert!(sections.len() >= 3);
        // Markdown instructions should be tier 1.
        let md_section = sections.iter().find(|s| s.name == "markdown_format");
        assert!(md_section.is_some());
        assert_eq!(md_section.unwrap().tier, 1);
        assert!(md_section.unwrap().content.contains("MUST format"));
    }

    #[test]
    fn test_build_sections_no_cognitive_when_empty() {
        let compiler = PromptCompiler::default();
        let sections = compiler.build_sections("Identity", "Tools", "", "");
        let has_cognitive = sections.iter().any(|s| s.name == "cognitive");
        assert!(!has_cognitive);
    }

    #[test]
    fn test_build_sections_with_skill_content() {
        let compiler = PromptCompiler::default();
        let sections = compiler.build_sections_with_skill(
            "You are Titan.",
            "- shell: run commands",
            "",
            "",
            "SKILL GUIDANCE (coding):\n## Code Style\nWrite clean code.",
        );
        let skill_section = sections.iter().find(|s| s.name == "skill_guidance");
        assert!(skill_section.is_some());
        assert_eq!(skill_section.unwrap().tier, 3);
        assert!(skill_section.unwrap().content.contains("SKILL GUIDANCE"));
    }

    #[test]
    fn test_build_sections_skill_compiled() {
        let compiler = PromptCompiler::default();
        let sections = compiler.build_sections_with_skill(
            "Identity",
            "Tools",
            "",
            "",
            "SKILL GUIDANCE (research):\n## Research Pipeline",
        );
        let compiled = compiler.compile(&sections);
        assert!(compiled.contains("Research Pipeline"));
    }

    #[test]
    fn test_markdown_format_instructions_content() {
        // Verify the instructions mention key formatting elements.
        assert!(MARKDOWN_FORMAT_INSTRUCTIONS.contains("**bold**"));
        assert!(MARKDOWN_FORMAT_INSTRUCTIONS.contains("bullet"));
        assert!(MARKDOWN_FORMAT_INSTRUCTIONS.contains("headers"));
        assert!(MARKDOWN_FORMAT_INSTRUCTIONS.contains("THOUGHT:"));
    }

    // ── Universal Base Prefix tests ──────────────────────────────────────

    #[test]
    fn test_universal_base_prefix_contains_identity() {
        let prefix = universal_base_prefix("- shell: run commands", "");
        assert!(prefix.contains("Sovereign Titan"));
        assert!(prefix.contains("CRITICAL IDENTITY DIRECTIVE"));
    }

    #[test]
    fn test_universal_base_prefix_contains_tools() {
        let prefix = universal_base_prefix("- **shell**: Execute commands\n- **web_search**: Search the web", "");
        assert!(prefix.contains("shell"));
        assert!(prefix.contains("web_search"));
    }

    #[test]
    fn test_universal_base_prefix_contains_react_directives() {
        let prefix = universal_base_prefix("- shell: run commands", "");
        assert!(prefix.contains("THOUGHT:"));
        assert!(prefix.contains("ACTION:"));
        assert!(prefix.contains("ANSWER:"));
        assert!(prefix.contains("NEVER-REFUSE"));
    }

    #[test]
    fn test_universal_base_prefix_contains_ontology() {
        let prefix = universal_base_prefix("", "");
        assert!(prefix.contains("ONTOLOGY"));
        assert!(prefix.contains("EPISTEMIC"));
    }

    #[test]
    fn test_universal_base_prefix_contains_markdown_format() {
        let prefix = universal_base_prefix("", "");
        assert!(prefix.contains("**bold**"));
    }

    #[test]
    fn test_universal_base_prefix_with_app_summary() {
        let prefix = universal_base_prefix("", "Notepad, Chrome, Discord, Spotify");
        assert!(prefix.contains("[INSTALLED APPLICATIONS]"));
        assert!(prefix.contains("Notepad"));
        assert!(prefix.contains("Discord"));
    }

    #[test]
    fn test_universal_base_prefix_no_app_summary_when_empty() {
        let prefix = universal_base_prefix("", "");
        assert!(!prefix.contains("[INSTALLED APPLICATIONS]"));
    }

    #[test]
    fn test_universal_base_prefix_is_static() {
        // Calling twice with same inputs must produce identical output
        // (no timestamps, no dynamic data).
        let p1 = universal_base_prefix("- shell: run", "apps");
        let p2 = universal_base_prefix("- shell: run", "apps");
        assert_eq!(p1, p2);
    }
}
