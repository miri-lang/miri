// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Constructor lowering — struct and class constructors.

use crate::ast::expression::Expression;
use crate::ast::types;
use crate::ast::{BuiltinCollectionKind, ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    AggregateKind, Constant, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::{collect_class_fields_all, ClassDefinition, StructDefinition};

use super::dispatch::resolve_inherited_method;
use super::helpers::coerce_rvalue_in;
use super::method_dispatch::{instantiated_callee, InstantiatedCallee};
use super::variable::canonical_declared_type;
use super::{
    apply_generic_sub, build_class_generic_substitution, lower_expression, monomorphized_arguments,
    LoweringContext,
};
use std::collections::HashMap;

/// The method a class constructor runs on the instance it builds.
const INIT_METHOD_NAME: &str = "init";

/// Lowers a struct constructor call to an Aggregate rvalue.
pub fn lower_struct_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    struct_name: &str,
    def: &StructDefinition,
    args: &[Expression],
    type_args: Option<&[Expression]>,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Check if this is a vector type (Vec2, Vec3, Vec4, etc.)
    let is_vec = types::vec_dim(struct_name).is_some();
    let arg_watermark = ctx.body.local_decls.len();
    let (positional_args, mut named_args) = partition_constructor_args(ctx, args)?;

    // Extract concrete element type for vectors from the type_args
    let concrete_elem_type = if is_vec && !def.fields.is_empty() {
        extract_vec_concrete_elem_type(type_args)
    } else {
        None
    };

    // A field is stored at the type the struct is instantiated with: the
    // declared `U` names the struct's own parameter, and converting an argument
    // to it would turn a float into an integer word or cut a 128-bit value.
    let field_subs = struct_type_arguments(def, type_args);

    // Build operands in field declaration order
    let mut operands = Vec::with_capacity(def.fields.len());
    let mut pos_iter = positional_args.into_iter();

    for (field_name, field_ty, _visibility) in &def.fields {
        let op = if let Some(op) = pos_iter.next() {
            op
        } else if let Some(op) = named_args.remove(field_name.as_str()) {
            op
        } else {
            return Err(LoweringError::missing_struct_field(
                field_name.clone(),
                struct_name.to_string(),
                *span,
            ));
        };

        // Both sides in canonical form: `Option<int>` and `int?` are one type,
        // and coercing between the two spellings would box the value twice.
        let op_ty = canonical_declared_type(ctx.type_checker, op.ty(&ctx.body));
        let target_ty =
            canonical_declared_type(ctx.type_checker, &apply_generic_sub(field_ty, &field_subs));

        let op = if op_ty.kind != target_ty.kind
            && !super::helpers::spellings_of_one_value(&op_ty, &target_ty)
        {
            let temp = ctx.push_temp(target_ty.clone(), *span);
            let rvalue = coerce_rvalue_in(ctx, op, &op_ty, &target_ty, *span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(Place::new(temp), rvalue),
                span: *span,
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    let struct_ty =
        build_struct_constructor_type(struct_name, is_vec, concrete_elem_type.as_ref(), *span);

    let destination = if let Some(d) = dest {
        d
    } else {
        Place::new(ctx.push_temp(struct_ty.clone(), *span))
    };

    let dest_local = destination.local;

    if is_vec {
        ctx.body.local_decls[dest_local.0].ty = struct_ty.clone();
    }

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination.clone(),
            Rvalue::Aggregate(AggregateKind::Struct(struct_ty), operands.clone()),
        ),
        span: *span,
    });

    let result_op = Operand::Copy(destination);

    for op in &operands {
        if let Operand::Copy(place) | Operand::Move(place) = op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, *span);
            }
        }
    }

    Ok(result_op)
}

