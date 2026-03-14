//! Agent module — ReAct agent loop, parser, DAG executor, intelligence layer,
//! tone detection, verbosity control, prediction capture, and skill registry.

pub mod context;
pub mod dag;
pub mod distiller;
pub mod execution_paths;
pub mod prediction_capture;
pub mod prompt_compiler;
pub mod prose_recovery;
pub mod quality;
pub mod react;
pub mod skill_registry;
pub mod task_planner;
pub mod tone;
pub mod tool_memory;
pub mod verbosity;
