//! Inter-Brain Messaging Protocol.
//!
//! Defines the message types that flow between the cognitive actors
//! (Prime, Subconscious, Warden) over Tokio `mpsc` channels.

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Messages that can be sent to the Prime actor.
#[derive(Debug)]
pub enum CognitiveMessage {
    /// A user chat request — Prime generates a response and sends it back
    /// through the `reply` channel.
    UserChat {
        text: String,
        reply: oneshot::Sender<String>,
    },

    /// A security alert from the Warden — Prime must interrupt any current
    /// generation and address the threat immediately.
    SecurityAlert {
        threat_level: String,
        details: String,
    },

    /// A background insight from the Subconscious — Prime can incorporate
    /// this into its context for richer responses.
    SubconsciousInsight { memory_summary: String },
}

/// Messages that the Warden actor produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub threat_level: String,
    pub details: String,
    pub timestamp: u64,
}

/// Messages sent to the Subconscious actor to trigger a reflection cycle.
#[derive(Debug)]
pub enum SubconsciousCommand {
    /// Trigger an immediate reflection with the given context.
    Reflect { context: String },
    /// Graceful shutdown.
    Shutdown,
}

/// Messages sent to the Warden actor.
#[derive(Debug)]
pub enum WardenCommand {
    /// Run a security scan with the given context.
    Scan { context: String },
    /// Graceful shutdown.
    Shutdown,
}
