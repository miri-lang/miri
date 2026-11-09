// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which operands lowering reads into a temporary the moment they are
//! evaluated.
//!
//! Operands are evaluated left to right, and each yields its value at the
//! moment it is evaluated. An operand that names a place — a binding, or a
//! field or an element reached from one — is lowered as that place, and the
//! place is read where the expression that evaluates it runs: a call after
//! every argument, an operator after its right side. A later operand that
//! writes the place in between would change the earlier value. Reading every
//! such place into a temporary would cost a copy in every call, so lowering
//! reads only the operands held here (`mir::lowering::sequencing`): a place
//! some later operand of the same sequence may write.
//!
//! A later operand may write an earlier one's place when it
//! - hands a binding the place is read through — its root, or a binding an
//!   index along it names — to an `out` parameter, of the same call or of a
//!   call inside it,
//! - assigns such a binding, or through it,
//! - calls a method that writes such a binding's collection receiver, or
//!   hands the binding to a runtime entry point that writes it in place, the
//!   writes [`super::ParameterWrites`] reads off a body, or
//! - runs a call other than an intrinsic or a variant constructor, an
//!   operator a class answers, or an assignment of any kind, when the place
//!   is reached through a value that may be shared by reference
//!   ([`crate::type_checker::reference_sharing`]): code that holds another
//!   reference to the object can write it. Such a place need not start at a
//!   binding: a field of a call's result, or of a conditional's, is one too.
//!
//! A call writes the operands it is handed to write when it runs, after
//! every operand is evaluated, so such a write reaches each other operand of
//! the same call, written before it or after it: `bump(a, a)` with
//! `bump(xs out [int], ys [int])` holds the second `a`.
//!
//! What a later operand runs in a closure it builds is not followed, since
//! a closure captures the values it reads; nor is a global a called function
//! writes, since only what the operands themselves name is followed.
//!
//! An operand the call it is passed to writes — an `out` argument, an
//! argument a runtime entry point writes in place, or the receiver of a
//! method that writes it — is the place itself and is never held. Nor is a
//! step of a path a call writes (a writing method's receiver, an argument a
//! runtime entry point writes in place) a sequence of its own: holding a
//! value along it would redirect the write into a copy. A list,
//! set or map literal stores each element as it is evaluated, so it holds
//! nothing either.
//!
//! Each body is walked once. The walk lays every expression's records out
//! as a run of positions, so whether a run of later operands writes a
//! binding, runs code or assigns is a search in a list sorted by position:
//! every sequence costs its own operands, however deeply it nests.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::parameters::{is_written, writes_argument, writes_receiver};
use super::{
    argument_value, binding_name, path_root, unlabelled, BodyWrites, Extent, Mention, Recorded,
    Role,
};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::Statement;
use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::type_checker::reference_sharing::{sharing, Sharing};
use crate::type_checker::{CalleeKind, TypeChecker};

/// The operands of every body of a program that lowering reads into a
/// temporary as they are evaluated.
#[derive(Debug, Default)]
pub struct HeldOperands {
    /// The id of each held operand, any named-argument label taken off.
    held: HashSet<usize>,
    /// The held operands of each function and closure body, by the id of
    /// the body's statement, outside any closure or function it declares.
    by_body: HashMap<usize, Vec<HeldOperand>>,
}

/// An operand lowering reads into a temporary as it is evaluated.
#[derive(Debug, Clone, Copy)]
pub struct HeldOperand {
    /// The operand's id, any named-argument label taken off.
    pub id: usize,
    pub span: Span,
}

impl HeldOperands {
    /// Whether lowering reads `operand` into a temporary the moment it is
    /// evaluated, because a later operand of the same sequence may write it.
    pub fn holds(&self, operand: &Expression) -> bool {
        !self.held.is_empty() && self.held.contains(&unlabelled(operand).id)
    }

    /// The operands of the function or closure body `body` that are held,
    /// outside any closure or function it declares.
    pub fn held_in(&self, body: &Statement) -> &[HeldOperand] {
        self.by_body.get(&body.id).map_or(&[], Vec::as_slice)
    }

    /// Records the held operands of the body `writes` walked.
    pub(super) fn record(&mut self, writes: &BodyWrites, checker: &TypeChecker) {
        if writes.sequences.is_empty() {
            return;
        }
        let index = WriteIndex::of(writes, checker);
        for sequence in &writes.sequences {
            if !index.written_steps.contains(&address(sequence.expression)) {
                self.sequence(*sequence, writes, &index, checker);
            }
        }
    }

