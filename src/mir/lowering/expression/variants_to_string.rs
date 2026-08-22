// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for rendering Option, Result, and enum types as strings.
//!
//! This module handles the inline expansion of f-string interpolation for
//! variant types (Option, Result, and user-defined enums). Each variant type
//! is rendered by switching on its discriminant and rendering each variant
//! with its payload(s).

use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{emit_string_concat, emit_to_string};
use crate::mir::terminator::Discriminant;
use crate::mir::{
    BasicBlock, Constant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind,
};

/// Bundle of parameters that flow together through variant rendering functions.
struct VariantRenderContext {
    result_local: crate::mir::place::Local,
    watermark: usize,
    join_block: BasicBlock,
    span: Span,
}

/// Emit MIR to render an Option value as a string.
///
/// Generates a SwitchInt on the option discriminant:
/// - discriminant 0: "None"
/// - otherwise: "Some(" + payload + ")"
pub(super) fn emit_option_to_string(
    ctx: &mut LoweringContext,
    operand: Operand,
    inner_ty: &Type,
    span: &crate::error::syntax::Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    // Watermark: any local created from here on is a temp belonging to this operation.
    let watermark = ctx.body.local_decls.len();

    // Materialize the operand as a place so we can read the discriminant.
    let option_place = crate::mir::lowering::helpers::ensure_place(ctx, operand, *span);

    // Create the result String temp that will hold the final output.
    let result_local = ctx.push_temp(Type::new(TypeKind::String, *span), *span);

    // Create blocks for None and Some branches, plus a join block.
    let none_block = ctx.new_basic_block();
    let some_block = ctx.new_basic_block();
    let join_block = ctx.new_basic_block();

    // Switch on the option itself (Option is a nullable pointer: 0 = None, non-zero = Some).
    // The Some branch is the otherwise/default case since any non-zero value is Some.
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(option_place.clone()),
            targets: vec![(Discriminant::new(0), none_block)],
            otherwise: some_block,
        },
        *span,
    ));

    // None block: assign "None" to result
    ctx.set_current_block(none_block);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: *span,
                ty: Type::new(TypeKind::String, *span),
                literal: Literal::String("None".to_string()),
            }))),
        ),
        span: *span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_block },
        *span,
    ));

    // Some block: render "Some(" + payload + ")"
    ctx.set_current_block(some_block);
    emit_option_some_block(
        ctx,
        option_place,
        inner_ty,
        VariantRenderContext {
            result_local,
            watermark,
            join_block,
            span: *span,
        },
    )?;

    // Join block: leave with result_local
    ctx.set_current_block(join_block);
    Ok(result_local)
}

/// Render the string representation of a single enum variant.
///
/// Returns the Local holding the rendered variant string.
/// Handles both payload-less variants (just the name) and
/// variants with payloads (name + comma-separated payloads + closing paren).
fn emit_enum_variant_string(
    ctx: &mut LoweringContext,
    variant_name: &str,
    payload_types: &[Type],
    concrete_payload_types: &[Type],
    enum_place: &Place,
    watermark: usize,
    span: &crate::error::syntax::Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    if payload_types.is_empty() {
        // Payload-less variant: just the name.
        emit_string_literal(ctx, variant_name, *span)
    } else {
        // Variant with payloads: "Name(" + payloads + ")"
        let name_open = format!("{}(", variant_name);
        let name_open_local = emit_string_literal(ctx, &name_open, *span)?;

        // Render each payload and concatenate with commas.
        let mut acc = name_open_local;
        for (payload_idx, payload_ty) in concrete_payload_types.iter().enumerate() {
            // Create a temp local with the correct payload type to ensure
            // the field read happens with the proper width (not the base slot width).
            let payload_temp = ctx.push_temp(payload_ty.clone(), *span);

            // Read the payload at Field(payload_idx + 1) and assign to the temp.
            let mut payload_place = enum_place.clone();
            payload_place
                .projection
                .push(PlaceElem::Field(payload_idx + 1));
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(payload_temp),
                    Rvalue::Use(Operand::Copy(payload_place)),
                ),
                span: *span,
            });

            // Render the payload from the temp (now the field is read with the correct width).
            let payload_operand = Operand::Copy(Place::new(payload_temp));
            let payload_str = emit_to_string(ctx, payload_operand, &payload_ty.kind, span)?;

            // Drop the payload temp (which held the reference from the field read).
            // This balances the reference created by the assignment from the projected field.
            if ctx.is_perceus_managed(&payload_ty.kind) {
                ctx.emit_temp_drop(payload_temp, watermark, *span);
            }

            // Concatenate: acc + payload
            let new_acc = emit_string_concat(ctx, acc, payload_str, span)?;
            // Release the old acc and payload_str
            ctx.emit_temp_drop(acc, watermark, *span);
            ctx.emit_temp_drop(payload_str, watermark, *span);
            acc = new_acc;

            // Add comma separator if not the last payload.
            if payload_idx < concrete_payload_types.len() - 1 {
                let comma = emit_string_literal(ctx, ", ", *span)?;
                let new_acc = emit_string_concat(ctx, acc, comma, span)?;
                // Release the old acc and comma
                ctx.emit_temp_drop(acc, watermark, *span);
                ctx.emit_temp_drop(comma, watermark, *span);
                acc = new_acc;
            }
        }

        // Add closing paren.
        let close_paren = emit_string_literal(ctx, ")", *span)?;
        let final_str = emit_string_concat(ctx, acc, close_paren, span)?;
        // Release acc and close_paren
        ctx.emit_temp_drop(acc, watermark, *span);
        ctx.emit_temp_drop(close_paren, watermark, *span);
        Ok(final_str)
    }
}

