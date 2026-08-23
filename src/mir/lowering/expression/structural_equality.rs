// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR-level lowering for structural equality (`==`) on enums, Option, Result, and structs.
//!
//! This module emits MIR to compare values recursively based on their type structure.
//! Enum and struct fields are compared recursively, managed types (String, etc.) use
//! the runtime equality helper, and scalars use primitive `BinOp::Eq`.

use crate::ast::types::{Type, TypeKind, EQUALS_METHOD_NAME};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::{
    BinOp, Constant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;

/// Emit MIR for structural equality comparison.
///
/// Returns a Local holding the boolean result. The function dispatches on the type:
/// - **Enums/Result**: compare discriminants, then recursively compare payloads
/// - **Option**: null/null check, then recursively compare Some payloads
/// - **Structs**: recursively compare all fields in declaration order
/// - **Strings**: call the runtime string equality helper
/// - **Scalars**: use primitive BinOp::Eq
///
/// # Arguments
/// - `ctx`: mutable lowering context
/// - `span`: source code span
/// - `kind`: the TypeKind of the values being compared
/// - `lhs_op`: left-hand side operand
/// - `rhs_op`: right-hand side operand
/// - `is_eq`: true for `==`, false for `!=`
///
/// # Returns
/// A Local holding the boolean result of the comparison.
pub fn emit_structural_equality(
    ctx: &mut LoweringContext,
    span: Span,
    kind: &TypeKind,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> Result<crate::mir::Local, LoweringError> {
    match kind {
        TypeKind::String => Ok(emit_string_equality(ctx, span, lhs_op, rhs_op, is_eq)),
        TypeKind::Option(inner_ty) => {
            emit_option_structural_equality(ctx, span, inner_ty, lhs_op, rhs_op, is_eq)
        }
        TypeKind::Custom(name, args) => {
            emit_named_type_equality(ctx, span, name, args.as_deref(), lhs_op, rhs_op, is_eq)
        }
        // The type checker rewrites this shape to the named form, which carries
        // a definition to read variants and their order from. Synthesizing that
        // order here instead would have to guess it, and guessing wrong compares
        // one variant's payload against another's without any sign that it did.
        TypeKind::Result(_, _) => Err(LoweringError::unsupported_expression(
            "cannot compare a result type that was not resolved to its declaration".to_string(),
            span,
        )),
        // Every scalar width compares directly. The sized widths reach here as
        // enum payloads and struct fields, so omitting one turns a legal
        // comparison into a compile error.
        TypeKind::Boolean
        | TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Identifier => Ok(emit_scalar_equality(ctx, span, lhs_op, rhs_op, is_eq)),
        TypeKind::List(_)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Array(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Function(_)
        | TypeKind::Future(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_)
        | TypeKind::Error => Err(LoweringError::unsupported_expression(
            format!("structural equality not supported for type {}", kind),
            span,
        )),
    }
}

/// Emit string equality using the runtime helper.
fn emit_string_equality(
    ctx: &mut LoweringContext,
    span: Span,
    lhs: Operand,
    rhs: Operand,
    is_eq: bool,
) -> crate::mir::Local {
    let result = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let next_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: identifier_constant(rt::STRING_EQUALS, span),
            args: vec![lhs, rhs],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(result),
            target: Some(next_bb),
        },
        span,
    ));
    ctx.set_current_block(next_bb);

    if !is_eq {
        let negated = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(negated),
                Rvalue::UnaryOp(
                    crate::mir::UnOp::Not,
                    Box::new(Operand::Copy(Place::new(result))),
                ),
            ),
            span,
        });
        negated
    } else {
        result
    }
}

