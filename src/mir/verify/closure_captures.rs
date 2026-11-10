// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every closure is built from the captures its body reads, and
//! that nothing reads a closure value's insides through a projection.
//!
//! A closure value is built from one operand per capture its body reads, in
//! the body's capture order. The store that builds the environment and the
//! thunk that releases it both place each capture by its position, and the
//! thunk releases a capture exactly when the body's own declaration of it is
//! counted. An operand count that differs from the body's, or an operand
//! counted where the body's capture is not (or the reverse), leaves a capture
//! the store never writes, or a reference released that no store retained.
//!
//! A closure's captures are reached only through its body's environment
//! pointer, so a projection on a value of a function type names nothing.
//!
//! A capture is also held the same way on both sides: the store writes it at
//! the width, register class and byte span the creating body holds the
//! operand at, and the body loads it as it holds its capture local. Two
//! types that differ there — an `i32` stored where an `i128` is loaded, or a
//! value held as its bytes stored where a word is loaded — leave the load
//! reading bytes the store never wrote. A type naming a parameter its body
//! leaves open has no settled representation and is left alone.
//!
//! A closure every host body builds must have its body among the compiled
//! bodies: the value's header names that body as the function a call enters.
//!
//! Counted-ness is read from the slot specification ([`is_slot_counted`]),
//! and how a value is held from the representation specification
//! ([`Representation`]), each once in the creating body's scope and once in
//! the closure body's; nothing the environment fact derives is consulted.

use std::collections::HashMap;

use super::{local_display_name, VerificationViolation};
use crate::ast::types::{Type, TypeKind};
use crate::mir::place::PlaceContext;
use crate::mir::place_type::PlaceTypes;
use crate::mir::rc::is_slot_counted;
use crate::mir::representation::Representation;
use crate::mir::symbol::Symbol;
use crate::mir::visitor::Visitor;
use crate::mir::{AggregateKind, BasicBlock, Body, Operand, Place, Rvalue, StatementKind};

/// Every closure one of `bodies` builds from operands that disagree with
/// the captures the closure's body reads, or whose body is not among
/// `bodies`, each named by the body that builds it. Bodies are named as they
/// are linked; a closure's body is the one its symbol links as.
pub fn verify_closure_captures(bodies: &[(String, Body)]) -> Vec<(String, VerificationViolation)> {
    let by_name: HashMap<&str, &Body> = (bodies.iter())
        .map(|(name, body)| (name.as_str(), body))
        .collect();
    (bodies.iter())
        .filter(|(_, body)| !body.is_gpu())
        .flat_map(|(name, creator)| {
            let site = Creator {
                body: creator,
                types: PlaceTypes::of(creator),
            };
            built_closures(creator)
                .flat_map(
                    |(closure, operands)| match by_name.get(closure.link_name().as_str()) {
                        Some(lambda) => site.capture_violations(closure, operands, lambda),
                        None => vec![site.violation(
                            operands.first(),
                            format!(
                                "closure `{closure}` is built, but it has no compiled body for a \
                                 call of it to enter"
                            ),
                        )],
                    },
                )
                .map(|violation| (name.clone(), violation))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every place in `body` that projects into a value of a function type.
pub fn verify_function_projections(body: &Body) -> Vec<VerificationViolation> {
    let mut finder = FunctionProjections {
        body,
        types: PlaceTypes::of(body),
        found: Vec::new(),
    };
    finder.visit_body(body);
    finder.found
}

/// Each closure `body` builds, with the operands it builds it from.
fn built_closures(body: &Body) -> impl Iterator<Item = (&Symbol, &[Operand])> {
    (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(
                _,
                Rvalue::Aggregate(AggregateKind::Closure(closure, _), ops),
            )
            | StatementKind::Reassign(
                _,
                Rvalue::Aggregate(AggregateKind::Closure(closure, _), ops),
            ) => Some((closure, ops.as_slice())),
            StatementKind::Assign(_, _)
            | StatementKind::Reassign(_, _)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_, _)
            | StatementKind::Nop
            | StatementKind::ReadHeader(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_) => None,
        })
}

/// A body that builds closures, with the types its places hold.
struct Creator<'b> {
    body: &'b Body,
    types: PlaceTypes<'b>,
}

