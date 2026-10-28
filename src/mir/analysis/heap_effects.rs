// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which calls may release a value their caller lends them.
//!
//! A call lends its arguments: the callee reads the caller's reference and
//! takes none of its own. That is safe while whatever holds the caller's
//! reference outlives the call. An argument read out of a class field, or
//! through a dereference, is held by a place the callee can reach another way
//! — through the instance passed beside it, a receiver, a capture — and a
//! callee that replaces or empties that place releases the argument while it
//! still reads it.
//!
//! An operation is **loud** when it may release a heap value some other name
//! holds, and **quiet** otherwise:
//!
//! - a store whose destination crosses a class field or a dereference
//!   ([`crosses_heap_field`]), or is rooted at the receiver, a parameter the
//!   body takes over, an `out` parameter or a capture, is loud;
//! - a call into the runtime is loud when it may call a closure, or writes in
//!   place (`runtime_fns::writes_in_place`) an argument that is not a whole
//!   counted local the body owns, or one whose release may run a drop hook;
//! - a virtual call, a call through a closure value, a kernel launch and a
//!   call to a symbol with no body of the program is loud;
//! - a release that may run a drop hook is loud: before reference counting,
//!   the end of a local's storage or a rebinding of a type whose release may
//!   run one; after it, the explicit release of such a type;
//! - a copy-on-write call, a count question, a retain, and a release no hook
//!   can observe are quiet: each leaves every value some other name holds
//!   alive.
//!
//! An argument a loud call must not be lent unretained ([`HeapLends`]) is a
//! counted value reached through a class field or a dereference, or an entry
//! a runtime getter borrowed from a collection reached that way or held by a
//! shared binding. A receiver the callee writes in place — stores through it,
//! or hands it to a runtime writer or uniqueness check — as a value its caller
//! copies on write is exempt: the caller made it unique for the call.
//!
//! A call to a function of the program is loud when its callee may be: a body
//! is loud when any operation it runs is, which the least fixpoint over the
//! whole program decides, starting from every body quiet so a recursive cycle
//! of quiet bodies stays quiet. Bodies are told apart by their link name, so
//! the program before reference counting, keyed by symbol, and after it,
//! keyed by the name each body is emitted under, are read alike.

use crate::ast::types::{Type, TypeKind};
use crate::mir::analysis::element_matching::{
    element_matching, matched_element, Matching, Question,
};
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::type_facts::{DropHookReach, TypeDefinition};
use crate::mir::{ArgMode, BasicBlock, Body, Local, Operand, Place, PlaceElem, StatementKind};
use crate::mir::{Terminator, TerminatorKind};
use crate::runtime_fns::{
    closure_positions, hands_back_a_borrow, matches_its_elements, orders_its_elements,
    reads_reference_count, writes_in_place,
};
use std::collections::{HashMap, HashSet};

/// Whether a program is read before reference counting inserts its retains
/// and releases, or after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Releases are implicit: the end of a local's storage and a rebinding.
    Lowered,
    /// Every release is an explicit `DecRef`.
    Counted,
}

/// What running one terminator does to the heap other names hold.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Noise<'t> {
    /// Releases nothing another name holds.
    Quiet,
    /// May release something another name holds.
    Loud,
    /// Calls the function of the program `symbol` names, handed `args`: as
    /// loud as its body, and as the code matching any container it is handed
    /// runs when that body matches elements of a type it leaves open.
    Calls(&'t Symbol, &'t [Operand]),
    /// Runs the code matching or ordering a container's elements runs.
    Matches(Matching),
}

/// Which bodies of a program may release a value their callers lend them.
#[derive(Debug)]
pub struct HeapEffects {
    by_link_name: HashMap<String, Vec<usize>>,
    loud: Vec<bool>,
    /// Whether each body may match or order elements of a type it leaves
    /// open, so a call to it runs the code its caller's element type does.
    matches_open: Vec<bool>,
    /// Whether each body writes its receiver in place as a value its caller
    /// copies on write, and so made unique for the call.
    writes_unique_receiver: Vec<bool>,
}