/// Emit equality for Option types.
fn emit_option_structural_equality(
    ctx: &mut LoweringContext,
    span: Span,
    inner_ty: &Type,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> Result<crate::mir::Local, LoweringError> {
    let result_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let final_bb = ctx.new_basic_block();

    emit_option_ptr_eq_check(ctx, &lhs_op, &rhs_op, result_local, is_eq, final_bb, span);

    emit_option_lhs_null_check(ctx, &lhs_op, &rhs_op, result_local, is_eq, final_bb, span);

    emit_option_payload_compare(
        ctx,
        lhs_op,
        rhs_op,
        inner_ty,
        OptionCompareContext {
            result_local,
            is_eq,
            final_bb,
            span,
        },
    )?;

    ctx.set_current_block(final_bb);
    Ok(result_local)
}

/// Check if both options have the same pointer (optimization for pointer-identical values).
fn emit_option_ptr_eq_check(
    ctx: &mut LoweringContext,
    lhs_op: &Operand,
    rhs_op: &Operand,
    result_local: crate::mir::Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: Span,
) {
    let ptr_eq_bb = ctx.new_basic_block();
    let check_null_bb = ctx.new_basic_block();
    let ptr_eq_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(ptr_eq_local),
            Rvalue::BinaryOp(
                BinOp::Eq,
                Box::new(lhs_op.clone()),
                Box::new(rhs_op.clone()),
            ),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(ptr_eq_local)),
            targets: vec![(crate::mir::Discriminant::bool_true(), ptr_eq_bb)],
            otherwise: check_null_bb,
        },
        span,
    ));

    ctx.set_current_block(ptr_eq_bb);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: crate::ast::literal::Literal::Boolean(is_eq),
            }))),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));

    ctx.set_current_block(check_null_bb);
}

/// Settle the comparison when exactly one side is `None`.
///
/// A pointer-equality check has already run, so two `None`s never reach here;
/// either side being `None` therefore means the values differ.
fn emit_option_lhs_null_check(
    ctx: &mut LoweringContext,
    lhs_op: &Operand,
    rhs_op: &Operand,
    result_local: crate::mir::Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: Span,
) {
    emit_none_branch(ctx, lhs_op, result_local, !is_eq, final_bb, span);
    emit_none_branch(ctx, rhs_op, result_local, !is_eq, final_bb, span);
}

/// Branch to `final_bb` with `verdict` when `operand` is `None`.
///
/// Leaves the current block on the path where it was not `None`, so these
/// compose one after another.
fn emit_none_branch(
    ctx: &mut LoweringContext,
    operand: &Operand,
    result_local: crate::mir::Local,
    verdict: bool,
    final_bb: crate::mir::BasicBlock,
    span: Span,
) {
    let null_val = Operand::Constant(Box::new(Constant {
        span,
        ty: operand.ty(&ctx.body).clone(),
        literal: crate::ast::literal::Literal::None,
    }));
    let is_null = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(is_null),
            Rvalue::BinaryOp(BinOp::Eq, Box::new(operand.clone()), Box::new(null_val)),
        ),
        span,
    });

    let was_null_bb = ctx.new_basic_block();
    let not_null_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(is_null)),
            targets: vec![(crate::mir::Discriminant::bool_true(), was_null_bb)],
            otherwise: not_null_bb,
        },
        span,
    ));

    ctx.set_current_block(was_null_bb);
    assign_bool(ctx, result_local, verdict, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));

    ctx.set_current_block(not_null_bb);
}

/// Where a payload comparison leaves its verdict, and where it goes next.
struct OptionCompareContext {
    result_local: crate::mir::Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: Span,
}

