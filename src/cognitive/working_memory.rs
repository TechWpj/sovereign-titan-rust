//! Working Memory — salience-scored context buffer with eviction.
//!
//! Ported from `sovereign_titan/cognitive/working_memory.py`.
//! Maintains a fixed-size buffer of memory items sorted by salience,
//! automatically evicting low-salience and stale items when the token
//! budget is exceeded.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use uuid::Uuid;

/// Maximum token budget for the working memory prompt block.
const DEFAULT_TOKEN_BUDGET: usize = 8192;

/// Approximate characters per token (conservative estimate).
const CHARS_PER_TOKEN: usize = 4;

/// Default staleness timeout (5 minutes).
const DEFAULT_STALE_SECS: u64 = 300;

/// Jaccard similarity threshold for dedup.
const DEDUP_THRESHOLD: f64 = 0.85;

/// A single item in working memory.
#[derive(Debug, Clone)]
pub struct WorkingMemoryItem {
    pub id: String,
    pub content: String,
    pub source: String,
    pub salience: f32,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub access_count: u32,
}

impl WorkingMemoryItem {
    /// Create a new memory item.
    pub fn new(content: &str, source: &str, salience: f32) -> Self {
        let now = Instant::now();
        // Cap content at 500 chars like the Python version.
        let content = if content.len() > 500 {
            content[..500].to_string()
        } else {
            content.to_string()
        };

        Self {
            id: Uuid::new_v4().to_string()[..8].to_string(),
            content,
            source: source.to_string(),
            salience: salience.clamp(0.0, 1.0),
            created_at: now,
            last_accessed: now,
            access_count: 0,
        }
    }
}

/// Fixed-size working memory buffer with salience-based eviction.
pub struct WorkingMemory {
    items: Vec<WorkingMemoryItem>,
    max_items: usize,
    token_budget: usize,
    stale_duration: Duration,
}

impl WorkingMemory {
    /// Create a new working memory with configurable limits.
    pub fn new(max_items: usize, token_budget: usize) -> Self {
        Self {
            items: Vec::new(),
            max_items,
            token_budget,
            stale_duration: Duration::from_secs(DEFAULT_STALE_SECS),
        }
    }

    /// Create with default settings (32 items, 8192 token budget).
    pub fn with_defaults() -> Self {
        Self::new(32, DEFAULT_TOKEN_BUDGET)
    }

    /// Add a memory item. Deduplicates via Jaccard similarity and evicts
    /// stale/low-salience items when at capacity.
    pub fn add(&mut self, content: &str, source: &str, salience: f32) -> String {
        // First, evict stale items.
        self.evict_stale();

        // Check for duplicate content via Jaccard similarity.
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|item| jaccard_similarity(&item.content, content) > DEDUP_THRESHOLD)
        {
            // Update existing item with higher salience.
            if salience > existing.salience {
                existing.salience = salience;
            }
            existing.last_accessed = Instant::now();
            existing.access_count += 1;
            return existing.id.clone();
        }

        // Evict lowest-salience item if at capacity.
        if self.items.len() >= self.max_items {
            // Find the index of the item with lowest salience.
            if let Some((min_idx, _)) = self
                .items
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.salience.partial_cmp(&b.salience).unwrap())
            {
                self.items.remove(min_idx);
            }
        }

        let item = WorkingMemoryItem::new(content, source, salience);
        let id = item.id.clone();
        self.items.push(item);
        id
    }

    /// Remove stale items (older than stale_duration since last access).
    fn evict_stale(&mut self) {
        let now = Instant::now();
        self.items
            .retain(|item| now.duration_since(item.last_accessed) < self.stale_duration);
    }

    /// Generate a formatted prompt block for injection into the LLM context.
    /// Returns items sorted by salience (highest first), trimmed to fit the
    /// token budget.
    pub fn get_prompt_block(&mut self) -> String {
        if self.items.is_empty() {
            return String::new();
        }

        // Update access metadata.
        let now = Instant::now();
        for item in &mut self.items {
            item.last_accessed = now;
            item.access_count += 1;
        }

        // Sort by salience descending.
        let mut sorted: Vec<&WorkingMemoryItem> = self.items.iter().collect();
        sorted.sort_by(|a, b| b.salience.partial_cmp(&a.salience).unwrap());

        let char_budget = self.token_budget * CHARS_PER_TOKEN;
        let mut block = String::from("[WORKING MEMORY]\n");
        let mut used = block.len();

        for item in sorted {
            let line = format!(
                "- [{}|{:.1}] {}\n",
                item.source, item.salience, item.content
            );
            if used + line.len() > char_budget {
                break;
            }
            block.push_str(&line);
            used += line.len();
        }

        block
    }

    /// Get all current items (read-only snapshot).
    pub fn get_items(&self) -> Vec<WorkingMemoryItem> {
        self.items.clone()
    }

    /// Number of items currently stored.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the memory is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Jaccard similarity between two strings (word-level).
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_retrieve() {
        let mut wm = WorkingMemory::with_defaults();
        let id = wm.add("Hello world", "user", 0.8);
        assert!(!id.is_empty());
        assert_eq!(wm.len(), 1);
    }

    #[test]
    fn test_dedup() {
        let mut wm = WorkingMemory::with_defaults();
        let id1 = wm.add("The quick brown fox jumps over the lazy dog", "user", 0.5);
        let id2 = wm.add("The quick brown fox jumps over the lazy dog", "user", 0.9);
        // Should deduplicate and return the same ID.
        assert_eq!(id1, id2);
        assert_eq!(wm.len(), 1);
        // Salience should be upgraded.
        assert!(wm.get_items()[0].salience >= 0.9);
    }

    #[test]
    fn test_eviction_at_capacity() {
        let mut wm = WorkingMemory::new(3, DEFAULT_TOKEN_BUDGET);
        wm.add("Item one", "test", 0.5);
        wm.add("Item two", "test", 0.3);
        wm.add("Item three", "test", 0.9);
        assert_eq!(wm.len(), 3);

        // Adding a 4th should evict the lowest salience (0.3).
        wm.add("Item four", "test", 0.7);
        assert_eq!(wm.len(), 3);

        // "Item two" (salience 0.3) should be gone.
        let items = wm.get_items();
        assert!(items.iter().all(|i| i.content != "Item two"));
    }

    #[test]
    fn test_prompt_block() {
        let mut wm = WorkingMemory::with_defaults();
        wm.add("Important fact", "knowledge", 0.9);
        wm.add("Less important", "context", 0.3);

        let block = wm.get_prompt_block();
        assert!(block.starts_with("[WORKING MEMORY]"));
        assert!(block.contains("Important fact"));
        // Higher salience items should appear first.
        let pos_high = block.find("Important fact").unwrap();
        let pos_low = block.find("Less important").unwrap();
        assert!(pos_high < pos_low);
    }

    #[test]
    fn test_jaccard_similarity() {
        assert!((jaccard_similarity("a b c", "a b c") - 1.0).abs() < 1e-5);
        assert!(jaccard_similarity("a b c", "d e f") < 0.01);
        assert!(jaccard_similarity("a b c d", "a b c e") > 0.5);
    }

    #[test]
    fn test_clear() {
        let mut wm = WorkingMemory::with_defaults();
        wm.add("Test", "test", 0.5);
        wm.clear();
        assert!(wm.is_empty());
    }

    #[test]
    fn test_content_cap() {
        let long = "x".repeat(1000);
        let mut wm = WorkingMemory::with_defaults();
        wm.add(&long, "test", 0.5);
        assert!(wm.get_items()[0].content.len() <= 500);
    }
}