    /// Holds each place operand of `sequence` a later operand may write,
    /// gathering what the later operands do from the last one leftwards.
    fn sequence(
        &mut self,
        sequence: Recorded,
        writes: &BodyWrites,
        index: &WriteIndex,
        checker: &TypeChecker,
    ) {
        let operands = SequenceOperand::of_sequence(sequence.expression, checker);
        let written_by_the_call = bindings_the_call_writes(&operands);
        let mut later = Later::default();
        for (position, operand) in operands.iter().enumerate().rev() {
            if let Some(next) = operands.get(position + 1) {
                later.join(next, writes.extents.get(&address(next.value)), index);
            }
            let Some(path) = PlacePath::of(operand, checker) else {
                continue;
            };
            let written_when_called =
                (path.bindings.iter()).any(|name| written_by_the_call.contains(name));
            if written_when_called || later.may_write(&path, sequence.owner, index) {
                self.hold(operand.value, sequence.owner);
            }
        }
    }

    fn hold(&mut self, value: &Expression, owner: usize) {
        if self.held.insert(value.id) {
            (self.by_body.entry(owner).or_default()).push(HeldOperand {
                id: value.id,
                span: value.span,
            });
        }
    }
}

/// What the mentions and effects of one body do, by position, so a run of
/// positions is searched rather than walked.
struct WriteIndex<'b> {
    /// The positions of the mentions writing their binding, by the body
    /// they run in and the binding's name.
    writing: HashMap<(usize, &'b str), Vec<usize>>,
    /// The positions of the mentions an assignment targets, by body.
    assigned: HashMap<usize, Vec<usize>>,
    /// The positions of the effects that may run code, by body.
    running: HashMap<usize, Vec<usize>>,
    /// The position of each mention, by its expression's address.
    positions: HashMap<*const Expression, usize>,
    /// Every step of a path a call writes.
    written_steps: HashSet<*const Expression>,
}

impl<'b> WriteIndex<'b> {
    fn of(writes: &BodyWrites<'b>, checker: &TypeChecker) -> Self {
        let mut index = Self {
            writing: HashMap::new(),
            assigned: HashMap::new(),
            running: HashMap::new(),
            positions: HashMap::new(),
            written_steps: HashSet::new(),
        };
        for (position, mention) in writes.mentions.iter().enumerate() {
            index.mention(position, mention, checker);
        }
        for (position, effect) in writes.effects.iter().enumerate() {
            if runs_code(checker, effect.expression) {
                index
                    .running
                    .entry(effect.owner)
                    .or_default()
                    .push(position);
            }
        }
        index
    }

    fn mention(&mut self, position: usize, mention: &Mention<'b>, checker: &TypeChecker) {
        self.positions.insert(address(mention.expression), position);
        if matches!(mention.role, Some(Role::Assigned { .. })) {
            self.assigned
                .entry(mention.owner)
                .or_default()
                .push(position);
        }
        if !is_written(checker, mention) {
            return;
        }
        (self
            .writing
            .entry((mention.owner, mention.name()))
            .or_default())
        .push(position);
        let top = match mention.role {
            Some(Role::Receiver { object, .. }) => object,
            Some(Role::Argument {
                arguments, index, ..
            }) => argument_value(&arguments[index]).0,
            Some(Role::Assigned { .. }) | None => return,
        };
        let mut step = top;
        while let ExpressionKind::Member(inner, _) | ExpressionKind::Index(inner, _) = &step.node {
            self.written_steps.insert(address(step));
            step = inner;
        }
    }
}

/// What the operands after the one being judged do, gathered from the last
/// one leftwards.
#[derive(Default)]
struct Later<'b> {
    /// The bindings a later operand is handed to its call to write.
    written_by_call: HashSet<&'b str>,
    /// The mentions that are a later argument's own root: their role is in
    /// the sequence's own call, which `written_by_call` reads.
    own_roots: HashSet<usize>,
    /// The positions the later operands' mentions lie at.
    mentions: Option<Range<usize>>,
    /// The positions the later operands' effects lie at.
    effects: Option<Range<usize>>,
}

impl<'b> Later<'b> {
    /// Adds `operand`, whose records lie at `extent`, before the operands
    /// gathered so far.
    fn join(&mut self, operand: &SequenceOperand<'b>, extent: Option<&Extent>, index: &WriteIndex) {
        if operand.is_written_by_call {
            self.written_by_call
                .extend(binding_name(path_root(operand.value)));
        }
        let root = operand.argument_root.map(address);
        self.own_roots
            .extend(root.and_then(|root| index.positions.get(&root)));
        if let Some(extent) = extent {
            widen(&mut self.mentions, &extent.mentions);
            widen(&mut self.effects, &extent.effects);
        }
    }

