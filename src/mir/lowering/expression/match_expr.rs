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

/// Classify a pattern into switch-able, predicate-based, or catch-all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternKind {
    Switch,
    Predicate,
    CatchAll,
}

fn classify_pattern(pattern: &Pattern) -> PatternKind {
    match pattern {
        Pattern::Literal(lit) => match lit {
            crate::ast::literal::Literal::String(_) => PatternKind::Predicate,
            crate::ast::literal::Literal::Float(_) => PatternKind::Predicate,
            _ => {
                if literal_to_u128(lit).is_some() {
                    PatternKind::Switch
                } else {
                    PatternKind::CatchAll
                }
            }
        },
        Pattern::Regex(_) => PatternKind::Predicate,
        Pattern::Default | Pattern::Identifier(_) | Pattern::Tuple(_) => PatternKind::CatchAll,
        _ => PatternKind::Switch,
    }
}

fn arm_has_only_predicate_patterns(branch: &crate::ast::pattern::MatchBranch) -> bool {
    branch
        .patterns
        .iter()
        .any(|p| classify_pattern(p) == PatternKind::Predicate)
}

/// Test a simple predicate pattern (String or Float) and assign the boolean result.
fn emit_simple_predicate_test(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    subject_local: crate::mir::Local,
    pattern_span: &crate::error::syntax::Span,
    result_local: crate::mir::Local,
) -> Result<(), LoweringError> {
    match pattern {
        Pattern::Literal(crate::ast::literal::Literal::String(s)) => {
            let subject_op = Operand::Copy(Place::new(subject_local));
            let string_const = Operand::Constant(Box::new(crate::mir::Constant {
                span: *pattern_span,
                ty: Type::new(TypeKind::String, *pattern_span),
                literal: crate::ast::literal::Literal::String(s.clone()),
            }));

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(result_local),
                    Rvalue::BinaryOp(
                        crate::mir::BinOp::Eq,
                        Box::new(subject_op),
                        Box::new(string_const),
                    ),
                ),
                span: *pattern_span,
            });
        }
        Pattern::Literal(crate::ast::literal::Literal::Float(float_lit)) => {
            use crate::ast::literal::FloatLiteral;
            let subject_op = Operand::Copy(Place::new(subject_local));
            let float_ty = match float_lit {
                FloatLiteral::F32(_) => Type::new(TypeKind::F32, *pattern_span),
                FloatLiteral::F64(_) => Type::new(TypeKind::F64, *pattern_span),
            };
            let float_const = Operand::Constant(Box::new(crate::mir::Constant {
                span: *pattern_span,
                ty: float_ty,
                literal: crate::ast::literal::Literal::Float(float_lit.clone()),
            }));

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(result_local),
                    Rvalue::BinaryOp(
                        crate::mir::BinOp::Eq,
                        Box::new(subject_op),
                        Box::new(float_const),
                    ),
                ),
                span: *pattern_span,
            });
        }
        _ => {
            unreachable!("Only String and Float patterns should reach here")
        }
    }

    Ok(())
}

/// Test a regex predicate pattern and branch to appropriate targets.
/// Uses method dispatch to call .matches(), avoiding hardcoded stdlib assumptions.
fn emit_regex_predicate_test(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    subject_local: crate::mir::Local,
    pattern_span: &crate::error::syntax::Span,
    match_bb: crate::mir::BasicBlock,
    fail_bb: crate::mir::BasicBlock,
) -> Result<(), LoweringError> {
    let Pattern::Regex(regex_token) = pattern else {
        unreachable!()
    };

    use crate::mir::lowering::expression::literal_expr::lower_regex_from_token;
    use crate::runtime_fns::rt;

    // Materialize the Regex using the standard path.
    // Allocate a temp destination so the call ABI is consistent regardless of type resolution.
    let regex_ty = Type::new(
        TypeKind::Custom(crate::ast::types::REGEX_TYPE_NAME.into(), None),
        *pattern_span,
    );
    let regex_temp = ctx.push_temp(regex_ty, *pattern_span);
    let regex_dest = Place::new(regex_temp);

    let _regex_op =
        lower_regex_from_token(ctx, regex_token, *pattern_span, 0, Some(regex_dest.clone()))?;

    // Extract the handle from the Regex object (Field 0) and call the runtime function.
    // This is the matches() method implementation from the Regex class.
    // INVARIANT: Regex.handle must remain field 0; see src/stdlib/system/text.mi line 95.
    let mut handle_place = regex_dest.clone();
    handle_place.projection.push(PlaceElem::Field(0));

    let handle_local = ctx.push_temp(Type::new(TypeKind::Int, *pattern_span), *pattern_span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(handle_local),
            Rvalue::Use(Operand::Copy(handle_place)),
        ),
        span: *pattern_span,
    });

    let func_op = Operand::Constant(Box::new(crate::mir::Constant {
        span: *pattern_span,
        ty: Type::new(TypeKind::Identifier, *pattern_span),
        literal: crate::ast::literal::Literal::Identifier(rt::REGEX_MATCHES.to_string()),
    }));

    let bool_ty = Type::new(TypeKind::Boolean, *pattern_span);
    let result_temp = ctx.push_temp(bool_ty, *pattern_span);
    let continuation_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![
                Operand::Copy(Place::new(handle_local)),
                Operand::Copy(Place::new(subject_local)),
            ],
            out_args: vec![],
            arg_handles: vec![],
            destination: Place::new(result_temp),
            target: Some(continuation_bb),
        },
        *pattern_span,
    ));

    ctx.set_current_block(continuation_bb);

    // Emit drop for the regex_temp after the call completes but before we branch.
    // regex_temp is a managed Regex object and must be DecRef'd.
    // Note: result_temp (bool) is a scalar and will be read in the SwitchInt terminator,
    // so we don't drop it here; MIR allows unused scalars to be implicitly dropped.
    ctx.emit_temp_drop(regex_temp, 0, *pattern_span);

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(result_temp)),
            targets: vec![(Discriminant::bool_true(), match_bb)],
            otherwise: fail_bb,
        },
        *pattern_span,
    ));

    Ok(())
}

