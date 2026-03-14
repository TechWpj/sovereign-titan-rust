//! GPU Scheduler — priority-based GPU access replacing a single model lock.
//!
//! Ported from `sovereign_titan/cognitive/gpu_scheduler.py`.
//! Uses a priority queue:
//!   P0 (CRITICAL) — User chat requests (lowest latency)
//!   P1 (NORMAL)   — Background tasks
//!   P2 (LOW)      — Consciousness / inner monologue cycles
//!
//! Starvation prevention: P2 requests promoted to P1 after 60s waiting.

use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

/// Priority levels (lower = higher priority).
pub const P0_CRITICAL: u8 = 0;
pub const P1_NORMAL: u8 = 1;
pub const P2_LOW: u8 = 2;

/// Starvation timeout — promote P2 to P1 after this many seconds.
const STARVATION_TIMEOUT_SECS: f64 = 60.0;

/// A queued request waiting for GPU access.
struct QueueEntry {
    /// Effective priority (may be promoted).
    priority: u8,
    /// Original priority before promotion.
    original_priority: u8,
    /// When the request was enqueued.
    enqueued_at: Instant,
    /// Monotonic ID for FIFO ordering within same priority.
    id: u64,
    /// Notifier to wake up the waiter.
    notify: Arc<tokio::sync::Notify>,
}

impl Eq for QueueEntry {}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lower priority value = higher priority. Ties broken by earlier ID.
        (self.priority, self.id).cmp(&(other.priority, other.id))
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Statistics for the GPU scheduler.
#[derive(Debug, Clone, Default)]
pub struct SchedulerStats {
    pub total_acquires: u64,
    pub p0_acquires: u64,
    pub p1_acquires: u64,
    pub p2_acquires: u64,
    pub promotions: u64,
    pub total_wait_ms: u64,
}

/// Priority-based GPU access scheduler.
pub struct GpuScheduler {
    /// Semaphore with 1 permit = only 1 holder at a time.
    semaphore: Arc<Semaphore>,
    /// Queue of waiting requests.
    queue: Mutex<BinaryHeap<Reverse<QueueEntry>>>,
    /// Monotonic counter for FIFO ordering.
    counter: AtomicU64,
    /// Stats.
    stats: Mutex<SchedulerStats>,
}

impl GpuScheduler {
    /// Create a new GPU scheduler.
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            queue: Mutex::new(BinaryHeap::new()),
            counter: AtomicU64::new(0),
            stats: Mutex::new(SchedulerStats::default()),
        }
    }

    /// Acquire GPU access with the given priority.
    /// Returns a guard that releases the GPU when dropped.
    pub async fn acquire(&self, priority: u8) -> GpuGuard {
        let start = Instant::now();

        // Try to acquire the semaphore immediately.
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            self.record_acquire(priority, start).await;
            return GpuGuard { _permit: permit };
        }

        // GPU is busy — we need to wait.
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        let notify = Arc::new(tokio::sync::Notify::new());

        {
            let mut queue = self.queue.lock().await;
            queue.push(Reverse(QueueEntry {
                priority,
                original_priority: priority,
                enqueued_at: Instant::now(),
                id,
                notify: notify.clone(),
            }));
        }

        // Wait for our turn.
        loop {
            // Check starvation promotion.
            self.promote_starved().await;

            // Try to acquire.
            if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
                // Check if we're the highest priority waiter.
                let mut queue = self.queue.lock().await;
                if let Some(Reverse(top)) = queue.peek() {
                    if top.id == id {
                        queue.pop();
                        self.record_acquire(priority, start).await;
                        return GpuGuard { _permit: permit };
                    }
                }
                // Not our turn — release and wait.
                drop(permit);
            }

            // Wait a bit before retrying.
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    }

    /// Promote starved P2 requests to P1.
    async fn promote_starved(&self) {
        let mut queue = self.queue.lock().await;
        let mut promoted = false;

        // Collect entries that need promotion
        let entries: Vec<Reverse<QueueEntry>> = queue.drain().collect();
        for Reverse(mut entry) in entries {
            if entry.original_priority == P2_LOW
                && entry.priority == P2_LOW
                && entry.enqueued_at.elapsed().as_secs_f64() > STARVATION_TIMEOUT_SECS
            {
                entry.priority = P1_NORMAL;
                promoted = true;
            }
            queue.push(Reverse(entry));
        }

        if promoted {
            let mut stats = self.stats.lock().await;
            stats.promotions += 1;
        }
    }

    /// Record an acquire in stats.
    async fn record_acquire(&self, priority: u8, start: Instant) {
        let mut stats = self.stats.lock().await;
        stats.total_acquires += 1;
        stats.total_wait_ms += start.elapsed().as_millis() as u64;
        match priority {
            P0_CRITICAL => stats.p0_acquires += 1,
            P1_NORMAL => stats.p1_acquires += 1,
            P2_LOW => stats.p2_acquires += 1,
            _ => {}
        }
    }

    /// Get current scheduler statistics.
    pub async fn get_stats(&self) -> SchedulerStats {
        self.stats.lock().await.clone()
    }

    /// Number of requests currently waiting.
    pub async fn queue_len(&self) -> usize {
        self.queue.lock().await.len()
    }
}

impl Default for GpuScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that releases GPU access when dropped.
pub struct GpuGuard {
    _permit: OwnedSemaphorePermit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_immediate_acquire() {
        let scheduler = GpuScheduler::new();
        let _guard = scheduler.acquire(P0_CRITICAL).await;
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_acquires, 1);
        assert_eq!(stats.p0_acquires, 1);
    }

    #[tokio::test]
    async fn test_sequential_acquires() {
        let scheduler = Arc::new(GpuScheduler::new());
        {
            let _g = scheduler.acquire(P0_CRITICAL).await;
        }
        {
            let _g = scheduler.acquire(P1_NORMAL).await;
        }
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_acquires, 2);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let scheduler = Arc::new(GpuScheduler::new());
        // Just verify the scheduler can handle multiple priorities
        let _g = scheduler.acquire(P2_LOW).await;
        drop(_g);
        let _g = scheduler.acquire(P0_CRITICAL).await;
        drop(_g);
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_acquires, 2);
        assert_eq!(stats.p0_acquires, 1);
        assert_eq!(stats.p2_acquires, 1);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let scheduler = GpuScheduler::new();
        let _g = scheduler.acquire(P0_CRITICAL).await;
        drop(_g);
        let _g = scheduler.acquire(P1_NORMAL).await;
        drop(_g);
        let _g = scheduler.acquire(P2_LOW).await;
        drop(_g);
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_acquires, 3);
        assert_eq!(stats.p0_acquires, 1);
        assert_eq!(stats.p1_acquires, 1);
        assert_eq!(stats.p2_acquires, 1);
    }

    #[test]
    fn test_queue_entry_ordering() {
        let n1 = Arc::new(tokio::sync::Notify::new());
        let n2 = Arc::new(tokio::sync::Notify::new());
        let e1 = QueueEntry {
            priority: P0_CRITICAL,
            original_priority: P0_CRITICAL,
            enqueued_at: Instant::now(),
            id: 1,
            notify: n1,
        };
        let e2 = QueueEntry {
            priority: P2_LOW,
            original_priority: P2_LOW,
            enqueued_at: Instant::now(),
            id: 2,
            notify: n2,
        };
        // P0 should be higher priority (comes first)
        assert!(e1 < e2);
    }

    #[tokio::test]
    async fn test_queue_len() {
        let scheduler = GpuScheduler::new();
        assert_eq!(scheduler.queue_len().await, 0);
    }
}