/// Separates positional and named arguments for a constructor call.
fn partition_constructor_args<'a>(
    ctx: &mut LoweringContext,
    args: &'a [Expression],
) -> Result<(Vec<Operand>, HashMap<&'a str, Operand>), LoweringError> {
    let mut positional_args = Vec::with_capacity(args.len());
    let mut named_args = HashMap::with_capacity(args.len());

    for arg in args {
        match &arg.node {
            ExpressionKind::NamedArgument(name, value) => {
                let op = lower_expression(ctx, value, None)?;
                named_args.insert(name.as_str(), op);
            }
            _ => {
                let op = lower_expression(ctx, arg, None)?;
                positional_args.push(op);
            }
        }
    }
    Ok((positional_args, named_args))
}

/// The substitution from a generic struct's parameters to the type arguments a
/// constructor names, or an empty one for a struct that declares none.
fn struct_type_arguments(
    def: &StructDefinition,
    type_args: Option<&[Expression]>,
) -> HashMap<String, Type> {
    let (Some(generics), Some(args)) = (def.generics.as_ref(), type_args) else {
        return HashMap::new();
    };
    generics
        .iter()
        .zip(args)
        .filter_map(|(generic, arg)| match &arg.node {
            ExpressionKind::Type(ty, _) => Some((generic.name.clone(), ty.as_ref().clone())),
            _ => None,
        })
        .collect()
}

/// Extracts concrete element type for vector type instantiation arguments if present.
fn extract_vec_concrete_elem_type(type_args: Option<&[Expression]>) -> Option<Type> {
    if let Some(args) = type_args {
        if let Some(first_arg) = args.first() {
            if let ExpressionKind::Type(elem_ty, _) = &first_arg.node {
                return Some(elem_ty.as_ref().clone());
            }
        }
    }
    None
}

/// Builds the struct Type for a struct constructor result, embedding vector element type if applicable.
fn build_struct_constructor_type(
    struct_name: &str,
    is_vec: bool,
    concrete_elem_type: Option<&Type>,
    span: Span,
) -> Type {
    if is_vec {
        if let Some(concrete_elem) = concrete_elem_type {
            let synthetic_args = vec![Expression {
                id: 0,
                span,
                node: ExpressionKind::Type(Box::new(concrete_elem.clone()), false),
            }];
            Type::new(
                TypeKind::Custom(struct_name.to_string(), Some(synthetic_args)),
                span,
            )
        } else {
            Type::new(TypeKind::Custom(struct_name.to_string(), None), span)
        }
    } else {
        Type::new(TypeKind::Custom(struct_name.to_string(), None), span)
    }
}

/// Lowers a class constructor call to an Aggregate rvalue,
/// then calls the `init` method if one exists.
///
/// `resolved_ty` is the fully-resolved constructor result type
/// (`Custom(class, Some(args))`) when the call site knows it. For a generic
/// class it drives the field-type substitution so a scalar `T` field
/// (`Box<float>.value`) stores at its concrete width instead of a pointer slot.
pub fn lower_class_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    class_name: &str,
    def: &ClassDefinition,
    args: &[Expression],
    resolved_ty: Option<&Type>,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let Some(ty) = resolved_ty {
        ctx.refuse_unnameable_instance(ty, *span)?;
        ctx.record_class_instantiations(ty);
    }
    let field_subs = build_class_field_substitution(ctx, def, resolved_ty);
    // The parameters travel with the class that defines the `init`, because an
    // argument has to be brought to the type that body declares for it before
    // the call — the same thing a plain function call does.
    let init_site: Option<(String, Vec<(String, Type)>)> = {
        if let Some(m) = def.methods.get(INIT_METHOD_NAME).filter(|m| !m.is_abstract) {
            Some((class_name.to_string(), m.params.clone()))
        } else if let Some(base) = &def.base_class {
            resolve_inherited_method(ctx.type_checker.type_definitions(), base, INIT_METHOD_NAME)
                .filter(|(_, m)| !m.is_abstract)
                .map(|(c, m)| (c, m.params.clone()))
        } else {
            None
        }
    };

    let all_fields: Vec<(String, crate::type_checker::context::FieldInfo)> = {
        collect_class_fields_all(def, ctx.type_checker.type_definitions())
            .into_iter()
            .map(|(n, f)| {
                let mut fi = f.clone();
                fi.ty = apply_generic_sub(&fi.ty, &field_subs);
                (n.to_string(), fi)
            })
            .collect()
    };

    let instance_ty = constructed_instance_type(class_name, resolved_ty, span);
    if let Some((init_class, init_params)) = init_site {
        let callee = instantiated_init(ctx, def, class_name, resolved_ty);
        // A parameter spelled with one of the declaring class's generic
        // parameters is declared at the argument the instance reaches it at,
        // not at the bare name.
        let (init_symbol, param_subs) = match callee {
            Some(callee) => (callee.symbol, callee.owner_subs),
            None => (format!("{init_class}_init"), field_subs),
        };
        let init_params: Vec<(String, Type)> = init_params
            .into_iter()
            .map(|(name, ty)| (name, apply_generic_sub(&ty, &param_subs)))
            .collect();
        lower_class_with_init(
            ctx,
            span,
            instance_ty,
            init_symbol,
            &all_fields,
            &init_params,
            args,
            dest,
        )
    } else {
        lower_class_without_init(ctx, span, instance_ty, &all_fields, args, dest)
    }
}

