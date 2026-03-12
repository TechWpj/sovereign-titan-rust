//! DAG Executor — parallel task execution with dependency tracking.
//!
//! Implements a Directed Acyclic Graph (DAG) task runner that resolves
//! dependencies and executes sub-tasks concurrently using Tokio, bounded
//! by a configurable semaphore.
//!
//! Ported from Python `dag_executor.py` and `research_dag.py`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore};
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Node status
// ─────────────────────────────────────────────────────────────────────────────

/// Lifecycle status of a DAG node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// DagNode
// ─────────────────────────────────────────────────────────────────────────────

/// A single node (step) in the DAG.
#[derive(Debug, Clone)]
pub struct DagNode {
    /// Unique identifier.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// IDs of nodes that must complete before this one can run.
    pub requires: Vec<String>,
    /// Variable names this node's result is stored under in the context.
    pub provides: Vec<String>,
    /// Optional tool name associated with this step.
    pub tool: Option<String>,
    /// Current lifecycle status.
    pub status: NodeStatus,
    /// Result value (set on completion).
    pub result: Option<String>,
}

impl DagNode {
    /// Create a new pending node.
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            requires: Vec::new(),
            provides: Vec::new(),
            tool: None,
            status: NodeStatus::Pending,
            result: None,
        }
    }

    /// Builder: set dependencies.
    pub fn requires(mut self, deps: Vec<String>) -> Self {
        self.requires = deps;
        self
    }

    /// Builder: set provided variable names.
    pub fn provides(mut self, vars: Vec<String>) -> Self {
        self.provides = vars;
        self
    }

    /// Builder: set associated tool name.
    pub fn tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DagGraph
// ─────────────────────────────────────────────────────────────────────────────

/// A DAG of nodes with dependency tracking.
pub struct DagGraph {
    pub nodes: HashMap<String, DagNode>,
}

impl DagGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: DagNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Get IDs of nodes whose dependencies are all completed and are still pending.
    pub fn get_ready(&self) -> Vec<String> {
        self.nodes
            .values()
            .filter(|n| n.status == NodeStatus::Pending)
            .filter(|n| {
                n.requires.iter().all(|dep| {
                    self.nodes
                        .get(dep)
                        .map(|d| d.status == NodeStatus::Completed)
                        .unwrap_or(false)
                })
            })
            .map(|n| n.id.clone())
            .collect()
    }

    /// Mark a node as running.
    pub fn mark_running(&mut self, id: &str) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.status = NodeStatus::Running;
        }
    }

    /// Mark a node as completed with a result.
    pub fn mark_completed(&mut self, id: &str, result: String) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.status = NodeStatus::Completed;
            node.result = Some(result);
        }
    }

    /// Mark a node as failed.
    pub fn mark_failed(&mut self, id: &str, error: String) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.status = NodeStatus::Failed(error);
        }
    }

    /// Check if all nodes are completed.
    pub fn all_completed(&self) -> bool {
        self.nodes.values().all(|n| n.status == NodeStatus::Completed)
    }

    /// Get the total number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl Default for DagGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step function type alias
// ─────────────────────────────────────────────────────────────────────────────