/// Compare the payloads of two Some values.
fn emit_option_payload_compare(
    ctx: &mut LoweringContext,
    lhs_op: Operand,
    rhs_op: Operand,
    inner_ty: &Type,
    octx: OptionCompareContext,
) -> Result<(), LoweringError> {
    let OptionCompareContext {
        result_local,
        is_eq,
        final_bb,
        span,
    } = octx;
    let watermark = ctx.body.local_decls.len();

    // A constant operand — `None` written literally — has no place to project
    // from, so `ensure_place` materialises one. The watermark keeps an operand
    // that already had a place from being released here as well as by its owner.
    let lhs_place = crate::mir::lowering::helpers::ensure_place(ctx, lhs_op, span);
    let rhs_place = crate::mir::lowering::helpers::ensure_place(ctx, rhs_op, span);
    let lhs_base = lhs_place.local;
    let rhs_base = rhs_place.local;

    let payload = PlaceElem::Field(0);
    let lhs_payload = materialize_field(ctx, &lhs_place, payload.clone(), inner_ty, span);
    let rhs_payload = materialize_field(ctx, &rhs_place, payload, inner_ty, span);

    if ctx.is_perceus_managed(&inner_ty.kind) {
        ctx.register_scope_temp(lhs_payload);
        ctx.register_scope_temp(rhs_payload);
    }

    let verdict = emit_structural_equality(
        ctx,
        span,
        &inner_ty.kind,
        Operand::Copy(Place::new(lhs_payload)),
        Operand::Copy(Place::new(rhs_payload)),
        true,
    )?;

    ctx.emit_temp_drop(lhs_base, watermark, span);
    ctx.emit_temp_drop(rhs_base, watermark, span);

    settle_verdict(ctx, result_local, verdict, is_eq, final_bb, span);
    Ok(())
}

/// Store a comparison's verdict, inverted for `!=`, and jump to `final_bb`.
fn settle_verdict(
    ctx: &mut LoweringContext,
    result_local: crate::mir::Local,
    verdict: crate::mir::Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: Span,
) {
    let value = if is_eq {
        Operand::Copy(Place::new(verdict))
    } else {
        let negated = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(negated),
                Rvalue::UnaryOp(
                    crate::mir::UnOp::Not,
                    Box::new(Operand::Copy(Place::new(verdict))),
                ),
            ),
            span,
        });
        Operand::Copy(Place::new(negated))
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(value)),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));
}

/// Compare two values of a named type.
///
/// A type that defines `equals` is compared by that method at every nesting
/// level, matching what `==` does at the top level. A class that defines none
/// keeps reference identity, which is also what `==` gives it; the assertion
/// lowering refuses such a class up front rather than letting a test compare
/// addresses.
fn emit_named_type_equality(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    args: Option<&[crate::ast::expression::Expression]>,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> Result<crate::mir::Local, LoweringError> {
    if type_defines_own_equality(ctx, name) {
        return Ok(emit_equals_method_call(
            ctx, span, name, lhs_op, rhs_op, is_eq,
        ));
    }
    if matches!(
        ctx.type_checker.type_definitions().get(name),
        Some(crate::type_checker::context::TypeDefinition::Class(_))
    ) {
        return Ok(emit_scalar_equality(ctx, span, lhs_op, rhs_op, is_eq));
    }
    emit_enum_or_struct_equality(ctx, span, name, args, lhs_op, rhs_op, is_eq)
}

/// True when the named type supplies its own `equals`.
pub(super) fn type_defines_own_equality(ctx: &LoweringContext, name: &str) -> bool {
    match ctx.type_checker.type_definitions().get(name) {
        Some(crate::type_checker::context::TypeDefinition::Class(class_def)) => {
            class_def.methods.contains_key(EQUALS_METHOD_NAME)
        }
        Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) => {
            enum_def.methods.contains_key(EQUALS_METHOD_NAME)
        }
        _ => false,
    }
}

/// Emit `{Type}_equals(lhs, rhs)`, negating the result for `!=`.
fn emit_equals_method_call(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> crate::mir::Local {
    let result = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let mut args = vec![lhs_op, rhs_op];
    if let Some(&allocator) = ctx.variable_map.get("allocator") {
        args.push(Operand::Copy(Place::new(allocator)));
    }
    let next_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: identifier_constant(&format!("{}_{}", name, EQUALS_METHOD_NAME), span),
            args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(result),
            target: Some(next_bb),
        },
        span,
    ));
    ctx.set_current_block(next_bb);

    if is_eq {
        return result;
    }
    let negated = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(negated),
            Rvalue::UnaryOp(
                crate::mir::UnOp::Not,
                Box::new(Operand::Copy(Place::new(result))),
            ),
        ),
        span,
    });
    negated
}

