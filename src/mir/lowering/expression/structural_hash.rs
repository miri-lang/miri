// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR-level lowering of the hash a type derives: the `hash()` a value answers
//! when its type declares none.
//!
//! The hash is built from exactly the parts `==` compares, in the order it
//! compares them, so two values `==` calls equal hash alike. Each step mirrors
//! a step of the structural equality walk in `structural_equality.rs`:
//!
//! - a scalar is its own hash, and a float the hash of its number, `-0.0` and
//!   `0.0` alike as `==` makes them;
//! - a string hashes its content through the runtime;
//! - an optional hashes its payload, and `None` to a constant of its own;
//! - a struct folds its fields, and an enum its discriminant and the payloads
//!   of the variant it holds;
//! - a type that declares `hash` is hashed by it, as `==` calls the `equals` it
//!   declares; a class without `equals`, which `==` compares by identity,
//!   hashes its address.

use super::structural_equality::{
    emit_corrupt_discriminant_panic, emit_variant_switch, identifier_constant, materialize_field,
    read_discriminant, substituted_member_types,
};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, HASH_METHOD_NAME};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::{
    BinOp, Constant, Local, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::TypeDefinition;

/// The hash a `None` contributes, whatever the optional's payload type.
const NONE_HASH: i128 = 0x6e6f_6e65;

/// Emit MIR computing the hash of `value`, a value of `kind`, into an `int`
/// local.
pub fn emit_structural_hash(
    ctx: &mut LoweringContext,
    span: Span,
    kind: &TypeKind,
    value: Operand,
) -> Result<Local, LoweringError> {
    match kind {
        TypeKind::String => Ok(call_runtime_hash(ctx, span, rt::STRING_HASH, vec![value])),
        TypeKind::Option(inner) => emit_option_hash(ctx, span, inner, value),
        // An atomic is held as the number it wraps, which `==` compares.
        TypeKind::Custom(_, _) if super::binary_expr::is_device_marker(kind) => {
            Ok(cast(ctx, value, TypeKind::Int, span))
        }
        TypeKind::Custom(name, args) => {
            emit_named_type_hash(ctx, span, name, args.as_deref(), value)
        }
        TypeKind::Float | TypeKind::F64 => {
            Ok(call_runtime_hash(ctx, span, rt::FLOAT_HASH, vec![value]))
        }
        // A narrower float is widened first, which keeps its number, so it
        // hashes as the number `==` compares.
        TypeKind::F16 | TypeKind::F32 => {
            let wide = cast(ctx, value, TypeKind::Float, span);
            Ok(call_runtime_hash(
                ctx,
                span,
                rt::FLOAT_HASH,
                vec![Operand::Copy(Place::new(wide))],
            ))
        }
        // Every integer width is its own hash. A 128-bit value keeps its low
        // word, which two equal values share.
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
        | TypeKind::U128 => Ok(cast(ctx, value, TypeKind::Int, span)),
        TypeKind::RawPtr | TypeKind::Identifier => Ok(address_hash(ctx, span, value)),
        // A type parameter is left open only in a generic body's shared copy,
        // which reads it as one machine word, as `==` there compares it; each
        // instantiation's own copy hashes the type it is bound to.
        TypeKind::Generic(_, _, _) => Ok(address_hash(ctx, span, value)),
        TypeKind::Void => Ok(int_constant(ctx, 0, span)),
        TypeKind::Result(_, _) => Err(LoweringError::unsupported_expression(
            "cannot hash a result type that was not resolved to its declaration".to_string(),
            span,
        )),
        // A list or array spelled with its own syntax is the class it names,
        // which declares the hash its `equals` agrees with.
        TypeKind::List(element) => emit_named_type_hash(
            ctx,
            span,
            BuiltinCollectionKind::List.name(),
            Some(std::slice::from_ref(element.as_ref())),
            value,
        ),
        TypeKind::Array(element, size) => emit_named_type_hash(
            ctx,
            span,
            BuiltinCollectionKind::Array.name(),
            Some(&[element.as_ref().clone(), size.as_ref().clone()]),
            value,
        ),
        // `==` compares these as one machine word — a tuple, map or set by the
        // block it points at, a function value by its closure — so they hash
        // that word, which two values `==` calls equal share.
        TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Function(_)
        | TypeKind::Future(_) => Ok(address_hash(ctx, span, value)),
        TypeKind::Meta(_) | TypeKind::Linear(_) | TypeKind::Error => {
            Err(LoweringError::unsupported_expression(
                format!("a derived hash is not supported for type {}", kind),
                span,
            ))
        }
    }
}

/// Hash an optional: its payload's hash, or [`NONE_HASH`] for `None`.
fn emit_option_hash(
    ctx: &mut LoweringContext,
    span: Span,
    inner_ty: &Type,
    value: Operand,
) -> Result<Local, LoweringError> {
    let result = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    let watermark = ctx.body.local_decls.len();
    let place = crate::mir::lowering::helpers::ensure_place(ctx, value, span);

    let none = Operand::Constant(Box::new(Constant {
        span,
        ty: ctx.body.local_decls[place.local.0].ty.clone(),
        literal: crate::ast::literal::Literal::None,
    }));
    let is_none = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(is_none),
            Rvalue::BinaryOp(
                BinOp::Eq,
                Box::new(Operand::Copy(place.clone())),
                Box::new(none),
            ),
        ),
        span,
    });
    let none_bb = ctx.new_basic_block();
    let some_bb = ctx.new_basic_block();
    let final_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(is_none)),
            targets: vec![(crate::mir::Discriminant::bool_true(), none_bb)],
            otherwise: some_bb,
        },
        span,
    ));

    ctx.set_current_block(none_bb);
    assign_int(ctx, result, NONE_HASH, span);
    goto(ctx, final_bb, span);

    ctx.set_current_block(some_bb);
    let payload = materialize_field(ctx, &place, PlaceElem::Field(0), inner_ty, span);
    if ctx.is_perceus_managed(&inner_ty.kind) {
        ctx.register_scope_temp(payload);
    }
    let hashed = emit_structural_hash(
        ctx,
        span,
        &inner_ty.kind,
        Operand::Copy(Place::new(payload)),
    )?;
    ctx.emit_temp_drop(place.local, watermark, span);
    assign_local(ctx, result, hashed, span);
    goto(ctx, final_bb, span);

    ctx.set_current_block(final_bb);
    Ok(result)
}