/// The type a constructed class instance is built at: the resolved constructor
/// type when the call site knows it, so an instance of a generic class keeps
/// its type arguments (`Box<String>`) and is released through the drop function
/// of its own instantiation. Without them an instance that is never bound —
/// `Box<String>(s).get()`, or one passed straight to a call — would be released
/// through the shared drop function, which skips a field typed by the class
/// parameter and leaks it.
fn constructed_instance_type(class_name: &str, resolved_ty: Option<&Type>, span: &Span) -> Type {
    if let Some(ty) = resolved_ty {
        if matches!(&ty.kind, TypeKind::Custom(name, _) if name == class_name) {
            return ty.clone();
        }
    }
    Type::new(TypeKind::Custom(class_name.to_string(), None), *span)
}

/// Build the generic-parameter → concrete-type map for one class instantiation.
///
/// Empty for a non-generic class or when the call site could not resolve the
/// instantiation's type arguments; in that case field types keep their generic
/// spelling and the constructor behaves exactly as before.
fn build_class_field_substitution(
    ctx: &LoweringContext,
    def: &ClassDefinition,
    resolved_ty: Option<&Type>,
) -> HashMap<String, Type> {
    match (def.generics.as_deref(), resolved_ty) {
        (Some(generics), Some(ty)) => {
            build_class_generic_substitution(ctx.type_checker, generics, ty)
        }
        _ => HashMap::new(),
    }
}

/// The per-instantiation `init` body a constructor of `class_name` at
/// `resolved_ty` calls: the one a static call on the instance names, so an
/// argument crosses into a body typed at the instance's own arguments — a
/// value-generic instance included — rather than the body shared by every
/// instantiation. `None` when the shared body serves: a class declaring no
/// parameters, or arguments with no per-instantiation body.
fn instantiated_init(
    ctx: &LoweringContext,
    def: &ClassDefinition,
    class_name: &str,
    resolved_ty: Option<&Type>,
) -> Option<InstantiatedCallee> {
    let TypeKind::Custom(_, Some(arg_exprs)) = &resolved_ty?.kind else {
        return None;
    };
    let defs = ctx.type_checker.type_definitions();
    let resolved = monomorphized_arguments(arg_exprs, def.generics.as_ref()?.len(), defs)?;
    instantiated_callee(defs, class_name, &resolved, INIT_METHOD_NAME)
}

/// The place a constructed class instance is built into: the caller's
/// destination, or a fresh temporary declared at the instance's type.
fn constructed_instance_place(
    ctx: &mut LoweringContext,
    instance_ty: &Type,
    dest: Option<Place>,
    span: &Span,
) -> (Place, Operand) {
    let destination = match dest {
        Some(d) => d,
        None => Place::new(ctx.push_temp(instance_ty.clone(), *span)),
    };
    let result_op = Operand::Copy(destination.clone());
    (destination, result_op)
}