/// Emit equality for enum or struct types.
fn emit_enum_or_struct_equality(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    args: Option<&[crate::ast::expression::Expression]>,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> Result<crate::mir::Local, LoweringError> {
    let Some(def) = ctx.type_checker.type_definitions().get(name) else {
        return Err(LoweringError::unsupported_expression(
            format!("unknown type: {}", name),
            span,
        ));
    };

    match def {
        crate::type_checker::context::TypeDefinition::Enum(_) => {
            emit_enum_equality(ctx, span, name, args, lhs_op, rhs_op, is_eq)
        }
        crate::type_checker::context::TypeDefinition::Struct(_) => {
            emit_struct_field_equality(ctx, span, name, lhs_op, rhs_op, is_eq, args)
        }
        crate::type_checker::context::TypeDefinition::Class(_)
        | crate::type_checker::context::TypeDefinition::Generic(_)
        | crate::type_checker::context::TypeDefinition::Alias(_)
        | crate::type_checker::context::TypeDefinition::Trait(_) => {
            Err(LoweringError::unsupported_expression(
                format!(
                    "structural equality not supported for this type definition: {}",
                    name
                ),
                span,
            ))
        }
    }
}

/// Emit equality for an enum value.
///
/// Two enum values are equal when their discriminants agree and every payload
/// of the selected variant is equal. The payloads are compared under a switch
/// on the discriminant rather than at a fixed slot count, because each variant
/// declares its own payload types: slot 1 of one variant and slot 1 of another
/// need not share a type, so a single comparison shape across all variants
/// would read at least one of them at the wrong type.
fn emit_enum_equality(
    ctx: &mut LoweringContext,
    span: Span,
    enum_name: &str,
    type_args: Option<&[crate::ast::expression::Expression]>,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> Result<crate::mir::Local, LoweringError> {
    let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
        ctx.type_checker.type_definitions().get(enum_name)
    else {
        return Err(LoweringError::unsupported_expression(
            format!("Enum '{}' not found", enum_name),
            span,
        ));
    };
    let generics = enum_def.generics.clone();
    let variants: Vec<Vec<Type>> = enum_def
        .variants
        .values()
        .map(|payload_types| concrete_payload_types(payload_types, type_args, generics.as_ref()))
        .collect();

    emit_tagged_union_equality(ctx, span, enum_name, &variants, lhs_op, rhs_op, is_eq)
}

/// Compare two values laid out as a discriminant followed by the selected
/// variant's payloads.
///
/// `variant_payloads` holds one entry per variant, in discriminant order,
/// carrying that variant's payload types already substituted.
fn emit_tagged_union_equality(
    ctx: &mut LoweringContext,
    span: Span,
    type_label: &str,
    variant_payloads: &[Vec<Type>],
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
) -> Result<crate::mir::Local, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let lhs_place = crate::mir::lowering::helpers::ensure_place(ctx, lhs_op, span);
    let rhs_place = crate::mir::lowering::helpers::ensure_place(ctx, rhs_op, span);

    let result_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let final_bb = ctx.new_basic_block();
    let same_discr_bb = ctx.new_basic_block();
    let discr_differ_bb = ctx.new_basic_block();

    let lhs_discr = emit_discriminant_agreement(
        ctx,
        &lhs_place,
        &rhs_place,
        DiscriminantBranch {
            result_local,
            is_eq,
            same_discr_bb,
            differ_bb: discr_differ_bb,
            final_bb,
            span,
        },
    );

    ctx.set_current_block(same_discr_bb);
    let (variant_blocks, corrupt_bb) =
        emit_variant_switch(ctx, lhs_discr, variant_payloads.len(), span);

    for (variant_idx, payload_types) in variant_payloads.iter().enumerate() {
        ctx.set_current_block(variant_blocks[variant_idx]);
        emit_variant_payload_equality(
            ctx,
            VariantEqualityContext {
                lhs_place: &lhs_place,
                rhs_place: &rhs_place,
                result_local,
                final_bb,
                watermark,
                is_eq,
                span,
            },
            payload_types,
        )?;
    }

    // A discriminant outside the declared variant set means the value is
    // corrupt; the rendering path reports the same condition the same way.
    ctx.set_current_block(corrupt_bb);
    emit_corrupt_discriminant_panic(ctx, type_label, final_bb, span);

    ctx.set_current_block(final_bb);
    Ok(result_local)
}

/// Switch on the discriminant to one block per variant.
///
/// Returns the per-variant blocks in discriminant order and the block reached
/// by a discriminant outside the declared set.
fn emit_variant_switch(
    ctx: &mut LoweringContext,
    discr: crate::mir::Local,
    variant_count: usize,
    span: Span,
) -> (Vec<crate::mir::BasicBlock>, crate::mir::BasicBlock) {
    let variant_blocks: Vec<crate::mir::BasicBlock> =
        (0..variant_count).map(|_| ctx.new_basic_block()).collect();
    let corrupt_bb = ctx.new_basic_block();
    let targets = variant_blocks
        .iter()
        .enumerate()
        .map(|(i, bb)| (crate::mir::Discriminant::new(i as u128), *bb))
        .collect();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(discr)),
            targets,
            otherwise: corrupt_bb,
        },
        span,
    ));
    (variant_blocks, corrupt_bb)
}

