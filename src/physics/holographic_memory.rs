//! Holographic Memory — Vector Symbolic Architecture memory system.
//!
//! Ported from `sovereign_titan/physics/memory.py`.
//! Features:
//! - High-dimensional vector encoding of text
//! - Cosine-similarity based recall
//! - Concept binding via element-wise multiplication
//! - LRU eviction when memory is full
//! - Configurable decay and similarity thresholds

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for holographic memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolographicConfig {
    /// Dimensionality of hypervectors.
    pub dimension: usize,
    /// Maximum number of stored memories.
    pub max_memories: usize,
    /// Minimum similarity for a recall match.
    pub similarity_threshold: f64,
    /// Time-based decay rate (unused in recall, reserved for future scoring).
    pub decay_rate: f64,
    /// Strength of concept binding operations.
    pub binding_strength: f64,
    /// Default top-k results for recall.
    pub recall_top_k: usize,
}

impl Default for HolographicConfig {
    fn default() -> Self {
        Self {
            dimension: 10000,
            max_memories: 100000,
            similarity_threshold: 0.65,
            decay_rate: 0.001,
            binding_strength: 0.85,
            recall_top_k: 10,
        }
    }
}

/// A single memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique key for this memory.
    pub key: String,
    /// Human-readable content.
    pub content: String,
    /// High-dimensional vector representation.
    #[serde(skip)]
    pub vector: Vec<f64>,
    /// Arbitrary metadata.
    pub metadata: serde_json::Value,
    /// Unix timestamp of creation.
    pub created_at: f64,
    /// Unix timestamp of last access.
    pub last_access: f64,
}

/// A single recall result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    /// Memory key.
    pub key: String,
    /// Content string.
    pub content: String,
    /// Cosine similarity score.
    pub similarity: f64,
    /// Associated metadata.
    pub metadata: serde_json::Value,
}

/// Statistics about holographic memory usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolographicStats {
    pub memory_count: usize,
    pub codebook_size: usize,
    pub total_binds: u64,
    pub total_recalls: u64,
    pub dimension: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper functions
// ─────────────────────────────────────────────────────────────────────────────

/// Generate a random hypervector of the given dimension with values in [-1, 1].
fn random_vector(dim: usize) -> Vec<f64> {
    let mut rng = rand::thread_rng();
    (0..dim).map(|_| rng.gen_range(-1.0..1.0)).collect()
}

/// Normalize a vector to unit length. Returns a zero vector if the input has
/// zero magnitude.
fn normalize(v: &[f64]) -> Vec<f64> {
    let mag: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if mag < f64::EPSILON {
        return vec![0.0; v.len()];
    }
    v.iter().map(|x| x / mag).collect()
}

/// Bind two vectors via element-wise multiplication.
fn bind(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).collect()
}

