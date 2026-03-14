//! Task Planner — LLM-based task decomposition into DAG steps.
//!
//! Uses the reasoning model to break multi-step tasks into parallel and
//! sequential steps, then builds a validated `DagGraph` for the `DagExecutor`.
//!
//! Ported from `sovereign_titan/agents/task_planner.py`.

use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use tracing::{debug, info};

use crate::agent::dag::{DagGraph, DagNode};
use crate::nexus::{ModelNexus, ModelTarget};
use crate::tools::ToolRegistry;

// ─────────────────────────────────────────────────────────────────────────────
// Complex task detection
// ─────────────────────────────────────────────────────────────────────────────

/// Heuristic check: does this task warrant DAG decomposition?
///
/// Returns `true` for tasks with conjunctions, parallel-implying words,
/// or sufficient length (8+ words) combined with action verbs.
pub fn is_complex_task(task: &str) -> bool {
    let words: Vec<&str> = task.split_whitespace().collect();
    if words.len() < 8 {
        return false;
    }

    let complex_re = Regex::new(
        r"(?ix)
        \b(?:and\s+then|then\s+also|also\s+|after\s+that|next\s+|
           simultaneously|in\s+parallel|at\s+the\s+same\s+time|
           both\s+|as\s+well\s+as|along\s+with|plus\s+|
           first\s+.+\s+then|step\s+\d|compare\s+.+\s+(?:and|with|vs|versus)|
           research\s+.+\s+(?:and|then)\s+write|
           find\s+.+\s+(?:and|then)\s+(?:open|create|send|write))
        "
    ).unwrap();

    complex_re.is_match(task)
}

// ─────────────────────────────────────────────────────────────────────────────
// Plan prompt
// ─────────────────────────────────────────────────────────────────────────────

const PLAN_PROMPT: &str = r#"You are a task planner. Decompose the following task into discrete steps that can be executed by tools.

Available tools: {tool_list}

Task: {task}

Output a JSON object with this exact structure:
{"steps": [
  {"id": "s1", "tool": "<tool_name>", "description": "<what this step does>", "params": {}, "requires": [], "provides": ["<variable_name>"]},
  {"id": "s2", "tool": "<tool_name>", "description": "<what this step does>", "params": {}, "requires": ["s1"], "provides": ["<variable_name>"]}
]}

Rules:
- Each step must use exactly one tool from the available list
- Steps with no dependencies can run in parallel (empty "requires")
- "provides" lists variable names this step produces
- "requires" lists step IDs that must complete first
- Keep the plan focused — 2-6 steps maximum
- params should contain the key parameters for the tool call

Output ONLY the JSON, no explanation."#;

// ─────────────────────────────────────────────────────────────────────────────
// Planned step (intermediate representation)
// ─────────────────────────────────────────────────────────────────────────────

/// A step parsed from the LLM's plan JSON.
#[derive(Debug, Clone)]
struct PlannedStep {
    id: String,
    tool: String,
    description: String,
    params: serde_json::Value,
    requires: Vec<String>,
    provides: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// TaskPlanner
// ─────────────────────────────────────────────────────────────────────────────

/// Decomposes complex tasks into DagGraph DAGs via LLM planning.
pub struct TaskPlanner {
    nexus: Arc<ModelNexus>,
}

impl TaskPlanner {
    /// Create a new task planner.
    pub fn new(nexus: Arc<ModelNexus>) -> Self {
        Self { nexus }
    }

