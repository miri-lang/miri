// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Removal of the block parameters a function's translation leaves that
//! every branch hands one value.
//!
//! The translator seals its blocks only once every block is translated, so
//! when Cranelift's frontend completes a loop header it can meet a variable
//! the loop never writes in a predecessor not yet sealed. The frontend then
//! gives the header a parameter for the variable, which every branch from
//! inside the loop hands back unchanged, and it never removes the parameter
//! once the predecessor is sealed and the value turns out to be the one the
//! loop is entered with. A loop nested in another so receives the outer
//! loop's counter as a fresh value on every inner iteration, and nothing
//! computed from it is invariant to the inner loop. Cranelift's own removal
//! of constant parameters keeps such a parameter too, since the value it
//! stands for is another block's parameter.
//!
//! A parameter of a reachable block whose incoming values, other than
//! itself along a back edge, are all one value is replaced by that value.
//! Every path into the block hands it the value, so the value dominates the
//! block and the replacement keeps the function's meaning.

use cranelift_codegen::dominator_tree::DominatorTree;
use cranelift_codegen::flowgraph::ControlFlowGraph;
use cranelift_codegen::ir::{Block, Function, InstructionData, Value};
use std::collections::BTreeSet;

/// The most rounds the removal may take. A round sees through the
/// parameters the rounds before it removed, so a chain of parameters each
/// handed only the one before needs a round per link it is not visited in
/// order; stopping early leaves parameters that are merely redundant.
const MAX_ROUNDS: usize = 32;

/// Replace every parameter of a reachable block of `function` that every
/// branch into the block, the block's own back edges aside, hands one
/// value, by that value.
pub(crate) fn forward_pass_through_parameters(function: &mut Function) {
    let entry = function.layout.entry_block();
    let has_parameters = (function.layout.blocks())
        .any(|block| Some(block) != entry && function.dfg.num_block_params(block) != 0);
    if !has_parameters {
        return;
    }
    let cfg = ControlFlowGraph::with_function(function);
    let domtree = DominatorTree::with_function(function, &cfg);
    let order: Vec<Block> = domtree.cfg_rpo().copied().collect();
    for _ in 0..MAX_ROUNDS {
        if !forward_where(function, &cfg, &order, |_, _| true) {
            return;
        }
        function.dfg.resolve_all_aliases();
    }
}

/// Replace each parameter of the blocks of `order`, a reverse postorder of
/// the reachable blocks, entry first, whose incoming values, other than
/// itself along a back edge, are all one value `accept` admits, by that
/// value, returning whether any was. Every branch to the block hands it the
/// value, so the value dominates the block and the replacement keeps the
/// function's meaning. The replaced parameters become aliases, resolved
/// within the round as each next parameter's incoming values are read.
pub(super) fn forward_where(
    function: &mut Function,
    cfg: &ControlFlowGraph,
    order: &[Block],
    accept: impl Fn(&Function, Value) -> bool,
) -> bool {
    let mut forwarded = false;
    for &block in order.iter().skip(1) {
        for index in (0..function.dfg.num_block_params(block)).rev() {
            let parameter = function.dfg.block_params(block)[index];
            let Some(source) = sole_incoming(function, cfg, block, index, parameter) else {
                continue;
            };
            if !accept(function, source) {
                continue;
            }
            remove_argument(function, cfg, block, index);
            function.dfg.remove_block_param(parameter);
            function.dfg.change_to_alias(parameter, source);
            forwarded = true;
        }
    }
    forwarded
}

/// The one value other than `parameter` itself that every branch to
/// `block` hands its parameter at `index`, if there is one.
fn sole_incoming(
    function: &Function,
    cfg: &ControlFlowGraph,
    block: Block,
    index: usize,
    parameter: Value,
) -> Option<Value> {
    let dfg = &function.dfg;
    let mut sole = None;
    for predecessor in cfg.pred_iter(block) {
        let calls =
            dfg.insts[predecessor.inst].branch_destination(&dfg.jump_tables, &dfg.exception_tables);
        for call in calls
            .iter()
            .filter(|call| call.block(&dfg.value_lists) == block)
        {
            let handed = call.args(&dfg.value_lists).nth(index)?.as_value()?;
            let handed = dfg.resolve_aliases(handed);
            if handed == parameter {
                continue;
            }
            if sole.is_some_and(|sole| sole != handed) {
                return None;
            }
            sole = Some(handed);
        }
    }
    sole
}

/// Remove the argument at `index` from every branch to `block`, in place,
/// each jump table shared by several branches once. No two block calls
/// share an argument list: the frontend and Cranelift's inliner build each
/// afresh, and a jump table copied by the rewrite copies its calls.
fn remove_argument(function: &mut Function, cfg: &ControlFlowGraph, block: Block, index: usize) {
    let mut tables = BTreeSet::new();
    for predecessor in cfg.pred_iter(block) {
        let dfg = &mut function.dfg;
        if let InstructionData::BranchTable { table, .. } = dfg.insts[predecessor.inst] {
            if !tables.insert(table) {
                continue;
            }
        }
        let calls = dfg.insts[predecessor.inst]
            .branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables);
        for call in calls {
            if call.block(&dfg.value_lists) != block {
                continue;
            }
            call.remove(index, &mut dfg.value_lists);
        }
    }
}

#[cfg(test)]
mod tests;