/// Where the discriminant comparison sends control, and what it records.
struct DiscriminantBranch {
    result_local: crate::mir::Local,
    is_eq: bool,
    same_discr_bb: crate::mir::BasicBlock,
    differ_bb: crate::mir::BasicBlock,
    final_bb: crate::mir::BasicBlock,
    span: Span,
}

/// Branch on whether the two values select the same variant.
///
/// Values of different variants are unequal whatever their payloads hold, so
/// that path is settled here. Returns the left discriminant, which the caller
/// switches on to reach the matching variant's payloads.
fn emit_discriminant_agreement(
    ctx: &mut LoweringContext,
    lhs_place: &Place,
    rhs_place: &Place,
    branch: DiscriminantBranch,
) -> crate::mir::Local {
    let span = branch.span;
    let lhs_discr = read_discriminant(ctx, lhs_place, span);
    let rhs_discr = read_discriminant(ctx, rhs_place, span);

    let discr_eq = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(discr_eq),
            Rvalue::BinaryOp(
                BinOp::Eq,
                Box::new(Operand::Copy(Place::new(lhs_discr))),
                Box::new(Operand::Copy(Place::new(rhs_discr))),
            ),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(discr_eq)),
            targets: vec![(crate::mir::Discriminant::bool_true(), branch.same_discr_bb)],
            otherwise: branch.differ_bb,
        },
        span,
    ));

    ctx.set_current_block(branch.differ_bb);
    assign_bool(ctx, branch.result_local, !branch.is_eq, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: branch.final_bb,
        },
        span,
    ));

    lhs_discr
}

/// Substitute the enum's generic parameters into its declared payload types.
fn concrete_payload_types(
    payload_types: &[Type],
    type_args: Option<&[crate::ast::expression::Expression]>,
    generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
) -> Vec<Type> {
    let Some(args) = type_args else {
        return payload_types.to_vec();
    };
    payload_types
        .iter()
        .map(|ty| {
            let substituted = crate::type_checker::generics::substitute_generic_field_kind(
                &ty.kind,
                Some(args),
                generics,
            );
            Type::new(substituted, ty.span)
        })
        .collect()
}

/// The places and bookkeeping a single variant's payload comparison needs.
struct VariantEqualityContext<'a> {
    lhs_place: &'a Place,
    rhs_place: &'a Place,
    result_local: crate::mir::Local,
    final_bb: crate::mir::BasicBlock,
    watermark: usize,
    is_eq: bool,
    span: Span,
}

