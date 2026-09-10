use super::*;

use crate::ir::InstructionKind;
use std::collections::{BTreeMap, HashMap, HashSet};
impl TensorGraphFunction {
    pub(crate) fn from_ir_function(function: &crate::ir::Function) -> Self {
        let mut extractor = TensorGraphExtractor::default();
        for block in &function.blocks {
            for instruction in &block.instructions {
                match &instruction.kind {
                    InstructionKind::ConstInt { result, value } => {
                        extractor.constants.insert(result.id, *value);
                    }
                    InstructionKind::Add { result, lhs, rhs } => {
                        extractor.fold_const_int(*result, *lhs, *rhs, i64::saturating_add);
                    }
                    InstructionKind::Sub { result, lhs, rhs } => {
                        extractor.fold_const_int(*result, *lhs, *rhs, i64::saturating_sub);
                    }
                    InstructionKind::Mul { result, lhs, rhs } => {
                        extractor.fold_const_int(*result, *lhs, *rhs, i64::saturating_mul);
                    }
                    InstructionKind::Div { result, lhs, rhs } => {
                        extractor.fold_const_int_checked(*result, *lhs, *rhs, |left, right| {
                            (right != 0).then(|| left / right)
                        });
                    }
                    InstructionKind::Rem { result, lhs, rhs } => {
                        extractor.fold_const_int_checked(*result, *lhs, *rhs, |left, right| {
                            (right != 0).then(|| left % right)
                        });
                    }
                    InstructionKind::HostCall {
                        result, host, args, ..
                    } => {
                        extractor.lower_host_call(block.id, instruction.id, *result, host, args);
                    }
                    _ => {}
                }
            }
        }
        Self {
            name: function.name.clone(),
            nodes: extractor.nodes,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<TensorGraphError>> {
        let mut errors = Vec::new();
        self.validate_into(&mut errors, true);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub(crate) fn planned_buffers(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.value.is_some())
            .count()
    }

    pub(crate) fn peak_live_buffers(&self) -> usize {
        let mut live = 0usize;
        let mut peak = 0usize;
        for node in &self.nodes {
            if node.value.is_some() {
                live += 1;
                peak = peak.max(live);
            }
            if node.inputs.len() > 1 {
                live = live.saturating_sub(node.inputs.len() - 1);
            }
        }
        peak
    }

    pub(crate) fn optimize_into(&self, report: &mut TensorGraphOptimizationReport) -> Self {
        let consumer_counts = self.consumer_counts();
        let mut old_to_new = HashMap::new();
        let mut skipped = HashSet::new();
        let mut nodes = Vec::new();

        for node in &self.nodes {
            if skipped.contains(&node.id) {
                continue;
            }
            if matches!(node.op, TensorGraphOp::Elementwise { .. })
                && self.elementwise_chain_feeds_reduction(node.id, &consumer_counts)
            {
                continue;
            }
            if let Some(fused) =
                self.try_fuse_reduction(node, &consumer_counts, &mut skipped, report)
            {
                let new_id = nodes.len();
                for old_id in fused.old_node_ids {
                    old_to_new.insert(old_id, new_id);
                }
                nodes.push(fused.node.with_id(new_id));
                continue;
            }
            if let Some(fused) =
                self.try_fuse_elementwise_chain(node, &consumer_counts, &mut skipped, report)
            {
                let new_id = nodes.len();
                for old_id in fused.old_node_ids {
                    old_to_new.insert(old_id, new_id);
                }
                nodes.push(fused.node.with_id(new_id));
                continue;
            }
            let new_id = nodes.len();
            old_to_new.insert(node.id, new_id);
            nodes.push(node.clone().with_id(new_id));
        }

        for node in &mut nodes {
            node.inputs = node
                .inputs
                .iter()
                .filter_map(|input| old_to_new.get(input).copied())
                .collect();
            node.inputs.sort_unstable();
            node.inputs.dedup();
        }

        report.reusable_edges += nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.op,
                    TensorGraphOp::FusedElementwise { .. } | TensorGraphOp::FusedReduction { .. }
                )
            })
            .map(|node| node.inputs.len())
            .sum::<usize>();

