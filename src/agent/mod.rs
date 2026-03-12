//! Agent module — ReAct agent loop, parser, DAG executor, intelligence layer,
//! tone detection, verbosity control, and skill registry.

pub mod context;
pub mod dag;
pub mod distiller;
pub mod execution_paths;
pub mod prompt_compiler;
pub mod quality;
pub mod react;
pub mod skill_registry;
pub mod tone;
pub mod tool_memory;
pub mod verbosity;