    /// Whether the operands gathered may write the place `path` reads,
    /// judged in the body `owner`.
    fn may_write(&self, path: &PlacePath, owner: usize, index: &WriteIndex) -> bool {
        let mentions = self.mentions.clone().unwrap_or_default();
        let writes_binding = path.bindings.iter().any(|&name| {
            self.written_by_call.contains(name)
                || (index.writing.get(&(owner, name))).is_some_and(|positions| {
                    lying_in(positions, &mentions).any(|p| !self.own_roots.contains(&p))
                })
        });
        writes_binding
            || path.through_reference
                && (lies_in(index.running.get(&owner), self.effects.as_ref())
                    || lies_in(index.assigned.get(&owner), Some(&mentions)))
    }
}

/// The bindings a call writes through the operands it is handed to write.
/// The call writes them when it runs, after every operand is evaluated, so
/// they are written after each operand of the call, before or after them in
/// the order written: `bump(a, a)` with `bump(xs out [int], ys [int])` must
/// still hand `ys` the list `a` held before the call.
fn bindings_the_call_writes<'e>(operands: &[SequenceOperand<'e>]) -> HashSet<&'e str> {
    (operands.iter())
        .filter(|operand| operand.is_written_by_call)
        .filter_map(|operand| binding_name(path_root(operand.value)))
        .collect()
}

/// Stretches `range` to start where `earlier` does, an operand evaluated
/// before every one it covers.
fn widen(range: &mut Option<Range<usize>>, earlier: &Range<usize>) {
    match range {
        Some(range) => range.start = earlier.start,
        None => *range = Some(earlier.clone()),
    }
}

/// The positions of the sorted `positions` that lie in `range`.
fn lying_in<'p>(positions: &'p [usize], range: &Range<usize>) -> impl Iterator<Item = usize> + 'p {
    let first = positions.partition_point(|&p| p < range.start);
    let end = range.end;
    positions[first..]
        .iter()
        .copied()
        .take_while(move |&p| p < end)
}

/// Whether any of the sorted `positions` lies in `range`.
fn lies_in(positions: Option<&Vec<usize>>, range: Option<&Range<usize>>) -> bool {
    match (positions, range) {
        (Some(positions), Some(range)) => lying_in(positions, range).next().is_some(),
        (Some(_) | None, Some(_) | None) => false,
    }
}

/// Where one operand of a sequence stands in it.
pub(super) enum Slot<'e> {
    /// The receiver `object` of a call of `method`.
    Receiver {
        object: &'e Expression,
        method: &'e Expression,
    },
    /// The argument written `index`th of a call of `callee`.
    Argument {
        callee: &'e Expression,
        arguments: &'e [Expression],
        index: usize,
    },
    /// An operator's side, an indexed value or its index, a range's bound,
    /// an element of an array, tuple or variant literal.
    Element(&'e Expression),
}

/// The operands `sequence` evaluates, in order: a method call's receiver
/// before its arguments, an operator's left side before its right, an
/// indexed value before its index, a range's start before its end. Empty
/// when `sequence` evaluates no operands one after another — a list, set or
/// map literal among them, which stores each element as it is evaluated.
pub(super) fn sequence_slots(sequence: &Expression) -> Vec<Slot<'_>> {
    match &sequence.node {
        ExpressionKind::Call(callee, arguments) => {
            let receiver = if let ExpressionKind::Member(object, method) = &callee.node {
                Some(Slot::Receiver { object, method })
            } else {
                None
            };
            let arguments = (0..arguments.len()).map(|index| Slot::Argument {
                callee,
                arguments,
                index,
            });
            receiver.into_iter().chain(arguments).collect()
        }
        ExpressionKind::Binary(left, _, right) | ExpressionKind::Index(left, right) => {
            vec![Slot::Element(left), Slot::Element(right)]
        }
        ExpressionKind::Range(start, end, _) => std::iter::once(&**start)
            .chain(end.as_deref())
            .map(Slot::Element)
            .collect(),
        ExpressionKind::Tuple(elements)
        | ExpressionKind::EnumValue(_, elements)
        | ExpressionKind::Array(elements, _) => elements.iter().map(Slot::Element).collect(),
        ExpressionKind::List(..)
        | ExpressionKind::Set(..)
        | ExpressionKind::Map(..)
        | ExpressionKind::Identifier(..)
        | ExpressionKind::Super
        | ExpressionKind::Assignment(..)
        | ExpressionKind::Member(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::Logical(..)
        | ExpressionKind::FormattedString(..)
        | ExpressionKind::Unary(..)
        | ExpressionKind::Guard(..)
        | ExpressionKind::NamedArgument(..)
        | ExpressionKind::Cast(..)
        | ExpressionKind::Conditional(..)
        | ExpressionKind::Block(..)
        | ExpressionKind::Match(..)
        | ExpressionKind::Lambda(..)
        | ExpressionKind::Literal(_)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::Type(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..) => Vec::new(),
    }
}