#[allow(clippy::too_many_arguments)]
fn lower_class_with_init(
    ctx: &mut LoweringContext,
    span: &Span,
    instance_ty: Type,
    init_symbol: String,
    all_fields: &[(String, crate::type_checker::context::FieldInfo)],
    init_params: &[(String, Type)],
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let field_defaults: Vec<Operand> = all_fields
        .iter()
        .map(|(_, fi)| create_default_value(&fi.ty, span))
        .collect();

    let (destination, result_op) = constructed_instance_place(ctx, &instance_ty, dest, span);

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination.clone(),
            Rvalue::Aggregate(AggregateKind::Class(instance_ty), field_defaults),
        ),
        span: *span,
    });

    let mut call_args = vec![Operand::Copy(destination)];
    let init_arg_watermark = ctx.body.local_decls.len();
    for (position, arg) in args.iter().enumerate() {
        let watermark = ctx.body.local_decls.len();
        // A named argument names its parameter; every other takes the one in its
        // own position.
        let (value, declared) = match &arg.node {
            ExpressionKind::NamedArgument(name, value) => (
                value.as_ref(),
                init_params.iter().find(|(p, _)| p == name).map(|(_, t)| t),
            ),
            _ => (arg, init_params.get(position).map(|(_, t)| t)),
        };
        let op = lower_expression(ctx, value, None)?;
        let op = match declared {
            Some(target_ty) => {
                super::dispatch::coerce_arg_to_declared(ctx, op, value, target_ty, watermark)
            }
            None => op,
        };
        call_args.push(op);
    }
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }

    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier(init_symbol),
    }));

    let void_ty = Type::new(TypeKind::Void, *span);
    let void_dest = ctx.push_temp(void_ty, *span);
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(crate::mir::Terminator::new(
        crate::mir::TerminatorKind::Call {
            func: func_op,
            args: call_args.clone(),
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(void_dest),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);

    for arg_op in call_args.iter().skip(1) {
        if let Operand::Copy(place) | Operand::Move(place) = arg_op {
            ctx.emit_temp_drop(place.local, init_arg_watermark, *span);
        }
    }

    Ok(result_op)
}

fn lower_class_without_init(
    ctx: &mut LoweringContext,
    span: &Span,
    instance_ty: Type,
    all_fields: &[(String, crate::type_checker::context::FieldInfo)],
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
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

    for (field_name, field_info) in all_fields {
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
            let rvalue = coerce_rvalue_in(ctx, op, &op_ty, &field_info.ty, *span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(Place::new(temp), rvalue),
                span: *span,
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    let (destination, result_op) = constructed_instance_place(ctx, &instance_ty, dest, span);

    let dest_local = destination.local;
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Class(instance_ty), operands.clone()),
        ),
        span: *span,
    });

    for op in &operands {
        if let Operand::Copy(place) | Operand::Move(place) = op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, *span);
            }
        }
    }

    Ok(result_op)
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

/// Function-pointer type shared by all built-in collection constructor handlers.
///
/// Every handler receives the full call context so the table dispatch site in
/// `dispatch.rs` is a single uniform call regardless of which collection is being
/// constructed.
pub(crate) type CollectionCtorFn = fn(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError>;

/// Maps each built-in collection to its constructor handler.
///
/// Collection constructors legitimately require `sizeof(T)` as a compile-time
/// argument; that value is only available during MIR lowering and cannot be
/// expressed in Miri source code until a `sizeof<T>` built-in is added to the
/// language.  When that built-in exists, each `init()` method can be moved to
/// stdlib and the corresponding entry removed from this table.
///
/// This table exists **solely for constructor dispatch**. Method dispatch for
/// collections goes through normal class method resolution. Adding a new
/// collection type requires: (1) a `.mi` file, (2) a runtime module,
/// (3) constants in `runtime_fns.rs`, and (4) one entry here plus one
/// handler function below.
pub(crate) const COLLECTION_CTORS: &[(BuiltinCollectionKind, CollectionCtorFn)] = &[
    (BuiltinCollectionKind::List, lower_list_constructor),
    (BuiltinCollectionKind::Map, lower_map_constructor),
    (BuiltinCollectionKind::Set, lower_set_constructor),
    (BuiltinCollectionKind::Array, lower_array_constructor),
];

/// Resolves the element type `T` of a `List<T>` or an `Array<T, N>` from its
/// type definition.
fn sequence_elem_kind(ctx: &LoweringContext, sequence_ty: &Type) -> Option<TypeKind> {
    let inner_expr = match &sequence_ty.kind {
        TypeKind::List(inner_expr) | TypeKind::Array(inner_expr, _) => inner_expr.as_ref(),
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
            ) && !args.is_empty() =>
        {
            &args[0]
        }
        _ => return None,
    };
    if let Some(ty) = ctx.type_checker.get_type(inner_expr.id) {
        Some(ty.kind.clone())
    } else {
        infer_type_from_generic_arg(inner_expr, ctx).map(|inferred| inferred.kind)
    }
}