/// Lower guard condition and emit branching to appropriate successor or fallback.
fn emit_guard_and_branch(
    ctx: &mut LoweringContext,
    guard: &Expression,
    arm_idx: usize,
    branch_blocks: &[(
        crate::mir::BasicBlock,
        &crate::ast::pattern::MatchBranch,
        Vec<u128>,
    )],
    this_discrs: &[u128],
    join_bb: crate::mir::BasicBlock,
) -> Result<(), LoweringError> {
    let guard_op = lower_expression(ctx, guard, None)?;
    let guard_true_bb = ctx.new_basic_block();

    let this_is_catchall = this_discrs.is_empty();
    let mut guard_fail_bb = join_bb;
    for (next_bb, _, next_discrs) in branch_blocks.iter().skip(arm_idx + 1) {
        let next_is_catchall = next_discrs.is_empty();
        if next_is_catchall {
            guard_fail_bb = *next_bb;
            break;
        }
        if !this_is_catchall && this_discrs.iter().any(|d| next_discrs.contains(d)) {
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
    Ok(())
}

/// Compute discriminant values and switch targets for a pattern in match arms.
fn compute_pattern_discriminants(
    ctx: &LoweringContext,
    pattern: &Pattern,
    is_option_subject: bool,
    branch_bb: crate::mir::BasicBlock,
    seen_discrs: &mut std::collections::HashSet<u128>,
    switch_targets: &mut Vec<(Discriminant, crate::mir::BasicBlock)>,
    otherwise_bb: &mut Option<crate::mir::BasicBlock>,
) -> Vec<u128> {
    let mut arm_discrs: Vec<u128> = Vec::new();

    if is_option_subject {
        match pattern {
            Pattern::Literal(crate::ast::literal::Literal::None) => {
                arm_discrs.push(0);
                if seen_discrs.insert(0) {
                    switch_targets.push((Discriminant::from(0u128), branch_bb));
                }
                return arm_discrs;
            }
            Pattern::Member(parent, member)
                if matches!(
                    &**parent,
                    Pattern::Identifier(n) if n == crate::ast::types::OPTION_TYPE_NAME
                ) && member == "None" =>
            {
                arm_discrs.push(0);
                if seen_discrs.insert(0) {
                    switch_targets.push((Discriminant::from(0u128), branch_bb));
                }
                return arm_discrs;
            }
            Pattern::EnumVariant(parent, _) => {
                let is_some = match &**parent {
                    Pattern::Identifier(name) => name == "Some",
                    Pattern::Member(enum_pat, variant) => {
                        matches!(
                            &**enum_pat,
                            Pattern::Identifier(n) if n == crate::ast::types::OPTION_TYPE_NAME
                        ) && variant == "Some"
                    }
                    _ => false,
                };
                if is_some {
                    if otherwise_bb.is_none() {
                        *otherwise_bb = Some(branch_bb);
                    }
                    return arm_discrs;
                }
            }
            _ => {}
        }
    }

    match pattern {
        Pattern::Literal(lit) => {
            if let Some(val) = literal_to_u128(lit) {
                arm_discrs.push(val);
                if seen_discrs.insert(val) {
                    switch_targets.push((Discriminant::from(val), branch_bb));
                }
            }
        }
        Pattern::Default => {
            *otherwise_bb = Some(branch_bb);
        }
        Pattern::Identifier(_) => {
            if otherwise_bb.is_none() {
                *otherwise_bb = Some(branch_bb);
            }
        }
        Pattern::Member(type_pattern, variant_name) => {
            if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) = ctx
                    .type_checker
                    .type_table
                    .global_type_definitions
                    .get(type_name)
                {
                    if let Some((idx, _)) = enum_def
                        .variants
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| *name == variant_name)
                    {
                        arm_discrs.push(idx as u128);
                        if seen_discrs.insert(idx as u128) {
                            switch_targets.push((Discriminant::from(idx as u128), branch_bb));
                        }
                    }
                }
            }
        }
        Pattern::EnumVariant(parent_pattern, _bindings) => {
            if let Pattern::Member(type_pattern, variant_name) = parent_pattern.as_ref() {
                if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                    if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) = ctx
                        .type_checker
                        .type_table
                        .global_type_definitions
                        .get(type_name)
                    {
                        if let Some((idx, _)) = enum_def
                            .variants
                            .iter()
                            .enumerate()
                            .find(|(_, (name, _))| *name == variant_name)
                        {
                            arm_discrs.push(idx as u128);
                            if seen_discrs.insert(idx as u128) {
                                switch_targets.push((Discriminant::from(idx as u128), branch_bb));
                            }
                        }
                    }
                }
            }
        }
        Pattern::Tuple(_) => {
            if otherwise_bb.is_none() {
                *otherwise_bb = Some(branch_bb);
            }
        }
        Pattern::Regex(_) => {}
    }

    arm_discrs
}