/// One operand of a sequence, in the order it is evaluated.
struct SequenceOperand<'e> {
    /// The expression evaluated, any named-argument label taken off.
    value: &'e Expression,
    /// Whether the call it is handed to writes it: an `out` argument, an
    /// argument a runtime entry point writes in place, or the receiver of a
    /// method that writes its receiver.
    is_written_by_call: bool,
    /// For an argument, the root of the path it hands on, whose role is in
    /// the call the operand belongs to.
    argument_root: Option<&'e Expression>,
}

impl<'e> SequenceOperand<'e> {
    fn of_sequence(sequence: &'e Expression, checker: &TypeChecker) -> Vec<Self> {
        (sequence_slots(sequence).into_iter())
            .map(|slot| Self::of(slot, checker))
            .collect()
    }

    fn of(slot: Slot<'e>, checker: &TypeChecker) -> Self {
        match slot {
            Slot::Receiver { object, method } => Self {
                value: object,
                is_written_by_call: writes_receiver(checker, object, method),
                argument_root: None,
            },
            Slot::Argument {
                callee,
                arguments,
                index,
            } => {
                let (handed, through_cast) = argument_value(&arguments[index]);
                Self {
                    value: unlabelled(&arguments[index]),
                    is_written_by_call: writes_argument(
                        checker,
                        callee,
                        arguments,
                        index,
                        through_cast,
                    ),
                    argument_root: Some(path_root(handed)),
                }
            }
            Slot::Element(value) => Self {
                value: unlabelled(value),
                is_written_by_call: false,
                argument_root: None,
            },
        }
    }
}

/// The bindings a place operand is read through, and whether it is reached
/// through a value that may be shared by reference.
#[derive(Default)]
struct PlacePath<'e> {
    /// The path's root, when a binding, and every binding an index along it
    /// names.
    bindings: Vec<&'e str>,
    /// Whether some value the path steps into may be a reference.
    through_reference: bool,
}

impl<'e> PlacePath<'e> {
    /// The path `operand` reads, when it is a place the evaluation of the
    /// sequence reads later; `None` for any other operand.
    fn of(operand: &SequenceOperand<'e>, checker: &TypeChecker) -> Option<Self> {
        if operand.is_written_by_call {
            return None;
        }
        let mut path = Self::default();
        path.step(operand.value, checker).then_some(path)
    }

    /// Adds the bindings `expression` is read through; false when it is no
    /// place.
    fn step(&mut self, expression: &'e Expression, checker: &TypeChecker) -> bool {
        if let Some(name) = binding_name(expression) {
            self.bindings.push(name);
            return true;
        }
        if let ExpressionKind::Member(object, _) = &expression.node {
            return self.step_into(object, checker);
        }
        if let ExpressionKind::Index(object, index) = &expression.node {
            // An index that is itself a place is read with the element.
            self.step(index, checker);
            return self.step_into(object, checker);
        }
        // A value no binding holds is a place only once the path has
        // stepped through a reference into it: an object other code can
        // reach and write.
        self.through_reference
    }

    /// Steps into a field or an element of `object`.
    fn step_into(&mut self, object: &'e Expression, checker: &TypeChecker) -> bool {
        let object_type = checker.get_type(object.id);
        self.through_reference |= object_type.is_some_and(|ty| may_be_reference(checker, ty));
        self.step(object, checker)
    }
}

/// Whether the call or operator `effect` may run code that writes an object
/// through a reference: any call but an intrinsic or a variant constructor,
/// and an operator the left operand's type may answer with a method.
fn runs_code(checker: &TypeChecker, effect: &Expression) -> bool {
    if let ExpressionKind::Binary(left, op, _) = &effect.node {
        return (checker.get_type(left.id))
            .is_some_and(|ty| checker.operator_may_run_method(ty, op));
    }
    let ExpressionKind::Call(callee, _) = &effect.node else {
        return false;
    };
    let is_pure = matches!(&callee.node, ExpressionKind::Identifier(..))
        && matches!(
            checker.callee_kind(callee.id),
            CalleeKind::Intrinsic | CalleeKind::VariantConstructor
        );
    !is_pure
}

/// Whether a value of `ty` may be a reference to an object other code can
/// write.
fn may_be_reference(checker: &TypeChecker, ty: &Type) -> bool {
    sharing(&ty.kind, checker.type_definitions()) != Sharing::Value
}

fn address(expression: &Expression) -> *const Expression {
    expression
}
