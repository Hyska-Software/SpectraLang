// Optimization passes for IR

pub mod constant_folding;
pub mod dead_code_elimination;
pub mod function_inlining;
pub mod tail_call_marking;
pub mod validation;
pub mod verification;

use crate::ir::Module;

/// Trait for optimization passes
pub trait Pass {
    fn name(&self) -> &str;
    fn run(&mut self, module: &mut Module) -> bool; // Returns true if modified
}

