//! Inference Cache — semantic caching of inference results.
//!
//! Ported from `sovereign_titan/cognitive/inference_cache.py`.
//! Caches LLM responses keyed by prompt hash, with TTL-based expiry.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// A cached inference entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    response: String,
    created: f64,
    hits: u64,
    prompt_preview: String,
}

/// Inference result cache with TTL-based expiry.
pub struct InferenceCache {
    /// Cache entries keyed by prompt hash.
    entries: HashMap<u64, CacheEntry>,
    /// Time-to-live for cache entries (seconds).
    ttl_secs: f64,
    /// Maximum cache size.
    max_entries: usize,
    /// Total cache hits.
    total_hits: u64,
    /// Total cache misses.
    total_misses: u64,
}

impl InferenceCache {
    /// Create a new inference cache.
    pub fn new(ttl_secs: f64, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl_secs,
            max_entries,
            total_hits: 0,
            total_misses: 0,
        }
    }

    /// Compute a hash for a prompt string.
    fn hash_prompt(prompt: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        prompt.hash(&mut hasher);
        hasher.finish()
    }

    /// Get a cached response for a prompt.
    pub fn get(&mut self, prompt: &str) -> Option<String> {
        let hash = Self::hash_prompt(prompt);

        if let Some(entry) = self.entries.get_mut(&hash) {
            // Check TTL
            if now_secs() - entry.created > self.ttl_secs {
                self.entries.remove(&hash);
                self.total_misses += 1;
                return None;
            }
            entry.hits += 1;
            self.total_hits += 1;
            Some(entry.response.clone())
        } else {
            self.total_misses += 1;
            None
        }
    }

    /// Store a response for a prompt.
    pub fn put(&mut self, prompt: &str, response: &str) {
        // Evict expired entries if at capacity
        if self.entries.len() >= self.max_entries {
            self.evict_expired();
        }

        // If still at capacity, evict least-hit entry
        if self.entries.len() >= self.max_entries {
            self.evict_least_used();
        }

        let hash = Self::hash_prompt(prompt);
        self.entries.insert(hash, CacheEntry {
            response: response.to_string(),
            created: now_secs(),
            hits: 0,
            prompt_preview: prompt.chars().take(80).collect(),
        });
    }

    /// Evict expired entries.
    fn evict_expired(&mut self) {
        let now = now_secs();
        self.entries.retain(|_, entry| now - entry.created <= self.ttl_secs);
    }

    /// Evict the least-used entry.
    fn evict_least_used(&mut self) {
        if let Some((&key, _)) = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.hits)
        {
            self.entries.remove(&key);
        }
    }

    /// Clear all cache entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Hit rate as a ratio.
    pub fn hit_rate(&self) -> f64 {
        let total = self.total_hits + self.total_misses;
        if total == 0 {
            0.0
        } else {
            self.total_hits as f64 / total as f64
        }
    }

    /// Cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            max_entries: self.max_entries,
            total_hits: self.total_hits,
            total_misses: self.total_misses,
            hit_rate: self.hit_rate(),
            ttl_secs: self.ttl_secs,
        }
    }
}

impl Default for InferenceCache {
    fn default() -> Self {
        Self::new(300.0, 500) // 5-minute TTL, 500 max entries
    }
}

/// Cache statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub entries: usize,
    pub max_entries: usize,
    pub total_hits: u64,
    pub total_misses: u64,
    pub hit_rate: f64,
    pub ttl_secs: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get() {
        let mut cache = InferenceCache::default();
        cache.put("What is 2+2?", "4");
        let result = cache.get("What is 2+2?");
        assert_eq!(result, Some("4".to_string()));
    }

    #[test]
    fn test_cache_miss() {
        let mut cache = InferenceCache::default();
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_ttl_expiry() {
        let mut cache = InferenceCache::new(0.0, 100); // 0 TTL = instant expiry
        cache.put("test", "value");
        // Should expire immediately (or very quickly)
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(cache.get("test").is_none());
    }

    #[test]
    fn test_max_entries() {
        let mut cache = InferenceCache::new(300.0, 2);
        cache.put("a", "1");
        cache.put("b", "2");
        cache.put("c", "3"); // Should evict one
        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_clear() {
        let mut cache = InferenceCache::default();
        cache.put("a", "1");
        cache.put("b", "2");
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_hit_rate() {
        let mut cache = InferenceCache::default();
        cache.put("key", "value");
        cache.get("key"); // hit
        cache.get("nonexistent"); // miss
        assert!((cache.hit_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_stats() {
        let cache = InferenceCache::default();
        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.max_entries, 500);
    }

    #[test]
    fn test_overwrite() {
        let mut cache = InferenceCache::default();
        cache.put("key", "value1");
        cache.put("key", "value2");
        assert_eq!(cache.get("key"), Some("value2".to_string()));
    }
}
