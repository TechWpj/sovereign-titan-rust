//! Vector Store — in-memory cosine similarity search for embeddings.
//!
//! Ported from `sovereign_titan/memory/vector_store.py`. Provides a basic
//! RAG foundation with `Vec<f32>` embedding storage and top-K retrieval
//! via cosine similarity.

use std::collections::HashMap;

use uuid::Uuid;

/// Minimum similarity threshold — results below this are noise.
const SIMILARITY_FLOOR: f32 = 0.78;

/// A stored document with its embedding and metadata.
#[derive(Debug, Clone)]
pub struct VectorDocument {
    pub id: String,
    pub text: String,
    pub embedding: Vec<f32>,
    pub metadata: HashMap<String, String>,
}

/// A search result with the matched document and similarity score.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub document: VectorDocument,
    pub score: f32,
}

/// In-memory vector store with cosine similarity search.
pub struct VectorStore {
    documents: Vec<VectorDocument>,
    query_count: usize,
}

impl VectorStore {
    /// Create an empty vector store.
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            query_count: 0,
        }
    }

    /// Add a document with its pre-computed embedding.
    pub fn add(
        &mut self,
        text: &str,
        embedding: Vec<f32>,
        metadata: HashMap<String, String>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        self.documents.push(VectorDocument {
            id: id.clone(),
            text: text.to_string(),
            embedding,
            metadata,
        });
        id
    }

    /// Add a document with a specific ID.
    pub fn add_with_id(
        &mut self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: HashMap<String, String>,
    ) {
        // Remove existing doc with same ID if present.
        self.documents.retain(|d| d.id != id);
        self.documents.push(VectorDocument {
            id: id.to_string(),
            text: text.to_string(),
            embedding,
            metadata,
        });
    }

    /// Search for the top-K most similar documents to a query embedding.
    pub fn search(&mut self, query_embedding: &[f32], top_k: usize) -> Vec<SearchResult> {
        self.query_count += 1;

        let mut scored: Vec<SearchResult> = self
            .documents
            .iter()
            .map(|doc| {
                let score = cosine_similarity(query_embedding, &doc.embedding);
                SearchResult {
                    document: doc.clone(),
                    score,
                }
            })
            .filter(|r| r.score >= SIMILARITY_FLOOR)
            .collect();

        // Sort by score descending.
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Search with metadata filtering.
    pub fn search_filtered(
        &mut self,
        query_embedding: &[f32],
        top_k: usize,
        filter: &HashMap<String, String>,
    ) -> Vec<SearchResult> {
        self.query_count += 1;

        let mut scored: Vec<SearchResult> = self
            .documents
            .iter()
            .filter(|doc| {
                filter.iter().all(|(k, v)| {
                    doc.metadata.get(k).map_or(false, |mv| mv == v)
                })
            })
            .map(|doc| {
                let score = cosine_similarity(query_embedding, &doc.embedding);
                SearchResult {
                    document: doc.clone(),
                    score,
                }
            })
            .filter(|r| r.score >= SIMILARITY_FLOOR)
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Delete a document by ID.
    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.documents.len();
        self.documents.retain(|d| d.id != id);
        self.documents.len() < before
    }

    /// Clear all documents.
    pub fn clear(&mut self) {
        self.documents.clear();
    }

    /// Number of stored documents.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Get store statistics.
    pub fn get_stats(&self) -> (usize, usize) {
        (self.documents.len(), self.query_count)
    }
}

impl Default for VectorStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 {
        return 0.0;
    }

    dot / denom
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_embedding(values: &[f32]) -> Vec<f32> {
        values.to_vec()
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&v, &v);
        assert!((score - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-5);
    }

    #[test]
    fn test_add_and_search() {
        let mut store = VectorStore::new();
        store.add("hello world", make_embedding(&[1.0, 0.0, 0.0]), HashMap::new());
        store.add("goodbye world", make_embedding(&[0.9, 0.1, 0.0]), HashMap::new());
        store.add("unrelated", make_embedding(&[0.0, 0.0, 1.0]), HashMap::new());

        let results = store.search(&make_embedding(&[1.0, 0.0, 0.0]), 2);
        assert!(!results.is_empty());
        assert_eq!(results[0].document.text, "hello world");
    }

    #[test]
    fn test_delete() {
        let mut store = VectorStore::new();
        let id = store.add("test", make_embedding(&[1.0]), HashMap::new());
        assert_eq!(store.len(), 1);
        assert!(store.delete(&id));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_similarity_floor() {
        let mut store = VectorStore::new();
        store.add("far away", make_embedding(&[0.0, 0.0, 1.0]), HashMap::new());
        let results = store.search(&make_embedding(&[1.0, 0.0, 0.0]), 10);
        assert!(results.is_empty()); // Below SIMILARITY_FLOOR
    }
}
