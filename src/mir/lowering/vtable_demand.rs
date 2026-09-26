// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which vtable slots a program reads, and so which bodies they name.
//!
//! A slot is filled only where a reached body constructs an instance and a
//! reached virtual call reads the slot through a receiver the instance can
//! stand behind. The receiver's type arguments count: a `Foldable<int>` call
//! to `sum` fills the slot of a `Bag<int>`, never of a `Bag<Pt>`, whose `sum`
//! over elements that do not add no well-typed call reaches.
//!
//! Reached means reachable from a body with no type parameter left open —
//! `main`, every body compiled at concrete arguments — or from a symbol codegen
//! names outside any call, through static calls, function values, closures and
//! filled slots. A body shared by every instantiation of a generic declaration
//! builds its instances at open arguments; one nothing reaches must not fill
//! the bare vtable's slots, whose shared bodies cannot be compiled at every
//! type.
//!
//! Every instance's slots name the bodies compiled for its own arguments; a
//! vtable's slot targets are a function of the instance alone, never of which
//! body built it first. An instance past a bound of
//! [`super::instantiation_limits`] — nested too deep, or one value
//! instantiation too many for its class — refuses the program. Each reached
//! body keeps the chain of slots that reached it, which explains a refusal and
//! decides nothing.

use super::dispatch_symbols::{constructed_class, VtableInstance, VtableLayout};
use super::instantiation_argument;
use super::instantiation_limits::{
    constructor_parts, exceeded_limit, has_value_argument, polymorphic_recursion, Growth,
};
use super::is_monomorphizable_type_argument;
use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::token::type_kind_to_mangle_str;
use crate::mir::visitor::Visitor;
use crate::mir::{
    AggregateKind, BasicBlock, BasicBlockData, Body, Constant, Operand, Place, Rvalue,
    TerminatorKind,
};
use crate::type_checker::context::{GenericDefinition, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

/// One filled vtable slot: its number, the method it stands for and the
/// symbol of the body it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilledSlot {
    pub slot: usize,
    pub method: String,
    pub symbol: String,
}

/// The filled slots of every vtable a program's reached bodies build, by
/// vtable symbol: what codegen writes into each vtable it defines.
#[derive(Debug, Default, Clone)]
pub struct VtableFills {
    slots: BTreeMap<String, Vec<FilledSlot>>,
}

/// The filled slot a body was reached through, newest first: `None` for a
/// root. Shared between every body reached along the same chain.
type Lineage = Option<Rc<Step>>;

/// One filled slot on the way to a body: the instance whose vtable held it,
/// the method it stands for, and how the body that built that instance was
/// reached. A body reached as a root that runs at an instance — a method
/// compiled for it before any slot named it — starts its chain at that
/// instance, through no slot.
#[derive(Debug)]
struct Step {
    instance: VtableInstance,
    method: Option<String>,
    outer: Lineage,
}

/// A virtual call's receiver and the slot it reads.
///
/// `receiver` is the trait or abstract class the call dispatches through, and
/// `pins` the spelling of each of its type arguments; `None` where an argument
/// is still open, or `receiver` itself where the receiver's type is unknown,
/// matches every instance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Dispatch {
    receiver: Option<String>,
    pins: Vec<Option<String>>,
    slot: usize,
}

/// What one body constructs, where, and what it dispatches and names, and
/// the instance it runs at when it is a method of one.
#[derive(Debug, Default)]
struct BodyDemand {
    owner: Option<VtableInstance>,
    constructed: Vec<(VtableInstance, Span)>,
    dispatched: Vec<Dispatch>,
    references: Vec<String>,
}

/// One vtable a reached body builds.
#[derive(Debug)]
struct ReachedVtable {
    instance: VtableInstance,
    /// Each type the instance's class stands behind — itself, its bases and
    /// every trait they implement — with the spelling of what the instance
    /// pins its parameters to.
    supertypes: Vec<(String, Vec<Option<String>>)>,
    /// How the body that built the instance was reached.
    lineage: Lineage,
    /// Each slot the instance gives a body that no reached call reads yet.
    unfilled: BTreeMap<usize, (String, String)>,
    filled: Vec<FilledSlot>,
}

