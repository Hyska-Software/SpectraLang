#[path = "tensor_graph_core.rs"]
mod tensor_graph_core;

#[path = "tensor_graph_function.rs"]
mod tensor_graph_function;

#[path = "tensor_graph_analysis.rs"]
mod tensor_graph_analysis;

// Flat namespace for the former include! monolith (consumed by children via `use super::*`).
#[allow(unused_imports)]
pub use {tensor_graph_analysis::*, tensor_graph_core::*, tensor_graph_function::*};
