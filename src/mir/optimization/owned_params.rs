// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Owned parameters: the arguments a function takes over from its callers.
//!
//! A call lends its managed arguments by default. The callee reads the caller's
//! reference, and a binding that keeps the value (`var items = start`) retains
//! it, so a callee that then writes its binding always finds the value shared
//! and copies it — even when the caller lets the argument die at the call and
//! nobody will ever read the original again.
//!
//! This pass finds the parameters whose only use is one such binding, in the
//! entry block, and makes them owned. A store into a field (`self.items =
//! items` in a constructor) is such a binding as well: the object keeps the
//! value. A copy of the parameter whose value is discarded — what an
//! expression statement leaves of a store — is no use. The binding moves the
//! parameter instead of copying it, and every call to the function passes the
//! argument
//! [`ArgMode::Owned`]: RC insertion retains the argument before the call, and the
//! callee releases that reference through the local it moved it into. Where the
//! argument dies at the call, RC elision cancels the retain against the release
//! the caller makes after it, so the callee holds the only reference and writes
//! in place.
//!
//! A parameter's mode is a contract between a body and every call to it, so the
//! pass runs once over the whole program, before RC insertion, and only for a
//! function every mention of which is a direct call from host code it can
//! stamp: a top-level host function or a statically dispatched method that no
//! GPU body, function value, vtable slot or compiler-synthesized caller
//! reaches. A method's receiver stays borrowed.
//!
//! Owning a parameter moves where the value's last release may happen — from
//! the caller, after the call, into the callee — so this pass never owns a
//! parameter whose release may run a drop hook anywhere inside it: when a hook
//! runs is the program's meaning, and an optimisation must not move it. A
//! resource parameter the callee keeps is owned by the semantics instead: the
//! type checker decides it, the callee's signature carries it
//! ([`crate::mir::Signature::kept_params`]), and lowering writes both sides
//! of every call to it. This pass starts from those and only adds to them.

use crate::ast::implicit_methods::is_called_by_a_runtime_thunk;
use crate::mir::facts::ProgramFacts;
use crate::mir::instantiation::shape::mentions_type_parameter;
use crate::mir::place::PlaceContext;
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::terminator::arg_mode;
use crate::mir::type_facts::DropHookReach;
use crate::mir::visitor::Visitor;
use crate::mir::{
    ArgMode, BasicBlock, Body, ExecutionModel, FunctionConstant, Local, Operand, Place, PlaceElem,
    Rvalue, Statement, StatementKind, TerminatorKind,
};
use std::collections::{HashMap, HashSet};

/// Decide which parameters each function of `program` takes over, rewrite each
/// owned parameter's binding into a move, and mark every call passing one.
///
/// `synthesized` names the bodies codegen calls without a MIR call carrying
/// them — the runtime thunks' and every filled vtable slot's; their parameters
/// stay borrowed, since no call site could be marked.
/// `drop_hooks` answers which parameter types may run a drop hook when
/// released. Bodies are visited in `program` order, so the result is
/// deterministic. Which parameters each body takes over is recorded in its
/// entry of `facts`, the facts of `program`'s bodies in order: they start
/// from the ones its signature keeps, and this pass only adds to them.
pub fn decide_owned_parameters(
    program: &mut [(Symbol, Body)],
    facts: &mut ProgramFacts,
    synthesized: &HashSet<Symbol>,
    drop_hooks: &DropHookReach,
) {
    debug_assert!(
        (program.iter().zip(facts.iter())).all(|((_, body), facts)| {
            let owned = &facts.ownership().owned_params;
            (body.signature.kept_params.iter().enumerate())
                .all(|(index, is_kept)| !is_kept || owned.get(index).copied().unwrap_or(false))
        }),
        "the facts own every parameter a signature keeps"
    );
    let mut candidates = collect_candidates(program, synthesized, drop_hooks);
    if candidates.is_empty() {
        return;
    }
    let arities = (candidates.iter())
        .map(|(symbol, candidate)| (symbol.clone(), candidate.arity))
        .collect();
    let reach = reach_of(program, arities);
    candidates.retain(|symbol, _| {
        reach
            .get(symbol)
            .is_some_and(|reach| reach.is_called() && reach.is_only_called_directly())
    });
    let mut owners = HashMap::new();
    for ((symbol, body), facts) in program.iter_mut().zip(facts.iter_mut()) {
        if let Some(candidate) = candidates.get(symbol) {
            let owned = take_over_parameters(body, &candidate.bindings);
            owners.insert(symbol.clone(), owned.clone());
            add_owned_parameters(&mut facts.ownership_mut().owned_params, &owned);
        }
    }
    for (_, body) in program.iter_mut() {
        stamp_owned_arguments(body, &owners);
    }
}