/// The calls of one body, by the block each ends: those that may release
/// nothing their caller lends them, and those handing their callee a
/// receiver it writes in place.
#[derive(Debug, Default)]
pub struct CallFacts {
    pub quiet: HashSet<BasicBlock>,
    pub unique_receivers: HashSet<BasicBlock>,
}

impl HeapEffects {
    /// The effects of every body of `program`, read in `phase`; `link_name`
    /// names a body the way a call to it names its callee.
    pub fn of<N>(
        program: &[(N, Body)],
        link_name: impl Fn(&N) -> String,
        phase: Phase,
        hooks: &DropHookReach,
    ) -> Self {
        let mut by_link_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, (name, _)) in program.iter().enumerate() {
            by_link_name.entry(link_name(name)).or_default().push(index);
        }
        let summaries: Vec<BodySummary> = program
            .iter()
            .map(|(_, body)| summarize(body, phase, hooks, &by_link_name))
            .collect();
        let matches_open = open_matchers(&summaries);
        let loud = loud_bodies(&summaries, &matches_open, &by_link_name);
        HeapEffects {
            by_link_name,
            loud,
            matches_open,
            writes_unique_receiver: program
                .iter()
                .map(|(_, body)| writes_unique_receiver(body))
                .collect(),
        }
    }

    /// Whether a call to `symbol` reaches a body of the program and no body
    /// lowered under it may release what its caller lends it, whatever it is
    /// handed.
    pub fn is_quiet_callee(&self, symbol: &Symbol) -> bool {
        self.bodies_of(symbol)
            .is_some_and(|bodies| self.are_quiet(bodies) && !self.match_open(bodies))
    }

    /// The quiet calls of `body` and its calls handing a receiver over
    /// unique, read in `phase`.
    pub fn call_facts(&self, body: &Body, phase: Phase, hooks: &DropHookReach) -> CallFacts {
        let classifier = Classifier::of(body, phase, hooks);
        let mut facts = CallFacts::default();
        for (index, block) in body.basic_blocks.iter().enumerate() {
            let Some(terminator) = &block.terminator else {
                continue;
            };
            let TerminatorKind::Call { args, .. } = &terminator.kind else {
                continue;
            };
            let Some(bodies) = called_program_symbol(terminator).and_then(|s| self.bodies_of(s))
            else {
                continue;
            };
            let arguments_are_quiet = !self.match_open(bodies) || {
                let matching = classifier.argument_matching(args);
                !matching.runs_unknown_code
                    && !matching.names_open_parameter
                    && self.callbacks_are_quiet(&matching.callbacks)
            };
            if self.are_quiet(bodies) && arguments_are_quiet {
                facts.quiet.insert(BasicBlock(index));
            }
            if self.write_unique_receiver(bodies) {
                facts.unique_receivers.insert(BasicBlock(index));
            }
        }
        facts
    }

    fn are_quiet(&self, bodies: &[usize]) -> bool {
        bodies.iter().all(|&index| !self.loud[index])
    }

    fn match_open(&self, bodies: &[usize]) -> bool {
        bodies.iter().any(|&index| self.matches_open[index])
    }

    fn write_unique_receiver(&self, bodies: &[usize]) -> bool {
        bodies
            .iter()
            .any(|&index| self.writes_unique_receiver[index])
    }

    /// Whether every callback is a body of the program, and a quiet one.
    fn callbacks_are_quiet(&self, callbacks: &[Symbol]) -> bool {
        callbacks.iter().all(|symbol| {
            self.bodies_of(symbol)
                .is_some_and(|bodies| self.are_quiet(bodies))
        })
    }

    fn bodies_of(&self, symbol: &Symbol) -> Option<&[usize]> {
        self.by_link_name
            .get(&symbol.link_name())
            .map(Vec::as_slice)
    }
}

