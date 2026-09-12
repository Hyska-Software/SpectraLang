// Spectra Intermediate Representation (SIR)
// SSA-based IR for optimization and code generation

pub mod autodiff;
pub mod builder;
pub mod ir;
pub mod layout;
pub mod lowering;
pub mod passes;
pub mod tensor_graph;

pub use autodiff::{
    materialize_autodiff_steps, AutodiffDiagnostic, AutodiffFunction, AutodiffGraph, AutodiffNode,
    AutodiffNodeKind,
};
pub use builder::IRBuilder;
pub use ir::{
    BasicBlock, Function as IRFunction, Instruction, Module as IRModule, Type as IRType, Value,
};
pub use lowering::ASTLowering;
pub use tensor_graph::{
    TensorDType, TensorDevice, TensorGraph, TensorGraphComparison, TensorGraphError,
    TensorGraphErrorKind, TensorGraphFunction, TensorGraphLoweringReport,
    TensorGraphLoweringResult, TensorGraphNode, TensorGraphOp, TensorGraphOptimizationReport,
    TensorGraphOptimizationResult, TensorGraphSource, TensorMetadata, TensorShape,
};