/// A body waiting to be reached, with the lineage it is reached with.
type Pending = (String, BodyDemand, Lineage);

/// The type table and slot numbering a demand reads.
#[derive(Clone, Copy)]
pub struct DemandTables<'a> {
    pub type_checker: &'a TypeChecker,
    pub layout: &'a VtableLayout,
}

/// The vtable slots a program reads, gathered body by body as the pipeline
/// lowers them: each [`VtableDemand::record`] adds the bodies lowered since
/// the one before and fills only what they newly reach.
#[derive(Debug, Default)]
pub struct VtableDemand {
    vtables: BTreeMap<String, ReachedVtable>,
    dispatches: BTreeSet<Dispatch>,
    reached: HashSet<String>,
    unreached: HashMap<String, BodyDemand>,
    named: HashMap<String, Lineage>,
    newly_named: Vec<String>,
    /// How many reached vtables each class has at value arguments.
    value_instances: HashMap<String, usize>,
}

impl VtableFills {
    /// The filled slots of the vtable `symbol`, in the order they were
    /// filled; none for a vtable no reached body builds.
    pub fn slots(&self, symbol: &str) -> &[FilledSlot] {
        self.slots.get(symbol).map_or(&[], Vec::as_slice)
    }
}

impl VtableDemand {
    /// A demand whose roots include `symbols`, the bodies codegen calls
    /// outside any MIR call.
    pub fn rooted_at(symbols: impl IntoIterator<Item = String>) -> Self {
        Self {
            named: symbols.into_iter().map(|symbol| (symbol, None)).collect(),
            ..Self::default()
        }
    }

