//! Skill Registry — discovers and matches user queries to baked-in skill/library content.
//!
//! All skill files (`src/prompts/skills/*.txt`) and prompt library files
//! (`src/prompts/library/*.md`) are baked into the binary at compile time
//! via `include_str!`. The registry uses a two-phase matcher:
//!
//! 1. **Regex patterns** for high-confidence matches (score >= 6.0)
//! 2. **Keyword overlap** between query and skill descriptions as fallback
//!
//! Matched content is returned for injection into the system prompt as a
//! Tier-3 section via the [`PromptCompiler`](super::prompt_compiler::PromptCompiler).
//!
//! Ported from Python `skill_registry.py`.

use regex::Regex;
use tracing::debug;

// ─────────────────────────────────────────────────────────────────────────────
// Baked-in content via include_str!
// ─────────────────────────────────────────────────────────────────────────────

// Prompt library (*.md)
const LIB_ANALYSIS: &str = include_str!("../prompts/library/analysis.md");
const LIB_CODING: &str = include_str!("../prompts/library/coding.md");
const LIB_CREATIVE_WRITING: &str = include_str!("../prompts/library/creative_writing.md");
const LIB_DATA_EXTRACTION: &str = include_str!("../prompts/library/data_extraction.md");
const LIB_EDUCATION: &str = include_str!("../prompts/library/education_tutoring.md");
const LIB_EMAIL: &str = include_str!("../prompts/library/email_communication.md");
const LIB_EXTERNAL_AI: &str = include_str!("../prompts/library/external_ai.md");
const LIB_GENERAL: &str = include_str!("../prompts/library/general.md");
const LIB_LEGAL: &str = include_str!("../prompts/library/legal_drafting.md");
const LIB_MATH_SCIENCE: &str = include_str!("../prompts/library/math_science.md");
const LIB_MEDICAL: &str = include_str!("../prompts/library/medical_health.md");
const LIB_OPINION: &str = include_str!("../prompts/library/opinion.md");
const LIB_PROFESSIONAL: &str = include_str!("../prompts/library/professional_documents.md");
const LIB_RESEARCH_SOURCING: &str = include_str!("../prompts/library/research_sourcing.md");
const LIB_SIMPLE_QA: &str = include_str!("../prompts/library/simple_qa.md");
const LIB_TECHNICAL_WRITING: &str = include_str!("../prompts/library/technical_writing.md");
const LIB_TOOL_USE: &str = include_str!("../prompts/library/tool_use.md");

// Skills (*.txt)
const SKILL_ALGORITHMIC_ART: &str = include_str!("../prompts/skills/algorithmic-artskill.txt");
const SKILL_CANVAS_DESIGN: &str = include_str!("../prompts/skills/canvas-designskill.txt");
const SKILL_DEEP_PIPELINE: &str = include_str!("../prompts/skills/deep-pipelineskill.txt");
const SKILL_DOC_COAUTHORING: &str = include_str!("../prompts/skills/doc-coauthoringskill.txt");
const SKILL_FRONTEND_DESIGN: &str = include_str!("../prompts/skills/frontend-designskill.txt");
const SKILL_INTERNAL_COMMS: &str = include_str!("../prompts/skills/internalcommsskill.txt");
const SKILL_MCP_BUILDER: &str = include_str!("../prompts/skills/mcp-builderskill.txt");
const SKILL_PDF: &str = include_str!("../prompts/skills/pdfskill.txt");
const SKILL_PPTX: &str = include_str!("../prompts/skills/pptxskill.txt");
const SKILL_PRODUCT_KNOWLEDGE: &str = include_str!("../prompts/skills/product-self-knowledgeskill.txt");
const SKILL_RESEARCH: &str = include_str!("../prompts/skills/researchskill.txt");
const SKILL_SKILL_CREATOR: &str = include_str!("../prompts/skills/skill-creator.txt");
const SKILL_DOCUMENT: &str = include_str!("../prompts/skills/Skill.txt");
const SKILL_SOURCE: &str = include_str!("../prompts/skills/sourceskill.txt");
const SKILL_THEME_MAKER: &str = include_str!("../prompts/skills/theme-makerskill.txt");
const SKILL_THEMES_DIR: &str = include_str!("../prompts/skills/themesdirectory.txt");
const SKILL_WEB_ARTIFACTS: &str = include_str!("../prompts/skills/web-artifactsskill.txt");
const SKILL_XLSX: &str = include_str!("../prompts/skills/xlsxskill.txt");