/// Mark each parameter this pass took over, `owned`, as owned in `params`,
/// keeping every parameter `params` already owns: the facts start from the
/// ones the body's signature keeps ([`crate::mir::facts::BodyFacts::new`]),
/// which are the semantics', never the optimisation's to give back.
fn add_owned_parameters(params: &mut Vec<bool>, owned: &[bool]) {
    if params.len() < owned.len() {
        params.resize(owned.len(), false);
    }
    for (param, is_owned) in params.iter_mut().zip(owned) {
        *param |= *is_owned;
    }
}

/// A function some of whose parameters could be owned.
struct Candidate {
    /// For each parameter, the entry-block statement binding it, when that
    /// binding is the parameter's only use.
    bindings: Vec<Option<Binding>>,
    /// How many parameters the function takes.
    arity: usize,
}

/// What a program does with a function's symbol: the one answer to whether
/// every caller reaches it through a MIR call naming it, which a function
/// owning a parameter needs, since only such a call can pass the argument
/// owned.
#[derive(Default)]
pub struct Reach {
    /// How many parameters the function takes.
    arity: usize,
    /// How many bodies were lowered under the symbol.
    bodies: usize,
    /// How many operands anywhere in the program name the symbol.
    named: usize,
    /// How many of those are the callee of a host call passing `arity`
    /// arguments.
    called: usize,
}

impl Reach {
    /// Whether some host call names the function as its callee.
    pub fn is_called(&self) -> bool {
        self.called > 0
    }

    /// Whether every mention of the symbol is a host call made to the one
    /// body lowered under it: no function value, no GPU call, no second body.
    pub fn is_only_called_directly(&self) -> bool {
        self.bodies == 1 && self.named == self.called
    }
}

/// How `program` reaches each function of `arities`, a map from its symbol to
/// the number of parameters it takes.
///
/// A GPU body's call is counted as a mention only, so a function a GPU body
/// reaches is not called directly. A body the runtime or a vtable slot calls
/// is reached without any MIR call: the caller checks `synthesized` beside
/// this.
pub fn reach_of(
    program: &[(Symbol, Body)],
    arities: HashMap<Symbol, usize>,
) -> HashMap<Symbol, Reach> {
    let mut reach: HashMap<Symbol, Reach> = (arities.into_iter())
        .map(|(symbol, arity)| {
            let reach = Reach {
                arity,
                ..Reach::default()
            };
            (symbol, reach)
        })
        .collect();
    if reach.is_empty() {
        return reach;
    }
    for (symbol, body) in program {
        if let Some(found) = reach.get_mut(symbol) {
            found.bodies += 1;
        }
        FunctionMentions { reach: &mut reach }.visit_body(body);
        if !body.is_gpu() {
            count_direct_calls(body, &mut reach);
        }
    }
    reach
}

/// Refuses a program reaching a function whose signature keeps a parameter
/// other than through a host call naming it and passing every kept position
/// [`ArgMode::Owned`]: as a value, from a GPU body, through a runtime thunk
/// (`synthesized`), under a symbol with a second body, or by a call lending
/// a kept position — a method's receiver, a call the compiler makes where
/// none is written. Such a caller lends the argument, so the callee would
/// release a reference nobody handed it. A vtable slot hands every body it
/// reaches what that body keeps, directly or through an adapter, which
/// `verify/slot_conventions.rs` checks.
///
/// The type checker keeps no parameter of a function it sees reached any of
/// these ways; this holds that decision to what the lowered program does, on
/// every build.
pub fn refuse_indirectly_reached_keepers(
    program: &[(Symbol, Body)],
    synthesized: &HashSet<Symbol>,
) -> Result<(), String> {
    let keepers: HashMap<&Symbol, &[bool]> = (program.iter())
        .filter(|(_, body)| body.signature.kept_params.contains(&true))
        .map(|(symbol, body)| (symbol, body.signature.kept_params.as_slice()))
        .collect();
    let arities: HashMap<Symbol, usize> = (program.iter())
        .filter(|(symbol, _)| keepers.contains_key(symbol))
        .map(|(symbol, body)| (symbol.clone(), body.arg_count))
        .collect();
    let reach = reach_of(program, arities);
    let mut refused: Vec<String> = (reach.iter())
        .filter(|(symbol, reach)| synthesized.contains(*symbol) || !reach.is_only_called_directly())
        .map(|(symbol, _)| symbol.link_name())
        .collect();
    if !refused.is_empty() {
        refused.sort_unstable();
        return Err(format!(
            "{} keeps a parameter, yet is reached other than by a host call naming it",
            refused.join(", ")
        ));
    }
    refuse_lent_kept_positions(program, &keepers)
}

