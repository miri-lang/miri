// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Inlining of small, loop-free user functions into their callers.
//!
//! Every user function is translated to CLIF first. The functions are then
//! visited callees before callers, one strongly connected component of the
//! call graph at a time, in an order fixed by the order the functions were
//! translated in, so the same program always makes the same decisions. Each
//! function has its candidate callees inlined into it, is weighed as a
//! candidate itself, and is then handed back to be defined, so its IR is
//! dropped as soon as nothing needs it. A copy can hold calls of its own,
//! those the callee kept because its own budget could not afford them, and
//! they are weighed against what is left of the caller's budget, so a small
//! helper's callee is inlined wherever the helper is copied into a caller
//! that can afford both. A candidate's body is held while one of its direct
//! callers is still to be visited or a held candidate's body still calls it,
//! and is dropped as soon as neither is so ([`lifetimes`]).
//!
//! What the copies need repaired afterwards is in [`repair`]. Once a
//! function's callees are inlined into it, and before it is weighed as a
//! candidate, its stack slots are promoted to SSA values where they can be,
//! so a candidate's size and stack are those of the promoted body, and the
//! repairs, which match each copy to its original by position, run before
//! promotion removes any copied instruction.

mod call_graph;
mod lifetimes;
mod repair;

use super::{slot_promotion, TranslatedFunction};
use crate::error::CodegenError;
use call_graph::CallGraph;
use cranelift_codegen::inline::{Inline, InlineCommand};
use cranelift_codegen::ir::{FuncRef, Function, Inst, Opcode, Value};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::traversals::{Dfs, Event};
use cranelift_codegen::Context;
use lifetimes::Lifetimes;
use repair::{CalleeSource, CopiedCall, CopiedSymbol};
use std::borrow::Cow;

/// The most instructions a callee called from several places may have.
const SMALL_CALLEE_SIZE: usize = 48;

/// The most instructions a callee called from one place may have.
/// Inlining it makes one copy of its body for the one call, beside the body
/// its own definition still emits. A call of it inside a copy of another
/// candidate, which may be copied many times, is held to
/// [`SMALL_CALLEE_SIZE`] instead.
const SINGLE_CALL_SITE_CALLEE_SIZE: usize = 120;

/// How many times its own size a caller may grow to by inlining.
const CALLER_GROWTH_FACTOR: usize = 4;

/// The most instructions a caller may grow to by inlining, however small
/// it started.
const GROWN_CALLER_LIMIT: usize = 2000;

/// The most rounds of inlining into one caller: far more than the nesting
/// of loop-free candidates the budgets afford, so reaching it means the
/// rounds would not settle.
const MAX_INLINING_ROUNDS: usize = 64;

/// The most bytes of stack slots inlining may add to one caller.
const ADDED_STACK_LIMIT: u64 = 512;

/// The most bytes of stack slots inlining may add to a caller that can
/// call itself, whose every frame on a deep recursion carries them.
const RECURSIVE_CALLER_ADDED_STACK_LIMIT: u64 = 128;

/// Inline candidates into each of `functions`, callees first, handing each
/// function with its position to `define` once it has been inlined into.
pub(super) fn inline_and_define<'b>(
    functions: Vec<TranslatedFunction<'b>>,
    isa: &dyn TargetIsa,
    mut define: impl FnMut(usize, TranslatedFunction<'b>) -> Result<(), CodegenError>,
) -> Result<(), CodegenError> {
    let mut inliner = Inliner::over(&functions);
    let mut pending: Vec<Option<TranslatedFunction<'b>>> =
        functions.into_iter().map(Some).collect();
    for component in inliner.graph.bottom_up_components() {
        let recursive = component.len() != 1
            || (component.iter()).any(|&member| inliner.graph.is_self_recursive(member));
        for &position in &component {
            let mut function = pending
                .get_mut(position)
                .and_then(Option::take)
                .ok_or_else(|| {
                    CodegenError::Internal("the inliner visited a function twice".to_string())
                })?;
            inliner.inline_into(&mut function, position, recursive)?;
            slot_promotion::promote(&mut inliner.context, &mut function.function, isa)?;
            if !recursive && !function.is_entry {
                inliner.admit(position, &function, isa)?;
            }
            inliner.release_callees_of(position);
            define(position, function)?;
        }
    }
    Ok(())
}

