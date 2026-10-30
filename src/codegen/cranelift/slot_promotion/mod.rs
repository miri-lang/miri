// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Promotion of explicit stack slots to SSA values.
//!
//! A value held as its bytes lives in an explicit stack slot, and once its
//! callees are inlined a function reaches most such slots only through plain
//! loads and stores at offsets known at compile time. Cranelift forwards a
//! store to a later load only for the last store it saw, so every field of
//! such a value is still written to the stack and read back. This pass holds
//! each run of a slot's bytes that the function reads or writes whole, its
//! cell, as an SSA value instead, and empties the slot.
//!
//! A slot is left alone when its address goes anywhere but a plain load, a
//! plain store or an addition of a constant, when two of its accesses
//! overlap without covering the same bytes, when one reaches past its end,
//! or when one of its cells may be read before it is written. Which cells
//! exist is in [`cells`]; how they become SSA values is in [`ssa`].

mod cells;
mod ssa;

use super::pass_through::forward_where;
use crate::error::CodegenError;
pub(super) use cells::constant;
use cells::Cells;
use cranelift_codegen::flowgraph::ControlFlowGraph;
use cranelift_codegen::ir::{
    Block, DebugTag, Function, Opcode, StackSlot, StackSlotKind, ValueDef,
};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::Context;
use std::collections::{BTreeMap, BTreeSet};

/// The most cells times reachable blocks one function's promotion may
/// track. Liveness costs a bit per cell per block on every pass over the
/// blocks.
const MAX_CELL_BLOCKS: usize = 300_000;

/// The most rounds forwarding block parameters may take. Each round
/// forwards the parameters whose incoming value the previous round made
/// an address, one level of a chain of derived addresses.
const MAX_FORWARDING_ROUNDS: usize = 32;

/// What promoting one function's slots did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Promotion {
    /// How many slots were emptied.
    pub(super) slots: usize,
    /// How many cells of those slots became SSA values.
    pub(super) cells: usize,
}

/// Promote every explicit stack slot of `function` that can be held as SSA
/// values, running the analyses in `context`. A function that is looked at
/// at all may change even when nothing is promoted: its aliases are
/// resolved, its unreachable blocks removed and its block parameters that
/// carry a slot's address forwarded, none of which changes what it does.
pub(super) fn promote(
    context: &mut Context,
    function: &mut Function,
    isa: &dyn TargetIsa,
) -> Result<Promotion, CodegenError> {
    if !has_explicit_slot(function)
        || !function.dfg.exception_tables.is_empty()
        || observes_its_frame(function)
    {
        return Ok(Promotion::default());
    }
    context.clear();
    context.func = std::mem::replace(function, Function::new());
    let promoted = promote_in(context, isa);
    *function = std::mem::replace(&mut context.func, Function::new());
    promoted
}

/// Promote the slots of the function `context` holds, after resolving its
/// aliases, removing its unreachable blocks and forwarding the block
/// parameters that carry an address, which happens whether or not a slot
/// is then promoted.
fn promote_in(context: &mut Context, isa: &dyn TargetIsa) -> Result<Promotion, CodegenError> {
    context.func.dfg.resolve_all_aliases();
    context.flowgraph();
    if has_unreachable_block(context) {
        context
            .eliminate_unreachable_code(isa)
            .map_err(|e| CodegenError::Internal(format!("slot promotion: {e}")))?;
    }
    let order: Vec<Block> = context.domtree.cfg_rpo().copied().collect();
    forward_carried_addresses(&mut context.func, &context.cfg, &order);
    context.func.dfg.resolve_all_aliases();
    let Some((cells, liveness)) = promotable_cells(&context.func, &context.cfg, &order) else {
        return Ok(Promotion::default());
    };
    ssa::rewrite(&mut context.func, &context.cfg, &order, &cells, &liveness)?;
    empty_slots(&mut context.func, &cells);
    context.func.dfg.resolve_all_aliases();
    Ok(Promotion {
        slots: cells.slots().len(),
        cells: cells.cells.len(),
    })
}

/// Copy-propagate block parameters toward slot addresses: replace every
/// block parameter whose incoming values, other than itself along a back
/// edge, are all one value that is a slot address or another block
/// parameter, by that value. The frontend leaves a variable holding a slot's
/// address as such a parameter in each loop header, nested headers handing
/// each other the parameter. Every branch to the block hands it the value,
/// so the value dominates the block and the replacement keeps the
/// function's meaning. Only addresses and parameters are propagated: a
/// parameter can be a link in a chain that ends at an address, while any
/// other value cannot make a slot promotable. Each round makes the
/// parameters it forwards plain uses of their values, so an address derived
/// from one is an address the next round sees; the rounds stop when one
/// forwards nothing, or after [`MAX_FORWARDING_ROUNDS`].
fn forward_carried_addresses(function: &mut Function, cfg: &ControlFlowGraph, order: &[Block]) {
    for _ in 0..MAX_FORWARDING_ROUNDS {
        if !forward_round(function, cfg, order) {
            return;
        }
        function.dfg.resolve_all_aliases();
    }
}

