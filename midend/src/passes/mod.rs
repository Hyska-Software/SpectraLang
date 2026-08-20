// Optimization passes for IR

pub mod concurrent_spawn_join_fusion;
pub mod constant_folding;
pub mod dead_code_elimination;
pub mod function_inlining;
pub mod strength_reduction;
pub mod validation;
pub mod verification;

use crate::ir::Module;

/// Trait for optimization passes
pub trait Pass {
    fn name(&self) -> &str;
    fn run(&mut self, module: &mut Module) -> bool; // Returns true if modified
}

/// Pass manager to orchestrate optimization passes
pub struct PassManager {
    passes: Vec<Box<dyn Pass>>,
}

impl PassManager {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(&mut self, pass: Box<dyn Pass>) {
        self.passes.push(pass);
    }

    pub fn run(&mut self, module: &mut Module) {
        for pass in &mut self.passes {
            let modified = pass.run(module);
            let _modified = modified;
        }
    }
}

impl Default for PassManager {
    fn default() -> Self {
        Self::new()
    }
}