/// Hash a value of a named type: by the `hash` it declares, by its address
/// for a class `==` compares by identity, or by its parts.
fn emit_named_type_hash(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    args: Option<&[crate::ast::expression::Expression]>,
    value: Operand,
) -> Result<Local, LoweringError> {
    if let Some((owner, method)) =
        super::binary_expr::operator_method_body(ctx, name, HASH_METHOD_NAME)
    {
        let recorded = Type::new(
            TypeKind::Custom(name.to_string(), args.map(<[_]>::to_vec)),
            span,
        );
        let receiver = crate::mir::lowering::apply_generic_sub(&recorded, &ctx.generic_subs);
        let (symbol, _) = crate::mir::lowering::method_dispatch::operator_method_callee(
            ctx,
            &receiver,
            &owner,
            HASH_METHOD_NAME,
            &method,
        );
        return Ok(call_hash_method(ctx, span, &symbol, value));
    }
    match ctx.type_checker.type_definitions().get(name) {
        Some(TypeDefinition::Enum(_)) => emit_enum_hash(ctx, span, name, args, value),
        Some(TypeDefinition::Struct(_)) => emit_struct_hash(ctx, span, name, args, value),
        // A class without `equals`, or a trait-typed value whose trait
        // declares no `hash`, is compared by `==` as the instance it is.
        Some(TypeDefinition::Class(_) | TypeDefinition::Trait(_)) => {
            Ok(address_hash(ctx, span, value))
        }
        Some(TypeDefinition::Generic(_) | TypeDefinition::Alias(_)) | None => {
            Err(LoweringError::unsupported_expression(
                format!("a derived hash is not supported for type {}", name),
                span,
            ))
        }
    }
}

/// Hash an enum value: its discriminant, folded with each payload of the
/// variant it holds, read under a switch on the discriminant since each
/// variant lays its payloads out at its own types.
fn emit_enum_hash(
    ctx: &mut LoweringContext,
    span: Span,
    enum_name: &str,
    type_args: Option<&[crate::ast::expression::Expression]>,
    value: Operand,
) -> Result<Local, LoweringError> {
    let Some(TypeDefinition::Enum(enum_def)) = ctx.type_checker.type_definitions().get(enum_name)
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
        .map(|payloads| substituted_member_types(payloads, type_args, generics.as_ref()))
        .collect();

    let watermark = ctx.body.local_decls.len();
    let place = crate::mir::lowering::helpers::ensure_place(ctx, value, span);
    let result = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    let final_bb = ctx.new_basic_block();
    let discriminant = read_discriminant(ctx, &place, span);
    let (variant_blocks, corrupt_bb) = emit_variant_switch(ctx, discriminant, variants.len(), span);

    for (variant_idx, payload_types) in variants.iter().enumerate() {
        ctx.set_current_block(variant_blocks[variant_idx]);
        let mut hash = discriminant;
        for (payload_idx, payload_ty) in payload_types.iter().enumerate() {
            let payload = materialize_field(
                ctx,
                &place,
                PlaceElem::Field(payload_idx + 1),
                payload_ty,
                span,
            );
            let part = emit_structural_hash(
                ctx,
                span,
                &payload_ty.kind,
                Operand::Copy(Place::new(payload)),
            )?;
            if ctx.is_perceus_managed(&payload_ty.kind) {
                ctx.emit_temp_drop(payload, watermark, span);
            }
            hash = combine(ctx, hash, part, span);
        }
        assign_local(ctx, result, hash, span);
        goto(ctx, final_bb, span);
    }

    ctx.set_current_block(corrupt_bb);
    emit_corrupt_discriminant_panic(ctx, enum_name, final_bb, span);

    ctx.set_current_block(final_bb);
    Ok(result)
}

