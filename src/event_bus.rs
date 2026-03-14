//! Event Bus — `tokio::sync::broadcast`-based publish/subscribe system.
//!
//! Replaces Python's circular `wire_subsystems()` pattern with a
//! borrow-checker-friendly event-driven architecture. Any subsystem can
//! emit events, and any number of listeners can subscribe.
//!
//! No subsystem holds a direct reference to another subsystem. Instead:
//! 1. Each subsystem receives a clone of the `Sender` to publish events
//! 2. Each subsystem subscribes to a `Receiver` to react to events
//! 3. The event bus handles one-to-many distribution automatically
//!
//! This eliminates the `Arc<Mutex<T>>` deadlock risks that arise from
//! replicating Python's `wire_subsystems()` circular dependency graph.

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::info;

// ─────────────────────────────────────────────────────────────────────────────
// Event types
// ─────────────────────────────────────────────────────────────────────────────

/// Events that flow through the cognitive event bus.
///
/// Every cross-subsystem interaction is encoded as a variant of this enum.
/// Subsystems publish events via `EventBus::publish()` or `Sender::send()`,
/// and react to events via their async listener loops.
#[derive(Debug, Clone, Serialize)]
pub enum CognitiveEvent {
    /// A thought was scored by the ThoughtQualityScorer.
    ///
    /// Consumed by: QuantumConceptLayer (Hebbian learning + concept excitation)
    ThoughtScored {
        /// Thought category (e.g. "self_awareness", "creativity", "security").
        category: String,
        /// Quality score (0.0–1.0).
        score: f64,
    },

    /// A consciousness mode was selected by the ThompsonSampler.
    ///
    /// Consumed by: QuantumConceptLayer (mode-linked concept excitation)
    ModeSelected {
        /// Mode name (e.g. "think", "learn", "act").
        mode: String,
        /// Whether the mode produced a successful outcome.
        success: bool,
    },

    /// A tool completed execution in the ReAct agent.
    ///
    /// Consumed by: QuantumConceptLayer (Operational concept excitation),
    ///              ToolOutcomeMemory (success/failure tracking)
    ToolOutcome {
        /// Tool name.
        tool_name: String,
        /// Whether execution succeeded.
        success: bool,
    },

    /// Security anomaly detected by the Warden or security subsystem.
    ///
    /// Consumed by: QuantumConceptLayer (Environmental concept excitation)
    SecurityAnomaly {
        /// Severity (0.0–1.0).
        severity: f64,
        /// Detailed description of the anomaly.
        description: String,
    },

    /// Knowledge was extracted and stored in the KnowledgeGraph.
    ///
    /// Consumed by: Future subsystems that react to knowledge growth.
    KnowledgeExtracted {
        /// Knowledge domain.
        domain: String,
        /// Number of triples extracted.
        triples: usize,
    },

    /// Metacognitive health score updated.
    ///
    /// Consumed by: QuantumConceptLayer (entanglement strength adjustment)
    MetacognitiveHealth {
        /// Health score (0.0–1.0). Low = poor, high = healthy.
        health_score: f64,
    },

    // ── Legacy variants (kept for backward compatibility) ──────────────

    /// A tool completed execution (legacy variant with duration).
    ToolExecuted {
        /// Tool name.
        tool: String,
        /// Whether execution succeeded.
        success: bool,
        /// Execution duration in milliseconds.
        duration_ms: u64,
    },

    /// Subconscious produced a background insight.
    InsightGenerated {
        /// The insight text.
        insight: String,
        /// Unix timestamp.
        timestamp: u64,
    },

    /// Warden flagged a security threat.
    ThreatDetected {
        /// Threat level: NONE, LOW, MEDIUM, HIGH, CRITICAL.
        level: String,
        /// Detailed threat description.
        details: String,
        /// Unix timestamp.
        timestamp: u64,
    },

    /// Knowledge graph or working memory was updated.
    MemoryUpdated {
        /// Type of memory operation.
        operation: String,
        /// Key or entity that changed.
        key: String,
    },