/// Async function signature for executing a DAG step.
///
/// Takes the node and a shared context, returns a result string.
pub type StepFn = Arc<
    dyn Fn(
            DagNode,
            Arc<Mutex<HashMap<String, String>>>,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

// ─────────────────────────────────────────────────────────────────────────────
// DagExecutor
// ─────────────────────────────────────────────────────────────────────────────

/// Executes a [`DagGraph`] with bounded-concurrency parallelism.
///
/// Steps whose dependencies are satisfied run concurrently, bounded by
/// a semaphore (default: 4 concurrent tasks).
pub struct DagExecutor {
    max_parallel: usize,
}

/// Result of a DAG execution.
#[derive(Debug)]
pub struct DagResult {
    /// True if all steps completed successfully.
    pub success: bool,
    /// Per-step results keyed by node ID.
    pub results: HashMap<String, String>,
    /// Shared context after execution.
    pub context: HashMap<String, String>,
    /// IDs of completed steps.
    pub completed_steps: Vec<String>,
    /// IDs of failed steps.
    pub failed_steps: Vec<String>,
}

impl DagExecutor {
    /// Create a new executor with the given concurrency limit.
    pub fn new(max_parallel: usize) -> Self {
        Self {
            max_parallel: max_parallel.max(1),
        }
    }

    /// Execute the DAG, running ready steps in parallel.
    ///
    /// `step_fn` is called for each step with `(node, context)` and should
    /// return `Ok(result_string)` or `Err(error_string)`.
    pub async fn run(&self, graph: Arc<Mutex<DagGraph>>, step_fn: StepFn) -> DagResult {
        let context: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let results: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let failed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let semaphore = Arc::new(Semaphore::new(self.max_parallel));

        let total_nodes = graph.lock().await.len();
        let max_iterations = total_nodes * 2 + 10;

        for _ in 0..max_iterations {
            // Find ready steps.
            let ready = {
                let g = graph.lock().await;
                g.get_ready()
            };

            // Launch ready steps.
            let mut handles = Vec::new();
            for step_id in ready {
                let node = {
                    let mut g = graph.lock().await;
                    g.mark_running(&step_id);
                    g.nodes.get(&step_id).cloned()
                };

                let Some(node) = node else { continue };

                let sem = Arc::clone(&semaphore);
                let graph = Arc::clone(&graph);
                let ctx = Arc::clone(&context);
                let res = Arc::clone(&results);
                let fail = Arc::clone(&failed);
                let step_fn = Arc::clone(&step_fn);
                let provides = node.provides.clone();
                let node_id = node.id.clone();

                let handle = tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    info!("DAG: running step {}: {}", node_id, node.description);

                    match step_fn(node, Arc::clone(&ctx)).await {
                        Ok(result) => {
                            // Store result in context.
                            {
                                let mut c = ctx.lock().await;
                                for var in &provides {
                                    c.insert(var.clone(), result.clone());
                                }
                            }
                            // Mark completed.
                            graph.lock().await.mark_completed(&node_id, result.clone());
                            res.lock().await.insert(node_id.clone(), result);
                            info!("DAG: step {} completed", node_id);
                        }
                        Err(e) => {
                            graph.lock().await.mark_failed(&node_id, e.clone());
                            fail.lock().await.push(node_id.clone());
                            warn!("DAG: step {} failed: {}", node_id, e);
                        }
                    }
                });
                handles.push(handle);
            }

            if handles.is_empty() {
                // No new tasks launched. Check if there are running tasks.
                let g = graph.lock().await;
                let has_running = g
                    .nodes
                    .values()
                    .any(|n| n.status == NodeStatus::Running);
                if !has_running {
                    break; // No running, no ready → done or stuck.
                }
                // Wait a moment for running tasks to finish.
                drop(g);
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                continue;
            }

            // Wait for all launched tasks to complete.
            for handle in handles {
                let _ = handle.await;
            }

            // Check completion.
            if graph.lock().await.all_completed() {
                break;
            }

            // Check if failures have stalled progress.
            {
                let f = failed.lock().await;
                if !f.is_empty() {
                    let g = graph.lock().await;
                    let ready = g.get_ready();
                    let has_running = g
                        .nodes
                        .values()
                        .any(|n| n.status == NodeStatus::Running);
                    if ready.is_empty() && !has_running {
                        warn!(
                            "DAG execution stalled: {} failed steps blocking progress",
                            f.len()
                        );
                        break;
                    }
                }
            }
        }

        let g = graph.lock().await;
        let completed_steps: Vec<String> = g
            .nodes
            .values()
            .filter(|n| n.status == NodeStatus::Completed)
            .map(|n| n.id.clone())
            .collect();

        let parallelizable = g
            .nodes
            .values()
            .filter(|n| n.requires.is_empty())
            .count();

        info!(
            "DAG execution complete: {}/{} steps completed ({} parallelizable)",
            completed_steps.len(),
            g.len(),
            parallelizable
        );

        let failed_steps = failed.lock().await.clone();
        let final_results = results.lock().await.clone();
        let final_context = context.lock().await.clone();

        DagResult {
            success: failed_steps.is_empty(),
            results: final_results,
            context: final_context,
            completed_steps,
            failed_steps,
        }
    }
}

