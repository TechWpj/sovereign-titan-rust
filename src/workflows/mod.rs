//! Workflows — event-driven and schedule-based background task automation.
//!
//! Provides a [`WorkflowEngine`] that manages workflows with schedule-based
//! (interval) or event-based triggers, executing actions when conditions match.

pub mod engine;