/// Emit MIR for the Some branch of Option rendering.
///
/// Renders "Some(" + payload + ")", assigns to result_local, and branches to join_block.
fn emit_option_some_block(
    ctx: &mut LoweringContext,
    option_place: Place,
    inner_ty: &Type,
    render_ctx: VariantRenderContext,
) -> Result<(), LoweringError> {
    // Create a temp local with the correct payload type to ensure
    // the field read happens with the proper width (not the base slot width).
    let payload_temp = ctx.push_temp(inner_ty.clone(), render_ctx.span);

    // Read the payload at Field(0) and assign to the temp.
    let mut payload_place = option_place;
    payload_place.projection.push(PlaceElem::Field(0));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(payload_temp),
            Rvalue::Use(Operand::Copy(payload_place)),
        ),
        span: render_ctx.span,
    });

    // Render the payload to a string recursively (from the temp with correct width).
    let payload_operand = Operand::Copy(Place::new(payload_temp));
    let payload_string = emit_to_string(ctx, payload_operand, &inner_ty.kind, &render_ctx.span)?;

    // Drop the payload temp (which held the reference from the field read).
    // This balances the reference created by the assignment from the projected field.
    if ctx.is_perceus_managed(&inner_ty.kind) {
        ctx.emit_temp_drop(payload_temp, render_ctx.watermark, render_ctx.span);
    }

    // Build the result: "Some(" + payload + ")"
    let some_prefix = emit_string_literal(ctx, "Some(", render_ctx.span)?;
    let concat1_result = emit_string_concat(ctx, some_prefix, payload_string, &render_ctx.span)?;
    // Release the Some( literal and the payload string after concat
    ctx.emit_temp_drop(some_prefix, render_ctx.watermark, render_ctx.span);
    ctx.emit_temp_drop(payload_string, render_ctx.watermark, render_ctx.span);

    let close_paren = emit_string_literal(ctx, ")", render_ctx.span)?;
    let final_result = emit_string_concat(ctx, concat1_result, close_paren, &render_ctx.span)?;
    // Release the concat result and close paren
    ctx.emit_temp_drop(concat1_result, render_ctx.watermark, render_ctx.span);
    ctx.emit_temp_drop(close_paren, render_ctx.watermark, render_ctx.span);

    // Assign to result_local
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(render_ctx.result_local),
            Rvalue::Use(Operand::Copy(Place::new(final_result))),
        ),
        span: render_ctx.span,
    });

    // Release final_result (which is now copied to result_local)
    ctx.emit_temp_drop(final_result, render_ctx.watermark, render_ctx.span);

    // Go to join block
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: render_ctx.join_block,
        },
        render_ctx.span,
    ));

    Ok(())
}