/// Record, on every body of `program`, the calls to a quiet callee and the
/// calls handing their callee a receiver it writes in place, keyed by the
/// block each call ends. Read before reference counting, which funds every
/// lent argument read through a class field that a loud call is handed.
pub fn stamp_quiet_calls(program: &mut [(Symbol, Body)], hooks: &DropHookReach) {
    let effects = HeapEffects::of(program, Symbol::link_name, Phase::Lowered, hooks);
    for (_, body) in program.iter_mut() {
        let facts = effects.call_facts(body, Phase::Lowered, hooks);
        body.quiet_calls = facts.quiet;
        body.unique_receiver_calls = facts.unique_receivers;
    }
}

/// The function of the program `terminator` calls directly, if any.
pub fn called_program_symbol(terminator: &Terminator) -> Option<&Symbol> {
    let TerminatorKind::Call { func, .. } = &terminator.kind else {
        return None;
    };
    func.called_symbol().filter(|symbol| !symbol.is_runtime())
}

/// One call of a body to bodies of the program.
struct CallSite {
    callees: Vec<usize>,
    /// The code matching the elements of the containers it hands over runs.
    arguments: Matching,
}

/// What one body does on its own, and the bodies it calls.
struct BodySummary {
    is_loud: bool,
    /// Whether the body itself matches or orders elements of a type it
    /// leaves open.
    matches_open: bool,
    sites: Vec<CallSite>,
    /// The bodies the body's own matching of elements calls.
    callbacks: Vec<Symbol>,
}

/// The summary of `body`: loud when an operation of its own is, or when it
/// calls a symbol no body of the program is lowered under.
fn summarize(
    body: &Body,
    phase: Phase,
    hooks: &DropHookReach,
    by_link_name: &HashMap<String, Vec<usize>>,
) -> BodySummary {
    let mut summary = BodySummary {
        is_loud: body.is_gpu(),
        matches_open: false,
        sites: Vec::new(),
        callbacks: Vec::new(),
    };
    if body.is_gpu() {
        return summary;
    }
    let classifier = Classifier::of(body, phase, hooks);
    for block in &body.basic_blocks {
        summary.is_loud |= (block.statements.iter()).any(|s| classifier.is_loud_statement(&s.kind));
        let noise = (block.terminator.as_ref()).map_or(Noise::Quiet, |t| classifier.noise(t));
        match noise {
            Noise::Quiet => {}
            Noise::Loud => summary.is_loud = true,
            Noise::Calls(symbol, args) => match by_link_name.get(&symbol.link_name()) {
                Some(bodies) => summary.sites.push(CallSite {
                    callees: bodies.clone(),
                    arguments: classifier.argument_matching(args),
                }),
                None => summary.is_loud = true,
            },
            Noise::Matches(matching) => {
                summary.is_loud |= matching.runs_unknown_code;
                summary.matches_open |= matching.names_open_parameter;
                summary.callbacks.extend(matching.callbacks);
            }
        }
    }
    summary
}

/// Which bodies may match or order elements of a type they leave open:
/// those that do so themselves, and those handing a container of such a
/// type to one that does.
fn open_matchers(summaries: &[BodySummary]) -> Vec<bool> {
    let edges: Vec<Vec<usize>> = summaries
        .iter()
        .map(|summary| {
            (summary.sites.iter())
                .filter(|site| site.arguments.names_open_parameter)
                .flat_map(|site| site.callees.iter().copied())
                .collect()
        })
        .collect();
    let initial = summaries.iter().map(|s| s.matches_open).collect();
    propagate_to_callers(initial, &edges)
}