/// Lowers a `List(args)` constructor call.
///
/// Three forms are supported:
/// - `List()` — allocates an empty list with the element stride determined by `T`.
/// - `List(array)` — copies an array into a list; see [`lower_list_from_array`].
/// - `List(list)` — copies a list; see [`lower_list_from_list`].
pub(crate) fn lower_list_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let list_ty = if let Some(call_ty) = ctx.recorded_type(call_expr_id) {
        call_ty
    } else {
        Type::new(TypeKind::Int, *span)
    };

    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(list_ty.clone(), *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let elem_kind = sequence_elem_kind(ctx, &list_ty);
    let elem_size = elem_kind.as_ref().map_or(8, compute_elem_size_from_type);

    match args {
        [list] if is_list_argument(ctx, list) => {
            lower_list_from_list(ctx, span, list, destination)?;
        }
        [array] => {
            let elem_kind = elem_kind.or_else(|| {
                let array_ty = ctx.recorded_type(array.id)?;
                sequence_elem_kind(ctx, &array_ty)
            });
            // An inline element is copied as its component bytes; there is no
            // reference in its slot for the list to take.
            let elems_are_managed = elem_kind.is_some_and(|kind| {
                ctx.is_perceus_managed(&kind) && !types::element_layout(&kind).is_address
            });
            lower_list_from_array(ctx, span, array, elem_size, elems_are_managed, destination)?;
        }
        _ => emit_empty_list(ctx, span, &list_ty, destination),
    }
    Ok(result_op)
}

/// Allocate an empty list of type `list_ty` into `destination`, with slots as
/// wide as its element type.
///
/// The allocation call is also where codegen registers how the list releases,
/// clones and orders its elements, read from the destination's declared type.
pub(crate) fn emit_empty_list(
    ctx: &mut LoweringContext,
    span: &Span,
    list_ty: &Type,
    destination: Place,
) {
    let elem_size = sequence_elem_kind(ctx, list_ty)
        .as_ref()
        .map_or(8, compute_elem_size_from_type);
    let size_op = int_constant(elem_size, span);
    emit_runtime_call(ctx, span, rt::LIST_NEW, vec![size_op], destination);
}

/// Lowers `List(array)`: the runtime copies the array's element words into a
/// new list, leaving the array itself untouched.
///
/// The argument may be an array literal, a call result, or a variable the
/// caller keeps reading — so the element type, not the argument's syntax,
/// decides whether the managed-array variant is called, which takes the list's
/// own reference to each element. Only a temporary the argument's lowering
/// created is released afterwards; a local that already existed stays owned by
/// its scope.
fn lower_list_from_array(
    ctx: &mut LoweringContext,
    span: &Span,
    array: &Expression,
    elem_size: i64,
    elems_are_managed: bool,
    destination: Place,
) -> Result<(), LoweringError> {
    // The runtime reads the length from the array header; this word is advisory.
    let literal_len = if let ExpressionKind::Array(elements, _) = &array.node {
        elements.len() as i64
    } else {
        0
    };
    let rt_fn_name = if elems_are_managed {
        rt::LIST_NEW_FROM_MANAGED_ARRAY
    } else {
        rt::LIST_NEW_FROM_RAW
    };
    let trailing_args = vec![
        int_constant(literal_len, span),
        int_constant(elem_size, span),
    ];
    lower_copy_from_source(ctx, span, array, rt_fn_name, trailing_args, destination)
}

