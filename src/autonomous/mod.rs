//! Autonomous Module — task execution, planning, and background processing.
//!
//! Provides an [`AutonomousRunner`] that orchestrates multi-step task execution
//! with checkpointing and approval gates, plus a [`BackgroundTaskManager`]
//! for concurrent background work.

pub mod background;
pub mod runner;
pub mod types;