    /// Add `bodies`, each with its symbol, to the demand, filling every slot
    /// they newly reach.
    ///
    /// Fails with [`crate::diagnostics::DiagnosticCode::MirPolymorphicRecursion`]
    /// where a reached body builds an instance past a bound of
    /// [`super::instantiation_limits`].
    pub fn record<'b>(
        &mut self,
        bodies: impl IntoIterator<Item = (&'b str, &'b Body)>,
        tables: DemandTables,
    ) -> Result<(), LoweringError> {
        let type_defs = tables.type_checker.type_definitions();
        for (symbol, body) in bodies {
            let mut demand = BodyDemand::of(body, tables);
            let lineage = match self.named.remove(symbol) {
                Some(lineage) => lineage,
                None if leaves_a_parameter_open(body, type_defs) => {
                    self.unreached.insert(symbol.to_string(), demand);
                    continue;
                }
                None => demand.owner.take().map(|instance| {
                    Rc::new(Step {
                        instance,
                        method: None,
                        outer: None,
                    })
                }),
            };
            self.reach((symbol.to_string(), demand, lineage), tables)?;
        }
        Ok(())
    }

    /// The symbols filled slots have named since the last call.
    pub fn take_named_slot_symbols(&mut self) -> Vec<String> {
        std::mem::take(&mut self.newly_named)
    }

    /// The symbol of every vtable a reached body builds, in order.
    pub fn vtable_symbols(&self) -> impl Iterator<Item = &str> {
        self.vtables.keys().map(String::as_str)
    }

    /// The filled slots of every vtable a reached body builds.
    pub fn fills(&self) -> VtableFills {
        VtableFills {
            slots: self
                .vtables
                .iter()
                .map(|(symbol, vtable)| (symbol.clone(), vtable.filled.clone()))
                .collect(),
        }
    }

    /// Reach `first` and everything it reaches in turn.
    fn reach(&mut self, first: Pending, tables: DemandTables) -> Result<(), LoweringError> {
        let mut pending = vec![first];
        while let Some((symbol, demand, lineage)) = pending.pop() {
            if !self.reached.insert(symbol) {
                continue;
            }
            for name in demand.references {
                self.name(name, &lineage, &mut pending);
            }
            for (instance, span) in demand.constructed {
                self.add_vtable(instance, span, &lineage, tables, &mut pending)?;
            }
            for dispatch in demand.dispatched {
                self.add_dispatch(dispatch, &mut pending);
            }
        }
        Ok(())
    }

    /// Mark `symbol` as named by a body reached through `lineage`: reach it
    /// now if it is recorded, else when it is.
    fn name(&mut self, symbol: String, lineage: &Lineage, pending: &mut Vec<Pending>) {
        if self.reached.contains(&symbol) {
            return;
        }
        match self.unreached.remove(&symbol) {
            Some(demand) => pending.push((symbol, demand, lineage.clone())),
            None => {
                self.named.entry(symbol).or_insert_with(|| lineage.clone());
            }
        }
    }

    /// Add the vtable `instance` points at, built at `span` by a body reached
    /// through `lineage`, and fill each of its slots a reached call already
    /// reads. Refuses an instance past a bound of
    /// [`super::instantiation_limits`].
    fn add_vtable(
        &mut self,
        instance: VtableInstance,
        span: Span,
        lineage: &Lineage,
        tables: DemandTables,
        pending: &mut Vec<Pending>,
    ) -> Result<(), LoweringError> {
        let symbol = instance.symbol();
        if self.vtables.contains_key(&symbol) {
            return Ok(());
        }
        self.admit(&instance, span, lineage, tables)?;
        let vtable = ReachedVtable::of(instance, lineage, tables);
        let read: Vec<usize> = self
            .dispatches
            .iter()
            .filter(|dispatch| vtable.answers(dispatch))
            .map(|dispatch| dispatch.slot)
            .collect();
        self.vtables.insert(symbol.clone(), vtable);
        for slot in read {
            self.fill(&symbol, slot, pending);
        }
        Ok(())
    }

    /// Count `instance` among its class's instances, refusing it where it
    /// passes a bound of [`super::instantiation_limits`].
    fn admit(
        &mut self,
        instance: &VtableInstance,
        span: Span,
        lineage: &Lineage,
        tables: DemandTables,
    ) -> Result<(), LoweringError> {
        let value_instances = if has_value_argument(instance.args()) {
            let count = self
                .value_instances
                .entry(instance.class().to_string())
                .or_default();
            *count += 1;
            *count
        } else {
            0
        };
        let type_defs = tables.type_checker.type_definitions();
        match exceeded_limit(
            instance.class(),
            instance.args(),
            value_instances,
            type_defs,
        ) {
            Some(limit) => Err(polymorphic_recursion(
                instance.class(),
                instance.args(),
                &limit,
                &growth_along(instance.class(), lineage),
                span,
            )),
            None => Ok(()),
        }
    }

    /// Add a reached virtual call and fill the slot it reads in every vtable
    /// that answers it.
    fn add_dispatch(&mut self, dispatch: Dispatch, pending: &mut Vec<Pending>) {
        if self.dispatches.contains(&dispatch) {
            return;
        }
        let answering: Vec<String> = self
            .vtables
            .iter()
            .filter(|(_, vtable)| vtable.answers(&dispatch))
            .map(|(symbol, _)| symbol.clone())
            .collect();
        let slot = dispatch.slot;
        self.dispatches.insert(dispatch);
        for symbol in answering {
            self.fill(&symbol, slot, pending);
        }
    }

    /// Fill `slot` of the vtable `symbol` when its instance gives it a body,
    /// naming that body.
    fn fill(&mut self, symbol: &str, slot: usize, pending: &mut Vec<Pending>) {
        let Some(vtable) = self.vtables.get_mut(symbol) else {
            return;
        };
        let Some((method, target)) = vtable.unfilled.remove(&slot) else {
            return;
        };
        let lineage = Some(Rc::new(Step {
            instance: vtable.instance.clone(),
            method: Some(method.clone()),
            outer: vtable.lineage.clone(),
        }));
        vtable.filled.push(FilledSlot {
            slot,
            method,
            symbol: target.clone(),
        });
        self.newly_named.push(target.clone());
        self.name(target, &lineage, pending);
    }
}

impl ReachedVtable {
    /// The vtable `instance` points at, built by a body reached through
    /// `lineage`. Its slots name the bodies compiled for the instance's own
    /// arguments, however it was reached.
    fn of(instance: VtableInstance, lineage: &Lineage, tables: DemandTables) -> Self {
        let type_defs = tables.type_checker.type_definitions();
        let unfilled = instance
            .slot_targets(type_defs)
            .into_iter()
            .filter_map(|(method, target)| {
                Some((tables.layout.slot(method)?, (method.to_string(), target?)))
            })
            .collect();
        let supertypes = supertype_pins(&instance, tables.type_checker);
        Self {
            instance,
            supertypes,
            lineage: lineage.clone(),
            unfilled,
            filled: Vec::new(),
        }
    }