    /// Decompose a task into a validated `DagGraph`.
    ///
    /// Returns `None` if:
    /// - Planning fails or produces invalid output
    /// - The LLM output can't be parsed as JSON
    /// - The plan has fewer than 2 steps or more than 10
    pub async fn plan(
        &self,
        task: &str,
        registry: &ToolRegistry,
    ) -> Option<(DagGraph, HashMap<String, serde_json::Value>)> {
        let tool_names = registry.names();
        let tool_list = tool_names.join(", ");

        let prompt = PLAN_PROMPT
            .replace("{task}", task)
            .replace("{tool_list}", &tool_list);

        // Generate plan using the model
        let response = self
            .nexus
            .generate_with_system(
                "You are a task planner that outputs only valid JSON.",
                &prompt,
                ModelTarget::Prime,
                1024,
                0.1,
            )
            .await
            .ok()?;

        // Extract JSON from response (may have markdown fences)
        let json_str = extract_json(&response)?;

        // Parse the plan
        let plan_data: serde_json::Value = serde_json::from_str(&json_str).ok()?;
        let steps_arr = plan_data.get("steps")?.as_array()?;

        if steps_arr.len() < 2 {
            debug!("Plan has fewer than 2 steps — not worth DAG");
            return None;
        }
        if steps_arr.len() > 10 {
            debug!("Plan has >10 steps — likely hallucinated");
            return None;
        }

        // Parse steps
        let mut planned_steps: Vec<PlannedStep> = Vec::new();
        let tool_names_set: std::collections::HashSet<&str> =
            tool_names.iter().map(|s| &**s).collect();

        for step in steps_arr {
            let tool = step.get("tool")?.as_str()?;
            if !tool_names_set.contains(tool) {
                debug!("Plan references unknown tool '{}'", tool);
                continue;
            }

            planned_steps.push(PlannedStep {
                id: step
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("s?")
                    .to_string(),
                tool: tool.to_string(),
                description: step
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                params: step
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::json!({})),
                requires: step
                    .get("requires")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                provides: step
                    .get("provides")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }

        if planned_steps.len() < 2 {
            return None;
        }

        // Build DagGraph + params map
        let mut graph = DagGraph::new();
        let mut params_map: HashMap<String, serde_json::Value> = HashMap::new();

        for step in &planned_steps {
            let mut node = DagNode::new(&step.id, &step.description)
                .requires(step.requires.clone())
                .provides(step.provides.clone())
                .tool(&step.tool);

            // Validate requires — all referenced IDs must exist in the plan
            let valid_ids: std::collections::HashSet<&str> =
                planned_steps.iter().map(|s| s.id.as_str()).collect();
            node.requires.retain(|r| valid_ids.contains(r.as_str()));

            graph.add_node(node);
            params_map.insert(step.id.clone(), step.params.clone());
        }

        let n_parallel = graph
            .nodes
            .values()
            .filter(|n| n.requires.is_empty())
            .count();

        info!(
            "Task decomposed into {} steps ({} parallelizable)",
            graph.len(),
            n_parallel
        );

        Some((graph, params_map))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract JSON from a model response that may contain markdown fences.
fn extract_json(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // Try to extract from markdown code fences
    if trimmed.contains("```") {
        let fence_re = Regex::new(r"```(?:json)?\s*\n?(.*?)\n?```").ok()?;
        if let Some(m) = fence_re.captures(trimmed) {
            let inner = m.get(1)?.as_str().trim();
            if !inner.is_empty() {
                return Some(inner.to_string());
            }
        }
    }

    // Find the first '{' and extract from there
    let brace_start = trimmed.find('{')?;
    let json_str = &trimmed[brace_start..];

    // Find matching closing brace
    let mut depth = 0;
    let mut end = 0;
    for (i, ch) in json_str.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end > 0 {
        Some(json_str[..end].to_string())
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_complex_task_short() {
        assert!(!is_complex_task("open discord"));
        assert!(!is_complex_task("what time is it"));
    }

    #[test]
    fn test_is_complex_task_conjunction() {
        assert!(is_complex_task(
            "search for rust programming and then open the first result in chrome"
        ));
    }

    #[test]
    fn test_is_complex_task_parallel() {
        assert!(is_complex_task(
            "simultaneously search for rust and python programming languages"
        ));
    }

    #[test]
    fn test_is_complex_task_compare() {
        assert!(is_complex_task(
            "compare the weather in New York and Los Angeles today"
        ));
    }

    #[test]
    fn test_is_complex_task_research_and_write() {
        assert!(is_complex_task(
            "research the latest AI developments and then write a summary report"
        ));
    }

    #[test]
    fn test_is_complex_task_find_and_open() {
        assert!(is_complex_task(
            "find the rust programming language website and then open it in browser"
        ));
    }

    #[test]
    fn test_is_complex_task_long_but_simple() {
        // Long but no conjunction patterns
        assert!(!is_complex_task(
            "what is the meaning of the word serendipity in english language"
        ));
    }

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"steps": [{"id": "s1"}]}"#;
        assert_eq!(extract_json(input), Some(input.to_string()));
    }

    #[test]
    fn test_extract_json_markdown_fence() {
        let input = "Here's the plan:\n```json\n{\"steps\": []}\n```";
        assert_eq!(extract_json(input), Some("{\"steps\": []}".to_string()));
    }

    #[test]
    fn test_extract_json_with_preamble() {
        let input = "Sure, here's the plan: {\"steps\": []}";
        assert_eq!(extract_json(input), Some("{\"steps\": []}".to_string()));
    }

    #[test]
    fn test_extract_json_nested() {
        let input = r#"{"steps": [{"id": "s1", "params": {"query": "test"}}]}"#;
        assert_eq!(extract_json(input), Some(input.to_string()));
    }

    #[test]
    fn test_extract_json_no_json() {
        let input = "This response has no JSON.";
        assert_eq!(extract_json(input), None);
    }
}