/// Refuses a direct call passing a position its callee's signature keeps
/// other than [`ArgMode::Owned`]: the call would lend what the callee
/// releases.
fn refuse_lent_kept_positions(
    program: &[(Symbol, Body)],
    keepers: &HashMap<&Symbol, &[bool]>,
) -> Result<(), String> {
    if keepers.is_empty() {
        return Ok(());
    }
    let calls = (program.iter())
        .flat_map(|(caller, body)| body.basic_blocks.iter().map(move |block| (caller, block)))
        .filter_map(|(caller, block)| Some((caller, &block.terminator.as_ref()?.kind)));
    let mut refused: Vec<String> = Vec::new();
    for (caller, kind) in calls {
        let TerminatorKind::Call {
            func: Operand::Function(function),
            arg_modes,
            ..
        } = kind
        else {
            continue;
        };
        let Some(kept) = keepers.get(&function.symbol) else {
            continue;
        };
        let lent = (kept.iter().enumerate()).find(|&(position, is_kept)| {
            *is_kept && arg_mode(arg_modes, position) != ArgMode::Owned
        });
        if let Some((position, _)) = lent {
            refused.push(format!(
                "{} lends argument {position} of {}, which keeps it",
                caller.link_name(),
                function.symbol.link_name()
            ));
        }
    }
    if refused.is_empty() {
        return Ok(());
    }
    refused.sort_unstable();
    Err(refused.join("; "))
}

/// The functions with at least one parameter that could be owned, keyed by
/// symbol.
fn collect_candidates(
    program: &[(Symbol, Body)],
    synthesized: &HashSet<Symbol>,
    drop_hooks: &DropHookReach,
) -> HashMap<Symbol, Candidate> {
    let mut candidates = HashMap::new();
    for (symbol, body) in program {
        if !can_own_parameters(symbol, body, synthesized) {
            continue;
        }
        let bindings = sole_bindings(body, drop_hooks);
        if bindings.iter().any(Option::is_some) {
            let candidate = Candidate {
                bindings,
                arity: body.arg_count,
            };
            candidates.insert(symbol.clone(), candidate);
        }
    }
    candidates
}

/// Whether `body`, lowered under `symbol`, may take any parameter over: a
/// host function or method compiled for one instantiation, captured nothing,
/// that every caller reaches through a MIR call, and whose entry block runs
/// once per call — nothing branches back to it, so a binding there takes the
/// reference over exactly once.
fn can_own_parameters(symbol: &Symbol, body: &Body, synthesized: &HashSet<Symbol>) -> bool {
    let is_static_callee = symbol.is_plain_function()
        || symbol
            .plain_method_name()
            .is_some_and(|method| !is_called_by_a_runtime_thunk(method));
    is_static_callee
        && !synthesized.contains(symbol)
        && matches!(body.execution_model, ExecutionModel::Cpu)
        && body.generics.open_params.is_empty()
        && body.captures.env_capture_locals.is_empty()
        && body.predecessor_counts().first() == Some(&1)
}

/// The entry-block statement binding a parameter, and the copies of it whose
/// value is discarded.
#[derive(Clone)]
struct Binding {
    statement: usize,
    discarded: Vec<Local>,
}

/// For each parameter of `body`, the entry-block statement binding it when the
/// parameter can be owned and that binding is its only use apart from copies
/// whose value is discarded.
fn sole_bindings(body: &Body, drop_hooks: &DropHookReach) -> Vec<Option<Binding>> {
    let mut uses = ParameterUses {
        counts: vec![0; body.local_decls.len()],
    };
    uses.visit_body(body);
    (1..=body.arg_count)
        .map(|index| {
            let param = Local(index);
            let discarded = discarded_copies(body, param, &uses.counts);
            let is_used_once = uses.counts[index] == discarded.len() + 1;
            let statement = (is_used_once && is_ownable_param(body, param, drop_hooks))
                .then(|| binding_of(body, param))
                .flatten()?;
            Some(Binding {
                statement,
                discarded,
            })
        })
        .collect()
}

