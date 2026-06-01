// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind, GPU_CONTEXT_DEPRECATED_IDENT, KERNEL_CONTEXT_IDENT};
use crate::error::lowering::LoweringError;
use crate::mir::{
    AggregateKind, Constant, Dimension, GpuIntrinsic, Operand, Place, PlaceElem, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use std::rc::Rc;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::{ensure_place, resolve_type};

/// Returns the kernel-context field name in `<kernel>.<field>` intermediate
/// identifiers (e.g. `"thread_idx"` for `"kernel.thread_idx"`), or `None` when
/// `sym` is not a kernel-context member access.
fn kernel_context_field(sym: &str) -> Option<&str> {
    sym.strip_prefix(KERNEL_CONTEXT_IDENT)
        .and_then(|rest| rest.strip_prefix('.'))
}

/// Extract integer index from an IntegerLiteral.
fn extract_integer_index(val: &crate::ast::literal::IntegerLiteral) -> usize {
    match val {
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
    }
}

/// Lower a field access on a tuple type.
fn lower_tuple_field_access(
    ctx: &mut LoweringContext,
    mut obj_place: Place,
    idx: usize,
    elements: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    obj_place.projection.push(PlaceElem::Field(idx));
    let element_ty = resolve_type(ctx.type_checker, &elements[idx]);

    let operand = if ctx.is_type_auto_copy(&element_ty) {
        Operand::Copy(obj_place.clone())
    } else {
        Operand::Move(obj_place.clone())
    };

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(operand)),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(operand)
    }
}

/// Lower a field access on a struct or class type.
fn lower_custom_field_access(
    ctx: &mut LoweringContext,
    mut place: Place,
    idx: usize,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    place.projection.push(PlaceElem::Field(idx));

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(Operand::Copy(place))),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(Operand::Copy(place))
    }
}

/// Lower a unit enum variant access (e.g., Status.Ok).
fn lower_enum_unit_variant(
    ctx: &mut LoweringContext,
    type_name: &str,
    variant_name: &str,
    discriminant: usize,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ty = resolve_type(ctx.type_checker, expr);
    let discr_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Int, expr.span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(
            discriminant as i32,
        )),
    }));

    let enum_type_rc: Rc<str> = Rc::from(type_name);
    let enum_variant_rc: Rc<str> = Rc::from(variant_name);

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
        Ok(Operand::Copy(d))
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
        Ok(Operand::Copy(Place::new(temp)))
    }
}

/// Lower a zero-argument method accessed as a property (e.g., s.length).
fn lower_zero_arg_method_as_property(
    ctx: &mut LoweringContext,
    class_name: &str,
    method_name: &str,
    return_ty: &Type,
    obj_operand: Operand,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let mut mangled_name = String::with_capacity(class_name.len() + 1 + method_name.len());
    mangled_name.push_str(class_name);
    mangled_name.push('_');
    mangled_name.push_str(method_name);

    let func_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(mangled_name),
    }));

    let mut call_args = vec![obj_operand];
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }

    let (destination, op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty.clone(), expr.span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args,
            out_args: Vec::new(),
            destination,
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);
    Ok(op)
}

fn try_module_alias_constant(
    ctx: &mut LoweringContext,
    obj: &Expression,
    prop: &Expression,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let ExpressionKind::Identifier(alias_name, _) = &obj.node {
        if ctx
            .type_checker
            .module_aliases
            .contains_key(alias_name.as_str())
        {
            if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                let constant_val = match prop_name.as_str() {
                    "PI" => Some(std::f64::consts::PI),
                    "E" => Some(std::f64::consts::E),
                    "INF" => Some(f64::INFINITY),
                    _ => None,
                };

                if let Some(val) = constant_val {
                    let ty = resolve_type(ctx.type_checker, expr);
                    let operand = Operand::Constant(Box::new(Constant {
                        span: expr.span,
                        ty: ty.clone(),
                        literal: crate::ast::literal::Literal::Float(
                            crate::ast::literal::FloatLiteral::F64(val.to_bits()),
                        ),
                    }));

                    if let Some(d) = dest {
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(operand)),
                            span: expr.span,
                        });
                        return Ok(Some(Operand::Copy(d)));
                    } else {
                        return Ok(Some(operand));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Handle GPU intrinsics accessed via kernel context (e.g., kernel.thread_idx.x).
fn emit_gpu_intrinsic_assign(
    ctx: &mut LoweringContext,
    rvalue: Rvalue,
    dest: Option<Place>,
    expr: &Expression,
) -> Operand {
    match &dest {
        Some(d) => {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(d.clone(), rvalue),
                span: expr.span,
            });
            Operand::Copy(d.clone())
        }
        None => {
            let temp = ctx.push_temp(Type::new(TypeKind::Int, expr.span), expr.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), rvalue),
                span: expr.span,
            });
            Operand::Copy(Place::new(temp))
        }
    }
}

