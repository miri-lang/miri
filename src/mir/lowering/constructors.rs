// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Constructor lowering — struct and class constructors.

use crate::ast::expression::Expression;
use crate::ast::{ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{AggregateKind, Constant, Operand, Place, Rvalue, StatementKind};
use crate::type_checker::context::{collect_class_fields_all, ClassDefinition, StructDefinition};

use super::dispatch::resolve_inherited_method;
use super::helpers::coerce_rvalue;
use super::{lower_expression, LoweringContext};

/// Lowers a struct constructor call to an Aggregate rvalue.
pub fn lower_struct_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    struct_name: &str,
    def: &StructDefinition,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Separate positional and named arguments
    let arg_watermark = ctx.body.local_decls.len();
    let mut positional_args = Vec::with_capacity(args.len());
    let mut named_args: std::collections::HashMap<&str, Operand> =
        std::collections::HashMap::with_capacity(args.len());

    for arg in args {
        match &arg.node {
            ExpressionKind::NamedArgument(name, value) => {
                let op = lower_expression(ctx, value, None)?;
                named_args.insert(name, op);
            }
            _ => {
                let op = lower_expression(ctx, arg, None)?;
                positional_args.push(op);
            }
        }
    }

    // Build operands in field declaration order
    let mut operands = Vec::with_capacity(def.fields.len());
    let mut pos_iter = positional_args.into_iter();

    for (field_name, field_ty, _visibility) in &def.fields {
        let op = if let Some(op) = pos_iter.next() {
            // Positional argument
            op
        } else if let Some(op) = named_args.remove(field_name.as_str()) {
            // Named argument
            op
        } else {
            // Missing field - this should have been caught by type checker
            return Err(LoweringError::missing_struct_field(
                field_name.clone(),
                struct_name.to_string(),
                *span,
            ));
        };

        // Cast if types don't match
        let op_ty = op.ty(&ctx.body).clone();
        let op = if op_ty.kind != field_ty.kind {
            let temp = ctx.push_temp(field_ty.clone(), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(Place::new(temp), coerce_rvalue(op, &op_ty, field_ty)),
                span: *span,
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    // Create the struct type
    let struct_ty = Type::new(TypeKind::Custom(struct_name.to_string(), None), *span);

    // Assign aggregate to destination
    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(struct_ty.clone(), *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let dest_local = destination.local;
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Struct(struct_ty), operands.clone()),
        ),
        span: *span,
    });

    // Release managed temporaries created while lowering the constructor arguments.
    // After the Aggregate assignment, Perceus has IncRef'd them (the struct now owns
    // the references). The caller's temporary locals are no longer needed.
    for op in &operands {
        if let Operand::Copy(place) | Operand::Move(place) = op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, *span);
            }
        }
    }

    Ok(result_op)
}