/// The temporaries holding an entry-block copy of `param` that nothing reads:
/// each is mentioned only by that copy and by the end of its storage, in the
/// entry block. A binding outlives the entry block, so none is among them.
/// `counts` is how many times each local is mentioned apart from that end.
fn discarded_copies(body: &Body, param: Local, counts: &[usize]) -> Vec<Local> {
    let Some(entry) = body.basic_blocks.first() else {
        return Vec::new();
    };
    entry
        .statements
        .iter()
        .filter_map(|statement| copied_parameter(statement, param))
        .filter(|dest| dest.projection.is_empty())
        .map(|dest| dest.local)
        .filter(|&temp| {
            temp.0 > body.arg_count
                && counts.get(temp.0) == Some(&1)
                && dies_in_entry_block(body, temp)
        })
        .collect()
}

/// How many times each local is mentioned, apart from the end of its storage.
struct ParameterUses {
    counts: Vec<usize>,
}

impl Visitor for ParameterUses {
    fn visit_local(&mut self, local: Local, context: PlaceContext, _location: BasicBlock) {
        if matches!(context, PlaceContext::StorageDead) {
            return;
        }
        if let Some(count) = self.counts.get_mut(local.0) {
            *count += 1;
        }
    }
}

/// Whether the parameter `param` holds a value whose ownership can move: a
/// managed, heap-allocated value whose release runs no drop hook anywhere
/// inside it, not a method's receiver, not an `out` slot, not a gpu-resident
/// buffer, and typed without any type parameter.
fn is_ownable_param(body: &Body, param: Local, drop_hooks: &DropHookReach) -> bool {
    let decl = &body.local_decls[param.0];
    let is_out = body
        .signature
        .out_params
        .get(param.0 - 1)
        .copied()
        .unwrap_or(false);
    let is_type_parameter = |name: &str| {
        body.generics.type_params.contains(name) || body.generics.open_params.contains(name)
    };
    body.is_counted_local(param)
        && body.signature.receiver != Some(param)
        && !is_out
        && decl.device_handle.is_none()
        && !mentions_type_parameter(&decl.ty, &is_type_parameter, true)
        && !drop_hooks.may_run_drop_hook(&decl.ty)
}

/// The entry-block statement binding `param`, if there is one: `x = copy
/// param` binding it to a local of its own type, or a store of it into a field
/// holding a reference to a value of its type.
fn binding_of(body: &Body, param: Local) -> Option<usize> {
    let entry = body.basic_blocks.first()?;
    entry.statements.iter().position(|statement| {
        let is_rebinding = matches!(statement.kind, StatementKind::Reassign(..));
        copied_parameter(statement, param).is_some_and(|dest| {
            is_field_store_target(body, param, dest)
                || (!is_rebinding && is_binding_target(body, param, dest))
        })
    })
}

/// The place `statement` copies `param` whole into, when it is an assignment
/// or a rebinding doing nothing else.
fn copied_parameter(statement: &Statement, param: Local) -> Option<&Place> {
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
        &statement.kind
    else {
        return None;
    };
    matches!(rvalue, Rvalue::Use(Operand::Copy(source)) if *source == Place::new(param))
        .then_some(dest)
}

/// Whether `dest` is a field, reached through fields alone from a local other
/// than `param`, declared at `param`'s type: the object holding it keeps the
/// reference a store hands it and releases it when it dies.
fn is_field_store_target(body: &Body, param: Local, dest: &Place) -> bool {
    let is_field_path = !dest.projection.is_empty()
        && dest
            .projection
            .iter()
            .all(|elem| matches!(elem, PlaceElem::Field(_)));
    is_field_path
        && dest.local != param
        && body.local_decls[dest.local.0].device_handle.is_none()
        && PlaceTypes::of(body)
            .declared(dest)
            .is_some_and(|field| field.kind == body.local_decls[param.0].ty.kind)
}

/// Whether `dest` can receive `param`'s reference: a whole local of the body's
/// own, of the parameter's type, holding no device buffer, that outlives the
/// entry block.
///
/// A binding that dies in the entry block costs nothing borrowed — RC elision
/// cancels its retain against its release — so owning it would only add the
/// caller's retain.
fn is_binding_target(body: &Body, param: Local, dest: &Place) -> bool {
    let decl = &body.local_decls[dest.local.0];
    dest.projection.is_empty()
        && dest.local.0 > body.arg_count
        && decl.mir_ty == body.local_decls[param.0].mir_ty
        && decl.device_handle.is_none()
        && !dies_in_entry_block(body, dest.local)
}