/// Compare every payload of one variant, leaving the verdict in `result_local`.
///
/// A payload-less variant is settled by its discriminant alone.
fn emit_variant_payload_equality(
    ctx: &mut LoweringContext,
    vctx: VariantEqualityContext<'_>,
    payload_types: &[Type],
) -> Result<(), LoweringError> {
    let span = vctx.span;
    let mut verdict: Option<crate::mir::Local> = None;

    for (payload_idx, payload_ty) in payload_types.iter().enumerate() {
        let field = PlaceElem::Field(payload_idx + 1);
        let lhs_payload = materialize_field(ctx, vctx.lhs_place, field.clone(), payload_ty, span);
        let rhs_payload = materialize_field(ctx, vctx.rhs_place, field, payload_ty, span);

        let payload_eq = emit_structural_equality(
            ctx,
            span,
            &payload_ty.kind,
            Operand::Copy(Place::new(lhs_payload)),
            Operand::Copy(Place::new(rhs_payload)),
            true,
        )?;

        if ctx.is_perceus_managed(&payload_ty.kind) {
            ctx.emit_temp_drop(lhs_payload, vctx.watermark, span);
            ctx.emit_temp_drop(rhs_payload, vctx.watermark, span);
        }

        verdict = Some(match verdict {
            None => payload_eq,
            Some(acc) => {
                let combined = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(combined),
                        Rvalue::BinaryOp(
                            BinOp::BitAnd,
                            Box::new(Operand::Copy(Place::new(acc))),
                            Box::new(Operand::Copy(Place::new(payload_eq))),
                        ),
                    ),
                    span,
                });
                combined
            }
        });
    }

    match verdict {
        None => {
            // A variant with no payloads is settled by its discriminant alone.
            assign_bool(ctx, vctx.result_local, vctx.is_eq, span);
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto {
                    target: vctx.final_bb,
                },
                span,
            ));
        }
        Some(all_equal) => settle_verdict(
            ctx,
            vctx.result_local,
            all_equal,
            vctx.is_eq,
            vctx.final_bb,
            span,
        ),
    }
    Ok(())
}

/// Read a field into a temp declared with the field's own type.
///
/// `Operand::ty` ignores projections, so reading a projected place directly
/// reads it at the base slot's width — which renders a float payload as its
/// own bit pattern. Materializing through a typed temp first is what keeps the
/// read at the payload's width.
fn materialize_field(
    ctx: &mut LoweringContext,
    base: &Place,
    field: PlaceElem,
    field_ty: &Type,
    span: Span,
) -> crate::mir::Local {
    let temp = ctx.push_temp(field_ty.clone(), span);
    let mut place = base.clone();
    place.projection.push(field);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(Operand::Copy(place))),
        span,
    });
    temp
}

/// Read an enum's discriminant, held at the first slot, into an `int` temp.
fn read_discriminant(ctx: &mut LoweringContext, place: &Place, span: Span) -> crate::mir::Local {
    materialize_field(
        ctx,
        place,
        PlaceElem::Field(0),
        &Type::new(TypeKind::Int, span),
        span,
    )
}

/// Assign a boolean constant to `local`.
fn assign_bool(ctx: &mut LoweringContext, local: crate::mir::Local, value: bool, span: Span) {
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: crate::ast::literal::Literal::Boolean(value),
            }))),
        ),
        span,
    });
}

/// Abort on a discriminant outside the enum's declared variant set.
fn emit_corrupt_discriminant_panic(
    ctx: &mut LoweringContext,
    enum_name: &str,
    final_bb: crate::mir::BasicBlock,
    span: Span,
) {
    let message = format!(
        "Enum '{}' has an invalid discriminant (corrupt value)",
        enum_name
    );
    let message_temp = ctx.push_temp(Type::new(TypeKind::String, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(message_temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::String, span),
                literal: crate::ast::literal::Literal::String(message),
            }))),
        ),
        span,
    });
    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: identifier_constant(rt::PANIC, span),
            args: vec![Operand::Copy(Place::new(message_temp))],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(void_temp),
            target: Some(final_bb),
        },
        span,
    ));
}