/// Forward each parameter of `order`'s blocks whose incoming value is
/// already known to be an address or is a parameter, returning whether any
/// was.
fn forward_round(function: &mut Function, cfg: &ControlFlowGraph, order: &[Block]) -> bool {
    let addresses = cells::addresses_in(function, order);
    forward_where(function, cfg, order, |function, source| {
        addresses[source].is_some() || matches!(function.dfg.value_def(source), ValueDef::Param(..))
    })
}

/// The cells of the slots of `function` that can be promoted, with their
/// liveness, `None` when there are none. A slot one of whose cells is live
/// into the entry block, read where it may not have been written, is kept.
fn promotable_cells(
    function: &Function,
    cfg: &ControlFlowGraph,
    order: &[Block],
) -> Option<(Cells, ssa::Liveness)> {
    let entry = *order.first()?;
    let mut cells = within_budget(cells::discover(function, order), order.len());
    let mut liveness = ssa::liveness(function, cfg, order, &cells);
    let read_unwritten: BTreeSet<StackSlot> = (liveness.live_in(entry).iter())
        .map(|cell| cells.cells[cell].slot)
        .collect();
    if !read_unwritten.is_empty() {
        cells = cells.without(|slot| read_unwritten.contains(&slot));
        liveness = ssa::liveness(function, cfg, order, &cells);
    }
    (!cells.slots().is_empty()).then_some((cells, liveness))
}

/// `cells` less the slots that would take the number of cells times the
/// `blocks` they are tracked over past [`MAX_CELL_BLOCKS`], the slots with
/// the fewest cells kept first.
fn within_budget(cells: Cells, blocks: usize) -> Cells {
    let budget = MAX_CELL_BLOCKS / blocks.max(1);
    if cells.cells.len() <= budget {
        return cells;
    }
    let mut counts: BTreeMap<StackSlot, usize> = BTreeMap::new();
    for cell in &cells.cells {
        *counts.entry(cell.slot).or_default() += 1;
    }
    let mut by_size: Vec<(usize, StackSlot)> = counts
        .into_iter()
        .map(|(slot, count)| (count, slot))
        .collect();
    by_size.sort_unstable();
    let mut spent = 0usize;
    let over_budget: BTreeSet<StackSlot> = (by_size.into_iter())
        .filter(|&(count, _)| {
            spent = spent.saturating_add(count);
            spent > budget
        })
        .map(|(_, slot)| slot)
        .collect();
    cells.without(|slot| over_budget.contains(&slot))
}

/// Remove every instruction computing an address in a promoted slot, and
/// size the slot to nothing.
fn empty_slots(function: &mut Function, cells: &Cells) {
    for (slot, addresses) in cells.slots() {
        for &inst in addresses {
            function.layout.remove_inst(inst);
        }
        if let Some(data) = function.sized_stack_slots.get_mut(*slot) {
            data.size = 0;
            data.align_shift = 0;
        }
    }
}

/// Whether `function` has an explicit stack slot of some size.
fn has_explicit_slot(function: &Function) -> bool {
    (function.sized_stack_slots.values())
        .any(|slot| slot.kind == StackSlotKind::ExplicitSlot && slot.size != 0)
}

/// Whether `function` reaches its own frame other than through a slot's
/// address: by reading the frame or stack pointer, by switching stacks, or
/// by naming a slot in a debug tag. Such a function is left as it is.
fn observes_its_frame(function: &Function) -> bool {
    (function.layout.blocks())
        .flat_map(|block| function.layout.block_insts(block))
        .any(|inst| {
            let reaches = matches!(
                function.dfg.insts[inst].opcode(),
                Opcode::GetFramePointer | Opcode::GetStackPointer | Opcode::StackSwitch
            );
            let tags_a_slot = (function.debug_tags.get(inst).iter())
                .any(|tag| matches!(tag, DebugTag::StackSlot(_)));
            reaches || tags_a_slot
        })
}

/// Whether the function `context` holds has a block its entry does not
/// reach.
fn has_unreachable_block(context: &Context) -> bool {
    (context.func.layout.blocks()).any(|block| !context.domtree.is_reachable(block))
}

#[cfg(test)]
mod tests;