impl Default for DagExecutor {
    fn default() -> Self {
        Self::new(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_node_builder() {
        let node = DagNode::new("step1", "First step")
            .requires(vec!["step0".into()])
            .provides(vec!["output1".into()])
            .tool("web_search");

        assert_eq!(node.id, "step1");
        assert_eq!(node.requires, vec!["step0"]);
        assert_eq!(node.provides, vec!["output1"]);
        assert_eq!(node.tool, Some("web_search".into()));
        assert_eq!(node.status, NodeStatus::Pending);
    }

    #[test]
    fn test_dag_graph_ready_detection() {
        let mut graph = DagGraph::new();

        // step_a: no deps → ready
        graph.add_node(DagNode::new("a", "Step A"));

        // step_b: depends on step_a → not ready
        graph.add_node(DagNode::new("b", "Step B").requires(vec!["a".into()]));

        // step_c: no deps → ready
        graph.add_node(DagNode::new("c", "Step C"));

        let ready = graph.get_ready();
        assert_eq!(ready.len(), 2);
        assert!(ready.contains(&"a".into()));
        assert!(ready.contains(&"c".into()));
        assert!(!ready.contains(&"b".into()));
    }

    #[test]
    fn test_dag_graph_dependency_resolution() {
        let mut graph = DagGraph::new();
        graph.add_node(DagNode::new("a", "Step A"));
        graph.add_node(DagNode::new("b", "Step B").requires(vec!["a".into()]));

        // Initially only 'a' is ready.
        assert_eq!(graph.get_ready(), vec!["a"]);

        // Complete 'a' → 'b' becomes ready.
        graph.mark_completed("a", "done".into());
        assert_eq!(graph.get_ready(), vec!["b"]);
    }

    #[test]
    fn test_dag_graph_all_completed() {
        let mut graph = DagGraph::new();
        graph.add_node(DagNode::new("a", "Step A"));
        graph.add_node(DagNode::new("b", "Step B"));

        assert!(!graph.all_completed());
        graph.mark_completed("a", "ok".into());
        assert!(!graph.all_completed());
        graph.mark_completed("b", "ok".into());
        assert!(graph.all_completed());
    }

    #[test]
    fn test_dag_graph_failure() {
        let mut graph = DagGraph::new();
        graph.add_node(DagNode::new("a", "Step A"));
        graph.add_node(DagNode::new("b", "Step B").requires(vec!["a".into()]));

        graph.mark_failed("a", "boom".into());

        // 'b' should NOT be ready since 'a' failed (not completed).
        assert!(graph.get_ready().is_empty());
        assert!(!graph.all_completed());
    }

    #[tokio::test]
    async fn test_dag_executor_parallel() {
        let mut graph = DagGraph::new();
        graph.add_node(DagNode::new("a", "Step A").provides(vec!["result_a".into()]));
        graph.add_node(DagNode::new("b", "Step B").provides(vec!["result_b".into()]));
        graph.add_node(
            DagNode::new("c", "Step C (depends on A and B)")
                .requires(vec!["a".into(), "b".into()])
                .provides(vec!["result_c".into()]),
        );

        let graph = Arc::new(Mutex::new(graph));
        let executor = DagExecutor::new(4);

        let step_fn: StepFn = Arc::new(|node, _ctx| {
            Box::pin(async move { Ok(format!("result_of_{}", node.id)) })
        });

        let result = executor.run(graph, step_fn).await;
        assert!(result.success);
        assert_eq!(result.completed_steps.len(), 3);
        assert!(result.failed_steps.is_empty());
        assert_eq!(result.results.get("a").unwrap(), "result_of_a");
        assert_eq!(result.results.get("b").unwrap(), "result_of_b");
        assert_eq!(result.results.get("c").unwrap(), "result_of_c");
    }

    #[tokio::test]
    async fn test_dag_executor_with_failure() {
        let mut graph = DagGraph::new();
        graph.add_node(DagNode::new("a", "Step A"));
        graph.add_node(DagNode::new("b", "Step B (will fail)"));
        graph.add_node(
            DagNode::new("c", "Step C (depends on B)")
                .requires(vec!["b".into()]),
        );

        let graph = Arc::new(Mutex::new(graph));
        let executor = DagExecutor::new(4);

        let step_fn: StepFn = Arc::new(|node, _ctx| {
            Box::pin(async move {
                if node.id == "b" {
                    Err("intentional failure".into())
                } else {
                    Ok(format!("result_of_{}", node.id))
                }
            })
        });

        let result = executor.run(graph, step_fn).await;
        assert!(!result.success);
        assert!(result.completed_steps.contains(&"a".into()));
        assert!(result.failed_steps.contains(&"b".into()));
        // 'c' should never run because 'b' failed.
        assert!(!result.completed_steps.contains(&"c".into()));
    }

    #[tokio::test]
    async fn test_dag_executor_context_propagation() {
        let mut graph = DagGraph::new();
        graph.add_node(DagNode::new("a", "Produce data").provides(vec!["data".into()]));
        graph.add_node(
            DagNode::new("b", "Consume data")
                .requires(vec!["a".into()])
                .provides(vec!["processed".into()]),
        );

        let graph = Arc::new(Mutex::new(graph));
        let executor = DagExecutor::new(2);

        let step_fn: StepFn = Arc::new(|node, ctx| {
            Box::pin(async move {
                if node.id == "a" {
                    Ok("hello_world".into())
                } else {
                    let c = ctx.lock().await;
                    let data = c.get("data").cloned().unwrap_or_default();
                    Ok(format!("processed_{data}"))
                }
            })
        });

        let result = executor.run(graph, step_fn).await;
        assert!(result.success);
        assert_eq!(result.context.get("data").unwrap(), "hello_world");
        assert_eq!(
            result.context.get("processed").unwrap(),
            "processed_hello_world"
        );
    }

    #[tokio::test]
    async fn test_dag_executor_empty_graph() {
        let graph = Arc::new(Mutex::new(DagGraph::new()));
        let executor = DagExecutor::default();

        let step_fn: StepFn = Arc::new(|_node, _ctx| {
            Box::pin(async move { Ok("should not run".into()) })
        });

        let result = executor.run(graph, step_fn).await;
        assert!(result.success);
        assert!(result.completed_steps.is_empty());
    }

    #[test]
    fn test_dag_graph_len() {
        let mut graph = DagGraph::new();
        assert!(graph.is_empty());
        graph.add_node(DagNode::new("a", "Step A"));
        assert_eq!(graph.len(), 1);
        assert!(!graph.is_empty());
    }
}