/// Max characters of skill content to inject (~3500 tokens).
const MAX_SKILL_CHARS: usize = 14_000;

/// Minimum keyword-overlap score for fallback matching.
const KEYWORD_THRESHOLD: f32 = 3.5;

// ─────────────────────────────────────────────────────────────────────────────
// Skill entry
// ─────────────────────────────────────────────────────────────────────────────

/// A registered skill or library entry.
struct SkillEntry {
    /// Canonical name (e.g., "research", "coding").
    name: &'static str,
    /// Keywords extracted from the description for fallback matching.
    keywords: &'static [&'static str],
    /// The raw baked-in content.
    content: &'static str,
}

// ─────────────────────────────────────────────────────────────────────────────
// Regex patterns — high-confidence matching
// ─────────────────────────────────────────────────────────────────────────────

/// A regex pattern with a match weight.
struct SkillPattern {
    skill_name: &'static str,
    pattern: &'static str,
    weight: f32,
}

/// All high-confidence regex patterns, ported from Python `_SKILL_PATTERNS`.
const PATTERNS: &[SkillPattern] = &[
    // product-self-knowledge
    SkillPattern { skill_name: "product-self-knowledge", pattern: r"(?i)\b(?:what are you|who (?:made|created|built) you|what can you do)\b", weight: 10.0 },
    SkillPattern { skill_name: "product-self-knowledge", pattern: r"(?i)\b(?:tell me about|what is)\b.+\b(?:titan|sovereign)\b", weight: 10.0 },
    SkillPattern { skill_name: "product-self-knowledge", pattern: r"(?i)\b(?:sovereign titan|meredosia)\b", weight: 8.0 },
    SkillPattern { skill_name: "product-self-knowledge", pattern: r"(?i)\bhow does (?:titan|this system|sovereign) work\b", weight: 8.0 },
    // docx
    SkillPattern { skill_name: "docx", pattern: r"(?i)\b(?:create|write|generate|make|build)\b.+\b(?:word\s*doc|docx|\.docx)\b", weight: 10.0 },
    SkillPattern { skill_name: "docx", pattern: r"(?i)\b(?:create|write|generate|make|build)\b.+\b(?:microsoft\s*(?:word|document))\b", weight: 10.0 },
    SkillPattern { skill_name: "docx", pattern: r"(?i)\b(?:word\s*document|docx\s*file)\b", weight: 8.0 },
    SkillPattern { skill_name: "docx", pattern: r"(?i)\b(?:create|write|generate)\b.+\b(?:document|report|memo)\b.+\b(?:about|on|for|regarding)\b", weight: 6.0 },
    // pdf
    SkillPattern { skill_name: "pdf", pattern: r"(?i)\b(?:create|generate|build)\b.+\bpdf\b", weight: 8.0 },
    SkillPattern { skill_name: "pdf", pattern: r"(?i)\bpdf\s*(?:document|file|report)\b", weight: 7.0 },
    // pptx
    SkillPattern { skill_name: "pptx", pattern: r"(?i)\b(?:create|generate|build|make)\b.+\b(?:presentation|powerpoint|pptx|slide\s*deck|slides)\b", weight: 10.0 },
    SkillPattern { skill_name: "pptx", pattern: r"(?i)\b(?:powerpoint|pptx|slide\s*deck)\b", weight: 8.0 },
    // xlsx
    SkillPattern { skill_name: "xlsx", pattern: r"(?i)\b(?:create|generate|build|make)\b.+\b(?:spreadsheet|excel|xlsx)\b", weight: 10.0 },
    SkillPattern { skill_name: "xlsx", pattern: r"(?i)\b(?:excel|xlsx|spreadsheet)\s*(?:file|document)?\b", weight: 7.0 },
    // frontend-design
    SkillPattern { skill_name: "frontend-design", pattern: r"(?i)\b(?:build|create|design|make)\b.+\b(?:website|landing\s*page|dashboard|web\s*(?:page|app|component|ui)|frontend)\b", weight: 8.0 },
    SkillPattern { skill_name: "frontend-design", pattern: r"(?i)\b(?:html|css|react|vue)\b.+\b(?:page|component|layout|design)\b", weight: 6.0 },
    // doc-coauthoring
    SkillPattern { skill_name: "doc-coauthoring", pattern: r"(?i)\b(?:co-?author|collaborate)\b.+\b(?:doc|document|writing)\b", weight: 10.0 },
    SkillPattern { skill_name: "doc-coauthoring", pattern: r"(?i)\b(?:write|draft|create)\b.+\b(?:proposal|spec|rfc|prd|design\s*doc|decision\s*doc|technical\s*spec)\b", weight: 8.0 },
    // algorithmic-art
    SkillPattern { skill_name: "algorithmic-art", pattern: r"(?i)\b(?:generative|algorithmic)\s*art\b", weight: 10.0 },
    SkillPattern { skill_name: "algorithmic-art", pattern: r"(?i)\bp5\.?js\b", weight: 8.0 },
    SkillPattern { skill_name: "algorithmic-art", pattern: r"(?i)\b(?:flow\s*field|particle\s*system|perlin\s*noise|generative\s*design)\b", weight: 7.0 },
    // canvas-design
    SkillPattern { skill_name: "canvas-design", pattern: r"(?i)\b(?:create|make|design)\b.+\b(?:poster|artwork|visual\s*art|canvas|flyer|banner)\b", weight: 8.0 },
    // mcp-builder
    SkillPattern { skill_name: "mcp-builder", pattern: r"(?i)\bmcp\s*server\b", weight: 10.0 },
    SkillPattern { skill_name: "mcp-builder", pattern: r"(?i)\bmodel\s*context\s*protocol\b", weight: 10.0 },
    SkillPattern { skill_name: "mcp-builder", pattern: r"(?i)\b(?:build|create)\b.+\bmcp\b", weight: 9.0 },
    // internal-comms
    SkillPattern { skill_name: "internal-comms", pattern: r"(?i)\b3p\s*update\b", weight: 10.0 },
    SkillPattern { skill_name: "internal-comms", pattern: r"(?i)\b(?:internal\s*comms?|company\s*newsletter|status\s*report|leadership\s*update)\b", weight: 8.0 },
    SkillPattern { skill_name: "internal-comms", pattern: r"(?i)\b(?:incident\s*report|project\s*update)\b", weight: 6.0 },
    // theme-factory
    SkillPattern { skill_name: "theme-factory", pattern: r"(?i)\b(?:apply|choose|pick|select)\b.+\btheme\b", weight: 8.0 },
    SkillPattern { skill_name: "theme-factory", pattern: r"(?i)\btheme\s*(?:showcase|factory|maker)\b", weight: 8.0 },
    // web-artifacts-builder
    SkillPattern { skill_name: "web-artifacts-builder", pattern: r"(?i)\b(?:react|shadcn)\b.+\b(?:artifact|component)\b", weight: 8.0 },
    SkillPattern { skill_name: "web-artifacts-builder", pattern: r"(?i)\bweb\s*artifact\b", weight: 8.0 },
    // research
    SkillPattern { skill_name: "research", pattern: r"(?i)\b(?:research|investigate)\b.+\b(?:document|report|paper|write[\s-]*up)\b", weight: 8.0 },
    SkillPattern { skill_name: "research", pattern: r"(?i)\b(?:write|create|generate)\b.+\b(?:research\s*(?:document|report|paper))\b", weight: 10.0 },
    SkillPattern { skill_name: "research", pattern: r"(?i)\b(?:deep|comprehensive|thorough)\s*research\b", weight: 8.0 },
    SkillPattern { skill_name: "research", pattern: r"(?i)\b(?:look\s+into|find\s+out\s+about|deep\s+dive)\b", weight: 6.0 },
    // deep-pipeline
    SkillPattern { skill_name: "deep-pipeline", pattern: r"(?i)\bresearch phase \d+ of \d+\b", weight: 10.0 },
    SkillPattern { skill_name: "deep-pipeline", pattern: r"(?i)\bdeep analysis\b", weight: 8.0 },
    // skill-creator
    SkillPattern { skill_name: "skill-creator", pattern: r"(?i)\b(?:create|write|build|make)\b.+\bskill\b", weight: 8.0 },
    SkillPattern { skill_name: "skill-creator", pattern: r"(?i)\bskill\s*(?:file|template|creator)\b", weight: 8.0 },
    // source-skill
    SkillPattern { skill_name: "source-skill", pattern: r"(?i)\b(?:academic|scholarly|peer[\s-]*review(?:ed)?)\s*(?:source|paper|journal|article)", weight: 8.0 },
    SkillPattern { skill_name: "source-skill", pattern: r"(?i)\b(?:find|search|look\s+for)\b.+\b(?:paper|study|citation|preprint)", weight: 7.0 },
    SkillPattern { skill_name: "source-skill", pattern: r"(?i)\b(?:semantic\s*scholar|arxiv|pubmed|doaj)\b", weight: 10.0 },
    SkillPattern { skill_name: "source-skill", pattern: r"(?i)\b(?:fact[\s-]*check|verify|cite|citation)\b", weight: 6.0 },
    // Library entries matched via regex
    SkillPattern { skill_name: "coding", pattern: r"(?i)\b(?:write|code|program|implement|debug|fix|refactor)\b.+\b(?:function|class|module|script|code|program|algorithm)\b", weight: 7.0 },
    SkillPattern { skill_name: "coding", pattern: r"(?i)\b(?:python|rust|javascript|typescript|java|c\+\+|golang)\b.+\b(?:code|function|class|program)\b", weight: 7.0 },
    SkillPattern { skill_name: "analysis", pattern: r"(?i)\b(?:analyze|analyse|evaluate|assess|compare)\b.+\b(?:data|trend|performance|metric|report)\b", weight: 7.0 },
    SkillPattern { skill_name: "creative-writing", pattern: r"(?i)\b(?:write|compose|draft)\b.+\b(?:story|poem|essay|narrative|fiction|novel)\b", weight: 8.0 },
    SkillPattern { skill_name: "data-extraction", pattern: r"(?i)\b(?:extract|parse|scrape)\b.+\b(?:data|table|csv|json|information)\b", weight: 7.0 },
    SkillPattern { skill_name: "education", pattern: r"(?i)\b(?:teach|explain|tutor|learn)\b.+\b(?:concept|topic|subject|lesson)\b", weight: 7.0 },
    SkillPattern { skill_name: "email", pattern: r"(?i)\b(?:write|draft|compose)\b.+\b(?:email|e-mail|mail|message|newsletter)\b", weight: 8.0 },
    SkillPattern { skill_name: "legal", pattern: r"(?i)\b(?:draft|write|create)\b.+\b(?:contract|agreement|terms|policy|legal|nda|clause)\b", weight: 8.0 },
    SkillPattern { skill_name: "math-science", pattern: r"(?i)\b(?:calculate|solve|compute|derive|prove|equation)\b", weight: 6.0 },
    SkillPattern { skill_name: "medical", pattern: r"(?i)\b(?:medical|health|symptom|diagnosis|treatment|clinical)\b.+\b(?:advice|info|question|help)\b", weight: 7.0 },
    SkillPattern { skill_name: "professional-docs", pattern: r"(?i)\b(?:write|create|draft)\b.+\b(?:resume|cv|cover\s*letter|business\s*plan|proposal)\b", weight: 8.0 },
    SkillPattern { skill_name: "technical-writing", pattern: r"(?i)\b(?:write|create|draft)\b.+\b(?:documentation|readme|api\s*doc|technical\s*doc|manual)\b", weight: 8.0 },
];