/// The least fixpoint: a body is loud when it is on its own, calls a loud
/// body, or hands a body matching elements of a type it leaves open a
/// container whose matching runs unknown code or a loud callback.
fn loud_bodies(
    summaries: &[BodySummary],
    matches_open: &[bool],
    by_link_name: &HashMap<String, Vec<usize>>,
) -> Vec<bool> {
    let mut initial: Vec<bool> = summaries.iter().map(|s| s.is_loud).collect();
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); summaries.len()];
    for (caller, summary) in summaries.iter().enumerate() {
        let mut callbacks: Vec<&Symbol> = summary.callbacks.iter().collect();
        for site in &summary.sites {
            edges[caller].extend(&site.callees);
            if site.callees.iter().any(|&callee| matches_open[callee]) {
                initial[caller] |= site.arguments.runs_unknown_code;
                callbacks.extend(&site.arguments.callbacks);
            }
        }
        for symbol in callbacks {
            match by_link_name.get(&symbol.link_name()) {
                Some(bodies) => edges[caller].extend(bodies),
                None => initial[caller] = true,
            }
        }
    }
    propagate_to_callers(initial, &edges)
}

/// `initial`, together with every body from which `edges` (each body's
/// callees) reach a body it holds. Propagated from each such body to its
/// callers, so a cycle reaching none stays out.
fn propagate_to_callers(initial: Vec<bool>, edges: &[Vec<usize>]) -> Vec<bool> {
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); edges.len()];
    for (caller, callees) in edges.iter().enumerate() {
        for &callee in callees {
            callers[callee].push(caller);
        }
    }
    let mut reached = initial;
    let mut pending: Vec<usize> = (0..reached.len()).filter(|&i| reached[i]).collect();
    while let Some(callee) = pending.pop() {
        for &caller in &callers[callee] {
            if !reached[caller] {
                reached[caller] = true;
                pending.push(caller);
            }
        }
    }
    reached
}

/// Whether `body` is a method writing its receiver in place as a value its
/// callers copy on write.
fn writes_unique_receiver(body: &Body) -> bool {
    let receiver = Local(1);
    body.receiver == Some(receiver)
        && body
            .local_decls
            .get(receiver.0)
            .is_some_and(|decl| is_copied_on_write(body, &decl.ty))
        && writes_in_place_through(body, receiver)
}

// TODO: a receiver exempted here is lent with no reference of its own, so a
// callee that writes it in place and also runs code of the program — a
// list's `remove` matching through an element's `equals`, a set's `add` or
// `remove`, a map's `set` hashing a key — can run code that replaces the
// field the receiver was read from and frees the collection being written.
// Retaining the receiver for such a call does not help: the callee would
// find it shared, copy it on write, and write a copy the field no longer
// holds. Closing this needs the caller to keep the field's collection alive
// some other way across a writing call that reaches program code.
/// Whether `body` writes what `local` holds in place: stores into it or
/// through it, or hands it, whole or through a projection, to a runtime entry
/// point that writes it in place or asks whether it is unique — the check a
/// method makes before writing its receiver through another method.
///
/// A method that only reads its receiver, binds it or hands it to a closure
/// does not: what it binds takes a reference of its own, so a write through
/// that copies first.
fn writes_in_place_through(body: &Body, local: Local) -> bool {
    let stores = (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter_map(crate::mir::Statement::written_place)
        .any(|place| place.local == local);
    stores
        || (body.basic_blocks.iter())
            .filter_map(|block| block.terminator.as_ref())
            .any(|terminator| hands_to_runtime_writer(terminator, local))
}

/// Whether `terminator` hands `local`, whole or through a projection, to a
/// runtime entry point that writes it in place or asks whether it is unique.
fn hands_to_runtime_writer(terminator: &Terminator, local: Local) -> bool {
    let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
        return false;
    };
    let Some(name) = func.called_runtime_name() else {
        return false;
    };
    args.iter().enumerate().any(|(position, arg)| {
        arg.place().is_some_and(|place| place.local == local)
            && (writes_in_place(name, position) || reads_reference_count(name))
    })
}