/// Bundle (superpose) multiple vectors via element-wise addition, then
/// normalize.
fn bundle(vectors: &[Vec<f64>]) -> Vec<f64> {
    if vectors.is_empty() {
        return Vec::new();
    }
    let dim = vectors[0].len();
    let mut result = vec![0.0; dim];
    for v in vectors {
        for (i, val) in v.iter().enumerate() {
            if i < dim {
                result[i] += val;
            }
        }
    }
    normalize(&result)
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if mag_a < f64::EPSILON || mag_b < f64::EPSILON {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Base concept names seeded into the codebook on construction.
const BASE_CONCEPTS: &[&str] = &[
    "time", "space", "action", "object", "person", "location",
    "number", "color", "emotion", "query", "command", "error",
    "success", "thought", "memory", "goal",
];

/// Holographic memory system using Vector Symbolic Architecture.
pub struct HolographicMemory {
    /// Dimensionality of all vectors.
    dimension: usize,
    /// Maximum stored memories before eviction.
    max_memories: usize,
    /// Minimum similarity for recall.
    similarity_threshold: f64,
    /// Decay rate (reserved).
    #[allow(dead_code)]
    decay_rate: f64,
    /// Binding strength multiplier.
    binding_strength: f64,
    /// Default top-k for recall.
    recall_top_k: usize,
    /// Codebook: concept name -> hypervector.
    codebook: HashMap<String, Vec<f64>>,
    /// Stored memories.
    memories: HashMap<String, MemoryEntry>,
    /// Access counts per memory key.
    access_counts: HashMap<String, u64>,
    /// Total bind operations performed.
    total_binds: u64,
    /// Total recall operations performed.
    total_recalls: u64,
}

impl HolographicMemory {
    /// Create a new holographic memory with the given configuration.
    pub fn new(config: HolographicConfig) -> Self {
        let mut codebook = HashMap::new();
        for concept in BASE_CONCEPTS {
            codebook.insert(concept.to_string(), random_vector(config.dimension));
        }

        Self {
            dimension: config.dimension,
            max_memories: config.max_memories,
            similarity_threshold: config.similarity_threshold,
            decay_rate: config.decay_rate,
            binding_strength: config.binding_strength,
            recall_top_k: config.recall_top_k,
            codebook,
            memories: HashMap::new(),
            access_counts: HashMap::new(),
            total_binds: 0,
            total_recalls: 0,
        }
    }

    /// Encode text into a hypervector by tokenizing, looking up or creating
    /// codebook entries, applying position encoding, and bundling.
    pub fn encode(&mut self, text: &str) -> Vec<f64> {
        let tokens: Vec<&str> = text
            .split_whitespace()
            .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|t| !t.is_empty())
            .collect();

        if tokens.is_empty() {
            return vec![0.0; self.dimension];
        }

        let mut token_vectors = Vec::with_capacity(tokens.len());

        for (pos, token) in tokens.iter().enumerate() {
            let lower = token.to_lowercase();

            // Get or create codebook entry
            let base = if let Some(v) = self.codebook.get(&lower) {
                v.clone()
            } else {
                let v = random_vector(self.dimension);
                self.codebook.insert(lower, v.clone());
                v
            };

            // Apply position encoding: rotate vector by `pos` positions
            let rotated = Self::rotate_vector(&base, pos);
            token_vectors.push(rotated);
        }

        bundle(&token_vectors)
    }

    /// Rotate a vector by `n` positions (circular shift) for position encoding.
    fn rotate_vector(v: &[f64], n: usize) -> Vec<f64> {
        if v.is_empty() {
            return Vec::new();
        }
        let n = n % v.len();
        let mut result = vec![0.0; v.len()];
        for (i, val) in v.iter().enumerate() {
            result[(i + n) % v.len()] = *val;
        }
        result
    }

    /// Store a memory. Returns the memory key. If the store is full, evicts
    /// the least-recently-used entry.
    pub fn store(
        &mut self,
        key: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
    ) -> String {
        // Evict LRU if at capacity
        if self.memories.len() >= self.max_memories && !self.memories.contains_key(key) {
            self.evict_lru();
        }

        let vector = self.encode(content);
        let now = now_secs();

        let entry = MemoryEntry {
            key: key.to_string(),
            content: content.to_string(),
            vector,
            metadata: metadata.unwrap_or(serde_json::Value::Null),
            created_at: now,
            last_access: now,
        };

        self.memories.insert(key.to_string(), entry);
        self.access_counts.insert(key.to_string(), 0);

        key.to_string()
    }

    /// Recall memories similar to the query text.
    pub fn recall(&mut self, query: &str, top_k: Option<usize>) -> Vec<RecallResult> {
        self.total_recalls += 1;
        let k = top_k.unwrap_or(self.recall_top_k);
        let query_vec = self.encode(query);

        let mut results: Vec<RecallResult> = self
            .memories
            .values()
            .filter_map(|entry| {
                let sim = cosine_similarity(&query_vec, &entry.vector);
                if sim >= self.similarity_threshold {
                    Some(RecallResult {
                        key: entry.key.clone(),
                        content: entry.content.clone(),
                        similarity: sim,
                        metadata: entry.metadata.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by similarity descending
        results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);

        // Update access counts and timestamps
        let now = now_secs();
        for r in &results {
            if let Some(entry) = self.memories.get_mut(&r.key) {
                entry.last_access = now;
            }
            *self.access_counts.entry(r.key.clone()).or_insert(0) += 1;
        }

        results
    }

    /// Bind multiple concepts together via element-wise multiplication.
    pub fn bind_concepts(&mut self, concepts: &[&str]) -> Vec<f64> {
        self.total_binds += 1;

        let vectors: Vec<Vec<f64>> = concepts
            .iter()
            .map(|c| {
                let lower = c.to_lowercase();
                if let Some(v) = self.codebook.get(&lower) {
                    v.clone()
                } else {
                    let v = random_vector(self.dimension);
                    self.codebook.insert(lower, v.clone());
                    v
                }
            })
            .collect();

        if vectors.is_empty() {
            return vec![0.0; self.dimension];
        }

        let mut result = vectors[0].clone();
        for v in &vectors[1..] {
            result = bind(&result, v);
        }

        // Apply binding strength scaling
        let result: Vec<f64> = result.iter().map(|x| x * self.binding_strength).collect();
        normalize(&result)
    }

    /// Evict the least-recently-used memory entry.
    fn evict_lru(&mut self) {
        if let Some(key) = self
            .memories
            .iter()
            .min_by(|a, b| {
                a.1.last_access
                    .partial_cmp(&b.1.last_access)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(k, _)| k.clone())
        {
            self.memories.remove(&key);
            self.access_counts.remove(&key);
        }
    }

    /// Get usage statistics.
    pub fn get_stats(&self) -> HolographicStats {
        HolographicStats {
            memory_count: self.memories.len(),
            codebook_size: self.codebook.len(),
            total_binds: self.total_binds,
            total_recalls: self.total_recalls,
            dimension: self.dimension,
        }
    }

    /// Number of stored memories.
    pub fn memory_count(&self) -> usize {
        self.memories.len()
    }

    /// Dimensionality of hypervectors.
    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

impl Default for HolographicMemory {
    fn default() -> Self {
        Self::new(HolographicConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn small_config() -> HolographicConfig {
        HolographicConfig {
            dimension: 500,
            max_memories: 100,
            similarity_threshold: 0.1,
            decay_rate: 0.001,
            binding_strength: 0.85,
            recall_top_k: 5,
        }
    }

    #[test]
    fn test_new_seeds_codebook() {
        let mem = HolographicMemory::new(small_config());
        assert_eq!(mem.codebook.len(), BASE_CONCEPTS.len());
        assert!(mem.codebook.contains_key("time"));
        assert!(mem.codebook.contains_key("memory"));
    }

    #[test]
    fn test_encode_returns_correct_dimension() {
        let mut mem = HolographicMemory::new(small_config());
        let vec = mem.encode("hello world test");
        assert_eq!(vec.len(), 500);
    }

    #[test]
    fn test_encode_empty_text() {
        let mut mem = HolographicMemory::new(small_config());
        let vec = mem.encode("");
        assert_eq!(vec.len(), 500);
        // Zero vector
        assert!(vec.iter().all(|v| v.abs() < f64::EPSILON));
    }

    #[test]
    fn test_store_and_recall() {
        let mut mem = HolographicMemory::new(small_config());
        mem.store("greeting", "hello world", None);
        mem.store("farewell", "goodbye world", None);

        let results = mem.recall("hello world", None);
        assert!(!results.is_empty());
        // The best match should be the greeting
        assert_eq!(results[0].key, "greeting");
    }

    #[test]
    fn test_store_returns_key() {
        let mut mem = HolographicMemory::new(small_config());
        let key = mem.store("test_key", "test content", None);
        assert_eq!(key, "test_key");
    }

    #[test]
    fn test_lru_eviction() {
        let mut config = small_config();
        config.max_memories = 3;
        let mut mem = HolographicMemory::new(config);

        mem.store("a", "alpha content", None);
        mem.store("b", "beta content", None);
        mem.store("c", "gamma content", None);
        // This should evict "a" (oldest, never accessed)
        mem.store("d", "delta content", None);

        assert_eq!(mem.memory_count(), 3);
        assert!(!mem.memories.contains_key("a"));
        assert!(mem.memories.contains_key("d"));
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn test_bind_concepts() {
        let mut mem = HolographicMemory::new(small_config());
        let bound = mem.bind_concepts(&["time", "action"]);
        assert_eq!(bound.len(), 500);
        // Bound vector should be normalized (unit length)
        let mag: f64 = bound.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((mag - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_stats() {
        let mut mem = HolographicMemory::new(small_config());
        mem.store("k1", "content one", None);
        mem.recall("content", None);
        mem.bind_concepts(&["time"]);

        let stats = mem.get_stats();
        assert_eq!(stats.memory_count, 1);
        assert!(stats.codebook_size >= BASE_CONCEPTS.len());
        assert_eq!(stats.total_recalls, 1);
        assert_eq!(stats.total_binds, 1);
    }

    #[test]
    fn test_normalize_zero_vector() {
        let v = vec![0.0, 0.0, 0.0];
        let n = normalize(&v);
        assert!(n.iter().all(|x| x.abs() < f64::EPSILON));
    }

    #[test]
    fn test_normalize_unit_vector() {
        let v = vec![3.0, 4.0];
        let n = normalize(&v);
        let mag: f64 = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((mag - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_metadata_stored() {
        let mut mem = HolographicMemory::new(small_config());
        let meta = serde_json::json!({"source": "test", "priority": 5});
        mem.store("m1", "content with metadata", Some(meta.clone()));

        let results = mem.recall("content with metadata", None);
        assert!(!results.is_empty());
        assert_eq!(results[0].metadata, meta);
    }
}