// ─────────────────────────────────────────────────────────────────────────────
// Stop words
// ─────────────────────────────────────────────────────────────────────────────

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "can", "to", "of", "in", "for", "on",
    "with", "at", "by", "from", "as", "and", "but", "or", "if", "this",
    "that", "these", "those", "it", "its", "i", "me", "my", "we", "our",
    "you", "your", "he", "she", "they", "them", "their", "what", "which",
    "who", "use", "skill", "whenever", "asked", "about", "also", "trigger",
    "includes", "questions", "like", "when", "user", "wants", "any", "not",
    "how", "than", "so", "just", "only", "very", "too", "more", "most",
    "some", "such", "no", "nor", "each", "every", "all", "both", "few",
    "other", "same", "into", "through", "during", "before", "after",
];

fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

// ─────────────────────────────────────────────────────────────────────────────
// SkillRegistry
// ─────────────────────────────────────────────────────────────────────────────

/// Registry of baked-in skills and prompt library entries.
///
/// Matches user queries to skill/library content via regex patterns and
/// keyword overlap, returning content for injection into the system prompt.
pub struct SkillRegistry {
    entries: Vec<SkillEntry>,
}

impl SkillRegistry {
    /// Create a new registry with all baked-in skills and library entries.
    pub fn new() -> Self {
        let entries = vec![
            // Skills
            SkillEntry {
                name: "product-self-knowledge",
                keywords: &["sovereign", "titan", "identity", "capabilities", "hardware", "model", "features"],
                content: SKILL_PRODUCT_KNOWLEDGE,
            },
            SkillEntry {
                name: "docx",
                keywords: &["word", "docx", "document", "microsoft", "report", "memo"],
                content: SKILL_DOCUMENT,
            },
            SkillEntry {
                name: "pdf",
                keywords: &["pdf", "document", "report", "generate"],
                content: SKILL_PDF,
            },
            SkillEntry {
                name: "pptx",
                keywords: &["presentation", "powerpoint", "pptx", "slides", "deck"],
                content: SKILL_PPTX,
            },
            SkillEntry {
                name: "xlsx",
                keywords: &["spreadsheet", "excel", "xlsx", "table", "data"],
                content: SKILL_XLSX,
            },
            SkillEntry {
                name: "frontend-design",
                keywords: &["website", "frontend", "html", "css", "react", "dashboard", "landing", "page", "component", "design", "ui"],
                content: SKILL_FRONTEND_DESIGN,
            },
            SkillEntry {
                name: "doc-coauthoring",
                keywords: &["coauthor", "collaborate", "proposal", "spec", "rfc", "prd", "technical", "decision"],
                content: SKILL_DOC_COAUTHORING,
            },
            SkillEntry {
                name: "algorithmic-art",
                keywords: &["generative", "algorithmic", "art", "p5js", "perlin", "noise", "particle", "flow"],
                content: SKILL_ALGORITHMIC_ART,
            },
            SkillEntry {
                name: "canvas-design",
                keywords: &["poster", "artwork", "canvas", "flyer", "banner", "visual"],
                content: SKILL_CANVAS_DESIGN,
            },
            SkillEntry {
                name: "mcp-builder",
                keywords: &["mcp", "server", "model", "context", "protocol"],
                content: SKILL_MCP_BUILDER,
            },
            SkillEntry {
                name: "internal-comms",
                keywords: &["internal", "comms", "newsletter", "status", "report", "leadership", "update", "incident"],
                content: SKILL_INTERNAL_COMMS,
            },
            SkillEntry {
                name: "theme-factory",
                keywords: &["theme", "color", "palette", "font", "pairing", "styling"],
                content: SKILL_THEME_MAKER,
            },
            SkillEntry {
                name: "web-artifacts-builder",
                keywords: &["react", "shadcn", "artifact", "component", "bundle", "html"],
                content: SKILL_WEB_ARTIFACTS,
            },
            SkillEntry {
                name: "research",
                keywords: &["research", "investigate", "report", "paper", "comprehensive", "thorough", "knowledge"],
                content: SKILL_RESEARCH,
            },
            SkillEntry {
                name: "deep-pipeline",
                keywords: &["deep", "analysis", "pipeline", "phase", "objectives"],
                content: SKILL_DEEP_PIPELINE,
            },
            SkillEntry {
                name: "skill-creator",
                keywords: &["skill", "file", "template", "creator"],
                content: SKILL_SKILL_CREATOR,
            },
            SkillEntry {
                name: "source-skill",
                keywords: &["academic", "scholarly", "peer", "review", "journal", "citation", "arxiv", "pubmed"],
                content: SKILL_SOURCE,
            },
            SkillEntry {
                name: "themes-directory",
                keywords: &["themes", "directory", "available", "showcase"],
                content: SKILL_THEMES_DIR,
            },
            // Prompt library
            SkillEntry {
                name: "analysis",
                keywords: &["analyze", "evaluate", "assess", "compare", "data", "trend", "metric", "performance"],
                content: LIB_ANALYSIS,
            },
            SkillEntry {
                name: "coding",
                keywords: &["code", "program", "function", "class", "debug", "implement", "algorithm", "script", "python", "rust", "javascript"],
                content: LIB_CODING,
            },
            SkillEntry {
                name: "creative-writing",
                keywords: &["story", "poem", "essay", "narrative", "fiction", "novel", "creative", "writing"],
                content: LIB_CREATIVE_WRITING,
            },
            SkillEntry {
                name: "data-extraction",
                keywords: &["extract", "parse", "scrape", "csv", "json", "table", "structured"],
                content: LIB_DATA_EXTRACTION,
            },
            SkillEntry {
                name: "education",
                keywords: &["teach", "explain", "tutor", "learn", "concept", "lesson", "student"],
                content: LIB_EDUCATION,
            },
            SkillEntry {
                name: "email",
                keywords: &["email", "mail", "message", "newsletter", "correspondence"],
                content: LIB_EMAIL,
            },
            SkillEntry {
                name: "external-ai",
                keywords: &["external", "api", "model", "integration", "chatgpt", "openai", "claude"],
                content: LIB_EXTERNAL_AI,
            },
            SkillEntry {
                name: "general",
                keywords: &["general", "help", "question", "information", "advice"],
                content: LIB_GENERAL,
            },
            SkillEntry {
                name: "legal",
                keywords: &["legal", "contract", "agreement", "terms", "policy", "nda", "clause", "law"],
                content: LIB_LEGAL,
            },
            SkillEntry {
                name: "math-science",
                keywords: &["math", "calculate", "equation", "solve", "formula", "physics", "chemistry", "science"],
                content: LIB_MATH_SCIENCE,
            },
            SkillEntry {
                name: "medical",
                keywords: &["medical", "health", "symptom", "diagnosis", "treatment", "clinical", "wellness"],
                content: LIB_MEDICAL,
            },
            SkillEntry {
                name: "opinion",
                keywords: &["opinion", "perspective", "viewpoint", "debate", "argue", "stance"],
                content: LIB_OPINION,
            },
            SkillEntry {
                name: "professional-docs",
                keywords: &["resume", "cv", "cover", "letter", "business", "plan", "proposal", "professional"],
                content: LIB_PROFESSIONAL,
            },
            SkillEntry {
                name: "research-sourcing",
                keywords: &["research", "source", "reference", "bibliography", "literature"],
                content: LIB_RESEARCH_SOURCING,
            },
            SkillEntry {
                name: "simple-qa",
                keywords: &["simple", "quick", "answer", "fact", "definition", "brief"],
                content: LIB_SIMPLE_QA,
            },
            SkillEntry {
                name: "technical-writing",
                keywords: &["documentation", "readme", "api", "manual", "technical", "doc"],
                content: LIB_TECHNICAL_WRITING,
            },
            SkillEntry {
                name: "tool-use",
                keywords: &["tool", "command", "execute", "run", "shell", "system"],
                content: LIB_TOOL_USE,
            },
        ];

        Self { entries }
    }