/// Try to match a GPU dimension name (x, y, z) to a Dimension enum.
fn try_parse_gpu_dimension(name: &str) -> Result<Dimension, LoweringError> {
    match name {
        "x" => Ok(Dimension::X),
        "y" => Ok(Dimension::Y),
        "z" => Ok(Dimension::Z),
        _ => Err(LoweringError::unsupported_expression(
            format!("Invalid GPU dimension: {}", name),
            crate::error::syntax::Span::default(),
        )),
    }
}

fn try_gpu_intrinsic(
    ctx: &mut LoweringContext,
    obj_operand: &Operand,
    prop: &Expression,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let Operand::Constant(c) = obj_operand {
        if let crate::ast::literal::Literal::Identifier(sym) = &c.literal {
            if sym == KERNEL_CONTEXT_IDENT || sym == GPU_CONTEXT_DEPRECATED_IDENT {
                if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                    return Ok(Some(Operand::Constant(Box::new(Constant {
                        span: expr.span,
                        ty: Type::new(TypeKind::Void, expr.span),
                        literal: crate::ast::literal::Literal::Identifier(format!(
                            "{}.{}",
                            KERNEL_CONTEXT_IDENT, prop_name
                        )),
                    }))));
                }
            } else if let Some(field) = kernel_context_field(sym) {
                if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                    let dim = try_parse_gpu_dimension(prop_name.as_str())?;

                    let rvalue = match field {
                        "thread_idx" => Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(dim)),
                        "block_idx" => Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(dim)),
                        "block_dim" => Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(dim)),
                        "grid_dim" => Rvalue::GpuIntrinsic(GpuIntrinsic::GridDim(dim)),
                        _ => {
                            return Err(LoweringError::unsupported_expression(
                                format!("Unknown GPU intrinsic: {}", sym),
                                expr.span,
                            ));
                        }
                    };

                    return Ok(Some(emit_gpu_intrinsic_assign(ctx, rvalue, dest, expr)));
                }
            }
        }
    }
    Ok(None)
}

pub(crate) fn lower_member_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Member(obj, prop) = &expr.node else {
        unreachable!()
    };

    if let Some(result) = try_module_alias_constant(ctx, obj, prop, expr, dest.clone())? {
        return Ok(result);
    }

    let obj_operand = lower_expression(ctx, obj, None)?;

    if let Some(result) = try_gpu_intrinsic(ctx, &obj_operand, prop, expr, dest.clone())? {
        return Ok(result);
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
            let idx = extract_integer_index(val);
            let obj_place = ensure_place(ctx, obj_operand, obj.span);
            return lower_tuple_field_access(ctx, obj_place, idx, elements, expr, dest);
        }
    }

    if let TypeKind::Custom(struct_name, _) = &obj_ty.kind {
        if let Some(crate::type_checker::context::TypeDefinition::Struct(def)) =
            ctx.type_checker.global_type_definitions.get(struct_name)
        {
            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                if let Some(idx) = def.fields.iter().position(|(f, _, _)| f == field_name) {
                    let place = ensure_place(ctx, obj_operand, obj.span);
                    return lower_custom_field_access(ctx, place, idx, expr, dest);
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

        if let Some(crate::type_checker::context::TypeDefinition::Class(def)) =
            ctx.type_checker.global_type_definitions.get(struct_name)
        {
            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                let all_fields = crate::type_checker::context::collect_class_fields_all(
                    def,
                    &ctx.type_checker.global_type_definitions,
                );
                if let Some(idx) = all_fields
                    .iter()
                    .position(|(n, _)| *n == field_name.as_str())
                {
                    let place = ensure_place(ctx, obj_operand, obj.span);
                    return lower_custom_field_access(ctx, place, idx, expr, dest);
                }
            }
        }
    }

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
                        return lower_enum_unit_variant(
                            ctx,
                            type_name,
                            variant_name,
                            discriminant,
                            expr,
                            dest,
                        );
                    }
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

    let class_name = match &obj_ty.kind {
        TypeKind::String => Some(crate::ast::types::STRING_TYPE_NAME.to_string()),
        TypeKind::Custom(name, _) => Some(name.clone()),
        k => k.as_builtin_collection().map(|b| b.name().to_string()),
    };

    if let Some(class_name) = class_name {
        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
            if let Some(crate::type_checker::context::TypeDefinition::Class(class_def)) =
                ctx.type_checker.global_type_definitions.get(&class_name)
            {
                if let Some(method_info) = class_def.methods.get(prop_name.as_str()) {
                    if method_info.params.is_empty() {
                        return lower_zero_arg_method_as_property(
                            ctx,
                            &class_name,
                            prop_name,
                            &method_info.return_type,
                            obj_operand,
                            expr,
                            dest,
                        );
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