/// Whether the one argument of `List(...)` is itself a list rather than an
/// array. The two share a constructor but not a runtime header, so each needs
/// its own copy routine.
fn is_list_argument(ctx: &LoweringContext, argument: &Expression) -> bool {
    ctx.recorded_type(argument.id)
        .is_some_and(|ty| ty.kind.as_builtin_collection() == Some(BuiltinCollectionKind::List))
}

/// Lowers `List(list)`: the runtime copies the source list exactly as its
/// `clone()` does, so the two lists grow and shrink independently. The copy
/// keeps the source's element callbacks: managed elements are retained, and
/// elements whose class implements `Cloneable` are cloned.
fn lower_list_from_list(
    ctx: &mut LoweringContext,
    span: &Span,
    list: &Expression,
    destination: Place,
) -> Result<(), LoweringError> {
    lower_copy_from_source(ctx, span, list, rt::LIST_CLONE, Vec::new(), destination)
}

/// Lowers `source`, then calls the runtime routine `rt_fn_name` with the
/// source followed by `trailing_args`, storing the new collection into
/// `destination`.
///
/// The routine only reads the source, which may be a variable the caller keeps
/// using. Only a temporary the source's lowering created is released
/// afterwards; a local that already existed stays owned by its scope.
fn lower_copy_from_source(
    ctx: &mut LoweringContext,
    span: &Span,
    source: &Expression,
    rt_fn_name: &str,
    trailing_args: Vec<Operand>,
    destination: Place,
) -> Result<(), LoweringError> {
    let arg_watermark = ctx.body.local_decls.len();
    let source_op = lower_expression(ctx, source, None)?;
    let source_local = match &source_op {
        Operand::Copy(p) | Operand::Move(p) => Some(p.local),
        Operand::Constant(_) => None,
    };

    let mut args = vec![source_op];
    args.extend(trailing_args);
    emit_runtime_call(ctx, span, rt_fn_name, args, destination);

    if let Some(local) = source_local {
        ctx.emit_temp_drop(local, arg_watermark, *span);
    }
    Ok(())
}

/// Emits a call to the runtime function `name` storing into `destination`,
/// and continues lowering in the call's successor block.
fn emit_runtime_call(
    ctx: &mut LoweringContext,
    span: &Span,
    name: &str,
    args: Vec<Operand>,
    destination: Place,
) {
    let target_bb = ctx.new_basic_block();
    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier(name.to_string()),
    }));
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination,
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
}

/// An `int` constant operand holding `value`.
pub(super) fn int_constant(value: i64, span: &Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Int, *span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I64(
            value,
        )),
    }))
}

/// Lowers a `Map()` / `Map(<map-literal>)` constructor call.
///
/// Two forms are supported:
/// - `Map()` / `Map<K, V>()` — allocates an empty map.
/// - `Map({"a": 1, "b": 2})` — delegates to the map-literal lowering so the
///   resulting map is populated with the literal's entries (RC handled there).
pub(crate) fn lower_map_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let Some(arg) = args.first() {
        if matches!(&arg.node, ExpressionKind::Map(_)) {
            return lower_expression(ctx, arg, dest);
        }
    }

    let return_ty = if let Some(call_ty) = ctx.recorded_type(call_expr_id) {
        call_ty
    } else {
        crate::ast::factory::type_map(
            crate::ast::factory::type_void(),
            crate::ast::factory::type_void(),
        )
    };

    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(destination, Rvalue::Aggregate(AggregateKind::Map, vec![])),
        span: *span,
    });

    Ok(result_op)
}