/// Emit all predicate test blocks that form a chain for pattern alternatives.
/// Each predicate arm's patterns are tested in order; if a pattern matches,
/// the branch is entered. If it fails, the next pattern (if any) is tested,
/// and only the last pattern's failure leaves the arm entirely.
#[allow(clippy::too_many_arguments)]
fn emit_predicate_test_chain(
    ctx: &mut LoweringContext,
    subject_local: crate::mir::Local,
    expr_span: &crate::error::syntax::Span,
    predicate_arm_indices: &[usize],
    predicate_test_blocks: &[crate::mir::BasicBlock],
    branch_blocks: &[(
        crate::mir::BasicBlock,
        &crate::ast::pattern::MatchBranch,
        Vec<u128>,
    )],
    otherwise_bb: Option<crate::mir::BasicBlock>,
    join_bb: crate::mir::BasicBlock,
) -> Result<(), crate::error::lowering::LoweringError> {
    for (test_idx, arm_idx) in predicate_arm_indices.iter().enumerate() {
        let test_bb = predicate_test_blocks[test_idx];
        let (branch_bb, branch, _) = &branch_blocks[*arm_idx];

        ctx.set_current_block(test_bb);

        let next_target = if test_idx + 1 < predicate_test_blocks.len() {
            predicate_test_blocks[test_idx + 1]
        } else {
            otherwise_bb.unwrap_or(join_bb)
        };

        // Collect all predicate patterns in this arm to chain them together.
        let predicate_patterns: Vec<_> = branch
            .patterns
            .iter()
            .filter(|p| classify_pattern(p) == PatternKind::Predicate)
            .collect();

        for (pattern_idx, pattern) in predicate_patterns.iter().enumerate() {
            let pattern_fail_target = if pattern_idx + 1 < predicate_patterns.len() {
                ctx.new_basic_block()
            } else {
                next_target
            };

            if matches!(pattern, Pattern::Regex(_)) {
                emit_regex_predicate_test(
                    ctx,
                    pattern,
                    subject_local,
                    expr_span,
                    *branch_bb,
                    pattern_fail_target,
                )?;
            } else {
                let bool_ty = Type::new(TypeKind::Boolean, *expr_span);
                let test_result = ctx.push_temp(bool_ty, *expr_span);
                emit_simple_predicate_test(ctx, pattern, subject_local, expr_span, test_result)?;
                ctx.emit_temp_drop(test_result, 0, *expr_span);

                ctx.set_terminator(Terminator::new(
                    TerminatorKind::SwitchInt {
                        discr: Operand::Copy(Place::new(test_result)),
                        targets: vec![(Discriminant::bool_true(), *branch_bb)],
                        otherwise: pattern_fail_target,
                    },
                    *expr_span,
                ));
            }

            if pattern_idx + 1 < predicate_patterns.len() {
                ctx.set_current_block(pattern_fail_target);
            }
        }
    }

    Ok(())
}