impl Creator<'_> {
    /// Where `operands`, building `closure`, disagree with the captures
    /// `lambda` reads: in number, in whether a capture is counted, or in how
    /// it is held.
    fn capture_violations(
        &self,
        closure: &Symbol,
        operands: &[Operand],
        lambda: &Body,
    ) -> Vec<VerificationViolation> {
        let captures = &lambda.captures.env_capture_locals;
        if operands.len() != captures.len() {
            return vec![self.violation(
                operands.first(),
                format!(
                    "closure `{closure}` is built from {} operand(s), but its body reads {} \
                     capture(s)",
                    operands.len(),
                    captures.len()
                ),
            )];
        }
        let lambda_types = PlaceTypes::of(lambda);
        (operands.iter().zip(captures).enumerate())
            .filter_map(|(index, (operand, capture))| {
                let built = self.types.operand_type(operand)?;
                let read = &lambda.local_decls.get(capture.0)?.ty;
                let message = self
                    .counted_disagreement(&built, lambda, read)
                    .or_else(|| self.held_disagreement(&built, (lambda, &lambda_types), read))?;
                Some(self.violation(
                    Some(operand),
                    format!("capture {index} of closure `{closure}` {message}"),
                ))
            })
            .collect()
    }

    /// How `built`, an operand of this body, and `read`, the capture `lambda`
    /// loads it into, disagree on whether the capture is counted.
    fn counted_disagreement(&self, built: &Type, lambda: &Body, read: &Type) -> Option<String> {
        let built_counted = is_slot_counted(&built.kind, self.body.layout_scope());
        let read_counted = is_slot_counted(&read.kind, lambda.layout_scope());
        (built_counted != read_counted).then(|| {
            format!(
                "is built from {} `{built}`, but its body reads {} `{read}`",
                counted_word(built_counted),
                counted_word(read_counted)
            )
        })
    }

    /// How `built` and `read` disagree on how the capture is held: its width,
    /// register class or byte span. Counting is the other rule's to judge.
    fn held_disagreement(
        &self,
        built: &Type,
        (lambda, lambda_types): (&Body, &PlaceTypes<'_>),
        read: &Type,
    ) -> Option<String> {
        if self.types.names_open_parameter(&built.kind)
            || lambda_types.names_open_parameter(&read.kind)
        {
            return None;
        }
        let held = |kind: &TypeKind, body: &Body| {
            Representation::of(kind, body.layout_scope()).without_counting()
        };
        (held(&built.kind, self.body) != held(&read.kind, lambda)).then(|| {
            format!("is built from `{built}`, but its body reads `{read}`, held differently")
        })
    }

    /// A finding about a closure built in this body, named by `operand`'s
    /// local when it reads one, else by the return slot.
    fn violation(&self, operand: Option<&Operand>, message: String) -> VerificationViolation {
        let local = match operand {
            Some(Operand::Copy(place) | Operand::Move(place)) => place.local,
            Some(Operand::Constant(_) | Operand::Function(_)) | None => crate::mir::Local(0),
        };
        VerificationViolation {
            local,
            local_name: local_display_name(self.body, local),
            message,
        }
    }
}

/// How a capture's counted-ness reads in a finding.
fn counted_word(counted: bool) -> &'static str {
    if counted {
        "a counted"
    } else {
        "an uncounted"
    }
}

/// Collects every place projecting into a value of a function type.
struct FunctionProjections<'b> {
    body: &'b Body,
    types: PlaceTypes<'b>,
    found: Vec<VerificationViolation>,
}

impl Visitor for FunctionProjections<'_> {
    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        if projects_into_function(&self.types, place) {
            self.found.push(VerificationViolation {
                local: place.local,
                local_name: local_display_name(self.body, place.local),
                message: format!(
                    "`{place}` projects into a value of a function type, whose captures only its body reads"
                ),
            });
        }
    }
}

/// Whether some projection of `place` applies to a value of a function type.
fn projects_into_function(types: &PlaceTypes<'_>, place: &Place) -> bool {
    (0..place.projection.len()).any(|prefix| {
        let base = Place {
            local: place.local,
            projection: place.projection[..prefix].to_vec(),
        };
        types
            .declared(&base)
            .is_some_and(|ty| matches!(ty.kind, TypeKind::Function(_)))
    })
}