/// The state inlining keeps across the functions.
struct Inliner {
    graph: CallGraph,
    /// Each function's candidate body, while some caller is still to come.
    candidates: Vec<Option<Candidate>>,
    /// What keeps each candidate alive.
    lifetimes: Lifetimes,
    /// The one context every inlining runs in.
    context: Context,
}

impl Inliner {
    fn over(functions: &[TranslatedFunction<'_>]) -> Self {
        let graph = CallGraph::of(functions);
        Self {
            lifetimes: Lifetimes::new(graph.distinct_callers()),
            candidates: std::iter::repeat_with(|| None)
                .take(functions.len())
                .collect(),
            graph,
            context: Context::new(),
        }
    }

    /// Weigh the function at `position` as a candidate.
    fn admit(
        &mut self,
        position: usize,
        translated: &TranslatedFunction<'_>,
        isa: &dyn TargetIsa,
    ) -> Result<(), CodegenError> {
        let call_sites = self.graph.call_sites(position);
        let candidate = Candidate::of(translated, call_sites, isa)?;
        if let Some(candidate) = &candidate {
            debug_assert!(candidate.size > 0, "a candidate holds no instruction");
            let calls = self.graph.called_by(&candidate.function);
            self.lifetimes.admit(position, calls);
        }
        self.candidates[position] = candidate;
        Ok(())
    }

    /// Count the function at `position` as visited by each of its direct
    /// callees, dropping every candidate nothing still to come can reach a
    /// call of.
    fn release_callees_of(&mut self, position: usize) {
        for dropped in self
            .lifetimes
            .visited(position, self.graph.callees(position))
        {
            self.candidates[dropped] = None;
        }
    }

    /// Inline the candidates `caller`, at `position`, calls into it and
    /// repair what the copies carry, round after round: a copy can hold calls
    /// of its own, which the next round weighs against what is left of the
    /// caller's budget. Each round leaves the copies it makes alone, so each
    /// is repaired whole, with the location of its call already restored,
    /// before the calls it holds are inlined. The rounds end when one inlines
    /// nothing; every copy spends some of the budget, and no candidate can
    /// reach itself, so they do, and [`MAX_INLINING_ROUNDS`] stops them
    /// with an error should that ever not hold.
    fn inline_into(
        &mut self,
        caller: &mut TranslatedFunction<'_>,
        position: usize,
        recursive: bool,
    ) -> Result<(), CodegenError> {
        let candidates = &self.candidates;
        if !(self.graph.callees(position).iter()).any(|&callee| candidates[callee].is_some()) {
            return Ok(());
        }
        let mut budget = GrowthBudget::new(instruction_count(&caller.function), recursive);
        for round in 0..MAX_INLINING_ROUNDS {
            let inlined = self.inline_round(caller, &mut budget, round != 0)?;
            if inlined.is_empty() {
                return Ok(());
            }
            self.repair_copies(caller, &inlined)?;
        }
        Err(CodegenError::inlining(
            caller.name,
            format!("inlining did not settle in {MAX_INLINING_ROUNDS} rounds"),
        ))
    }

    /// Inline into `caller` each call of a candidate it holds that `budget`
    /// affords, the calls the copies hold left for a later round. In a round
    /// `within_copies`, after the first, the calls weighed can sit in copies.
    fn inline_round(
        &mut self,
        caller: &mut TranslatedFunction<'_>,
        budget: &mut GrowthBudget,
        within_copies: bool,
    ) -> Result<Vec<InlinedCall>, CodegenError> {
        let mut hook = SmallFunctionInliner {
            graph: &self.graph,
            candidates: &self.candidates,
            budget: *budget,
            within_copies,
            inlined: Vec::new(),
        };
        self.context.clear();
        self.context.func = std::mem::replace(&mut caller.function, Function::new());
        let inlined = self.context.inline(&mut hook);
        caller.function = std::mem::replace(&mut self.context.func, Function::new());
        inlined.map_err(|e| CodegenError::inlining(caller.name, e.to_string()))?;
        *budget = hook.budget;
        Ok(hook.inlined)
    }

    /// Repair the copies `inlined` lists in `caller`: re-declare the symbols
    /// they refer to and restore their source locations.
    fn repair_copies(
        &self,
        caller: &mut TranslatedFunction<'_>,
        inlined: &[InlinedCall],
    ) -> Result<(), CodegenError> {
        let candidates = &self.candidates;
        for call in inlined {
            let callee = candidates[call.callee]
                .as_ref()
                .ok_or_else(|| CodegenError::inlining(caller.name, "an inlined callee is gone"))?;
            repair::remap_symbols(&mut caller.function, &callee.symbols, call.first_global)
                .map_err(|e| CodegenError::inlining(caller.name, e))?;
            if callee.source.is_empty() && call.copied.call_site.is_default() {
                continue;
            }
            let points = &mut caller.source_points;
            repair::restore_sources(&mut caller.function, points, &callee.source, call.copied)
                .map_err(|e| CodegenError::inlining(caller.name, e))?;
        }
        Ok(())
    }
}

/// A function its callers may inline: its body, what inlining it costs,
/// the symbols its copies need re-declared, and its source points.
struct Candidate {
    function: Function,
    size: usize,
    stack: u64,
    symbols: Vec<CopiedSymbol>,
    source: CalleeSource,
}

impl Candidate {
    /// `translated` as a candidate, when it is small enough for its
    /// `call_sites`, is a valid body holding no loop, and has stack slots of
    /// a size that can be counted.
    fn of(
        translated: &TranslatedFunction<'_>,
        call_sites: usize,
        isa: &dyn TargetIsa,
    ) -> Result<Option<Self>, CodegenError> {
        let function = &translated.function;
        let size = instruction_count(function);
        // An unverifiable body is not inlined, and its own definition is
        // verified again, so the fault is reported there with its name.
        let usable = call_sites != 0
            && size <= size_limit(call_sites)
            && !has_back_edge(function)
            && cranelift_codegen::verify_function(function, isa).is_ok();
        let Some(stack) = usable.then(|| stack_size(function)).flatten() else {
            return Ok(None);
        };
        let symbols = repair::copied_symbols(function)
            .map_err(|e| CodegenError::inlining(translated.name, e))?;
        Ok(Some(Self {
            source: CalleeSource::of(function, &translated.source_points),
            function: function.clone(),
            size,
            stack,
            symbols,
        }))
    }
}

/// The most instructions a callee with `call_sites` calls may have.
fn size_limit(call_sites: usize) -> usize {
    if call_sites == 1 {
        SINGLE_CALL_SITE_CALLEE_SIZE
    } else {
        SMALL_CALLEE_SIZE
    }
}

/// The bytes of `function`'s stack slots, `None` past what can be counted.
fn stack_size(function: &Function) -> Option<u64> {
    (function.sized_stack_slots.values())
        .try_fold(0u64, |total, slot| total.checked_add(u64::from(slot.size)))
}

/// How much more one caller may grow by inlining.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GrowthBudget {
    instructions: usize,
    stack: u64,
}

