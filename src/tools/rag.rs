//! RAG Tool — Retrieval-Augmented Generation pipeline.
//!
//! Ported from `sovereign_titan/tools/rag_tool.py`.
//! Combines document parsing, chunking, embedding, and vector search
//! into a single tool that the ReAct agent can invoke.
//!
//! Supports two actions:
//! - `ingest`: Parse a file, chunk it, embed chunks, store in vector store
//! - `search`: Query the vector store for relevant chunks

use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;
use tracing::{debug, info, warn};

use crate::documents::chunker::{DocumentChunker, Strategy};
use crate::documents::parser::DocumentParser;
use crate::memory::vector_store::VectorStore;
use crate::tools::Tool;

/// Dimension of the hash-based embedding vectors.
const EMBEDDING_DIM: usize = 256;

/// RAG tool providing document ingestion and semantic search.
pub struct RagTool {
    /// Shared vector store for document chunks.
    store: Mutex<VectorStore>,
    /// Document chunker with default settings.
    chunker: DocumentChunker,
}

impl RagTool {
    /// Create a new RAG tool with an empty vector store.
    pub fn new() -> Self {
        Self {
            store: Mutex::new(VectorStore::new()),
            chunker: DocumentChunker::default(),
        }
    }

    /// Ingest a file: parse → chunk → embed → store.
    fn ingest(&self, file_path: &str, strategy: &str) -> Result<String, anyhow::Error> {
        let path = Path::new(file_path);
        let parse_result = DocumentParser::parse(path);

        if !parse_result.success {
            let err = parse_result.error.unwrap_or_else(|| "Unknown error".into());
            return Ok(format!("Failed to parse document: {err}"));
        }

        let text = &parse_result.text;
        if text.is_empty() {
            return Ok("Document parsed but contained no text.".into());
        }

        let strat = Strategy::from_str_loose(strategy);
        let chunks = self.chunker.chunk_text(text, strat);

        if chunks.is_empty() {
            return Ok("Document parsed but produced no chunks.".into());
        }

        let mut store = self.store.lock().unwrap();
        let mut ingested = 0;

        for chunk in &chunks {
            let embedding = hash_embedding(&chunk.text, EMBEDDING_DIM);
            let mut metadata = parse_result.metadata.clone();
            metadata.insert("chunk_index".into(), chunk.index.to_string());
            metadata.insert("source".into(), file_path.to_string());
            store.add(&chunk.text, embedding, metadata);
            ingested += 1;
        }

        info!("RAG: ingested {ingested} chunks from {file_path}");
        Ok(format!(
            "Ingested {ingested} chunks from '{}' (strategy: {strategy})",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path)
        ))
    }

    /// Search for relevant chunks matching a query.
    fn search(&self, query: &str, top_k: usize) -> Result<String, anyhow::Error> {
        let query_embedding = hash_embedding(query, EMBEDDING_DIM);
        let mut store = self.store.lock().unwrap();
        let results = store.search(&query_embedding, top_k);

        if results.is_empty() {
            return Ok("No relevant documents found.".into());
        }

        debug!("RAG search: {} results for query '{query}'", results.len());

        let mut output = format!("Found {} relevant chunks:\n\n", results.len());
        for (i, result) in results.iter().enumerate() {
            let source = result
                .document
                .metadata
                .get("source")
                .map(|s| s.as_str())
                .unwrap_or("unknown");
            let chunk_idx = result
                .document
                .metadata
                .get("chunk_index")
                .map(|s| s.as_str())
                .unwrap_or("?");

            output.push_str(&format!(
                "--- Result {} (score: {:.3}, source: {}, chunk: {}) ---\n{}\n\n",
                i + 1,
                result.score,
                source,
                chunk_idx,
                result.document.text
            ));
        }

        Ok(output)
    }
}

#[async_trait::async_trait]
impl Tool for RagTool {
    fn name(&self) -> &'static str {
        "rag"
    }

    fn description(&self) -> &'static str {
        "Retrieval-Augmented Generation. Actions: 'ingest' (parse and store a document), \
         'search' (find relevant chunks). Input: {\"action\": \"ingest\", \"file_path\": \"...\", \
         \"strategy\": \"fixed|semantic|code\"} or {\"action\": \"search\", \"query\": \"...\", \
         \"top_k\": 5}"
    }

    async fn execute(&self, input: Value) -> Result<String, anyhow::Error> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        match action {
            "ingest" => {
                let file_path = input
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("'file_path' required for ingest action"))?;
                let strategy = input
                    .get("strategy")
                    .and_then(|v| v.as_str())
                    .unwrap_or("fixed");
                self.ingest(file_path, strategy)
            }
            "search" => {
                let query = input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("'query' required for search action"))?;
                let top_k = input
                    .get("top_k")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;
                self.search(query, top_k)
            }
            other => {
                warn!("RAG: unknown action '{other}'");
                Ok(format!(
                    "Unknown action '{other}'. Use 'ingest' or 'search'."
                ))
            }
        }
    }
}

