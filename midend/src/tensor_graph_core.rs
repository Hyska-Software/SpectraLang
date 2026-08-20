use crate::ir::{InstructionKind, Module, Type, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraph {
    pub module: String,
    pub functions: Vec<TensorGraphFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphFunction {
    pub name: String,
    pub nodes: Vec<TensorGraphNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphNode {
    pub id: usize,
    pub value: Option<usize>,
    pub op: TensorGraphOp,
    pub inputs: Vec<usize>,
    pub output: TensorMetadata,
    pub source: TensorGraphSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphSource {
    pub block: usize,
    pub instruction: usize,
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorGraphOp {
    Parameter,
    Create {
        name: String,
    },
    Reshape,
    Transpose,
    Matmul,
    BatchedMatmul,
    Elementwise {
        name: String,
    },
    FusedElementwise {
        ops: Vec<String>,
    },
    Reduction {
        name: String,
    },
    FusedReduction {
        elementwise_ops: Vec<String>,
        reduction: String,
    },
    DeviceTransfer {
        target: TensorDevice,
    },
    Linear,
    Conv2d,
    Dropout,
    MaxPool2d,
    Loss {
        name: String,
    },
    UnknownHost {
        host: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorMetadata {
    pub dtype: TensorDType,
    pub shape: TensorShape,
    pub layout: TensorLayout,
    pub device: TensorDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorDType {
    Int,
    Float,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorShape {
    Ranked(Vec<Option<usize>>),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorLayout {
    Contiguous,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorDevice {
    Cpu,
    Wgpu,
    Reserved(i64),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphError {
    pub function: String,
    pub node: Option<usize>,
    pub kind: TensorGraphErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorGraphErrorKind {
    Cycle,
    ShapeMismatch,
    DtypeMismatch,
    DeviceMismatch,
    InvalidLayout,
    FallbackNotAllowed,
    UnsupportedOperator,
    InvalidDependency,
}

impl TensorGraphErrorKind {
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::UnsupportedOperator => "E2907",
            Self::ShapeMismatch => "E2908",
            Self::DtypeMismatch => "E2909",
            Self::DeviceMismatch => "E2910",
            Self::InvalidLayout => "E2911",
            Self::FallbackNotAllowed => "E2912",
            Self::InvalidDependency | Self::Cycle => "E2907",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphOptimizationResult {
    pub graph: TensorGraph,
    pub report: TensorGraphOptimizationReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphOptimizationReport {
    pub original_nodes: usize,
    pub optimized_nodes: usize,
    pub fused_groups: usize,
    pub fused_elementwise_ops: usize,
    pub fused_reductions: usize,
    pub reusable_edges: usize,
    pub tolerance_abs: String,
    pub tolerance_rel: String,
}

/// Backend-facing legalization evidence for the first-class tensor graph.
/// Runtime handles remain the ABI boundary, while this report records the
/// compiler decisions made before dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphLoweringResult {
    pub graph: TensorGraph,
    pub report: TensorGraphLoweringReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphLoweringReport {
    pub backend: TensorDevice,
    pub ir_nodes: usize,
    pub legalized_nodes: usize,
    pub external_fallback_nodes: usize,
    pub fusion_groups: usize,
    pub planned_buffers: usize,
    pub peak_live_buffers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorGraphComparison {
    pub equivalent: bool,
    pub checked_outputs: usize,
    pub tolerance_abs: String,
    pub tolerance_rel: String,
    pub diagnostics: Vec<String>,
}

impl TensorGraph {
    pub fn from_ir_module(module: &Module) -> Self {
        let functions = module
            .functions
            .iter()
            .map(TensorGraphFunction::from_ir_function)
            .collect();
        Self {
            module: module.name.clone(),
            functions,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<TensorGraphError>> {
        let mut errors = Vec::new();
        for function in &self.functions {
            function.validate_into(&mut errors, true);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate, optimize, and legalize the graph for a concrete execution
    /// backend. Unknown tensor-returning host calls are retained as explicit
    /// compatibility fallbacks; they are counted in the report instead of
    /// being silently mistaken for compiler-native tensor operations.
    pub fn lower_for_backend(
        &self,
        backend: TensorDevice,
    ) -> Result<TensorGraphLoweringResult, Vec<TensorGraphError>> {
        let mut errors = Vec::new();
        for function in &self.functions {
            function.validate_into(&mut errors, false);
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut optimization = TensorGraphOptimizationReport {
            original_nodes: self.functions.iter().map(|f| f.nodes.len()).sum(),
            optimized_nodes: 0,
            fused_groups: 0,
            fused_elementwise_ops: 0,
            fused_reductions: 0,
            reusable_edges: 0,
            tolerance_abs: "1e-9".to_string(),
            tolerance_rel: "1e-9".to_string(),
        };
        let functions = self
            .functions
            .iter()
            .map(|function| function.optimize_into(&mut optimization))
            .collect::<Vec<_>>();
        let optimized = TensorGraph {
            module: self.module.clone(),
            functions,
        };
        optimization.optimized_nodes = optimized.functions.iter().map(|f| f.nodes.len()).sum();

        let external_fallback_nodes = optimized
            .functions
            .iter()
            .flat_map(|function| function.nodes.iter())
            .filter(|node| matches!(node.op, TensorGraphOp::UnknownHost { .. }))
            .count();
        let planned_buffers = optimized
            .functions
            .iter()
            .map(TensorGraphFunction::planned_buffers)
            .sum();
        let peak_live_buffers = optimized
            .functions
            .iter()
            .map(TensorGraphFunction::peak_live_buffers)
            .max()
            .unwrap_or(0);

        Ok(TensorGraphLoweringResult {
            graph: optimized,
            report: TensorGraphLoweringReport {
                backend,
                ir_nodes: optimization.original_nodes,
                legalized_nodes: optimization.optimized_nodes,
                external_fallback_nodes,
                fusion_groups: optimization.fused_groups,
                planned_buffers,
                peak_live_buffers,
            },
        })
    }

    pub fn stable_dump(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "tensor_graph module {}", self.module);
        for function in &self.functions {
            let _ = writeln!(out, "fn {} {{", function.name);
            for node in &function.nodes {
                let value = node
                    .value
                    .map(|id| format!("%{id}"))
                    .unwrap_or_else(|| "_".to_string());
                let inputs = if node.inputs.is_empty() {
                    "-".to_string()
                } else {
                    node.inputs
                        .iter()
                        .map(|id| format!("n{id}"))
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let _ = writeln!(
                    out,
                    "  n{} {} = {}({}) -> {} @b{}:i{}",
                    node.id,
                    value,
                    node.op.stable_name(),
                    inputs,
                    node.output.stable_name(),
                    node.source.block,
                    node.source.instruction
                );
            }
            let _ = writeln!(out, "}}");
        }
        out
    }

    pub fn optimize(&self) -> Result<TensorGraphOptimizationResult, Vec<TensorGraphError>> {
        self.validate()?;
        let mut report = TensorGraphOptimizationReport {
            original_nodes: self
                .functions
                .iter()
                .map(|function| function.nodes.len())
                .sum(),
            optimized_nodes: 0,
            fused_groups: 0,
            fused_elementwise_ops: 0,
            fused_reductions: 0,
            reusable_edges: 0,
            tolerance_abs: "1e-9".to_string(),
            tolerance_rel: "1e-9".to_string(),
        };
        let functions = self
            .functions
            .iter()
            .map(|function| function.optimize_into(&mut report))
            .collect::<Vec<_>>();
        report.optimized_nodes = functions.iter().map(|function| function.nodes.len()).sum();
        Ok(TensorGraphOptimizationResult {
            graph: TensorGraph {
                module: self.module.clone(),
                functions,
            },
            report,
        })
    }

    pub fn compare_optimized(&self, optimized: &TensorGraph) -> TensorGraphComparison {
        let mut diagnostics = Vec::new();
        let mut checked_outputs = 0;
        if self.module != optimized.module {
            diagnostics.push(format!(
                "module mismatch: '{}' != '{}'",
                self.module, optimized.module
            ));
        }
        for original_function in &self.functions {
            let Some(optimized_function) = optimized
                .functions
                .iter()
                .find(|function| function.name == original_function.name)
            else {
                diagnostics.push(format!(
                    "optimized graph is missing function '{}'",
                    original_function.name
                ));
                continue;
            };
            let original_outputs = original_function.observable_outputs();
            let optimized_outputs = optimized_function.observable_outputs();
            checked_outputs += original_outputs.len();
            if original_outputs != optimized_outputs {
                diagnostics.push(format!(
                    "function '{}' observable outputs differ: {:?} != {:?}",
                    original_function.name, original_outputs, optimized_outputs
                ));
            }
        }
        TensorGraphComparison {
            equivalent: diagnostics.is_empty(),
            checked_outputs,
            tolerance_abs: "1e-9".to_string(),
            tolerance_rel: "1e-9".to_string(),
            diagnostics,
        }
    }
}

