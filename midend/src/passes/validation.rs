// IR validation passes to ensure control-flow correctness prior to lowering into backend.
// These checks catch structural issues early to keep tests consistent with language semantics.

use super::Pass;
use crate::ir::{BasicBlock, Function, Module, Terminator};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Ensures that loops have properly linked exit blocks so `break` and `continue` work as expected.
pub struct LoopStructureValidation {
    errors: Vec<LoopValidationError>,
}

type BlockId = usize;

#[derive(Debug, Clone)]
pub struct LoopValidationError {
    pub function: String,
    pub header_block: BlockId,
    pub header_label: String,
    pub message: String,
}

impl fmt::Display for LoopValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Loop validation failure in function '{}' (block {} '{}'): {}",
            self.function, self.header_block, self.header_label, self.message
        )
    }
}

impl Default for LoopStructureValidation {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopStructureValidation {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn errors(&self) -> &[LoopValidationError] {
        &self.errors
    }

    pub fn take_errors(&mut self) -> Vec<LoopValidationError> {
        std::mem::take(&mut self.errors)
    }

    fn block_successors(block: &BasicBlock) -> Vec<BlockId> {
        match &block.terminator {
            Some(Terminator::Branch { target }) => vec![*target],
            Some(Terminator::CondBranch {
                true_block,
                false_block,
                ..
            }) => vec![*true_block, *false_block],
            Some(Terminator::Switch { cases, default, .. }) => {
                let mut targets: Vec<BlockId> = cases.iter().map(|(_, t)| *t).collect();
                targets.push(*default);
                targets.sort_unstable();
                targets.dedup();
                targets
            }
            _ => Vec::new(),
        }
    }

    fn build_successors(
        function: &Function,
        id_to_index: &HashMap<BlockId, usize>,
    ) -> Vec<Vec<usize>> {
        function
            .blocks
            .iter()
            .map(|block| {
                Self::block_successors(block)
                    .into_iter()
                    .filter_map(|target| id_to_index.get(&target).copied())
                    .collect()
            })
            .collect()
    }

    fn compute_predecessors(successors: &[Vec<usize>]) -> Vec<usize> {
        let mut counts = vec![0; successors.len()];
        for edges in successors {
            for &succ in edges {
                if let Some(entry) = counts.get_mut(succ) {
                    *entry += 1;
                }
            }
        }
        counts
    }

    fn strongly_connected_components(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
        #[allow(clippy::too_many_arguments)]
        fn strongconnect(
            v: usize,
            index: &mut usize,
            successors: &[Vec<usize>],
            indices: &mut [Option<usize>],
            lowlink: &mut [usize],
            stack: &mut Vec<usize>,
            on_stack: &mut [bool],
            result: &mut Vec<Vec<usize>>,
        ) {
            indices[v] = Some(*index);
            lowlink[v] = *index;
            *index += 1;
            stack.push(v);
            on_stack[v] = true;

            for &w in &successors[v] {
                if indices[w].is_none() {
                    strongconnect(
                        w, index, successors, indices, lowlink, stack, on_stack, result,
                    );
                    lowlink[v] = lowlink[v].min(lowlink[w]);
                } else if on_stack[w] {
                    lowlink[v] = lowlink[v].min(indices[w].unwrap());
                }
            }

            if lowlink[v] == indices[v].unwrap() {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                result.push(component);
            }
        }

        let mut index = 0;
        let mut indices = vec![None; successors.len()];
        let mut lowlink = vec![0; successors.len()];
        let mut stack = Vec::new();
        let mut on_stack = vec![false; successors.len()];
        let mut result = Vec::new();

        for v in 0..successors.len() {
            if indices[v].is_none() {
                strongconnect(
                    v,
                    &mut index,
                    successors,
                    &mut indices,
                    &mut lowlink,
                    &mut stack,
                    &mut on_stack,
                    &mut result,
                );
            }
        }

        result
    }

    fn is_loop_component(component: &[usize], successors: &[Vec<usize>]) -> bool {
        if component.len() > 1 {
            return true;
        }
        if let Some(&idx) = component.first() {
            return successors[idx].contains(&idx);
        }
        false
    }

    fn component_has_exit(component: &[usize], successors: &[Vec<usize>]) -> bool {
        let members: HashSet<usize> = component.iter().copied().collect();
        for &idx in component {
            for &succ in &successors[idx] {
                if !members.contains(&succ) {
                    return true;
                }
            }
        }
        false
    }

    fn component_has_return(component: &[usize], blocks: &[BasicBlock]) -> bool {
        component
            .iter()
            .any(|&idx| matches!(blocks[idx].terminator, Some(Terminator::Return { .. })))
    }

    fn select_header(component: &[usize], blocks: &[BasicBlock]) -> usize {
        component
            .iter()
            .copied()
            .min_by_key(|&idx| {
                let block = &blocks[idx];
                let priority = if block.label.contains(".header") {
                    0
                } else {
                    1
                };
                (priority, block.id)
            })
            .unwrap_or(component[0])
    }
}

impl Pass for LoopStructureValidation {
    fn name(&self) -> &str {
        "LoopStructureValidation"
    }

    fn run(&mut self, module: &mut Module) -> bool {
        self.errors.clear();
        let mut found_issue = false;

        for function in &module.functions {
            if function.blocks.is_empty() {
                continue;
            }

            let id_to_index: HashMap<BlockId, usize> = function
                .blocks
                .iter()
                .enumerate()
                .map(|(idx, block)| (block.id, idx))
                .collect();

            let index_to_id: Vec<BlockId> = function.blocks.iter().map(|block| block.id).collect();
            let successors = Self::build_successors(function, &id_to_index);
            let predecessors = Self::compute_predecessors(&successors);
            let components = Self::strongly_connected_components(&successors);

            for component in components {
                if !Self::is_loop_component(&component, &successors) {
                    continue;
                }

                let header_idx = Self::select_header(&component, &function.blocks);
                let header_block_id = index_to_id[header_idx];
                let header_label = function
                    .get_block(header_block_id)
                    .map(|b| b.label.clone())
                    .unwrap_or_else(|| "<unknown>".to_string());

                let requires_exit = !header_label.starts_with("loop");
                let has_exit_edge = Self::component_has_exit(&component, &successors);
                let has_return_exit = Self::component_has_return(&component, &function.blocks);

                if requires_exit && !has_exit_edge && !has_return_exit {
                    self.errors.push(LoopValidationError {
                        function: function.name.clone(),
                        header_block: header_block_id,
                        header_label: header_label.clone(),
                        message: "loop does not have any edge that leaves the loop; ensure the condition leads to an exit block or introduce a `break`".into(),
                    });
                    found_issue = true;
                }
            }

            for (idx, block) in function.blocks.iter().enumerate() {
                if !block.label.contains(".exit") {
                    continue;
                }
                if block.label.starts_with("loop") {
                    continue;
                }
                if predecessors.get(idx).copied().unwrap_or(0) == 0 {
                    self.errors.push(LoopValidationError {
                        function: function.name.clone(),
                        header_block: block.id,
                        header_label: block.label.clone(),
                        message: "exit block is unreachable; no branch targets this block".into(),
                    });
                    found_issue = true;
                }
            }
        }

        if found_issue {
            for err in &self.errors {
                eprintln!("{}", err);
            }
        }

        false
    }
}