impl GrowthBudget {
    /// The budget of a caller of `size` instructions, one that can call
    /// itself when `recursive`.
    fn new(size: usize, recursive: bool) -> Self {
        let limit = size
            .saturating_mul(CALLER_GROWTH_FACTOR)
            .min(GROWN_CALLER_LIMIT);
        Self {
            instructions: limit.saturating_sub(size),
            stack: if recursive {
                RECURSIVE_CALLER_ADDED_STACK_LIMIT
            } else {
                ADDED_STACK_LIMIT
            },
        }
    }

    /// Spend `size` instructions and `stack` bytes, when both fit.
    fn spend(&mut self, size: usize, stack: u64) -> bool {
        if size > self.instructions || stack > self.stack {
            return false;
        }
        self.instructions -= size;
        self.stack -= stack;
        true
    }
}

/// One call Cranelift inlined: the callee, and where the caller's entities
/// stood just before its copy was appended.
struct InlinedCall {
    callee: usize,
    first_global: u32,
    copied: CopiedCall,
}

/// The `Inline` hook: inline a direct call of a candidate whose signature is
/// the call's, while the caller's budget lasts.
struct SmallFunctionInliner<'g> {
    graph: &'g CallGraph,
    candidates: &'g [Option<Candidate>],
    budget: GrowthBudget,
    /// Whether the calls weighed can sit in copies of other candidates,
    /// each of which may be copied many times, so that a callee is held to
    /// [`SMALL_CALLEE_SIZE`] whatever its call sites.
    within_copies: bool,
    inlined: Vec<InlinedCall>,
}