/// Emit equality for struct fields - field-wise recursive comparison.
fn emit_struct_field_equality(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    lhs_op: Operand,
    rhs_op: Operand,
    is_eq: bool,
    _args: Option<&[crate::ast::expression::Expression]>,
) -> Result<crate::mir::Local, LoweringError> {
    let Some(crate::type_checker::context::TypeDefinition::Struct(struct_def)) =
        ctx.type_checker.type_definitions().get(name)
    else {
        return Err(LoweringError::unsupported_expression(
            format!("unknown struct: {}", name),
            span,
        ));
    };
    let field_types: Vec<Type> = struct_def
        .fields
        .iter()
        .map(|(_, field_ty, _)| field_ty.clone())
        .collect();

    let lhs_place = crate::mir::lowering::helpers::ensure_place(ctx, lhs_op, span);
    let rhs_place = crate::mir::lowering::helpers::ensure_place(ctx, rhs_op, span);

    // A struct has no discriminant: every field must agree, so the verdict
    // starts true and each field narrows it.
    let mut verdict = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    assign_bool(ctx, verdict, true, span);

    for (field_idx, field_ty) in field_types.iter().enumerate() {
        let field_eq = emit_field_equality(ctx, &lhs_place, &rhs_place, field_idx, field_ty, span)?;
        verdict = emit_and(ctx, verdict, field_eq, span);
    }

    if is_eq {
        return Ok(verdict);
    }
    let negated = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(negated),
            Rvalue::UnaryOp(
                crate::mir::UnOp::Not,
                Box::new(Operand::Copy(Place::new(verdict))),
            ),
        ),
        span,
    });
    Ok(negated)
}

/// Compare one field of two struct values, reading it at its declared type.
fn emit_field_equality(
    ctx: &mut LoweringContext,
    lhs_place: &Place,
    rhs_place: &Place,
    field_idx: usize,
    field_ty: &Type,
    span: Span,
) -> Result<crate::mir::Local, LoweringError> {
    let field = PlaceElem::Field(field_idx);
    let lhs_field = materialize_field(ctx, lhs_place, field.clone(), field_ty, span);
    let rhs_field = materialize_field(ctx, rhs_place, field, field_ty, span);

    if ctx.is_perceus_managed(&field_ty.kind) {
        ctx.register_scope_temp(lhs_field);
        ctx.register_scope_temp(rhs_field);
    }

    emit_structural_equality(
        ctx,
        span,
        &field_ty.kind,
        Operand::Copy(Place::new(lhs_field)),
        Operand::Copy(Place::new(rhs_field)),
        true,
    )
}

/// Combine two boolean locals with `&`.
fn emit_and(
    ctx: &mut LoweringContext,
    lhs: crate::mir::Local,
    rhs: crate::mir::Local,
    span: Span,
) -> crate::mir::Local {
    let combined = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(combined),
            Rvalue::BinaryOp(
                BinOp::BitAnd,
                Box::new(Operand::Copy(Place::new(lhs))),
                Box::new(Operand::Copy(Place::new(rhs))),
            ),
        ),
        span,
    });
    combined
}

/// Emit a scalar equality using primitive BinOp::Eq.
fn emit_scalar_equality(
    ctx: &mut LoweringContext,
    span: Span,
    lhs: Operand,
    rhs: Operand,
    is_eq: bool,
) -> crate::mir::Local {
    let result = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let op = if is_eq { BinOp::Eq } else { BinOp::Ne };

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result),
            Rvalue::BinaryOp(op, Box::new(lhs), Box::new(rhs)),
        ),
        span,
    });

    result
}

/// Helper to construct an identifier Constant operand.
fn identifier_constant(name: &str, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(name.to_string()),
    }))
}