        Self {
            name: self.name.clone(),
            nodes,
        }
    }

    pub(crate) fn consumer_counts(&self) -> HashMap<usize, usize> {
        let mut counts = HashMap::new();
        for node in &self.nodes {
            for input in &node.inputs {
                *counts.entry(*input).or_insert(0) += 1;
            }
        }
        counts
    }

    pub(crate) fn try_fuse_reduction(
        &self,
        node: &TensorGraphNode,
        consumer_counts: &HashMap<usize, usize>,
        skipped: &mut HashSet<usize>,
        report: &mut TensorGraphOptimizationReport,
    ) -> Option<FusedTensorNode> {
        let TensorGraphOp::Reduction { name } = &node.op else {
            return None;
        };
        let chain = self.collect_elementwise_chain(*node.inputs.first()?, consumer_counts);
        if chain.len() < 2 {
            return None;
        }
        let first = chain.first()?;
        let base_inputs = first.inputs.clone();
        let elementwise_ops = chain
            .iter()
            .map(|chain_node| match &chain_node.op {
                TensorGraphOp::Elementwise { name } => name.clone(),
                _ => unreachable!("chain only contains elementwise nodes"),
            })
            .collect::<Vec<_>>();
        let old_node_ids = chain
            .iter()
            .map(|chain_node| chain_node.id)
            .chain(std::iter::once(node.id))
            .collect::<Vec<_>>();
        for old_id in &old_node_ids {
            skipped.insert(*old_id);
        }
        report.fused_groups += 1;
        report.fused_elementwise_ops += elementwise_ops.len();
        report.fused_reductions += 1;
        Some(FusedTensorNode {
            old_node_ids,
            node: TensorGraphNode {
                id: node.id,
                value: node.value,
                op: TensorGraphOp::FusedReduction {
                    elementwise_ops,
                    reduction: name.clone(),
                },
                inputs: base_inputs,
                output: node.output.clone(),
                source: node.source.clone(),
            },
        })
    }

    pub(crate) fn try_fuse_elementwise_chain(
        &self,
        node: &TensorGraphNode,
        consumer_counts: &HashMap<usize, usize>,
        skipped: &mut HashSet<usize>,
        report: &mut TensorGraphOptimizationReport,
    ) -> Option<FusedTensorNode> {
        if !matches!(node.op, TensorGraphOp::Elementwise { .. }) {
            return None;
        }
        let chain = self.collect_forward_elementwise_chain(node.id, consumer_counts);
        if chain.len() < 2 {
            return None;
        }
        if chain
            .last()
            .and_then(|last| self.single_consumer(last.id, consumer_counts))
            .is_some_and(|consumer| matches!(consumer.op, TensorGraphOp::Reduction { .. }))
        {
            return None;
        }
        let first = chain.first()?;
        let last = chain.last()?;
        let ops = chain
            .iter()
            .map(|chain_node| match &chain_node.op {
                TensorGraphOp::Elementwise { name } => name.clone(),
                _ => unreachable!("chain only contains elementwise nodes"),
            })
            .collect::<Vec<_>>();
        let old_node_ids = chain
            .iter()
            .map(|chain_node| chain_node.id)
            .collect::<Vec<_>>();
        for old_id in &old_node_ids {
            skipped.insert(*old_id);
        }
        report.fused_groups += 1;
        report.fused_elementwise_ops += ops.len();
        Some(FusedTensorNode {
            old_node_ids,
            node: TensorGraphNode {
                id: first.id,
                value: last.value,
                op: TensorGraphOp::FusedElementwise { ops },
                inputs: first.inputs.clone(),
                output: last.output.clone(),
                source: last.source.clone(),
            },
        })
    }

    pub(crate) fn collect_forward_elementwise_chain(
        &self,
        start_id: usize,
        consumer_counts: &HashMap<usize, usize>,
    ) -> Vec<&TensorGraphNode> {
        let by_id = self
            .nodes
            .iter()
            .map(|node| (node.id, node))
            .collect::<HashMap<_, _>>();
        let mut chain = Vec::new();
        let mut current_id = start_id;
        while let Some(current) = by_id.get(&current_id).copied() {
            if !matches!(current.op, TensorGraphOp::Elementwise { .. }) {
                break;
            }
            chain.push(current);
            if consumer_counts.get(&current_id).copied().unwrap_or(0) != 1 {
                break;
            }
            let Some(next) = self.nodes.iter().find(|candidate| {
                candidate.inputs.len() == 1
                    && candidate.inputs[0] == current_id
                    && matches!(candidate.op, TensorGraphOp::Elementwise { .. })
            }) else {
                break;
            };
            current_id = next.id;
        }
        chain
    }

    pub(crate) fn elementwise_chain_feeds_reduction(
        &self,
        start_id: usize,
        consumer_counts: &HashMap<usize, usize>,
    ) -> bool {
        let mut current_id = start_id;
        loop {
            let Some(current) = self.nodes.iter().find(|node| node.id == current_id) else {
                return false;
            };
            if !matches!(current.op, TensorGraphOp::Elementwise { .. }) {
                return false;
            }
            let Some(consumer) = self.single_consumer(current_id, consumer_counts) else {
                return false;
            };
            if matches!(consumer.op, TensorGraphOp::Reduction { .. }) {
                return true;
            }
            if !matches!(consumer.op, TensorGraphOp::Elementwise { .. }) {
                return false;
            }
            current_id = consumer.id;
        }
    }

    pub(crate) fn single_consumer(
        &self,
        node_id: usize,
        consumer_counts: &HashMap<usize, usize>,
    ) -> Option<&TensorGraphNode> {
        if consumer_counts.get(&node_id).copied().unwrap_or(0) != 1 {
            return None;
        }
        self.nodes.iter().find(|candidate| {
            candidate.inputs.len() == 1 && candidate.inputs.first().copied() == Some(node_id)
        })
    }

    pub(crate) fn collect_elementwise_chain(
        &self,
        start_id: usize,
        consumer_counts: &HashMap<usize, usize>,
    ) -> Vec<&TensorGraphNode> {
        let by_id = self
            .nodes
            .iter()
            .map(|node| (node.id, node))
            .collect::<HashMap<_, _>>();
        let mut reversed = Vec::new();
        let mut current_id = start_id;
        while let Some(current) = by_id.get(&current_id).copied() {
            if !matches!(current.op, TensorGraphOp::Elementwise { .. }) {
                break;
            }
            if consumer_counts.get(&current_id).copied().unwrap_or(0) != 1 {
                break;
            }
            reversed.push(current);
            let Some(previous) = current.inputs.first().and_then(|id| by_id.get(id)).copied()
            else {
                break;
            };
            current_id = previous.id;
        }
        reversed.reverse();
        reversed
    }

    pub(crate) fn observable_outputs(&self) -> BTreeMap<usize, TensorMetadata> {
        let consumed = self
            .nodes
            .iter()
            .flat_map(|node| node.inputs.iter().copied())
            .collect::<HashSet<_>>();
        self.nodes
            .iter()
            .filter(|node| !consumed.contains(&node.id))
            .filter_map(|node| node.value.map(|value| (value, node.output.clone())))
            .collect()
    }

    pub(crate) fn validate_into(&self, errors: &mut Vec<TensorGraphError>, reject_external: bool) {
        let ids = self
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<HashSet<_>>();
        let by_id = self
            .nodes
            .iter()
            .map(|node| (node.id, node))
            .collect::<BTreeMap<_, _>>();

        for node in &self.nodes {
            for input in &node.inputs {
                if !ids.contains(input) {
                    errors.push(TensorGraphError::new(
                        &self.name,
                        Some(node.id),
                        TensorGraphErrorKind::InvalidDependency,
                        format!("node n{} references missing dependency n{}", node.id, input),
                    ));
                }
            }
            self.validate_node_contract(node, &by_id, errors, reject_external);
        }

        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for node in &self.nodes {
            if has_cycle(node.id, &by_id, &mut visiting, &mut visited) {
                errors.push(TensorGraphError::new(
                    &self.name,
                    Some(node.id),
                    TensorGraphErrorKind::Cycle,
                    format!("cycle detected through node n{}", node.id),
                ));
                break;
            }
        }
    }

    pub(crate) fn validate_node_contract(
        &self,
        node: &TensorGraphNode,
        by_id: &BTreeMap<usize, &TensorGraphNode>,
        errors: &mut Vec<TensorGraphError>,
        reject_external: bool,
    ) {
        match &node.op {
            TensorGraphOp::Matmul => {
                let Some([left, right]) = input_pair(node, by_id) else {
                    return;
                };
                if let (Some(l), Some(r)) = (left.output.shape.dim(1), right.output.shape.dim(0)) {
                    if l != r {
                        errors.push(TensorGraphError::new(
                            &self.name,
                            Some(node.id),
                            TensorGraphErrorKind::ShapeMismatch,
                            format!("matmul expects lhs dim1 == rhs dim0, got {l} and {r}"),
                        ));
                    }
                }
                validate_same_device(&self.name, node, left, right, errors);
            }
            TensorGraphOp::Elementwise { name } | TensorGraphOp::Loss { name } => {
                if node.inputs.len() >= 2 {
                    let Some([left, right]) = input_pair(node, by_id) else {
                        return;
                    };
                    if !left.output.shape.compatible_with(&right.output.shape) {
                        errors.push(TensorGraphError::new(
                            &self.name,
                            Some(node.id),
                            TensorGraphErrorKind::ShapeMismatch,
                            format!(
                                "{name} expects compatible input shapes, got {} and {}",
                                left.output.shape.stable_name(),
                                right.output.shape.stable_name()
                            ),
                        ));
                    }
                    validate_same_device(&self.name, node, left, right, errors);
                    validate_same_dtype(&self.name, node, left, right, errors);
                }
            }
            TensorGraphOp::Conv2d => {
                if let (Some(input), Some(kernel)) = (
                    node.inputs.first().and_then(|id| by_id.get(id)).copied(),
                    node.inputs.get(1).and_then(|id| by_id.get(id)).copied(),
                ) {
                    validate_same_device(&self.name, node, input, kernel, errors);
                    validate_same_dtype(&self.name, node, input, kernel, errors);
                }
            }
            TensorGraphOp::UnknownHost { host } if reject_external => {
                errors.push(TensorGraphError::new(
                    &self.name,
                    Some(node.id),
                    TensorGraphErrorKind::FallbackNotAllowed,
                    format!("external tensor fallback is not allowed for '{host}'"),
                ));
            }
            _ => {}
        }
    }
}