/// Lowers a `Set()` / `Set(<set-literal>)` constructor call.
///
/// Two forms are supported:
/// - `Set()` / `Set<T>()` — allocates an empty set.
/// - `Set({1, 2, 3})` — delegates to the set-literal lowering so the resulting
///   set is populated with the literal's elements (RC handled there).
pub(crate) fn lower_set_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let Some(arg) = args.first() {
        if matches!(&arg.node, ExpressionKind::Set(_)) {
            return lower_expression(ctx, arg, dest);
        }
    }

    let return_ty = if let Some(call_ty) = ctx.recorded_type(call_expr_id) {
        call_ty
    } else {
        crate::ast::factory::type_set(crate::ast::factory::type_void())
    };

    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(destination, Rvalue::Aggregate(AggregateKind::Set, vec![])),
        span: *span,
    });

    Ok(result_op)
}

/// Lowers `Array<T, N>(e1, …, eN)`: the array built from its written
/// elements, the way an array literal is.
///
/// The type checker has already required one argument per element, each
/// compatible with `T`. Each is converted to the element slot's type here, so a
/// value written at a narrower width is stored at `T`'s.
fn lower_array_from_elements(
    ctx: &mut LoweringContext,
    span: &Span,
    array_ty: &Type,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let elem_watermark = ctx.body.local_decls.len();
    let ops: Vec<Operand> = args
        .iter()
        .map(|arg| {
            super::dispatch::lower_stored_value(ctx, arg, array_ty, super::dispatch::ELEMENT_SLOT)
                .map(|(op, _)| op)
        })
        .collect::<Result<_, _>>()?;
    let destination = dest.unwrap_or_else(|| Place::new(ctx.push_temp(array_ty.clone(), *span)));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination.clone(),
            Rvalue::Aggregate(AggregateKind::Array, ops.clone()),
        ),
        span: *span,
    });
    // The aggregate took its own reference to each managed element, so the
    // temps that built them are released here.
    for op in &ops {
        if let Operand::Copy(p) = op {
            ctx.emit_temp_drop(p.local, elem_watermark, *span);
        }
    }
    Ok(Operand::Copy(destination))
}

/// Lowers an `Array<T, N>()` constructor call with compile-time sized allocation.
///
/// Supports one form:
/// - `Array<T, N>()` where N is a compile-time constant integer expression
///   (integer literals and simple arithmetic like `4 * 4`).
///
/// Error cases:
/// - Non-constant size expressions → compile error
/// - Managed element types → compile error
pub(crate) fn lower_array_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Get the inferred type: Custom("Array", Some([elem_expr, size_expr]))
    let array_ty = if let Some(call_ty) = ctx.recorded_type(call_expr_id) {
        call_ty
    } else {
        return Err(LoweringError::unsupported_expression(
            "Unable to infer Array<T, N>() constructor type".to_string(),
            *span,
        ));
    };
    if !args.is_empty() {
        return lower_array_from_elements(ctx, span, &array_ty, args, dest);
    }

    // Extract the size expression from the type generic arguments
    let size_expr = match &array_ty.kind {
        TypeKind::Custom(name, Some(args)) if name == "Array" && args.len() == 2 => args[1].clone(),
        _ => {
            return Err(LoweringError::unsupported_expression(
                "Expected Array<T, N> type in constructor".to_string(),
                *span,
            ));
        }
    };

    // Const-evaluate the size expression. The size is a literal here because the
    // type checker folds a named `const` in the type (`instantiate_generic_type`)
    // before MIR sees it. Top-level `const` bindings are hoisted in the
    // declaration-collection pass, so a const-sized constructor resolves the same
    // whether it appears at module scope or inside a function body declared
    // earlier in source order.
    let size_value =
        crate::type_checker::TypeChecker::try_eval_const_int(&size_expr).ok_or_else(|| {
            LoweringError::unsupported_expression(
                "Array<T, N>() requires a compile-time constant size".to_string(),
                size_expr.span,
            )
        })?;

    if size_value < 0 {
        return Err(LoweringError::unsupported_expression(
            "Array<T, N>() size must be non-negative".to_string(),
            size_expr.span,
        ));
    }

    let size_value = size_value as i64;

    // Extract element type and compute its size
    let elem_type = match &array_ty.kind {
        TypeKind::Custom(_, Some(args)) if args.len() == 2 => {
            // Try to get the type from type checker first
            if let Some(ty) = ctx.type_checker.get_type(args[0].id) {
                ty.clone()
            } else {
                // Fall back to synthesizing from the expression
                infer_type_from_generic_arg(&args[0], ctx)
                    .unwrap_or_else(|| Type::new(TypeKind::Int, *span))
            }
        }
        _ => Type::new(TypeKind::Int, *span),
    };

    // An element stored inline lives in the array's own bytes, so the zeroed
    // storage the runtime hands back is already a valid value of its type and no
    // managed-element handling applies. Every other managed element is a
    // reference the type checker refuses; reaching here with one is a compiler bug.
    if ctx.is_perceus_managed(&elem_type.kind) && !types::element_layout(&elem_type.kind).is_address
    {
        return Err(LoweringError::unsupported_expression(
            format!(
                "Array<T, N>() with managed element type '{}' should have been rejected at type-check time",
                elem_type
            ),
            *span,
        ));
    }

    let elem_size = compute_elem_size_from_type(&elem_type.kind);

    // Set up the destination
    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(array_ty.clone(), *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    // Create operands for the runtime call
    let size_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Int, *span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I64(
            size_value,
        )),
    }));

    let elem_size_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Int, *span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I64(
            elem_size,
        )),
    }));

    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier(rt::ARRAY_NEW.to_string()),
    }));

    let target_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![size_op, elem_size_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: destination.clone(),
            target: Some(target_bb),
        },
        *span,
    ));

    ctx.set_current_block(target_bb);
    Ok(result_op)
}

