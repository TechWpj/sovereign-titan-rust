//! Context Manager — compresses conversation history to stay within budget.
//!
//! Ported from `sovereign_titan/agents/distiller.py` context management.
//! Drops old OBSERVATION blocks when conversation exceeds the compression
//! threshold, keeping the most recent observations intact.

/// Default compression threshold (chars) — compress when context exceeds this.
const DEFAULT_COMPRESS_THRESHOLD: usize = 20_000;

/// Default maximum context length (chars).
const DEFAULT_MAX_CONTEXT_LEN: usize = 24_000;

/// Number of recent observations to always preserve.
const KEEP_RECENT_OBSERVATIONS: usize = 3;

/// Manages conversation context size by selectively compressing old observations.
pub struct ContextManager {
    /// Trigger compression when context exceeds this length.
    compress_threshold: usize,
    /// Hard maximum context length — truncate from front if exceeded.
    max_context_len: usize,
    /// Number of recent observations to preserve.
    keep_recent: usize,
}

impl ContextManager {
    /// Create a new context manager with default thresholds.
    pub fn new() -> Self {
        Self {
            compress_threshold: DEFAULT_COMPRESS_THRESHOLD,
            max_context_len: DEFAULT_MAX_CONTEXT_LEN,
            keep_recent: KEEP_RECENT_OBSERVATIONS,
        }
    }

    /// Create a context manager with custom thresholds.
    pub fn with_thresholds(compress_threshold: usize, max_context_len: usize) -> Self {
        Self {
            compress_threshold,
            max_context_len,
            keep_recent: KEEP_RECENT_OBSERVATIONS,
        }
    }

    /// Compress conversation context if it exceeds the threshold.
    ///
    /// Strategy:
    /// 1. If under threshold, return unchanged.
    /// 2. Replace old OBSERVATION blocks with a placeholder, keeping the
    ///    most recent `keep_recent` observations.
    /// 3. If still over `max_context_len`, hard-truncate from the front.
    pub fn compress(&self, conversation: &str) -> String {
        if conversation.len() <= self.compress_threshold {
            return conversation.to_string();
        }

        let mut result = String::new();
        let mut in_observation = false;
        let mut observation_count = 0;

        // Count total observations
        let total_observations = conversation.matches("OBSERVATION:").count();
        let keep_from = total_observations.saturating_sub(self.keep_recent);

        for line in conversation.lines() {
            if line.starts_with("OBSERVATION:") {
                observation_count += 1;
                if observation_count <= keep_from {
                    result.push_str("OBSERVATION: [compressed — see recent observations]\n");
                    in_observation = true;
                    continue;
                }
                in_observation = false;
            } else if in_observation {
                // End observation block when we hit a known structural marker
                if line.starts_with("THOUGHT:")
                    || line.starts_with("ACTION:")
                    || line.starts_with("ACTION_INPUT:")
                    || line.starts_with("User:")
                    || line.starts_with("SYSTEM:")
                    || line.starts_with("FINAL_ANSWER:")
                {
                    in_observation = false;
                }
            }

            if in_observation {
                continue;
            }

            result.push_str(line);
            result.push('\n');
        }

        // Hard-truncate from front if still too long
        if result.len() > self.max_context_len {
            let skip = result.len() - self.max_context_len;
            format!("[...context truncated...]\n{}", &result[skip..])
        } else {
            result
        }
    }

    /// Returns the current compression threshold.
    pub fn compress_threshold(&self) -> usize {
        self.compress_threshold
    }

    /// Returns the maximum context length.
    pub fn max_context_len(&self) -> usize {
        self.max_context_len
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_context_unchanged() {
        let cm = ContextManager::new();
        let ctx = "User: hello\nTHOUGHT: thinking\n";
        assert_eq!(cm.compress(ctx), ctx);
    }

    #[test]
    fn test_compress_drops_old_observations() {
        let cm = ContextManager::with_thresholds(1000, 5000);
        let mut ctx = String::from("User: test\n");
        for i in 0..6 {
            ctx.push_str(&format!("THOUGHT: step {i}\n"));
            ctx.push_str(&format!("ACTION: tool_{i}\n"));
            let obs = "x".repeat(300);
            ctx.push_str(&format!("OBSERVATION: {obs}\n"));
        }

        let compressed = cm.compress(&ctx);
        assert!(compressed.len() < ctx.len());
        assert!(compressed.contains("compressed"));
        // Last 3 observations should be preserved
        assert!(compressed.contains(&"x".repeat(300)));
    }

    #[test]
    fn test_compress_keeps_recent_observations() {
        let cm = ContextManager::with_thresholds(100, 50_000);
        let mut ctx = String::from("User: test\n");
        for i in 0..5 {
            ctx.push_str(&format!("OBSERVATION: result_{i}\n"));
        }

        let compressed = cm.compress(&ctx);
        // Should keep last 3
        assert!(compressed.contains("result_2"));
        assert!(compressed.contains("result_3"));
        assert!(compressed.contains("result_4"));
    }

    #[test]
    fn test_hard_truncate_when_still_too_long() {
        let cm = ContextManager::with_thresholds(100, 500);
        let ctx = "a".repeat(1000);
        let compressed = cm.compress(&ctx);
        assert!(compressed.len() <= 600); // 500 + some overhead for marker
        assert!(compressed.contains("context truncated"));
    }

    #[test]
    fn test_default_thresholds() {
        let cm = ContextManager::new();
        assert_eq!(cm.compress_threshold(), 20_000);
        assert_eq!(cm.max_context_len(), 24_000);
    }

    #[test]
    fn test_custom_thresholds() {
        let cm = ContextManager::with_thresholds(5000, 10000);
        assert_eq!(cm.compress_threshold(), 5000);
        assert_eq!(cm.max_context_len(), 10000);
    }

    #[test]
    fn test_no_observations_to_compress() {
        let cm = ContextManager::with_thresholds(10, 50_000);
        let ctx = "User: hello\nTHOUGHT: thinking about this\nFINAL_ANSWER: hi\n";
        let compressed = cm.compress(ctx);
        // No observations to drop, just returns as-is (or slightly modified)
        assert!(compressed.contains("THOUGHT: thinking"));
    }

    #[test]
    fn test_compress_preserves_thoughts_and_actions() {
        let cm = ContextManager::with_thresholds(100, 50_000);
        let mut ctx = String::from("User: test query\n");
        for i in 0..5 {
            ctx.push_str(&format!("THOUGHT: reasoning step {i}\n"));
            ctx.push_str(&format!("ACTION: tool_{i}\n"));
            ctx.push_str(&format!("OBSERVATION: {}\n", "data ".repeat(50)));
        }

        let compressed = cm.compress(&ctx);
        // All thoughts should be preserved
        for i in 0..5 {
            assert!(compressed.contains(&format!("THOUGHT: reasoning step {i}")));
        }
    }
}
