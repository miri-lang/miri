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

use crate::error::CodegenError;
use cells::Cells;
use cranelift_codegen::flowgraph::ControlFlowGraph;
use cranelift_codegen::ir::{Block, Function, StackSlot, StackSlotKind};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::Context;
use std::collections::BTreeSet;

/// What promoting one function's slots did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Promotion {
    /// How many slots were emptied.
    pub(super) slots: usize,
    /// How many cells of those slots became SSA values.
    pub(super) cells: usize,
}

/// Promote every explicit stack slot of `function` that can be held as SSA
/// values, running the analyses in `context`.
pub(super) fn promote(
    context: &mut Context,
    function: &mut Function,
    isa: &dyn TargetIsa,
) -> Result<Promotion, CodegenError> {
    if !has_explicit_slot(function) || !function.dfg.exception_tables.is_empty() {
        return Ok(Promotion::default());
    }
    context.clear();
    context.func = std::mem::replace(function, Function::new());
    let promoted = promote_in(context, isa);
    *function = std::mem::replace(&mut context.func, Function::new());
    promoted
}

/// Promote the slots of the function `context` holds.
fn promote_in(context: &mut Context, isa: &dyn TargetIsa) -> Result<Promotion, CodegenError> {
    context.func.dfg.resolve_all_aliases();
    context.flowgraph();
    if has_unreachable_block(context) {
        context
            .eliminate_unreachable_code(isa)
            .map_err(|e| CodegenError::Internal(format!("slot promotion: {e}")))?;
    }
    let order: Vec<Block> = context.domtree.cfg_rpo().copied().collect();
    let Some((cells, liveness)) = promotable_cells(&context.func, &context.cfg, &order) else {
        return Ok(Promotion::default());
    };
    ssa::rewrite(&mut context.func, &context.cfg, &order, &cells, &liveness)?;
    empty_slots(&mut context.func, &cells);
    Ok(Promotion {
        slots: cells.slots().len(),
        cells: cells.cells.len(),
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
    let mut cells = cells::discover(function, order);
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

/// Whether the function `context` holds has a block its entry does not
/// reach.
fn has_unreachable_block(context: &Context) -> bool {
    (context.func.layout.blocks()).any(|block| !context.domtree.is_reachable(block))
}

#[cfg(test)]
mod tests;