    /// Match a user query to the best skill/library entry.
    ///
    /// Returns the skill name, or `None` if no match exceeds the threshold.
    pub fn match_query(&self, query: &str) -> Option<&'static str> {
        if query.is_empty() {
            return None;
        }

        let query_lower = query.to_lowercase();
        let mut best_name: Option<&'static str> = None;
        let mut best_score: f32 = 0.0;

        // Phase 1: regex patterns
        for pat in PATTERNS {
            // Only match if we have an entry for this skill.
            if !self.entries.iter().any(|e| e.name == pat.skill_name) {
                continue;
            }
            if let Ok(re) = Regex::new(pat.pattern) {
                if re.is_match(&query_lower) && pat.weight > best_score {
                    best_score = pat.weight;
                    best_name = Some(pat.skill_name);
                }
            }
        }

        if best_name.is_some() && best_score >= 6.0 {
            debug!("Skill matched via regex: '{}' (score={})", best_name.unwrap(), best_score);
            return best_name;
        }

        // Phase 2: keyword overlap fallback
        let query_words: Vec<&str> = query_lower
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .filter(|w| w.len() >= 2 && !is_stop_word(w))
            .collect();

        if query_words.is_empty() {
            return best_name.filter(|_| best_score >= KEYWORD_THRESHOLD);
        }