impl Inline for SmallFunctionInliner<'_> {
    fn inline(
        &mut self,
        caller: &Function,
        call_inst: Inst,
        call_opcode: Opcode,
        callee: FuncRef,
        _call_args: &[Value],
    ) -> InlineCommand<'_> {
        if call_opcode != Opcode::Call {
            return InlineCommand::KeepCall;
        }
        let Some(position) = self.graph.position_of(caller, callee) else {
            return InlineCommand::KeepCall;
        };
        let Some(candidate) = self.candidates.get(position).and_then(Option::as_ref) else {
            return InlineCommand::KeepCall;
        };
        let call_signature = (caller.dfg.ext_funcs.get(callee))
            .and_then(|data| caller.dfg.signatures.get(data.signature));
        let entities = u32::try_from(caller.global_values.len())
            .ok()
            .zip(u32::try_from(caller.dfg.num_blocks()).ok());
        let Some((first_global, first_block)) = entities else {
            return InlineCommand::KeepCall;
        };
        if call_signature != Some(&candidate.function.signature)
            || (self.within_copies && candidate.size > SMALL_CALLEE_SIZE)
            || !self.budget.spend(candidate.size, candidate.stack)
        {
            return InlineCommand::KeepCall;
        }
        self.inlined.push(InlinedCall {
            callee: position,
            first_global,
            copied: CopiedCall {
                first_block,
                call_site: caller.srcloc(call_inst),
            },
        });
        // The calls the copy holds are left to the next round, after the copy
        // is repaired: Cranelift would otherwise split a copy by inlining
        // into it before it is matched to its original, and the calls in it
        // would carry no location yet to inline them through.
        InlineCommand::Inline {
            callee: Cow::Borrowed(&candidate.function),
            visit_callee: false,
        }
    }
}

/// How many instructions `function`'s layout holds.
fn instruction_count(function: &Function) -> usize {
    (function.layout.blocks())
        .map(|block| function.layout.block_insts(block).count())
        .sum()
}

/// Whether some branch of `function` jumps back to a block it is reached
/// from: an edge to an ancestor in a depth-first walk of the blocks, the
/// block itself included.
///
/// A callee holding a loop is never inlined. A loop's own iterations
/// dwarf the call around it, and inlining one costs a copy of the whole
/// loop per call site; measured across the benchmark workloads, letting
/// such callees in moved instruction counts between -1.5% and +3% with no
/// consistent gain, so the call stays.
fn has_back_edge(function: &Function) -> bool {
    use cranelift_codegen::entity::SecondaryMap;
    use cranelift_codegen::ir::Block;
    let mut entered: SecondaryMap<Block, Option<usize>> = SecondaryMap::new();
    let mut exited: SecondaryMap<Block, Option<usize>> = SecondaryMap::new();
    let mut dfs = Dfs::new();
    for (clock, (event, block)) in dfs.iter(function).enumerate() {
        match event {
            Event::Enter => entered[block] = Some(clock),
            Event::Exit => exited[block] = Some(clock),
        }
    }
    let span = |block: Block| Some((entered[block]?, exited[block]?));
    function.layout.blocks().any(|block| {
        let Some((block_in, block_out)) = span(block) else {
            return false;
        };
        function.block_successors(block).any(|successor| {
            span(successor).is_some_and(|(successor_in, successor_out)| {
                successor_in <= block_in && block_out <= successor_out
            })
        })
    })
}

#[cfg(test)]
mod tests;
