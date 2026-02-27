// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::Pattern;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    Discriminant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::{bind_pattern, literal_to_u128, lower_to_local, resolve_type};

pub(crate) fn lower_match_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Match(subject, branches) = &expr.node else {
        unreachable!()
    };
    // Lower the subject expression
    let subject_op = lower_expression(ctx, subject, None)?;

    // Store subject in a temp so we can reference it multiple times
    let subject_ty = resolve_type(ctx.type_checker, subject);
    let subject_local = ctx.push_temp(subject_ty.clone(), subject.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(subject_local), Rvalue::Use(subject_op)),
        span: subject.span,
    });

    // Use dest if provided (DPS), otherwise create a temp
    let result_ty = resolve_type(ctx.type_checker, expr);
    let result_local = if let Some(ref dest_place) = dest {
        dest_place.local
    } else {
        ctx.push_temp(result_ty.clone(), expr.span)
    };

    // Create join block where all branches converge
    let join_bb = ctx.new_basic_block();

    // Collect literal patterns for SwitchInt.
    // branch_blocks stores (block, branch, discriminants) where discriminants is
    // non-empty for arms with specific literal/enum patterns and empty for catch-all
    // arms (identifier, default, tuple, regex).  The discriminants are used when
    // computing guard-failure targets (see second pass below).
    //
    // IMPORTANT: only the *first* arm that covers a given discriminant value is
    // registered in switch_targets.  Subsequent arms with the same discriminant
    // (e.g. a guarded arm followed by an unguarded fallback for the same literal)
    // are reachable only via the guard-failure chain, NOT via a second SwitchInt
    // dispatch.  Adding duplicate discriminants to switch_targets causes the
    // Cranelift translator (which uses `.pop()` to build a brif chain in reverse)
    // to dispatch to the *last* duplicate first, bypassing any earlier guarded arm.
    let mut switch_targets: Vec<(Discriminant, crate::mir::block::BasicBlock)> = Vec::new();
    let mut seen_discrs: std::collections::HashSet<u128> = std::collections::HashSet::new();
    let mut otherwise_bb = None;
    let mut branch_blocks: Vec<(
        crate::mir::block::BasicBlock,
        &crate::ast::pattern::MatchBranch,
        Vec<u128>, // discriminants covered; empty ⇒ catch-all
    )> = Vec::new();

    for branch in branches {
        let branch_bb = ctx.new_basic_block();
        let mut arm_discrs: Vec<u128> = Vec::new();

        for pattern in &branch.patterns {
            match pattern {
                Pattern::Literal(lit) => {
                    if let Some(val) = literal_to_u128(lit) {
                        arm_discrs.push(val);
                        // Only register the first arm per discriminant in switch_targets.
                        if seen_discrs.insert(val) {
                            switch_targets.push((Discriminant::from(val), branch_bb));
                        }
                    }
                }
                Pattern::Default => {
                    otherwise_bb = Some(branch_bb);
                }
                Pattern::Identifier(_) => {
                    // Identifier pattern matches everything - treat as otherwise
                    if otherwise_bb.is_none() {
                        otherwise_bb = Some(branch_bb);
                    }
                }
                Pattern::Member(type_pattern, variant_name) => {
                    // Member pattern for unit enum variants: Status.Ok
                    if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                        if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                            ctx.type_checker.global_type_definitions.get(type_name)
                        {
                            if let Some((idx, _)) = enum_def
                                .variants
                                .iter()
                                .enumerate()
                                .find(|(_, (name, _))| *name == variant_name)
                            {
                                arm_discrs.push(idx as u128);
                                if seen_discrs.insert(idx as u128) {
                                    switch_targets
                                        .push((Discriminant::from(idx as u128), branch_bb));
                                }
                            }
                        }
                    }
                }
                Pattern::EnumVariant(parent_pattern, _bindings) => {
                    // Enum variant with bindings: Color.Red(x, y)
                    if let Pattern::Member(type_pattern, variant_name) = parent_pattern.as_ref() {
                        if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                            if let Some(crate::type_checker::context::TypeDefinition::Enum(
                                enum_def,
                            )) = ctx.type_checker.global_type_definitions.get(type_name)
                            {
                                if let Some((idx, _)) = enum_def
                                    .variants
                                    .iter()
                                    .enumerate()
                                    .find(|(_, (name, _))| *name == variant_name)
                                {
                                    arm_discrs.push(idx as u128);
                                    if seen_discrs.insert(idx as u128) {
                                        switch_targets
                                            .push((Discriminant::from(idx as u128), branch_bb));
                                    }
                                }
                            }
                        }
                    }
                }
                Pattern::Tuple(_) => {
                    // Tuple patterns match by structure - treat as otherwise for now
                    if otherwise_bb.is_none() {
                        otherwise_bb = Some(branch_bb);
                    }
                }
                Pattern::Regex(_) => {
                    // Regex patterns require runtime matching - treat as otherwise
                    if otherwise_bb.is_none() {
                        otherwise_bb = Some(branch_bb);
                    }
                }
            }
        }

        branch_blocks.push((branch_bb, branch, arm_discrs));
    }

    // Set otherwise to join if no default pattern
    let otherwise_target = otherwise_bb.unwrap_or(join_bb);

    // For enum types, we need to extract the discriminant (Field 0) to switch on
    let switch_discr = if let TypeKind::Custom(type_name, _) = &subject_ty.kind {
        if ctx
            .type_checker
            .global_type_definitions
            .get(type_name)
            .is_some_and(|td| matches!(td, crate::type_checker::context::TypeDefinition::Enum(_)))
        {
            // Extract discriminant from enum value at Field(0)
            let discr_ty = Type::new(TypeKind::Int, subject.span);
            let discr_local = ctx.push_temp(discr_ty, subject.span);

            let mut discr_place = Place::new(subject_local);
            discr_place.projection.push(PlaceElem::Field(0));

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(discr_local),
                    Rvalue::Use(Operand::Copy(discr_place)),
                ),
                span: subject.span,
            });

            Operand::Copy(Place::new(discr_local))
        } else {
            Operand::Copy(Place::new(subject_local))
        }
    } else {
        Operand::Copy(Place::new(subject_local))
    };

    // Set SwitchInt terminator
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: switch_discr,
            targets: switch_targets,
            otherwise: otherwise_target,
        },
        expr.span,
    ));

    // Lower each branch body
    for (arm_idx, (branch_bb, branch, this_discrs)) in branch_blocks.iter().enumerate() {
        ctx.set_current_block(*branch_bb);
        ctx.push_scope();

        // Bind pattern variables
        for pattern in &branch.patterns {
            bind_pattern(ctx, pattern, subject_local, &subject.span)?;
        }

        // Handle guard if present
        if let Some(guard) = &branch.guard {
            let guard_op = lower_expression(ctx, guard, None)?;
            let guard_true_bb = ctx.new_basic_block();

            // Compute guard-failure target: the next arm that could match the same
            // subject value.
            //
            // • If this arm has specific discriminants (literal / enum variant), scan
            //   forward for the next arm that shares at least one of those discriminants,
            //   OR the first catch-all arm (identifier / default / tuple / regex) —
            //   whichever comes first in source order.
            //
            // • If this arm is itself a catch-all (empty discriminant set), scan forward
            //   for the next catch-all arm.
            //
            // Falling off the end means no more arms can match → jump to join_bb.
            let this_is_catchall = this_discrs.is_empty();
            let mut guard_fail_bb = join_bb;
            for (next_bb, _, next_discrs) in branch_blocks.iter().skip(arm_idx + 1) {
                let next_is_catchall = next_discrs.is_empty();
                if next_is_catchall {
                    // A catch-all arm can always receive control.
                    guard_fail_bb = *next_bb;
                    break;
                }
                if !this_is_catchall && this_discrs.iter().any(|d| next_discrs.contains(d)) {
                    // Next arm covers the same discriminant value.
                    guard_fail_bb = *next_bb;
                    break;
                }
            }

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: guard_op,
                    targets: vec![(Discriminant::bool_true(), guard_true_bb)],
                    otherwise: guard_fail_bb,
                },
                guard.span,
            ));

            ctx.set_current_block(guard_true_bb);
        }

        // Lower branch body and assign result to result_local
        lower_to_local(ctx, &branch.body, result_local, &result_ty)?;

        // Goto join if body didn't terminate (e.g., with return)
        if ctx.body.basic_blocks[ctx.current_block.0]
            .terminator
            .is_none()
        {
            ctx.pop_scope(expr.span);
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: join_bb },
                expr.span,
            ));
        }
    }

    ctx.set_current_block(join_bb);
    Ok(Operand::Copy(Place::new(result_local)))
}