/// The per-operation rule, for one body.
struct Classifier<'b> {
    body: &'b Body,
    types: PlaceTypes<'b>,
    hooks: &'b DropHookReach<'b>,
    phase: Phase,
}

impl<'b> Classifier<'b> {
    fn of(body: &'b Body, phase: Phase, hooks: &'b DropHookReach<'b>) -> Self {
        Classifier {
            body,
            types: PlaceTypes::of(body),
            hooks,
            phase,
        }
    }

    /// Whether the statement `kind` may release a value another name holds.
    fn is_loud_statement(&self, kind: &StatementKind) -> bool {
        match kind {
            StatementKind::Assign(dest, _) => self.is_shared_store(dest),
            StatementKind::Reassign(dest, _) => {
                self.is_shared_store(dest)
                    || (self.phase == Phase::Lowered && self.may_run_drop_hook(dest))
            }
            StatementKind::StorageDead(place) => {
                self.phase == Phase::Lowered
                    && self.may_release(place.local)
                    && self.may_run_drop_hook(place)
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
                self.may_run_drop_hook(place)
            }
            StatementKind::StorageLive(_) | StatementKind::IncRef(_) | StatementKind::Nop => false,
        }
    }

    /// What running `terminator` does to the heap other names hold.
    fn noise<'t>(&self, terminator: &'t Terminator) -> Noise<'t> {
        match &terminator.kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => Noise::Quiet,
            TerminatorKind::Call { func, args, .. } => self.call_noise(func, args),
            TerminatorKind::VirtualCall { .. } | TerminatorKind::GpuLaunch { .. } => Noise::Loud,
        }
    }

    fn call_noise<'t>(&self, func: &'t Operand, args: &'t [Operand]) -> Noise<'t> {
        let Some(symbol) = func.called_symbol() else {
            return Noise::Loud;
        };
        let Some(name) = symbol.runtime_name() else {
            return Noise::Calls(symbol, args);
        };
        if self.is_loud_runtime_call(name, args) {
            return Noise::Loud;
        }
        let question = if orders_its_elements(name) {
            Question::Orders
        } else if matches_its_elements(name) {
            Question::Matches
        } else {
            return Noise::Quiet;
        };
        let matching = self.container_matching(args.first(), question);
        if matching.is_by_value() {
            Noise::Quiet
        } else {
            Noise::Matches(matching)
        }
    }

    /// The code the runtime runs answering `question` about the elements of
    /// `container`; unknown code where its element type cannot be read.
    fn container_matching(&self, container: Option<&Operand>, question: Question) -> Matching {
        let declared = container
            .and_then(Operand::place)
            .and_then(|place| self.types.declared(place));
        let definitions = self.types.definitions;
        match declared.and_then(|ty| matched_element(&ty, definitions)) {
            Some(element) => element_matching(&element, &self.types, question),
            None => Matching {
                runs_unknown_code: true,
                ..Matching::default()
            },
        }
    }

    /// The code matching or ordering the elements of every container among
    /// `args` runs: what a callee handed them runs when it matches elements
    /// of a type it leaves open.
    /// An argument whose type cannot be read may be such a container, and
    /// is taken to run unknown code.
    fn argument_matching(&self, args: &[Operand]) -> Matching {
        let mut matching = Matching::default();
        for place in args.iter().filter_map(Operand::place) {
            let Some(declared) = self.types.declared(place) else {
                matching.runs_unknown_code = true;
                continue;
            };
            let Some(element) = matched_element(&declared, self.types.definitions) else {
                continue;
            };
            for question in [Question::Matches, Question::Orders] {
                matching.merge(element_matching(&element, &self.types, question));
            }
        }
        matching
    }

    /// Whether the runtime entry point `name`, handed `args`, may release a
    /// value another name holds.
    fn is_loud_runtime_call(&self, name: &str, args: &[Operand]) -> bool {
        if !closure_positions(name).is_empty() {
            return true;
        }
        args.iter().enumerate().any(|(position, arg)| {
            let Some(place) = arg.place() else {
                return false;
            };
            writes_in_place(name, position)
                && !self.holds_no_counted_contents(place)
                && (!self.is_own_whole_local(place) || self.may_run_drop_hook(place))
        })
    }

    /// Whether what `place` holds has no contents a write in place could
    /// release: a value with no count, a string, or an instance of a class
    /// with no fields and no type arguments that is not copied on write and
    /// whose release runs no drop hook.
    ///
    /// A runtime entry point keeps no reference to what it writes in place
    /// and leaves its count as it found it (`runtime_fns::writes_in_place`),
    /// so all it can release is what the value holds.
    fn holds_no_counted_contents(&self, place: &Place) -> bool {
        let Some(ty) = self.types.declared(place) else {
            return false;
        };
        if !self.body.is_counted(&ty.kind) || matches!(ty.kind, TypeKind::String) {
            return true;
        }
        let TypeKind::Custom(name, args) = &ty.kind else {
            return false;
        };
        let Some(TypeDefinition::Class(class)) = self.types.definitions.get(name) else {
            return false;
        };
        class.fields.is_empty()
            && class.base_class.is_none()
            && args.as_ref().is_none_or(Vec::is_empty)
            && !is_copied_on_write(self.body, &ty)
            && !self.hooks.may_run_drop_hook(&ty)
    }

    /// Whether `place` is a whole counted local the body owns: no parameter
    /// it borrows, no capture and no borrow of another local's value.
    fn is_own_whole_local(&self, place: &Place) -> bool {
        let local = place.local;
        place.projection.is_empty()
            && self.may_release(local)
            && !self.body.env_capture_locals.contains(&local)
            && !self.body.borrowed_locals.contains_key(&local)
            && self.body.local_decls.get(local.0).is_some()
            && self.body.is_counted_local(local)
    }

    /// Whether the body may release what `local` holds: it is no parameter,
    /// or one the body takes over.
    fn may_release(&self, local: Local) -> bool {
        !(1..=self.body.arg_count).contains(&local.0) || self.body.is_owned_param(local)
    }

    /// Whether a store into `dest` may replace a value another name holds.
    fn is_shared_store(&self, dest: &Place) -> bool {
        crosses_heap_field(&self.types, dest) || self.is_shared_root(dest.local)
    }

    fn is_shared_root(&self, local: Local) -> bool {
        is_shared_root(self.body, local)
    }

    /// Whether releasing what `place` holds may run a drop hook; a place
    /// whose type cannot be read may.
    fn may_run_drop_hook(&self, place: &Place) -> bool {
        self.types
            .declared(place)
            .is_none_or(|ty| self.hooks.may_run_drop_hook(&ty))
    }
}