/// Infer type from a generic argument expression.
/// Used when the type checker doesn't have a stored type for the generic arg.
fn infer_type_from_generic_arg(arg: &Expression, ctx: &LoweringContext) -> Option<Type> {
    use crate::ast::expression::ExpressionKind;

    match &arg.node {
        ExpressionKind::Type(inner_ty, _) => {
            // Type expressions created by create_type_expression
            Some((**inner_ty).clone())
        }
        ExpressionKind::Identifier(name, _) => {
            // Simple identifier like a primitive keyword or a type-table name.
            let kind = match name.as_str() {
                "int" => TypeKind::Int,
                "i32" => TypeKind::I32,
                "i64" => TypeKind::I64,
                "i8" => TypeKind::I8,
                "i16" => TypeKind::I16,
                "u32" => TypeKind::U32,
                "u64" => TypeKind::U64,
                "u8" => TypeKind::U8,
                "u16" => TypeKind::U16,
                "f32" => TypeKind::F32,
                "f64" => TypeKind::F64,
                "float" => TypeKind::Float,
                "bool" => TypeKind::Boolean,
                n if n == types::STRING_TYPE_NAME => TypeKind::String,
                _ => {
                    // Any other name (stdlib collections included) resolves
                    // through the type table, never by string special-case.
                    if ctx
                        .type_checker
                        .type_table
                        .global_type_definitions
                        .contains_key(name)
                    {
                        TypeKind::Custom(name.clone(), None)
                    } else {
                        return None;
                    }
                }
            };
            Some(Type::new(kind, arg.span))
        }
        ExpressionKind::TypeDeclaration(base_expr, Some(generics), _, _) => {
            // Generic type application like `List<int>` or `Map<int, string>`.
            // The base name resolves through the type table — no stdlib
            // special-casing — and every generic argument must itself infer.
            if let ExpressionKind::Identifier(name, _) = &base_expr.node {
                if generics.is_empty()
                    || !ctx
                        .type_checker
                        .type_table
                        .global_type_definitions
                        .contains_key(name)
                    || !generics
                        .iter()
                        .all(|g| infer_type_from_generic_arg(g, ctx).is_some())
                {
                    return None;
                }
                Some(Type::new(
                    TypeKind::Custom(name.clone(), Some(generics.clone())),
                    arg.span,
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Computes the element size in bytes for a collection element type: the
/// stride [`types::element_layout`] gives it.
pub(crate) fn compute_elem_size_from_type(kind: &TypeKind) -> i64 {
    types::element_layout(kind).stride
}
