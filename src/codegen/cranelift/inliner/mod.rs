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
//! dropped as soon as nothing needs it. A candidate is held, legalized as
//! Cranelift's inliner requires, until the last function that refers to it
//! has been visited.
//!
//! What the copies need repaired afterwards is in [`repair`].

mod call_graph;
mod repair;

use super::TranslatedFunction;
use crate::error::CodegenError;
use call_graph::CallGraph;
use cranelift_codegen::inline::{Inline, InlineCommand};
use cranelift_codegen::ir::{FuncRef, Function, Inst, Opcode, Value};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::traversals::{Dfs, Event};
use cranelift_codegen::Context;
use repair::{CalleeSource, CopiedCall, CopiedSymbol};
use std::borrow::Cow;

/// The most legalized instructions a callee called from several places may
/// have.
const SMALL_CALLEE_SIZE: usize = 48;

/// The most legalized instructions a callee called from one place may have.
/// Inlining it makes one copy of its body for the one call, beside the body
/// its own definition still emits.
const SINGLE_CALL_SITE_CALLEE_SIZE: usize = 120;

/// How many times its own size a caller may grow to by inlining.
const CALLER_GROWTH_FACTOR: usize = 4;

/// The most instructions a caller may grow to by inlining, however small
/// it started.
const GROWN_CALLER_LIMIT: usize = 2000;

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
    /// How many of each function's callers are still to be visited.
    callers_left: Vec<usize>,
    /// The one context every inlining and legalization runs in.
    context: Context,
}

impl Inliner {
    fn over(functions: &[TranslatedFunction<'_>]) -> Self {
        let graph = CallGraph::of(functions);
        Self {
            callers_left: graph.distinct_callers(),
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
        self.candidates[position] = Candidate::of(&mut self.context, translated, call_sites, isa)?;
        Ok(())
    }

    /// Count the function at `position` as visited by each of its callees,
    /// dropping the candidate whose last caller it was.
    fn release_callees_of(&mut self, position: usize) {
        for &callee in self.graph.callees(position) {
            if callee == position {
                continue;
            }
            let left = &mut self.callers_left[callee];
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.candidates[callee] = None;
            }
        }
    }

    /// Inline the candidates `caller`, at `position`, calls into it and
    /// repair what the copies carry.
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
        let mut hook = SmallFunctionInliner {
            graph: &self.graph,
            candidates,
            budget: GrowthBudget::new(instruction_count(&caller.function), recursive),
            inlined: Vec::new(),
        };
        self.context.clear();
        self.context.func = std::mem::replace(&mut caller.function, Function::new());
        let inlined = self.context.inline(&mut hook);
        caller.function = std::mem::replace(&mut self.context.func, Function::new());
        inlined.map_err(|e| CodegenError::inlining(caller.name, e.to_string()))?;
        for call in &hook.inlined {
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

/// A function its callers may inline: its legalized body, what inlining it
/// costs, the symbols its copies need re-declared, and its source points.
struct Candidate {
    function: Function,
    size: usize,
    stack: u64,
    symbols: Vec<CopiedSymbol>,
    source: CalleeSource,
}

impl Candidate {
    /// `translated` as a candidate, when it is small enough for its
    /// `call_sites`, legalizes to a valid body holding no loop and no
    /// `global_value` instruction, and has stack slots of a size that can be
    /// counted.
    fn of(
        context: &mut Context,
        translated: &TranslatedFunction<'_>,
        call_sites: usize,
        isa: &dyn TargetIsa,
    ) -> Result<Option<Self>, CodegenError> {
        let limit = size_limit(call_sites);
        if call_sites == 0 || instruction_count(&translated.function) > limit {
            return Ok(None);
        }
        context.clear();
        context.func = translated.function.clone();
        context
            .legalize(isa)
            .map_err(|e| CodegenError::inlining(translated.name, e.to_string()))?;
        let function = std::mem::replace(&mut context.func, Function::new());
        let size = instruction_count(&function);
        // An unverifiable body is not inlined, and its own definition is
        // verified again, so the fault is reported there with its name.
        let usable = size <= limit
            && !has_back_edge(&function)
            && !has_global_value_instruction(&function)
            && cranelift_codegen::verify_function(&function, isa).is_ok();
        let Some(stack) = usable.then(|| stack_size(&function)).flatten() else {
            return Ok(None);
        };
        let symbols = repair::copied_symbols(&function)
            .map_err(|e| CodegenError::inlining(translated.name, e))?;
        Ok(Some(Self {
            source: CalleeSource::of(&function, &translated.source_points),
            function,
            size,
            stack,
            symbols,
        }))
    }
}

/// The most legalized instructions a callee with `call_sites` calls may
/// have.
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

/// Whether `function` still holds a `global_value` instruction, which
/// Cranelift refuses to inline. Legalization lowers every one Miri emits,
/// so one left over is a body this pass does not understand.
fn has_global_value_instruction(function: &Function) -> bool {
    (function.layout.blocks()).any(|block| {
        function
            .layout
            .block_insts(block)
            .any(|inst| function.dfg.insts[inst].opcode() == Opcode::GlobalValue)
    })
}

/// Whether some branch of `function` jumps back to a block it is reached
/// from: an edge to an ancestor in a depth-first walk of the blocks, the
/// block itself included.
///
/// TODO: a callee holding a loop is never inlined. Cranelift's loop-invariant
/// code motion mishandles an inlined loop and the inlined code runs markedly
/// slower than the call it replaced; inlining one needs that fixed first.
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