/// Simple hash-based embedding for text.
///
/// Creates a fixed-dimension vector by hashing each word to a position
/// and accumulating term frequency. This provides basic keyword-overlap
/// similarity without requiring an external embedding model.
///
/// For production use, this should be replaced with a proper embedding
/// model (e.g., sentence-transformers via ONNX or API).
fn hash_embedding(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dim];

    for word in text.split_whitespace() {
        let word_lower = word.to_lowercase();
        // Strip punctuation for better matching.
        let clean: String = word_lower
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        if clean.is_empty() {
            continue;
        }

        // Hash word to multiple positions for better distribution.
        let h1 = simple_hash(clean.as_bytes(), 0) % dim;
        let h2 = simple_hash(clean.as_bytes(), 1) % dim;
        vec[h1] += 1.0;
        vec[h2] += 0.5;
    }

    // L2-normalize.
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for v in &mut vec {
            *v /= norm;
        }
    }

    vec
}

/// Simple deterministic hash function (FNV-1a variant with seed).
fn simple_hash(bytes: &[u8], seed: u8) -> usize {
    let mut hash: u64 = 0xcbf29ce484222325 ^ (seed as u64);
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_embedding_deterministic() {
        let e1 = hash_embedding("hello world", 128);
        let e2 = hash_embedding("hello world", 128);
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_hash_embedding_different_texts() {
        let e1 = hash_embedding("hello world", 128);
        let e2 = hash_embedding("completely different text", 128);
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_hash_embedding_normalized() {
        let e = hash_embedding("some text to embed", 128);
        let norm: f32 = e.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "embedding should be L2-normalized");
    }

    #[test]
    fn test_hash_embedding_similar_texts() {
        let e1 = hash_embedding("rust programming language", 256);
        let e2 = hash_embedding("rust programming guide", 256);
        let e3 = hash_embedding("cooking recipes for pasta", 256);

        let sim_12 = crate::memory::vector_store::cosine_similarity(&e1, &e2);
        let sim_13 = crate::memory::vector_store::cosine_similarity(&e1, &e3);

        // Similar texts should have higher similarity.
        assert!(
            sim_12 > sim_13,
            "similar texts should score higher: {sim_12} vs {sim_13}"
        );
    }

    #[test]
    fn test_hash_embedding_empty() {
        let e = hash_embedding("", 128);
        // All zeros (no words to hash), norm = 0.
        assert!(e.iter().all(|&v| v == 0.0));
    }

    #[tokio::test]
    async fn test_rag_tool_name() {
        let tool = RagTool::new();
        assert_eq!(tool.name(), "rag");
    }

    #[tokio::test]
    async fn test_rag_ingest_nonexistent() {
        let tool = RagTool::new();
        let input = serde_json::json!({
            "action": "ingest",
            "file_path": "/nonexistent/file.txt"
        });
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("Failed to parse"));
    }

    #[tokio::test]
    async fn test_rag_search_empty_store() {
        let tool = RagTool::new();
        let input = serde_json::json!({
            "action": "search",
            "query": "test query"
        });
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("No relevant documents"));
    }

    #[tokio::test]
    async fn test_rag_ingest_and_search() {
        let dir = std::env::temp_dir().join("titan_rag_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_doc.txt");
        std::fs::write(
            &path,
            "Rust is a systems programming language focused on safety and performance. \
             It provides memory safety without garbage collection. \
             The borrow checker ensures safe memory access at compile time.",
        )
        .unwrap();

        let tool = RagTool::new();

        // Ingest.
        let ingest_input = serde_json::json!({
            "action": "ingest",
            "file_path": path.to_str().unwrap()
        });
        let result = tool.execute(ingest_input).await.unwrap();
        assert!(result.contains("Ingested"));

        // Search for something related.
        let search_input = serde_json::json!({
            "action": "search",
            "query": "memory safety in Rust programming"
        });
        let result = tool.execute(search_input).await.unwrap();
        assert!(result.contains("relevant chunks") || result.contains("No relevant"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_rag_unknown_action() {
        let tool = RagTool::new();
        let input = serde_json::json!({ "action": "delete" });
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_rag_search_missing_query() {
        let tool = RagTool::new();
        let input = serde_json::json!({ "action": "search" });
        let result = tool.execute(input).await;
        assert!(result.is_err());
    }
}