/// Lowers a class constructor call to an Aggregate rvalue,
/// then calls the `init` method if one exists.
pub fn lower_class_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    class_name: &str,
    def: &ClassDefinition,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Resolve init method: own class first, then walk the inheritance chain.
    let init_class_name: Option<String> = {
        if def.methods.get("init").is_some_and(|m| !m.is_abstract) {
            Some(class_name.to_string())
        } else if let Some(base) = &def.base_class {
            resolve_inherited_method(&ctx.type_checker.global_type_definitions, base, "init")
                .filter(|(_, m)| !m.is_abstract)
                .map(|(c, _)| c)
        } else {
            None
        }
    };

    // Collect ALL fields in inheritance order (base class fields first).
    // This defines the canonical memory layout for the class instance.
    let all_fields: Vec<(String, crate::type_checker::context::FieldInfo)> = {
        collect_class_fields_all(def, &ctx.type_checker.global_type_definitions)
            .into_iter()
            .map(|(n, f)| (n.to_string(), f.clone()))
            .collect()
    };

    if let Some(init_class) = init_class_name {
        // When init exists (own or inherited), constructor args are init params.
        // Allocate the object with default field values for ALL fields, then call init.
        let field_defaults: Vec<Operand> = all_fields
            .iter()
            .map(|(_, fi)| create_default_value(&fi.ty, span))
            .collect();

        let class_ty = Type::new(TypeKind::Custom(class_name.to_string(), None), *span);

        let (destination, result_op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(class_ty.clone(), *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        };

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                destination.clone(),
                Rvalue::Aggregate(AggregateKind::Class(class_ty), field_defaults),
            ),
            span: *span,
        });

        // Build init call args: self + constructor args + allocator
        let mut call_args = vec![Operand::Copy(destination)];
        let init_arg_watermark = ctx.body.local_decls.len();
        for arg in args {
            match &arg.node {
                ExpressionKind::NamedArgument(_name, value) => {
                    call_args.push(lower_expression(ctx, value, None)?);
                }
                _ => {
                    call_args.push(lower_expression(ctx, arg, None)?);
                }
            }
        }
        if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
            call_args.push(Operand::Copy(Place::new(alloc_local)));
        }

        let mangled_name = format!("{}_init", init_class);
        let func_op = Operand::Constant(Box::new(Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(mangled_name),
        }));

        // init returns void; use a temp destination for the call
        let void_ty = Type::new(TypeKind::Void, *span);
        let void_dest = ctx.push_temp(void_ty, *span);
        let target_bb = ctx.new_basic_block();
        ctx.set_terminator(crate::mir::Terminator::new(
            crate::mir::TerminatorKind::Call {
                func: func_op,
                args: call_args.clone(),
                destination: Place::new(void_dest),
                target: Some(target_bb),
            },
            *span,
        ));
        ctx.set_current_block(target_bb);

        // Release managed temporaries created while lowering init call arguments.
        // Skip call_args[0] which is `self` (the destination, not a fresh temp).
        for arg_op in call_args.iter().skip(1) {
            if let Operand::Copy(place) | Operand::Move(place) = arg_op {
                ctx.emit_temp_drop(place.local, init_arg_watermark, *span);
            }
        }

        Ok(result_op)
    } else {
        // No init method anywhere in the chain — map constructor args directly to ALL fields.
        let arg_watermark = ctx.body.local_decls.len();
        let mut positional_args = Vec::with_capacity(args.len());
        let mut named_args: std::collections::HashMap<&str, Operand> =
            std::collections::HashMap::with_capacity(args.len());

        for arg in args {
            match &arg.node {
                ExpressionKind::NamedArgument(name, value) => {
                    let op = lower_expression(ctx, value, None)?;
                    named_args.insert(name, op);
                }
                _ => {
                    let op = lower_expression(ctx, arg, None)?;
                    positional_args.push(op);
                }
            }
        }

        let mut operands = Vec::with_capacity(all_fields.len());
        let mut pos_iter = positional_args.into_iter();

        for (field_name, field_info) in &all_fields {
            let op = if let Some(op) = pos_iter.next() {
                op
            } else if let Some(op) = named_args.remove(field_name.as_str()) {
                op
            } else {
                create_default_value(&field_info.ty, span)
            };

            let op_ty = op.ty(&ctx.body).clone();
            let op = if op_ty.kind != field_info.ty.kind {
                let temp = ctx.push_temp(field_info.ty.clone(), *span);
                ctx.push_statement(crate::mir::Statement {
                    kind: StatementKind::Assign(
                        Place::new(temp),
                        coerce_rvalue(op, &op_ty, &field_info.ty),
                    ),
                    span: *span,
                });
                Operand::Copy(Place::new(temp))
            } else {
                op
            };

            operands.push(op);
        }

        let class_ty = Type::new(TypeKind::Custom(class_name.to_string(), None), *span);

        let (destination, result_op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(class_ty.clone(), *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        };

        let dest_local = destination.local;
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                destination,
                Rvalue::Aggregate(AggregateKind::Class(class_ty), operands.clone()),
            ),
            span: *span,
        });

        // Release managed temporaries created while lowering the constructor arguments.
        for op in &operands {
            if let Operand::Copy(place) | Operand::Move(place) = op {
                if place.local != dest_local {
                    ctx.emit_temp_drop(place.local, arg_watermark, *span);
                }
            }
        }

        Ok(result_op)
    }
}

/// Creates a default value operand for a given type.
pub(crate) fn create_default_value(ty: &Type, span: &Span) -> Operand {
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::mir::Constant;

    let literal = match &ty.kind {
        TypeKind::Int | TypeKind::I32 => Literal::Integer(IntegerLiteral::I32(0)),
        TypeKind::I8 => Literal::Integer(IntegerLiteral::I8(0)),
        TypeKind::I16 => Literal::Integer(IntegerLiteral::I16(0)),
        TypeKind::I64 => Literal::Integer(IntegerLiteral::I64(0)),
        TypeKind::I128 => Literal::Integer(IntegerLiteral::I128(0)),
        TypeKind::U8 => Literal::Integer(IntegerLiteral::U8(0)),
        TypeKind::U16 => Literal::Integer(IntegerLiteral::U16(0)),
        TypeKind::U32 => Literal::Integer(IntegerLiteral::U32(0)),
        TypeKind::U64 => Literal::Integer(IntegerLiteral::U64(0)),
        TypeKind::U128 => Literal::Integer(IntegerLiteral::U128(0)),
        TypeKind::Boolean => Literal::Boolean(false),
        TypeKind::String => Literal::String(String::new()),
        _ => Literal::None,
    };

    Operand::Constant(Box::new(Constant {
        span: *span,
        ty: ty.clone(),
        literal,
    }))
}

/// Computes the element size in bytes for a collection element type.
///
/// Primitives use their natural size. Managed types (String, collections,
/// custom types/classes) are pointer-sized since they are heap-allocated.
pub(crate) fn compute_elem_size_from_type(kind: &TypeKind) -> i64 {
    match kind {
        TypeKind::I8 | TypeKind::U8 | TypeKind::Boolean => 1,
        TypeKind::I16 | TypeKind::U16 => 2,
        TypeKind::I32 | TypeKind::U32 | TypeKind::F32 => 4,
        TypeKind::Int | TypeKind::I64 | TypeKind::U64 | TypeKind::Float | TypeKind::F64 => 8,
        TypeKind::I128 | TypeKind::U128 => 16,
        // All heap-allocated types are pointer-sized (8 bytes on 64-bit).
        // This includes String, List, Array, Map, Set, Custom (structs/enums/classes).
        TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Custom(_, _)
        | TypeKind::RawPtr => 8,
        // Default to 8 for unknown/complex types
        _ => 8,
    }
}
