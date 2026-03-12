//! Security Warden — native Windows ETW monitoring, NDR, and threat response.
//!
//! Integrates IDS, encrypted traffic analysis, and DNS intelligence sensors
//! into the warden actor's periodic scan cycle.

pub mod etw;
pub mod network;
pub mod response;
pub mod sensors;