/// Whether the entry block ends `local`'s storage.
fn dies_in_entry_block(body: &Body, local: Local) -> bool {
    body.basic_blocks.first().is_some_and(|entry| {
        entry.statements.iter().any(|statement| {
            matches!(&statement.kind, StatementKind::StorageDead(place, _) if place.local == local)
        })
    })
}

/// Counts every operand naming a function whose reach is wanted.
struct FunctionMentions<'c> {
    reach: &'c mut HashMap<Symbol, Reach>,
}

impl Visitor for FunctionMentions<'_> {
    fn visit_function(&mut self, function: &FunctionConstant, _location: BasicBlock) {
        if let Some(reach) = self.reach.get_mut(&function.symbol) {
            reach.named += 1;
        }
    }
}

/// Count each call in `body` naming a function whose reach is wanted as its
/// callee with as many arguments as the function takes.
fn count_direct_calls(body: &Body, reach: &mut HashMap<Symbol, Reach>) {
    for terminator in body
        .basic_blocks
        .iter()
        .filter_map(|b| b.terminator.as_ref())
    {
        let TerminatorKind::Call {
            func: Operand::Function(function),
            args,
            ..
        } = &terminator.kind
        else {
            continue;
        };
        if let Some(found) = reach.get_mut(&function.symbol) {
            if args.len() == found.arity {
                found.called += 1;
            }
        }
    }
}

/// Turn each owned parameter's binding into a move out of the parameter, and
/// return which parameters `body` owns: exactly those whose binding was
/// rewritten.
///
/// The discarded copies of an owned parameter go, along with their
/// temporaries' storage ends: nothing reads them, and past the move they would
/// read a parameter whose reference has left.
fn take_over_parameters(body: &mut Body, bindings: &[Option<Binding>]) -> Vec<bool> {
    let owned: Vec<bool> = bindings
        .iter()
        .map(|binding| {
            binding.as_ref().is_some_and(|binding| {
                body.basic_blocks
                    .first_mut()
                    .and_then(|entry| entry.statements.get_mut(binding.statement))
                    .is_some_and(move_bound_value)
            })
        })
        .collect();
    let discarded: HashSet<Local> = bindings
        .iter()
        .zip(&owned)
        .filter(|(_, is_owned)| **is_owned)
        .filter_map(|(binding, _)| binding.as_ref())
        .flat_map(|binding| binding.discarded.iter().copied())
        .collect();
    remove_discarded_copies(body, &discarded);
    owned
}

/// Turn every statement naming one of `discarded` into a no-op.
fn remove_discarded_copies(body: &mut Body, discarded: &HashSet<Local>) {
    if discarded.is_empty() {
        return;
    }
    for statement in body
        .basic_blocks
        .iter_mut()
        .flat_map(|block| &mut block.statements)
    {
        let names_discarded = match &statement.kind {
            StatementKind::Assign(place, _)
            | StatementKind::Reassign(place, _)
            | StatementKind::StorageDead(place, _)
            | StatementKind::StorageLive(place) => discarded.contains(&place.local),
            StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop
            | StatementKind::ReadHeader(_) => false,
        };
        if names_discarded {
            statement.kind = StatementKind::Nop;
        }
    }
}

/// Rewrite `x = copy p` into `x = move p`, reporting whether the statement
/// had that shape.
fn move_bound_value(statement: &mut Statement) -> bool {
    let (StatementKind::Assign(_, Rvalue::Use(operand))
    | StatementKind::Reassign(_, Rvalue::Use(operand))) = &mut statement.kind
    else {
        return false;
    };
    let Operand::Copy(place) = operand else {
        return false;
    };
    *operand = Operand::Move(place.clone());
    true
}

/// Mark each argument a call in `body` passes to a parameter its callee owns;
/// `owners` lists, for each function owning any, which parameters it owns.
fn stamp_owned_arguments(body: &mut Body, owners: &HashMap<Symbol, Vec<bool>>) {
    for block in &mut body.basic_blocks {
        let Some(terminator) = block.terminator.as_mut() else {
            continue;
        };
        let TerminatorKind::Call {
            func: Operand::Function(function),
            args,
            arg_modes,
            ..
        } = &mut terminator.kind
        else {
            continue;
        };
        let Some(owned) = owners.get(&function.symbol) else {
            continue;
        };
        if arg_modes.len() < args.len() {
            arg_modes.resize(args.len(), ArgMode::Borrowed);
        }
        for (mode, is_owned) in arg_modes.iter_mut().zip(owned) {
            if *is_owned {
                *mode = ArgMode::Owned;
            }
        }
    }
}