    /// Generic state change notification.
    StateChanged {
        /// Component that changed.
        component: String,
        /// Description of the change.
        description: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Event Bus
// ─────────────────────────────────────────────────────────────────────────────

/// Broadcast-based event bus for inter-subsystem communication.
///
/// Uses `tokio::sync::broadcast` for one-to-many event distribution.
/// Each subscriber gets its own copy of every event, satisfying Rust's
/// ownership model without `Arc<Mutex<>>` contention.
///
/// # Usage
///
/// ```ignore
/// let bus = EventBus::new(256);
///
/// // Give subsystems a Sender clone to publish events
/// let sender = bus.sender();
/// sender.send(CognitiveEvent::ToolOutcome { ... });
///
/// // Give subsystems a Receiver to react to events
/// let receiver = bus.subscribe();
/// tokio::spawn(async move {
///     while let Ok(event) = receiver.recv().await {
///         // handle event
///     }
/// });
/// ```
pub struct EventBus {
    tx: broadcast::Sender<CognitiveEvent>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    ///
    /// Capacity determines how many events can be buffered before slow
    /// subscribers start missing events (they receive `RecvError::Lagged`).
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        info!("Event bus initialized (capacity={capacity})");
        Self { tx }
    }

    /// Get a clone of the broadcast sender.
    ///
    /// Distribute this to subsystems so they can publish events without
    /// holding a reference to the EventBus itself.
    pub fn sender(&self) -> broadcast::Sender<CognitiveEvent> {
        self.tx.clone()
    }

    /// Subscribe to the event bus. Returns a receiver that yields all
    /// future events.
    pub fn subscribe(&self) -> broadcast::Receiver<CognitiveEvent> {
        self.tx.subscribe()
    }

    /// Publish an event to all subscribers.
    ///
    /// Returns the number of active subscribers that received the event.
    /// If there are no subscribers, the event is silently dropped.
    pub fn publish(&self, event: CognitiveEvent) -> usize {
        match self.tx.send(event) {
            Ok(n) => n,
            Err(_) => 0, // No active receivers
        }
    }

    /// Get the current number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::StateChanged {
            component: "test".to_string(),
            description: "hello".to_string(),
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::StateChanged {
                component,
                description,
            } => {
                assert_eq!(component, "test");
                assert_eq!(description, "hello");
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        assert_eq!(bus.subscriber_count(), 2);

        bus.publish(CognitiveEvent::ToolExecuted {
            tool: "shell".to_string(),
            success: true,
            duration_ms: 42,
        });

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();

        match (e1, e2) {
            (
                CognitiveEvent::ToolExecuted { tool: t1, .. },
                CognitiveEvent::ToolExecuted { tool: t2, .. },
            ) => {
                assert_eq!(t1, "shell");
                assert_eq!(t2, "shell");
            }
            _ => panic!("wrong event types"),
        }
    }

    #[test]
    fn test_publish_no_subscribers() {
        let bus = EventBus::new(16);
        let count = bus.publish(CognitiveEvent::StateChanged {
            component: "test".to_string(),
            description: "nobody listening".to_string(),
        });
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_thought_scored_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::ThoughtScored {
            category: "creativity".to_string(),
            score: 0.85,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::ThoughtScored { category, score } => {
                assert_eq!(category, "creativity");
                assert!((score - 0.85).abs() < f64::EPSILON);
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_mode_selected_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::ModeSelected {
            mode: "think".to_string(),
            success: true,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::ModeSelected { mode, success } => {
                assert_eq!(mode, "think");
                assert!(success);
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_tool_outcome_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::ToolOutcome {
            tool_name: "web_search".to_string(),
            success: true,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::ToolOutcome {
                tool_name,
                success,
            } => {
                assert_eq!(tool_name, "web_search");
                assert!(success);
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_security_anomaly_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::SecurityAnomaly {
            severity: 0.8,
            description: "Suspicious process detected".to_string(),
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::SecurityAnomaly {
                severity,
                description,
            } => {
                assert!((severity - 0.8).abs() < f64::EPSILON);
                assert!(description.contains("Suspicious"));
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_knowledge_extracted_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::KnowledgeExtracted {
            domain: "rust".to_string(),
            triples: 15,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::KnowledgeExtracted { domain, triples } => {
                assert_eq!(domain, "rust");
                assert_eq!(triples, 15);
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_metacognitive_health_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::MetacognitiveHealth {
            health_score: 0.75,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::MetacognitiveHealth { health_score } => {
                assert!((health_score - 0.75).abs() < f64::EPSILON);
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_sender_clone() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        let sender = bus.sender();

        // Publish via cloned sender (simulating a subsystem)
        let _ = sender.send(CognitiveEvent::ToolOutcome {
            tool_name: "shell".to_string(),
            success: false,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::ToolOutcome {
                tool_name,
                success,
            } => {
                assert_eq!(tool_name, "shell");
                assert!(!success);
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_threat_detected_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(CognitiveEvent::ThreatDetected {
            level: "HIGH".to_string(),
            details: "Suspicious network activity".to_string(),
            timestamp: 1234567890,
        });

        let event = rx.recv().await.unwrap();
        match event {
            CognitiveEvent::ThreatDetected { level, .. } => {
                assert_eq!(level, "HIGH");
            }
            _ => panic!("wrong event type"),
        }
    }
}