/// Generate blocks for each variant of an enum, rendering their string representations.
fn emit_enum_variant_blocks(
    ctx: &mut LoweringContext,
    enum_variants: &std::collections::BTreeMap<String, Vec<Type>>,
    type_args: Option<&[crate::ast::expression::Expression]>,
    generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
    variant_blocks: &[crate::mir::BasicBlock],
    enum_place: &Place,
    render_ctx: VariantRenderContext,
) -> Result<(), LoweringError> {
    // Generate code for each variant.
    for (variant_idx, (variant_name, payload_types)) in enum_variants.iter().enumerate() {
        ctx.set_current_block(variant_blocks[variant_idx]);

        // Substitute generic type parameters if present.
        let concrete_payload_types = if let Some(args) = type_args {
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
                .collect::<Vec<_>>()
        } else {
            payload_types.clone()
        };

        let variant_str = emit_enum_variant_string(
            ctx,
            variant_name,
            payload_types,
            &concrete_payload_types,
            enum_place,
            render_ctx.watermark,
            &render_ctx.span,
        )?;

        // Assign to result_local.
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(render_ctx.result_local),
                Rvalue::Use(Operand::Copy(Place::new(variant_str))),
            ),
            span: render_ctx.span,
        });

        // Release variant_str (which is now copied to result_local)
        ctx.emit_temp_drop(variant_str, render_ctx.watermark, render_ctx.span);

        // Go to join block.
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto {
                target: render_ctx.join_block,
            },
            render_ctx.span,
        ));
    }

    Ok(())
}

/// Emit MIR to render an enum value as a string.
///
/// For each variant, generates a block that renders the variant name
/// and its payloads (comma-separated if multiple).
pub(super) fn emit_enum_to_string(
    ctx: &mut LoweringContext,
    operand: Operand,
    enum_name: &str,
    type_args: Option<&[crate::ast::expression::Expression]>,
    span: &crate::error::syntax::Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    // Watermark: track any temps created during this operation.
    let watermark = ctx.body.local_decls.len();

    // Get the enum definition from the type table.
    let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) = ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(enum_name)
    else {
        return Err(LoweringError::unsupported_expression(
            format!("Enum '{}' not found", enum_name),
            *span,
        ));
    };

    // Materialize the operand as a place.
    let enum_place = crate::mir::lowering::helpers::ensure_place(ctx, operand, *span);

    // Create result temp
    let result_local = ctx.push_temp(Type::new(TypeKind::String, *span), *span);

    // Create blocks for each variant, plus a join block and a panic block.
    let variant_count = enum_def.variants.len();
    let mut variant_blocks = Vec::with_capacity(variant_count);
    let join_block = ctx.new_basic_block();
    let panic_block = ctx.new_basic_block();
    for _ in 0..variant_count {
        variant_blocks.push(ctx.new_basic_block());
    }

    // Read the discriminant at Field(0).
    let mut disc_place = enum_place.clone();
    disc_place.projection.push(PlaceElem::Field(0));

    // Switch on the discriminant.
    let mut targets = Vec::with_capacity(variant_count);
    for (i, _) in enum_def.variants.iter().enumerate() {
        targets.push((Discriminant::new(i as u128), variant_blocks[i]));
    }
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(disc_place),
            targets,
            otherwise: panic_block,
        },
        *span,
    ));

    // Generate code for each variant.
    emit_enum_variant_blocks(
        ctx,
        &enum_def.variants,
        type_args,
        enum_def.generics.as_ref(),
        &variant_blocks,
        &enum_place,
        VariantRenderContext {
            result_local,
            watermark,
            join_block,
            span: *span,
        },
    )?;

    // Panic block: out-of-range discriminant indicates a corrupt value.
    ctx.set_current_block(panic_block);
    let panic_msg = format!(
        "Enum '{}' has an invalid discriminant (corrupt value)",
        enum_name
    );
    let panic_msg_temp = ctx.push_temp(Type::new(TypeKind::String, *span), *span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(panic_msg_temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: *span,
                ty: Type::new(TypeKind::String, *span),
                literal: Literal::String(panic_msg),
            }))),
        ),
        span: *span,
    });
    let panic_func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: Literal::Identifier(crate::runtime_fns::rt::PANIC.to_string()),
    }));
    let panic_msg_op = Operand::Copy(Place::new(panic_msg_temp));
    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: panic_func_op,
            args: vec![panic_msg_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(void_temp),
            target: Some(join_block),
        },
        *span,
    ));

    // Join block
    ctx.set_current_block(join_block);
    Ok(result_local)
}

/// Emit a constant string literal and return its local.
fn emit_string_literal(
    ctx: &mut LoweringContext,
    text: &str,
    span: crate::error::syntax::Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    let temp = ctx.push_temp(Type::new(TypeKind::String, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::String, span),
                literal: Literal::String(text.to_string()),
            }))),
        ),
        span,
    });
    Ok(temp)
}
