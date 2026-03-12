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
}
