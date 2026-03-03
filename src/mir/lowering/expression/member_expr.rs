// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    AggregateKind, Constant, Dimension, GpuIntrinsic, Operand, Place, PlaceElem, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use std::rc::Rc;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::{ensure_place, resolve_type};

pub(crate) fn lower_member_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Member(obj, prop) = &expr.node else {
        unreachable!()
    };
    let obj_operand = lower_expression(ctx, obj, None)?;

    // Handle GPU Intrinsics (gpu_context.thread_idx.x, etc.)
    // This uses a two-step lowering: gpu_context.thread_idx => intermediate symbol,
    // then intermediate_symbol.x => actual GpuIntrinsic rvalue.
    if let Operand::Constant(c) = &obj_operand {
        if let crate::ast::literal::Literal::Symbol(sym) = &c.literal {
            if sym == "gpu_context" {
                if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                    // Return intermediate symbol for chained access
                    return Ok(Operand::Constant(Box::new(Constant {
                        span: expr.span,
                        ty: Type::new(TypeKind::Void, expr.span),
                        literal: crate::ast::literal::Literal::Symbol(format!(
                            "gpu_context.{}",
                            prop_name
                        )),
                    })));
                }
            } else if sym.starts_with("gpu_context.") {
                if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                    let dim = match prop_name.as_str() {
                        "x" => Dimension::X,
                        "y" => Dimension::Y,
                        "z" => Dimension::Z,
                        _ => {
                            return Err(LoweringError::unsupported_expression(
                                format!("Invalid GPU dimension: {}", prop_name),
                                expr.span,
                            ));
                        }
                    };

                    let rvalue = match sym.as_str() {
                        "gpu_context.thread_idx" => {
                            Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(dim))
                        }
                        "gpu_context.block_idx" => {
                            Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(dim))
                        }
                        "gpu_context.block_dim" => {
                            Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(dim))
                        }
                        "gpu_context.grid_dim" => Rvalue::GpuIntrinsic(GpuIntrinsic::GridDim(dim)),
                        _ => {
                            return Err(LoweringError::unsupported_expression(
                                format!("Unknown GPU intrinsic: {}", sym),
                                expr.span,
                            ));
                        }
                    };

                    match &dest {
                        Some(d) => {
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(d.clone(), rvalue),
                                span: expr.span,
                            });
                            return Ok(Operand::Copy(d.clone()));
                        }
                        None => {
                            let temp =
                                ctx.push_temp(Type::new(TypeKind::Int, expr.span), expr.span);
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(Place::new(temp), rvalue),
                                span: expr.span,
                            });
                            return Ok(Operand::Copy(Place::new(temp)));
                        }
                    }
                }
            }
        }
    }

    // 2. Handle General Struct Member Access
    let obj_ty = if let Some(ty) = ctx.type_checker.get_type(obj.id) {
        ty
    } else {
        return Err(LoweringError::type_not_found(obj.id, expr.span));
    };

    // Handle Tuple Member Access
    if let TypeKind::Tuple(elements) = &obj_ty.kind {
        if let ExpressionKind::Literal(crate::ast::literal::Literal::Integer(val)) = &prop.node {
            let idx = match val {
                crate::ast::literal::IntegerLiteral::I8(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::I16(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::I32(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::I64(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::I128(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::U8(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::U16(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::U32(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::U64(v) => *v as usize,
                crate::ast::literal::IntegerLiteral::U128(v) => *v as usize,
            };

            let obj_place = ensure_place(ctx, obj_operand, obj.span);

            let mut target_place = obj_place;
            target_place.projection.push(PlaceElem::Field(idx));

            let element_ty = resolve_type(ctx.type_checker, &elements[idx]);

            let operand = if element_ty.is_copy() {
                Operand::Copy(target_place.clone())
            } else {
                Operand::Move(target_place.clone())
            };

            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(operand)),
                    span: expr.span,
                });
                return Ok(Operand::Copy(d));
            } else {
                return Ok(operand);
            }
        }
    }

    if let TypeKind::Custom(struct_name, _) = &obj_ty.kind {
        // Find field index
        // We need to look up the struct definition in the type checker.
        // The type checker doesn't expose a direct "get_field_index" method,
        // but we can look up the definition.
        // Note: Global type definitions are available.
        if let Some(crate::type_checker::context::TypeDefinition::Struct(def)) =
            ctx.type_checker.global_type_definitions.get(struct_name)
        {
            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                if let Some(idx) = def.fields.iter().position(|(f, _, _)| f == field_name) {
                    let place = ensure_place(ctx, obj_operand, obj.span);

                    // Create new place with projection
                    let mut new_place = place.clone();
                    new_place.projection.push(PlaceElem::Field(idx));

                    if let Some(d) = dest {
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(
                                d.clone(),
                                Rvalue::Use(Operand::Copy(new_place)),
                            ),
                            span: expr.span,
                        });
                        return Ok(Operand::Copy(d));
                    } else {
                        return Ok(Operand::Copy(new_place));
                    }
                } else {
                    return Err(LoweringError::unsupported_lhs(
                        format!(
                            "Field '{}' not found in struct '{}'",
                            field_name, struct_name
                        ),
                        obj.span,
                    ));
                }
            }
        }

        // Also check for class field access
        if let Some(crate::type_checker::context::TypeDefinition::Class(def)) =
            ctx.type_checker.global_type_definitions.get(struct_name)
        {
            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                // Check fields in BTreeMap (note: index is stored in FieldInfo)
                if let Some((idx, _)) = def
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, (f, _))| *f == field_name)
                {
                    let place = ensure_place(ctx, obj_operand, obj.span);

                    // Create new place with projection
                    let mut new_place = place.clone();
                    new_place.projection.push(PlaceElem::Field(idx));

                    if let Some(d) = dest {
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(
                                d.clone(),
                                Rvalue::Use(Operand::Copy(new_place)),
                            ),
                            span: expr.span,
                        });
                        return Ok(Operand::Copy(d));
                    } else {
                        return Ok(Operand::Copy(new_place));
                    }
                }
                // If field not found, might be a method call, which is handled in Call
            }
        }
    }

    // 3. Handle Enum Unit Variant Access (e.g., Status.Ok)
    // Check if obj is a type identifier and prop is an enum variant
    if let ExpressionKind::Identifier(type_name, _) = &obj.node {
        if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
            ctx.type_checker.global_type_definitions.get(type_name)
        {
            if let ExpressionKind::Identifier(variant_name, _) = &prop.node {
                if let Some((discriminant, _)) = enum_def
                    .variants
                    .iter()
                    .enumerate()
                    .find(|(_, (name, _))| name.as_str() == variant_name)
                {
                    // Unit variant: create Aggregate with just discriminant
                    let associated_types = match enum_def.variants.get(variant_name) {
                        Some(types) => types,
                        None => {
                            return Err(LoweringError::unsupported_expression(
                                format!(
                                    "Unknown variant '{}' for enum '{}'",
                                    variant_name, type_name
                                ),
                                expr.span,
                            ));
                        }
                    };
                    if associated_types.is_empty() {
                        let ty = resolve_type(ctx.type_checker, expr);

                        // Create discriminant constant
                        let discr_op = Operand::Constant(Box::new(Constant {
                            span: expr.span,
                            ty: Type::new(TypeKind::Int, expr.span),
                            literal: crate::ast::literal::Literal::Integer(
                                crate::ast::literal::IntegerLiteral::I32(discriminant as i32),
                            ),
                        }));

                        let enum_type_rc: Rc<str> = Rc::from(type_name.as_str());
                        let enum_variant_rc: Rc<str> = Rc::from(variant_name.as_str());

                        if let Some(d) = dest {
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(
                                    d.clone(),
                                    Rvalue::Aggregate(
                                        AggregateKind::Enum(enum_type_rc, enum_variant_rc),
                                        vec![discr_op],
                                    ),
                                ),
                                span: expr.span,
                            });
                            return Ok(Operand::Copy(d));
                        } else {
                            let temp = ctx.push_temp(ty, expr.span);
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(
                                    Place::new(temp),
                                    Rvalue::Aggregate(
                                        AggregateKind::Enum(enum_type_rc, enum_variant_rc),
                                        vec![discr_op],
                                    ),
                                ),
                                span: expr.span,
                            });
                            return Ok(Operand::Copy(Place::new(temp)));
                        }
                    }
                    // Variant with associated values - handled in Call
                }
            }
        }
    }

    // Handle Option member access — delegate to the inner type.
    // For example, Option<String>.length should still work via unwrap semantics.
    // Note: most Option method calls (unwrap, is_some, is_none) are handled in
    // lower_call in control_flow.rs. This handles property-style access.
    if let TypeKind::Option(_) = &obj_ty.kind {
        // Option member access as property is not supported directly.
        // Method calls like .unwrap(), .is_some(), .is_none() go through lower_call.
        // If we get here, it means a non-call member access on Option which the
        // type checker should have rejected. Return a clear error.
        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
            return Err(LoweringError::unsupported_expression(
                format!(
                    "Cannot access property '{}' on optional type '{}'. Use .unwrap() first",
                    prop_name, obj_ty
                ),
                expr.span,
            ));
        }
    }

    // Handle class method-as-property access (e.g. s.length, s.size).
    // Zero-arg methods on class types can be accessed as properties.
    let class_name = match &obj_ty.kind {
        TypeKind::String => Some("String".to_string()),
        TypeKind::Custom(name, _) => Some(name.clone()),
        _ => None,
    };

    if let Some(class_name) = class_name {
        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
            if let Some(crate::type_checker::context::TypeDefinition::Class(class_def)) =
                ctx.type_checker.global_type_definitions.get(&class_name)
            {
                if let Some(method_info) = class_def.methods.get(prop_name.as_str()) {
                    // Only treat zero-arg methods as property access
                    if method_info.params.is_empty() {
                        let mangled_name = format!("{}_{}", class_name, prop_name);
                        let return_ty = method_info.return_type.clone();

                        let func_op = Operand::Constant(Box::new(Constant {
                            span: expr.span,
                            ty: crate::ast::types::Type::new(TypeKind::Symbol, expr.span),
                            literal: crate::ast::literal::Literal::Symbol(mangled_name),
                        }));

                        let mut call_args = vec![obj_operand];
                        if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
                            call_args.push(Operand::Copy(Place::new(alloc_local)));
                        }

                        let (destination, op) = if let Some(d) = dest {
                            (d.clone(), Operand::Copy(d))
                        } else {
                            let temp = ctx.push_temp(return_ty, expr.span);
                            let p = Place::new(temp);
                            (p.clone(), Operand::Copy(p))
                        };

                        let target_bb = ctx.new_basic_block();
                        ctx.set_terminator(Terminator::new(
                            TerminatorKind::Call {
                                func: func_op,
                                args: call_args,
                                destination,
                                target: Some(target_bb),
                            },
                            expr.span,
                        ));
                        ctx.set_current_block(target_bb);
                        return Ok(op);
                    }
                }
            }
        }
    }

    Err(LoweringError::unsupported_expression(
        format!("Unsupported member access on type: {}", obj_ty),
        expr.span,
    ))
}