/// Whether reaching `place` steps through a class field or a dereference:
/// a value held by a place other code may reach and replace, rather than by
/// the root local's own value.
///
/// A field of a struct, a tuple, an optional or an enum's payload is part of
/// the value holding it, which a write copies first when another name holds
/// it. A field of a class instance is shared by every name holding the
/// instance. A field of a value whose type is a type parameter, a trait, a
/// closure or a type that cannot be read is taken to be shared.
pub fn crosses_heap_field(types: &PlaceTypes, place: &Place) -> bool {
    place
        .projection
        .iter()
        .enumerate()
        .any(|(depth, elem)| match elem {
            PlaceElem::Deref => true,
            PlaceElem::Field(_) => {
                let holder = Place {
                    local: place.local,
                    projection: place.projection[..depth].to_vec(),
                };
                is_shared_holder(types.declared(&holder).as_ref(), types.definitions)
            }
            PlaceElem::Index(_) => false,
        })
}

/// Whether a value of type `ty` holds its fields where every name holding it
/// reaches them: a class, or any type whose fields' holder is not known.
fn is_shared_holder(ty: Option<&Type>, definitions: &HashMap<String, TypeDefinition>) -> bool {
    let Some(ty) = ty else {
        return true;
    };
    if let TypeKind::Custom(name, _) = &ty.kind {
        return match definitions.get(name) {
            Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_)) => false,
            Some(
                TypeDefinition::Class(_)
                | TypeDefinition::Trait(_)
                | TypeDefinition::Generic(_)
                | TypeDefinition::Alias(_),
            )
            | None => ty.kind.as_builtin_collection().is_none(),
        };
    }
    matches!(
        ty.kind,
        TypeKind::Generic(..) | TypeKind::Function(_) | TypeKind::Identifier | TypeKind::RawPtr
    )
}