    /// Whether a call through `dispatch`'s receiver can reach this instance:
    /// its class stands behind the receiver, and every argument the receiver
    /// spells agrees with what the instance pins it to.
    fn answers(&self, dispatch: &Dispatch) -> bool {
        let Some(receiver) = &dispatch.receiver else {
            return true;
        };
        self.supertypes
            .iter()
            .find(|(name, _)| name == receiver)
            .is_some_and(|(_, pins)| pins_agree(pins, &dispatch.pins))
    }
}

/// Whether two spellings of one type's arguments can name one instantiation:
/// every argument both spell is spelled alike. Counts that disagree compare
/// nothing and so agree.
fn pins_agree(left: &[Option<String>], right: &[Option<String>]) -> bool {
    left.len() != right.len()
        || left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (Some(left), Some(right)) => left == right,
                (None, _) | (_, None) => true,
            })
}

/// Each type `instance`'s class stands behind, the class itself first, with
/// the spelling of the arguments the instance reaches it at.
fn supertype_pins(
    instance: &VtableInstance,
    type_checker: &TypeChecker,
) -> Vec<(String, Vec<Option<String>>)> {
    let type_defs = type_checker.type_definitions();
    let params = declared_parameters(type_defs.get(instance.class()));
    let substitution: HashMap<String, Type> = params
        .iter()
        .map(|param| param.name.clone())
        .zip(instance.args().iter().cloned())
        .collect();
    let own = (
        instance.class().to_string(),
        instance
            .args()
            .iter()
            .map(|arg| spelled_argument(arg, type_defs))
            .collect(),
    );
    let above = type_checker
        .declaring_types_above(instance.class(), &substitution)
        .into_iter()
        .map(|(name, pinned)| {
            let spelled = declared_parameters(type_defs.get(&name))
                .iter()
                .map(|param| {
                    pinned
                        .get(&param.name)
                        .and_then(|arg| spelled_argument(arg, type_defs))
                })
                .collect();
            (name, spelled)
        });
    std::iter::once(own).chain(above).collect()
}

/// The type parameters `definition` declares, none for one declaring none.
fn declared_parameters(definition: Option<&TypeDefinition>) -> &[GenericDefinition] {
    match definition {
        Some(TypeDefinition::Trait(trait_def)) => trait_def.generics.as_deref().unwrap_or_default(),
        Some(
            definition @ (TypeDefinition::Class(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_)),
        ) => definition.generics().unwrap_or_default(),
        None => &[],
    }
}

/// The mangled spelling of `arg`, or `None` when it names no instantiation —
/// a parameter still open, or a type with no per-instantiation body.
fn spelled_argument(arg: &Type, type_defs: &HashMap<String, TypeDefinition>) -> Option<String> {
    is_monomorphizable_type_argument(&arg.kind, type_defs)
        .then(|| type_kind_to_mangle_str(&arg.kind).into_owned())
}

impl BodyDemand {
    /// What `body` constructs, dispatches and names.
    fn of(body: &Body, tables: DemandTables) -> Self {
        let owner = (body.arg_count > 0)
            .then(|| body.local_decls.get(1))
            .flatten()
            .and_then(|decl| VtableInstance::of(&decl.ty, tables.type_checker.type_definitions()));
        let mut collector = DemandCollector {
            body,
            tables,
            span: body.span,
            demand: Self {
                owner,
                ..Self::default()
            },
        };
        collector.visit_body(body);
        collector.demand
    }
}

/// The visitor [`BodyDemand::of`] reads a body with.
struct DemandCollector<'a> {
    body: &'a Body,
    tables: DemandTables<'a>,
    /// The span of the statement being read.
    span: Span,
    demand: BodyDemand,
}