        for entry in &self.entries {
            let mut score: f32 = 0.0;
            for qw in &query_words {
                for kw in entry.keywords {
                    if qw == kw || kw.contains(qw) || qw.contains(kw) {
                        score += 1.5;
                        if qw.len() >= 6 {
                            score += 1.0;
                        }
                    }
                }
            }
            if score > best_score {
                best_score = score;
                best_name = Some(entry.name);
            }
        }

        if best_score >= KEYWORD_THRESHOLD {
            debug!("Skill matched via keywords: '{}' (score={})", best_name.unwrap(), best_score);
            best_name
        } else {
            None
        }
    }

    /// Get the content for a named skill/library entry.
    ///
    /// Strips YAML frontmatter and truncates to [`MAX_SKILL_CHARS`].
    pub fn get_content(&self, name: &str) -> Option<String> {
        let entry = self.entries.iter().find(|e| e.name == name)?;
        let content = strip_frontmatter(entry.content);
        let truncated = if content.len() > MAX_SKILL_CHARS {
            let cut = &content[..MAX_SKILL_CHARS];
            // Cut at a line boundary.
            let end = cut.rfind('\n').unwrap_or(MAX_SKILL_CHARS);
            format!(
                "{}\n\n[Skill content truncated at {} chars.]",
                &cut[..end],
                MAX_SKILL_CHARS
            )
        } else {
            content.to_string()
        };

        Some(format!("SKILL GUIDANCE ({}):\n{}", entry.name, truncated))
    }

    /// One-shot: match query and return content, or empty string.
    pub fn get_matched_content(&self, query: &str) -> String {
        match self.match_query(query) {
            Some(name) => self.get_content(name).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Return names of all registered entries.
    pub fn skill_names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|e| e.name).collect()
    }

    /// Return the total count of registered entries.
    pub fn skill_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Strip YAML frontmatter (`---\n...\n---\n`) from text.