/// Hash a struct value: its fields folded in declaration order.
fn emit_struct_hash(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    type_args: Option<&[crate::ast::expression::Expression]>,
    value: Operand,
) -> Result<Local, LoweringError> {
    let Some(TypeDefinition::Struct(struct_def)) = ctx.type_checker.type_definitions().get(name)
    else {
        return Err(LoweringError::unsupported_expression(
            format!("unknown struct: {}", name),
            span,
        ));
    };
    let generics = struct_def.generics.clone();
    let declared: Vec<Type> = struct_def
        .fields
        .iter()
        .map(|(_, field_ty, _)| field_ty.clone())
        .collect();
    let field_types = substituted_member_types(&declared, type_args, generics.as_ref());

    let place = crate::mir::lowering::helpers::ensure_place(ctx, value, span);
    let mut hash = int_constant(ctx, field_types.len() as i128, span);
    for (field_idx, field_ty) in field_types.iter().enumerate() {
        let field = materialize_field(ctx, &place, PlaceElem::Field(field_idx), field_ty, span);
        if ctx.is_perceus_managed(&field_ty.kind) {
            ctx.register_scope_temp(field);
        }
        let part =
            emit_structural_hash(ctx, span, &field_ty.kind, Operand::Copy(Place::new(field)))?;
        hash = combine(ctx, hash, part, span);
    }
    Ok(hash)
}

/// Call the `hash` body named `symbol` on `value`.
fn call_hash_method(ctx: &mut LoweringContext, span: Span, symbol: &str, value: Operand) -> Local {
    let mut args = vec![value];
    if let Some(&allocator) = ctx.variable_map.get("allocator") {
        args.push(Operand::Copy(Place::new(allocator)));
    }
    call_into_int(ctx, span, identifier_constant(symbol, span), args)
}

/// The hash of a value `==` compares by identity: its address, mixed.
fn address_hash(ctx: &mut LoweringContext, span: Span, value: Operand) -> Local {
    let seed = Operand::Copy(Place::new(int_constant(ctx, 0, span)));
    call_runtime_hash(ctx, span, rt::HASH_COMBINE, vec![seed, value])
}

/// `seed` with `part` mixed into it.
fn combine(ctx: &mut LoweringContext, seed: Local, part: Local, span: Span) -> Local {
    call_runtime_hash(
        ctx,
        span,
        rt::HASH_COMBINE,
        vec![
            Operand::Copy(Place::new(seed)),
            Operand::Copy(Place::new(part)),
        ],
    )
}

/// Call the runtime hashing helper `name` with `args`, into an `int` local.
fn call_runtime_hash(
    ctx: &mut LoweringContext,
    span: Span,
    name: &str,
    args: Vec<Operand>,
) -> Local {
    call_into_int(ctx, span, identifier_constant(name, span), args)
}

fn call_into_int(
    ctx: &mut LoweringContext,
    span: Span,
    func: Operand,
    args: Vec<Operand>,
) -> Local {
    let result = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    let next_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(result),
            target: Some(next_bb),
        },
        span,
    ));
    ctx.set_current_block(next_bb);
    result
}

/// `value` converted to `target`, in a fresh local.
fn cast(ctx: &mut LoweringContext, value: Operand, target: TypeKind, span: Span) -> Local {
    let target = Type::new(target, span);
    let result = ctx.push_temp(target.clone(), span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(result), Rvalue::Cast(Box::new(value), target)),
        span,
    });
    result
}

fn int_constant(ctx: &mut LoweringContext, value: i128, span: Span) -> Local {
    let result = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    assign_int(ctx, result, value, span);
    result
}

fn assign_int(ctx: &mut LoweringContext, local: Local, value: i128, span: Span) {
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Int, span),
                literal: crate::ast::literal::Literal::Integer(
                    crate::ast::literal::IntegerLiteral::I64(value as i64),
                ),
            }))),
        ),
        span,
    });
}

fn assign_local(ctx: &mut LoweringContext, target: Local, source: Local, span: Span) {
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(target),
            Rvalue::Use(Operand::Copy(Place::new(source))),
        ),
        span,
    });
}

fn goto(ctx: &mut LoweringContext, target: crate::mir::BasicBlock, span: Span) {
    ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, span));
}