impl Visitor for DemandCollector<'_> {
    fn visit_basic_block(&mut self, block: BasicBlock, data: &BasicBlockData) {
        for statement in &data.statements {
            self.span = statement.span;
            self.visit_statement(block, statement);
        }
        if let Some(terminator) = &data.terminator {
            if let TerminatorKind::VirtualCall {
                vtable_slot, args, ..
            } = &terminator.kind
            {
                let dispatch = dispatch_through(args.first(), *vtable_slot, self.body, self.tables);
                self.demand.dispatched.push(dispatch);
            }
            self.visit_terminator(block, terminator);
        }
    }

    fn visit_assign(&mut self, block: BasicBlock, _place: &Place, rvalue: &Rvalue) {
        let type_defs = self.tables.type_checker.type_definitions();
        if let Some(instance) =
            constructed_class(rvalue).and_then(|ty| VtableInstance::of(ty, type_defs))
        {
            self.demand.constructed.push((instance, self.span));
        }
        if let Rvalue::Aggregate(AggregateKind::Closure(name, _), _) = rvalue {
            self.demand.references.push(name.to_string());
        }
        self.visit_rvalue(rvalue, block);
    }

    fn visit_constant(&mut self, constant: &Constant, _location: BasicBlock) {
        if let Literal::Identifier(name) = &constant.literal {
            self.demand.references.push(name.clone());
        }
    }
}

/// The dispatch a virtual call reading `slot` through `receiver` makes.
///
/// Only a receiver read straight out of a local has a type to read; any other
/// is taken to reach every instance.
fn dispatch_through(
    receiver: Option<&Operand>,
    slot: usize,
    body: &Body,
    tables: DemandTables,
) -> Dispatch {
    let receiver_ty = receiver.and_then(|operand| match operand {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            Some(operand.ty(body))
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => None,
    });
    let Some(TypeKind::Custom(name, args)) = receiver_ty.map(|ty| &ty.kind) else {
        return Dispatch {
            receiver: None,
            pins: Vec::new(),
            slot,
        };
    };
    let type_defs = tables.type_checker.type_definitions();
    let pins = args
        .iter()
        .flatten()
        .map(|arg| instantiation_argument(arg).and_then(|ty| spelled_argument(&ty, type_defs)))
        .collect();
    Dispatch {
        receiver: Some(name.clone()),
        pins,
        slot,
    }
}

/// Whether `body` still names a type parameter in some local's type: it is
/// the shared body of a generic declaration rather than one compiled at
/// concrete arguments.
fn leaves_a_parameter_open(body: &Body, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    body.local_decls
        .iter()
        .any(|decl| names_a_parameter(&decl.ty, body, type_defs))
}

/// Whether `ty` names a type parameter of `body`: a bare name the type table
/// does not define, or a generic placeholder for one of the body's own
/// parameters. `Self`, which a struct method spells its explicit `self` with,
/// names the type the method belongs to, never a parameter.
///
/// A closure lowered inside a shared body records no parameters of its own
/// but spells its enclosing body's by bare name. A placeholder for a parameter
/// the body does not declare is left over from a callee's signature — a
/// generic function whose parameter appears only in its return type — and
/// says nothing about the body itself.
fn names_a_parameter(ty: &Type, body: &Body, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    if let TypeKind::Generic(name, _, _) = &ty.kind {
        return body.type_params.contains(name.as_str());
    }
    if let TypeKind::Custom(name, None) = &ty.kind {
        return name != SELF_TYPE_NAME && !type_defs.contains_key(name.as_str());
    }
    constructor_parts(ty)
        .1
        .iter()
        .any(|part| names_a_parameter(part, body, type_defs))
}

/// How `lineage` reached a body building an instance of `class`: the
/// instances of `class` whose slots it passed through, outermost first, and
/// the method of the last of them.
fn growth_along<'l>(class: &str, lineage: &'l Lineage) -> Growth<'l> {
    let mut steps = Vec::new();
    let mut current = lineage.as_deref();
    while let Some(step) = current {
        if step.instance.class() == class {
            steps.push(step);
        }
        current = step.outer.as_deref();
    }
    Growth {
        method: steps.first().and_then(|step| step.method.as_deref()),
        chain: steps
            .iter()
            .rev()
            .map(|step| step.instance.args())
            .collect(),
        through_trait: true,
    }
}