fn strip_frontmatter(text: &str) -> &str {
    if !text.starts_with("---") {
        return text;
    }
    // Find the closing `---`.
    if let Some(end) = text[3..].find("\n---") {
        let after = end + 3 + 4; // skip past "\n---"
        if after < text.len() {
            return text[after..].trim_start();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let reg = SkillRegistry::new();
        assert!(reg.skill_count() >= 30);
    }

    #[test]
    fn test_skill_names() {
        let reg = SkillRegistry::new();
        let names = reg.skill_names();
        assert!(names.contains(&"research"));
        assert!(names.contains(&"coding"));
        assert!(names.contains(&"frontend-design"));
        assert!(names.contains(&"docx"));
    }

    #[test]
    fn test_match_research_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("write a research report on AI"), Some("research"));
    }

    #[test]
    fn test_match_docx_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("create a word document about climate change"), Some("docx"));
    }

    #[test]
    fn test_match_pptx_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("make a powerpoint presentation"), Some("pptx"));
    }

    #[test]
    fn test_match_frontend_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("build a dashboard web app"), Some("frontend-design"));
    }

    #[test]
    fn test_match_coding_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("write a python function to sort a list"), Some("coding"));
    }

    #[test]
    fn test_match_email_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("draft an email to my manager"), Some("email"));
    }

    #[test]
    fn test_match_mcp_regex() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("build an MCP server"), Some("mcp-builder"));
    }

    #[test]
    fn test_match_product_knowledge() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("what are you?"), Some("product-self-knowledge"));
        assert_eq!(reg.match_query("who created you?"), Some("product-self-knowledge"));
    }

    #[test]
    fn test_match_source_skill() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query("find papers on arxiv about transformers"), Some("source-skill"));
    }

    #[test]
    fn test_match_keyword_fallback() {
        let reg = SkillRegistry::new();
        // Should match via keyword overlap when regex doesn't hit >= 6.0
        let result = reg.match_query("spreadsheet data table");
        assert!(result == Some("xlsx") || result == Some("data-extraction"));
    }

    #[test]
    fn test_no_match_empty() {
        let reg = SkillRegistry::new();
        assert_eq!(reg.match_query(""), None);
    }

    #[test]
    fn test_no_match_gibberish() {
        let reg = SkillRegistry::new();
        // Very short/stop-word-heavy query.
        assert_eq!(reg.match_query("the a an is"), None);
    }

    #[test]
    fn test_get_content_exists() {
        let reg = SkillRegistry::new();
        let content = reg.get_content("coding");
        assert!(content.is_some());
        let text = content.unwrap();
        assert!(text.starts_with("SKILL GUIDANCE (coding):"));
        assert!(text.contains("Code"));
    }

    #[test]
    fn test_get_content_nonexistent() {
        let reg = SkillRegistry::new();
        assert!(reg.get_content("nonexistent-skill").is_none());
    }

    #[test]
    fn test_get_matched_content() {
        let reg = SkillRegistry::new();
        let content = reg.get_matched_content("create a word document about AI");
        assert!(!content.is_empty());
        assert!(content.contains("SKILL GUIDANCE"));
    }

    #[test]
    fn test_get_matched_content_no_match() {
        let reg = SkillRegistry::new();
        let content = reg.get_matched_content("the a an is");
        assert!(content.is_empty());
    }

    #[test]
    fn test_strip_frontmatter() {
        let text = "---\nname: test\ndescription: test skill\n---\n\n# Hello World";
        assert_eq!(strip_frontmatter(text), "# Hello World");
    }

    #[test]
    fn test_strip_frontmatter_no_frontmatter() {
        let text = "# Just some content";
        assert_eq!(strip_frontmatter(text), "# Just some content");
    }

    #[test]
    fn test_stop_words() {
        assert!(is_stop_word("the"));
        assert!(is_stop_word("is"));
        assert!(!is_stop_word("python"));
        assert!(!is_stop_word("research"));
    }

    #[test]
    fn test_all_skills_have_content() {
        let reg = SkillRegistry::new();
        for name in reg.skill_names() {
            let content = reg.get_content(name);
            assert!(content.is_some(), "Skill '{}' has no content", name);
            assert!(!content.unwrap().is_empty(), "Skill '{}' has empty content", name);
        }
    }
}