/// Whether `local` is the receiver, a parameter the body takes over, an
/// `out` parameter or a capture of `body`: a binding whose value a caller or
/// a closure's environment still reads after the body writes it.
fn is_shared_root(body: &Body, local: Local) -> bool {
    let is_out = (local.0.checked_sub(1))
        .and_then(|index| body.out_params.get(index))
        .copied()
        .unwrap_or(false);
    body.receiver == Some(local)
        || body.is_owned_param(local)
        || is_out
        || body.env_capture_locals.contains(&local)
}

/// Which arguments one body lends that a call able to release them must not
/// be handed without a reference of their own.
pub struct HeapLends<'b> {
    body: &'b Body,
    types: PlaceTypes<'b>,
    /// The locals holding an entry a runtime getter hands back as a borrow
    /// of a collection other code may reach.
    entry_borrows: HashSet<Local>,
}

impl<'b> HeapLends<'b> {
    pub fn of(body: &'b Body) -> Self {
        let types = PlaceTypes::of(body);
        let entry_borrows = entry_borrows(body, &types);
        HeapLends {
            body,
            types,
            entry_borrows,
        }
    }

    /// The declared type of what `place` reaches.
    pub fn declared(&self, place: &Place) -> Option<Type> {
        self.types.declared(place)
    }

    /// The place of an argument the body lends a call — passed `mode` as
    /// `arg` — that is reference counted and held only by a place other code
    /// may reach and replace: through a class field or a dereference, or as
    /// an entry a runtime getter borrowed from a collection held that way.
    /// `None` for any other argument.
    pub fn lent_place<'a>(&self, mode: ArgMode, arg: &'a Operand) -> Option<&'a Place> {
        if mode != ArgMode::Borrowed {
            return None;
        }
        let place = arg.place()?;
        let is_counted =
            (self.types.declared(place)).is_some_and(|ty| self.body.is_counted(&ty.kind));
        let is_reachable =
            crosses_heap_field(&self.types, place) || self.entry_borrows.contains(&place.local);
        (is_counted && is_reachable).then_some(place)
    }
}

/// The locals of `body` a runtime getter hands an entry to as a borrow of
/// the collection it reads (`runtime_fns::hands_back_a_borrow`), when that
/// collection is reached through a class field or a dereference, or is the
/// receiver, an owned or `out` parameter or a capture.
fn entry_borrows(body: &Body, types: &PlaceTypes) -> HashSet<Local> {
    let scope = body.layout_scope();
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => Some((func.called_runtime_name()?, args, destination)),
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::VirtualCall { .. } => None,
        })
        .filter(|(name, args, destination)| {
            let result = body.local_decls.get(destination.local.0);
            let collection = args.first().and_then(Operand::place);
            destination.projection.is_empty()
                && result.is_some_and(|decl| hands_back_a_borrow(name, &decl.ty.kind, scope))
                && collection.is_some_and(|place| {
                    crosses_heap_field(types, place) || is_shared_root(body, place.local)
                })
        })
        .map(|(_, _, destination)| destination.local)
        .collect()
}
