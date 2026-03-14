//! Self-Improvement — feedback-driven learning, proposal lifecycle, and audit trail.
//!
//! Provides a [`SelfImprovementEngine`] that learns from user feedback,
//! a [`ProposalManager`] for managing code improvement proposals, and
//! a [`SelfImprovementLog`] for tracking all changes.

pub mod change_log;
pub mod engine;
pub mod proposals;
pub mod types;