pub(crate) fn lower_match_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Match(subject, branches) = &expr.node else {
        unreachable!()
    };
    let subject_info = lower_match_subject(ctx, subject)?;
    let MatchSubject {
        local: subject_local,
        ty: ref subject_ty,
        ..
    } = subject_info;

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
    // arms (identifier, default, tuple). Predicate arms (string/float/regex literals)
    // are tested separately. The discriminants are used when computing guard-failure
    // targets (see second pass below).
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

    let is_option_subject = matches!(subject_ty.kind, TypeKind::Option(_));

    for branch in branches {
        let branch_bb = ctx.new_basic_block();
        let mut arm_discrs: Vec<u128> = Vec::new();

        for pattern in &branch.patterns {
            let discrs = compute_pattern_discriminants(
                ctx,
                pattern,
                is_option_subject,
                branch_bb,
                &mut seen_discrs,
                &mut switch_targets,
                &mut otherwise_bb,
            );
            arm_discrs.extend(discrs);
        }

        branch_blocks.push((branch_bb, branch, arm_discrs));
    }

    // Find arms with only predicate patterns, to set up predicate test chain
    let mut predicate_arm_indices: Vec<usize> = Vec::new();
    let mut predicate_test_blocks: Vec<crate::mir::BasicBlock> = Vec::new();
    for (idx, (_bb, branch, _discrs)) in branch_blocks.iter().enumerate() {
        if arm_has_only_predicate_patterns(branch) {
            predicate_arm_indices.push(idx);
            predicate_test_blocks.push(ctx.new_basic_block());
        }
    }

    // Determine the target for the switch's otherwise:
    // - First predicate test block if one exists
    // - Otherwise default pattern if one exists
    // - Otherwise join_bb
    let otherwise_target = if !predicate_test_blocks.is_empty() {
        predicate_test_blocks[0]
    } else {
        otherwise_bb.unwrap_or(join_bb)
    };

    // For enum types, we need to extract the discriminant (Field 0) to switch on
    let switch_discr = if let TypeKind::Custom(type_name, _) = &subject_ty.kind {
        if ctx
            .type_checker
            .type_table
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

    // Emit predicate test blocks for all predicate arms
    emit_predicate_test_chain(
        ctx,
        subject_local,
        &expr.span,
        &predicate_arm_indices,
        &predicate_test_blocks,
        &branch_blocks,
        otherwise_bb,
        join_bb,
    )?;

    // Lower each branch body
    for (arm_idx, (branch_bb, branch, this_discrs)) in branch_blocks.iter().enumerate() {
        ctx.set_current_block(*branch_bb);
        ctx.push_scope();

        // Bind pattern variables
        for pattern in &branch.patterns {
            bind_pattern(ctx, pattern, subject_local, &subject.span)?;
        }

        if let Some(guard) = &branch.guard {
            emit_guard_and_branch(ctx, guard, arm_idx, &branch_blocks, this_discrs, join_bb)?;
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
    release_subject_temps(ctx, &subject_info, subject.span);
    Ok(Operand::Copy(Place::new(result_local)))
}

/// The temp a match dispatches on, plus what is needed to release it again.
struct MatchSubject {
    /// Temp holding the subject value, readable once per arm.
    local: crate::mir::Local,
    /// The subject's resolved type.
    ty: Type,
    /// Local backing the operand that was assigned into `local`, if the subject
    /// lowered to a place rather than a constant.
    source_local: Option<crate::mir::Local>,
    /// Local count before the subject was lowered, separating locals the
    /// subject allocated here from ones that already existed.
    watermark: usize,
}

/// Lower a match subject into a temp so every arm can read it.
fn lower_match_subject(
    ctx: &mut LoweringContext,
    subject: &Expression,
) -> Result<MatchSubject, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let subject_op = lower_expression(ctx, subject, None)?;
    let source_local = match &subject_op {
        Operand::Copy(place) | Operand::Move(place) => Some(place.local),
        Operand::Constant(_) => None,
    };
    let ty = resolve_type(ctx.type_checker, subject);
    let local = ctx.push_temp(ty.clone(), subject.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(local), Rvalue::Use(subject_op)),
        span: subject.span,
    });
    Ok(MatchSubject {
        local,
        ty,
        source_local,
        watermark,
    })
}

/// Release what the match held on to once every arm has read from it: the
/// subject temp, which was reference-counted when the subject operand was
/// assigned into it, and then the operand's own local if the subject expression
/// allocated it here. Without the second drop, matching directly on an
/// expression that allocates — `match make_result()` — never releases what it
/// returned, so an enum's payload outlives the match.
fn release_subject_temps(
    ctx: &mut LoweringContext,
    subject: &MatchSubject,
    span: crate::error::syntax::Span,
) {
    ctx.emit_temp_drop(subject.local, 0, span);
    if let Some(source_local) = subject.source_local {
        if source_local != subject.local {
            ctx.emit_temp_drop(source_local, subject.watermark, span);
        }
    }
}
