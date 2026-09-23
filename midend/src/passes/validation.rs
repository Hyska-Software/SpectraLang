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

    /// Tarjan's strongly connected components, implemented iteratively.
    ///
    /// The recursive formulation ties the compiler stack to the program's CFG
    /// depth; an explicit work stack keeps arbitrarily deep CFGs from
    /// overflowing the runtime stack. The work items hold `(node,
    /// next_successor_index)` cursors, mirroring the recursive call frames.
    fn strongly_connected_components(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
        let node_count = successors.len();
        let mut indices: Vec<Option<usize>> = vec![None; node_count];
        let mut lowlink = vec![0usize; node_count];
        let mut stack: Vec<usize> = Vec::new();
        let mut on_stack = vec![false; node_count];
        let mut result: Vec<Vec<usize>> = Vec::new();
        let mut next_index = 0usize;

        for root in 0..node_count {
            if indices[root].is_some() {
                continue;
            }
            indices[root] = Some(next_index);
            lowlink[root] = next_index;
            next_index += 1;
            stack.push(root);
            on_stack[root] = true;
            let mut work: Vec<(usize, usize)> = vec![(root, 0)];

            while let Some((node, successor_index)) = work.last().copied() {
                if successor_index < successors[node].len() {
                    // Advance the cursor, then descend into or cross-link the
                    // next successor.
                    if let Some(frame) = work.last_mut() {
                        frame.1 += 1;
                    }
                    let successor = successors[node][successor_index];
                    if indices[successor].is_none() {
                        indices[successor] = Some(next_index);
                        lowlink[successor] = next_index;
                        next_index += 1;
                        stack.push(successor);
                        on_stack[successor] = true;
                        work.push((successor, 0));
                    } else if on_stack[successor] {
                        lowlink[node] = lowlink[node].min(indices[successor].unwrap());
                    }
                } else {
                    // All successors visited: pop the frame and propagate the
                    // lowlink to the parent, exactly like the return path of
                    // the recursive variant.
                    work.pop();
                    if let Some((parent, _)) = work.last().copied() {
                        lowlink[parent] = lowlink[parent].min(lowlink[node]);
                    }
                    if lowlink[node] == indices[node].unwrap() {
                        let mut component = Vec::new();
                        while let Some(top) = stack.pop() {
                            on_stack[top] = false;
                            component.push(top);
                            if top == node {
                                break;
                            }
                        }
                        result.push(component);
                    }
                }
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

    /// Heuristic header choice for a loop component: prefer the block whose
    /// label carries `.header`, falling back to the lowest block id.
    ///
    /// NOTE: this heuristic, and the `.exit`/`loop` label scan in `run`, are
    /// coupled to the block-naming conventions emitted by lowering (e.g.
    /// `while.header`, `do_while.header`, `loop.body`, `*.exit` in
    /// `midend/src/lowering_impl_statements.rs`). The IR does not yet tag
    /// loop blocks structurally, so if lowering renames its blocks these
    /// checks must be updated together with it. Kept as-is deliberately.
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
                }
            }

            // Unreachable-exit heuristic, coupled to lowering's block naming
            // (see the note on `select_header`): only blocks labelled
            // `*.exit` that are not loop exits are checked.
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
                }
            }
        }

        // Errors are reported through `self.errors`; the CLI re-emits them as
        // compiler diagnostics (`compiler_integration_core.rs` converts every
        // `LoopValidationError` into a `MidendError` and fails the build), so
        // this pass must not print to stderr itself.

        false
    }
}
