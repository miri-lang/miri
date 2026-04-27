// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method dispatch lowering — name mangling, inheritance resolution, `lower_call`.

use crate::ast::expression::Expression;
use crate::ast::{BuiltinCollectionKind, ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind};
use crate::runtime_fns::rt;
use crate::type_checker::context::{
    class_needs_vtable, vtable_slot_index, MethodInfo, TypeDefinition,
};

use super::constructors::{lower_class_constructor, lower_struct_constructor, COLLECTION_CTORS};
use super::helpers::coerce_rvalue;
use super::{lower_expression, LoweringContext};

/// Context for lowering a collection intrinsic method (push/get/index).
struct CollectionIntrinsicCall<'a> {
    span: &'a Span,
    call_expr_id: usize,
    obj: &'a Expression,
    obj_ty: &'a Type,
    class_name: &'a str,
    method_name: &'a str,
    args: &'a [Expression],
}

/// Produce a mangled function name for a generic instantiation.
///
/// Example: `identity` with `[("T", int)]` → `identity__int`
pub(crate) fn mangle_generic_name(
    base: &str,
    type_args: &[(String, crate::ast::types::Type)],
) -> String {
    if type_args.is_empty() {
        return base.to_string();
    }

    // Convert all types to strings first so we can compute the exact capacity needed.
    // We avoid building an intermediate `Vec<String>` and calling `.join("__")`
    // which requires an extra pass and format! macro overhead.
    let mut total_len = base.len();
    let mangled_types: Vec<String> = type_args
        .iter()
        .map(|(_, ty)| {
            let s = type_kind_to_mangle_str(&ty.kind);
            total_len += 2 + s.len(); // "__" + type string length
            s
        })
        .collect();

    let mut path = String::with_capacity(total_len);
    path.push_str(base);
    for s in &mangled_types {
        path.push_str("__");
        path.push_str(s);
    }
    path
}

fn type_kind_to_mangle_str(kind: &TypeKind) -> String {
    match kind {
        TypeKind::Int => "int".to_string(),
        TypeKind::Float | TypeKind::F64 => "float".to_string(),
        TypeKind::F32 => "f32".to_string(),
        TypeKind::Boolean => "bool".to_string(),
        TypeKind::String => "String".to_string(),
        TypeKind::Void => "void".to_string(),
        TypeKind::Custom(name, None) => name.clone(),
        TypeKind::Custom(name, Some(_)) => name.clone(),
        // Canonical collection variants are normalized to Custom before this point.
        TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        TypeKind::Option(_) => "option".to_string(),
        TypeKind::I8 => "i8".to_string(),
        TypeKind::I16 => "i16".to_string(),
        TypeKind::I32 => "i32".to_string(),
        TypeKind::I64 => "i64".to_string(),
        TypeKind::U8 => "u8".to_string(),
        TypeKind::U16 => "u16".to_string(),
        TypeKind::U32 => "u32".to_string(),
        TypeKind::U64 => "u64".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Walk the inheritance chain starting at `class_name` to find the first class
/// or trait that directly declares `method_name`. Returns the defining class/trait
/// name and a clone of its [`MethodInfo`] so the caller can mangle the symbol correctly.
///
/// This is the core of inherited method resolution: if `Dog extends Animal` and
/// only `Animal` defines `speak`, the returned defining class is `"Animal"` and
/// the call is mangled to `Animal_speak`.
///
/// **Concrete caller / abstract definer rule**: when the original `class_name` is a
/// *concrete* class and the method is found in an *abstract* ancestor, the caller's
/// name is returned instead of the ancestor's name.  This ensures static dispatch
/// goes to the per-concrete-class compiled version (e.g. `Array_is_empty`) rather
/// than the abstract-class version (`Collection_is_empty`), which would use virtual
/// dispatch internally and crash for objects that have no vtable pointer (Array, List).
///
/// Also handles:
/// - Trait-typed receivers: walks the trait hierarchy to find the method.
/// - Default trait methods: if the class doesn't define the method, checks all
///   implemented traits (and their parent traits) for a default (non-abstract) impl.
pub(crate) fn resolve_inherited_method(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    // Handle trait-typed receiver (polymorphic trait dispatch).
    if matches!(type_defs.get(class_name), Some(TypeDefinition::Trait(_))) {
        return resolve_in_trait_hierarchy(type_defs, class_name, method_name);
    }

    // Is the original caller itself abstract?  If it is, the "concrete caller" rule
    // does not apply — we use the normal defining-class name.
    let caller_is_abstract = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(cd)) if cd.is_abstract
    );

    let mut current = class_name.to_string();
    loop {
        let (base, traits) = match type_defs.get(&current) {
            Some(TypeDefinition::Class(class_def)) => {
                if let Some(method_info) = class_def.methods.get(method_name) {
                    // When a concrete caller finds the method in an abstract ancestor,
                    // return the original caller's name so dispatch goes to the
                    // per-concrete-class compiled version rather than the abstract one.
                    let defining = if class_def.is_abstract && !caller_is_abstract {
                        class_name.to_string()
                    } else {
                        current.clone()
                    };
                    return Some((defining, method_info.clone()));
                }
                (class_def.base_class.clone(), class_def.traits.clone())
            }
            _ => return None,
        };
        // Check default (non-abstract) methods in implemented traits.
        for trait_name in &traits {
            if let Some(result) = resolve_trait_default_method(type_defs, trait_name, method_name) {
                return Some(result);
            }
        }
        match base {
            Some(b) => current = b,
            None => return None,
        }
    }
}

/// Walk the trait hierarchy to find `method_name`. Returns the defining trait
/// name and method info (abstract or concrete).
fn resolve_in_trait_hierarchy(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    trait_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    let mut to_check = vec![trait_name];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(t_name) {
            if let Some(method_info) = td.methods.get(method_name) {
                return Some((t_name.to_string(), method_info.clone()));
            }
            to_check.extend(td.parent_traits.iter().map(|s| s.as_str()));
        }
    }
    None
}

/// Walk the trait hierarchy (starting from `trait_name`) to find a non-abstract
/// (default) implementation of `method_name`. Returns None if only abstract
/// declarations exist or the method is not found.
fn resolve_trait_default_method(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    trait_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    let mut to_check = vec![trait_name];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(t_name) {
            if let Some(method_info) = td.methods.get(method_name) {
                if !method_info.is_abstract {
                    return Some((t_name.to_string(), method_info.clone()));
                }
            }
            to_check.extend(td.parent_traits.iter().map(|s| s.as_str()));
        }
    }
    None
}

pub fn lower_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // 1. Try Module Alias Call: `M.foo(args)` where `M` is a module alias.
    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) =
            try_lower_module_alias_call(ctx, span, call_expr_id, obj, method, args, dest.clone())?
        {
            return Ok(op);
        }
    }

    // 2. Try GPU Kernel Launch: `kernel_handle.launch(grid, block)`.
    if let ExpressionKind::Member(obj, prop) = &func.node {
        if let Some(op) =
            try_lower_kernel_launch(ctx, span, call_expr_id, obj, prop, args, dest.clone())?
        {
            return Ok(op);
        }
    }

    // 3. Try Method Call (including optimized collection intrinsics).
    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) =
            try_lower_method_call(ctx, span, call_expr_id, obj, method, args, dest.clone())?
        {
            return Ok(op);
        }
    }

    // 4. Try Constructor Call (Struct or Class).
    if let Some(op) = try_lower_constructor_call(ctx, span, call_expr_id, func, args, dest.clone())?
    {
        return Ok(op);
    }

    // 5. Fallback: Direct Function Call.
    lower_direct_call(ctx, span, call_expr_id, func, args, dest)
}

/// Lower a call to a function in another module via its alias: `M.foo(args)`.
fn try_lower_module_alias_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj_expr: &Expression,
    method_expr: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let ExpressionKind::Identifier(alias_name, _) = &obj_expr.node {
        let is_module_alias = ctx
            .type_checker
            .module_aliases
            .contains_key(alias_name.as_str());
        if is_module_alias {
            if let ExpressionKind::Identifier(func_name, _) = &method_expr.node {
                // If this is a generic instantiation, mangle the name.
                let mangled = if let Some(generic_args) =
                    ctx.type_checker.call_generic_mappings.get(&call_expr_id)
                {
                    mangle_generic_name(func_name, generic_args)
                } else {
                    func_name.clone()
                };

                let func_op = Operand::Constant(Box::new(crate::mir::Constant {
                    span: *span,
                    ty: Type::new(TypeKind::Identifier, *span),
                    literal: crate::ast::literal::Literal::Identifier(mangled),
                }));

                // Lower all arguments.
                let mut arg_ops = Vec::with_capacity(args.len());
                for arg in args {
                    arg_ops.push(lower_expression(ctx, arg, None)?);
                }

                // Inject allocator if the target function expects it.
                if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
                    let already_has_alloc = arg_ops.iter().any(|op| {
                        if let Operand::Copy(p) | Operand::Move(p) = op {
                            p.local == alloc_local
                        } else {
                            false
                        }
                    });
                    if !already_has_alloc {
                        arg_ops.push(Operand::Copy(Place::new(alloc_local)));
                    }
                }

                // Prepare return destination.
                let return_ty = ctx
                    .type_checker
                    .get_type(call_expr_id)
                    .cloned()
                    .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
                let (destination, result_op) = if let Some(d) = dest {
                    (d.clone(), Operand::Copy(d))
                } else {
                    let temp = ctx.push_temp(return_ty, *span);
                    let p = Place::new(temp);
                    (p.clone(), Operand::Copy(p))
                };

                let target_bb = ctx.new_basic_block();
                ctx.set_terminator(crate::mir::Terminator::new(
                    TerminatorKind::Call {
                        func: func_op,
                        args: arg_ops,
                        destination,
                        target: Some(target_bb),
                    },
                    *span,
                ));
                ctx.set_current_block(target_bb);
                return Ok(Some(result_op));
            }
        }
    }
    Ok(None)
}

/// Lower a GPU kernel launch: `kernel_handle.launch(grid, block)`.
fn try_lower_kernel_launch(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    prop: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let ExpressionKind::Identifier(name, _) = &prop.node {
        if name == "launch" {
            // Check if objective is a Kernel type.
            if let Some(ty) = ctx.type_checker.get_type(obj.id) {
                if let TypeKind::Custom(type_name, _) = &ty.kind {
                    if type_name == "Kernel" {
                        let kernel_op = lower_expression(ctx, obj, None)?;

                        if args.len() != 2 {
                            return Err(LoweringError::invalid_gpu_launch_args(
                                2,
                                args.len(),
                                *span,
                            ));
                        }

                        let grid_op = lower_expression(ctx, &args[0], None)?;
                        let block_op = lower_expression(ctx, &args[1], None)?;

                        let mut return_ty = Type::new(TypeKind::Void, *span);
                        if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
                            return_ty = ty.clone();
                        }

                        let (destination, op) = if let Some(d) = dest {
                            (d.clone(), Operand::Copy(d))
                        } else {
                            let temp = ctx.push_temp(return_ty, *span);
                            let p = Place::new(temp);
                            (p.clone(), Operand::Copy(p))
                        };
                        let target_bb = ctx.new_basic_block();

                        ctx.set_terminator(Terminator::new(
                            TerminatorKind::GpuLaunch {
                                kernel: kernel_op,
                                grid: grid_op,
                                block: block_op,
                                destination,
                                target: Some(target_bb),
                            },
                            *span,
                        ));

                        ctx.set_current_block(target_bb);
                        return Ok(Some(op));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Lower a method call on a class or trait object.
///
/// This handles inheritance resolution, virtual vs static dispatch, and specialized
/// collection intrinsics (`push`, `get`, etc.).
fn try_lower_method_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    method_expr: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let raw_obj_ty = match ctx.type_checker.get_type(obj.id) {
        Some(ty) => ty,
        None => return Ok(None),
    };

    // Concrete receiver resolution for inherited methods inside abstract classes.
    let obj_ty_override: Option<Type> = if let TypeKind::Custom(name, _) = &raw_obj_ty.kind {
        let is_abstract = matches!(
            ctx.type_checker.global_type_definitions.get(name.as_str()),
            Some(TypeDefinition::Class(cd)) if cd.is_abstract
        );
        if is_abstract {
            if let ExpressionKind::Identifier(var_name, _) = &obj.node {
                if let Some(&local) = ctx.variable_map.get(var_name.as_str()) {
                    Some(ctx.body.local_decls[local.0].ty.clone())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let obj_ty = obj_ty_override.as_ref().unwrap_or(raw_obj_ty);
    let class_name = match &obj_ty.kind {
        TypeKind::String => Some("String".to_string()),
        TypeKind::Tuple(_) => Some("Tuple".to_string()),
        TypeKind::Custom(name, _) => Some(name.clone()),
        k => k.as_builtin_collection().map(|b| b.name().to_string()),
    };

    let class_name = match class_name {
        Some(name) => name,
        None => return Ok(None),
    };

    let method_name = match &method_expr.node {
        ExpressionKind::Identifier(name, _) => name,
        _ => return Ok(None),
    };

    // 1. Try specialized collection optimizations: emit direct index-reads or
    // runtime intrinsic calls to avoid monomorphization conflicts and enable
    // better RC analysis.
    let call = CollectionIntrinsicCall {
        span,
        call_expr_id,
        obj,
        obj_ty,
        class_name: &class_name,
        method_name,
        args,
    };
    if let Some(op) = try_lower_collection_intrinsic(ctx, call, dest.clone())? {
        return Ok(Some(op));
    }

    // 2. Regular inherited method resolution and dispatch.
    if let Some((defining_class, method_info)) = resolve_inherited_method(
        &ctx.type_checker.global_type_definitions,
        &class_name,
        method_name,
    ) {
        let return_ty = method_info.return_type.clone();

        // For `super.method()`, the receiver must be `self`.
        let obj_watermark = ctx.body.local_decls.len();
        let self_op = if matches!(&obj.node, ExpressionKind::Super) {
            if let Some(&self_local) = ctx.variable_map.get("self") {
                Operand::Copy(Place::new(self_local))
            } else {
                lower_expression(ctx, obj, None)?
            }
        } else {
            lower_expression(ctx, obj, None)?
        };

        // Track the receiver local for Perceus-compatible temporary drop.
        let obj_temp_local = if let Operand::Copy(ref p) = self_op {
            Some(p.local)
        } else {
            None
        };

        let (destination, op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(return_ty, *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        };

        let target_bb = ctx.new_basic_block();

        // Virtual dispatch: when the receiver's static type is an abstract class
        // or a trait, look up the function pointer through the vtable at runtime.
        let use_virtual_dispatch = !matches!(&obj.node, ExpressionKind::Super)
            && ((class_needs_vtable(&class_name, &ctx.type_checker.global_type_definitions)
                && matches!(
                    ctx.type_checker.global_type_definitions.get(&class_name),
                    Some(TypeDefinition::Class(cd)) if cd.is_abstract
                ))
                || matches!(
                    ctx.type_checker.global_type_definitions.get(&class_name),
                    Some(TypeDefinition::Trait(_))
                ));

        if use_virtual_dispatch {
            if let Some(slot) = vtable_slot_index(
                &class_name,
                method_name,
                &ctx.type_checker.global_type_definitions,
            ) {
                let mut call_args = vec![self_op];
                for arg in args {
                    call_args.push(lower_expression(ctx, arg, None)?);
                }
                if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
                    call_args.push(Operand::Copy(Place::new(alloc_local)));
                }

                ctx.set_terminator(Terminator::new(
                    TerminatorKind::VirtualCall {
                        vtable_slot: slot,
                        args: call_args,
                        destination,
                        target: Some(target_bb),
                    },
                    *span,
                ));
                ctx.set_current_block(target_bb);
                if let Some(local) = obj_temp_local {
                    ctx.emit_temp_drop(local, obj_watermark, *span);
                }
                return Ok(Some(op));
            }
        }

        // Static dispatch path.
        let mut mangled_name = String::with_capacity(defining_class.len() + 1 + method_name.len());
        mangled_name.push_str(&defining_class);
        mangled_name.push('_');
        mangled_name.push_str(method_name);
        let mut call_args = vec![self_op];
        for arg in args {
            call_args.push(lower_expression(ctx, arg, None)?);
        }
        if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
            call_args.push(Operand::Copy(Place::new(alloc_local)));
        }

        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(mangled_name),
        }));

        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: call_args,
                destination,
                target: Some(target_bb),
            },
            *span,
        ));
        ctx.set_current_block(target_bb);
        if let Some(local) = obj_temp_local {
            ctx.emit_temp_drop(local, obj_watermark, *span);
        }
        return Ok(Some(op));
    }

    Ok(None)
}

/// Lower optimized collection methods directly to MIR instructions or intrinsics.
///
/// This prevents monomorphization conflicts when multiple instantiations (e.g., List<int>, List<bool>)
/// try to define the same method, and enables more precise RC analysis by keeping the concrete
/// element type visible at the call site.
fn try_lower_collection_intrinsic(
    ctx: &mut LoweringContext,
    call: CollectionIntrinsicCall,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let CollectionIntrinsicCall {
        span,
        call_expr_id,
        obj,
        obj_ty,
        class_name,
        method_name,
        args,
    } = call;
    // element_at / get on List, Array, Tuple: emit a direct index-read.
    if args.len() == 1
        && matches!(method_name, "element_at" | "get")
        && matches!(class_name, "List" | "Array" | "Tuple")
    {
        let obj_watermark = ctx.body.local_decls.len();
        let obj_op = lower_expression(ctx, obj, None)?;
        // Track the source local for potential cleanup of expression temps.
        let obj_op_src = match &obj_op {
            Operand::Copy(p) | Operand::Move(p) => Some(p.local),
            _ => None,
        };
        let index_op = lower_expression(ctx, &args[0], None)?;

        // Always use Copy semantics for the assignment to obj_local.  The
        // source local will get its own DecRef at scope exit (or, for params,
        // won't be DecRef'd at all).  Using Copy here ensures Perceus inserts
        // an IncRef, and the StorageDead of obj_local below provides the
        // matching DecRef — keeping the pair balanced regardless of whether
        // the source is a local variable or a parameter.
        let obj_op = match obj_op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        let obj_local = ctx.push_temp(obj_ty.clone(), *span);
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
            span: *span,
        });

        let index_local = match index_op {
            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
            _ => {
                let temp = ctx.push_temp(Type::new(TypeKind::Int, args[0].span), args[0].span);
                ctx.push_statement(crate::mir::Statement {
                    kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(index_op)),
                    span: args[0].span,
                });
                temp
            }
        };

        let mut indexed_place = Place::new(obj_local);
        indexed_place
            .projection
            .push(crate::mir::PlaceElem::Index(index_local));

        let elem_ty = if let Some(t) = ctx.type_checker.get_type(call_expr_id) {
            t.clone()
        } else {
            Type::new(TypeKind::Int, *span)
        };

        let (destination, op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(elem_ty, *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        };

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(destination, Rvalue::Use(Operand::Copy(indexed_place))),
            span: *span,
        });

        // Always clean up the obj_local temp (holds the collection for indexing).
        // The assignment above uses Copy semantics, so Perceus inserts IncRef
        // for the copy source.  This StorageDead triggers the matching DecRef.
        ctx.emit_temp_drop(obj_local, obj_watermark, *span);
        // Also clean up the source local if it was a temp created during
        // expression lowering (e.g. a function-call return value).
        if let Some(src_local) = obj_op_src {
            ctx.emit_temp_drop(src_local, obj_watermark, *span);
        }
        return Ok(Some(op));
    }

    // push(item) on List: emit miri_rt_list_push directly.
    if args.len() == 1 && method_name == "push" && class_name == "List" {
        let item_watermark = ctx.body.local_decls.len();
        let obj_op = lower_expression(ctx, obj, None)?;
        let item_op = lower_expression(ctx, &args[0], None)?;
        // Capture the source local before Move→Copy conversion so we can emit
        // StorageDead for fresh temps after the call.  This gives Perceus the
        // DecRef that balances the IncRef it inserts for the Copy use below.
        let item_op_src = match &item_op {
            Operand::Copy(p) | Operand::Move(p) => Some(p.local),
            _ => None,
        };
        // Force Copy semantics so Perceus emits IncRef for managed sources.
        let item_copy = match item_op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        let item_ty = item_copy.ty(&ctx.body).clone();
        let item_local = ctx.push_temp(item_ty, args[0].span);
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(item_local), Rvalue::Use(item_copy)),
            span: args[0].span,
        });
        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(rt::LIST_PUSH.to_string()),
        }));
        let target_bb = ctx.new_basic_block();
        let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: vec![obj_op, Operand::Copy(Place::new(item_local))],
                destination: Place::new(dummy_dest),
                target: Some(target_bb),
            },
            *span,
        ));
        ctx.set_current_block(target_bb);
        // Release the source local if it was a fresh temp (e.g. a literal or
        // function-call result).  Perceus then inserts the DecRef that balances
        // the IncRef from the Copy in the Assign above, leaving the list with
        // exactly one reference to the pushed element.
        if let Some(src) = item_op_src {
            ctx.emit_temp_drop(src, item_watermark, args[0].span);
        }
        return Ok(Some(Operand::Copy(Place::new(dummy_dest))));
    }

    // insert(index, item) on List: emit miri_rt_list_insert directly.
    if args.len() == 2 && method_name == "insert" && class_name == "List" {
        let item_watermark = ctx.body.local_decls.len();
        let obj_op = lower_expression(ctx, obj, None)?;
        let index_op = lower_expression(ctx, &args[0], None)?;
        let item_op = lower_expression(ctx, &args[1], None)?;
        // Capture the source local before Move→Copy conversion.
        let item_op_src = match &item_op {
            Operand::Copy(p) | Operand::Move(p) => Some(p.local),
            _ => None,
        };
        // Force Copy semantics so Perceus emits IncRef for managed sources.
        let item_copy = match item_op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        let item_ty = item_copy.ty(&ctx.body).clone();
        let item_local = ctx.push_temp(item_ty, args[1].span);
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(item_local), Rvalue::Use(item_copy)),
            span: args[1].span,
        });
        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(rt::LIST_INSERT.to_string()),
        }));
        let target_bb = ctx.new_basic_block();
        let result_temp = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: vec![obj_op, index_op, Operand::Copy(Place::new(item_local))],
                destination: Place::new(result_temp),
                target: Some(target_bb),
            },
            *span,
        ));
        ctx.set_current_block(target_bb);
        if let Some(src) = item_op_src {
            ctx.emit_temp_drop(src, item_watermark, args[1].span);
        }
        return Ok(Some(Operand::Copy(Place::new(result_temp))));
    }

    // set(index, value) on List / Array: emit a direct indexed assignment.
    if args.len() == 2 && method_name == "set" && matches!(class_name, "List" | "Array") {
        let obj_watermark = ctx.body.local_decls.len();
        let obj_op = lower_expression(ctx, obj, None)?;
        let obj_op_src = match &obj_op {
            Operand::Copy(p) | Operand::Move(p) => Some(p.local),
            _ => None,
        };
        let index_op = lower_expression(ctx, &args[0], None)?;
        let item_op = lower_expression(ctx, &args[1], None)?;
        // Capture the item source local before converting Move→Copy so we can
        // emit StorageDead for it after the assignment.  This gives Perceus the
        // matching DecRef for the IncRef it inserts for the Copy use below.
        let item_op_src = match &item_op {
            Operand::Copy(p) | Operand::Move(p) => Some(p.local),
            _ => None,
        };
        // Use Copy semantics so Perceus inserts IncRef; the StorageDead
        // provides the matching DecRef.
        let obj_op = match obj_op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        let item_op = match item_op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        let obj_local = ctx.push_temp(obj_ty.clone(), *span);
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
            span: *span,
        });
        let index_local = match index_op {
            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
            _ => {
                let temp = ctx.push_temp(Type::new(TypeKind::Int, args[0].span), args[0].span);
                ctx.push_statement(crate::mir::Statement {
                    kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(index_op)),
                    span: args[0].span,
                });
                temp
            }
        };
        let mut indexed_place = Place::new(obj_local);
        indexed_place
            .projection
            .push(crate::mir::PlaceElem::Index(index_local));
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(indexed_place, Rvalue::Use(item_op)),
            span: *span,
        });
        // Release the obj_local temp — triggers DecRef to match the IncRef above.
        ctx.emit_temp_drop(obj_local, obj_watermark, *span);
        // Release the obj source local if it was a fresh temp from expression lowering.
        if let Some(src_local) = obj_op_src {
            ctx.emit_temp_drop(src_local, obj_watermark, *span);
        }
        // Release the item source local so Perceus inserts the matching DecRef for
        // the IncRef it inserted when the Copy was used in the indexed assignment.
        // Without this, a concat result (e.g. "x"+"y") would have RC=2 after the
        // assignment instead of RC=1, leaking on every subsequent overwrite.
        if let Some(item_src) = item_op_src {
            ctx.emit_temp_drop(item_src, obj_watermark, *span);
        }
        return Ok(Some(Operand::Constant(Box::new(crate::mir::Constant {
            span: *span,
            ty: Type::new(TypeKind::Void, *span),
            literal: crate::ast::literal::Literal::None,
        }))));
    }

    Ok(None)
}

/// Lower a constructor call for a struct or class.
fn try_lower_constructor_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let Some(func_ty) = ctx.type_checker.get_type(func.id) {
        if let TypeKind::Meta(inner) = &func_ty.kind {
            if let TypeKind::Custom(type_name, _) = &inner.kind {
                // Struct constructor.
                if let Some(TypeDefinition::Struct(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    return lower_struct_constructor(ctx, span, type_name, def, args, dest)
                        .map(Some);
                }
                // Class constructor.
                if let Some(TypeDefinition::Class(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    // Built-in collection constructors.
                    if let Some(kind) = BuiltinCollectionKind::from_name(type_name) {
                        if let Some((_, ctor_fn)) =
                            COLLECTION_CTORS.iter().find(|(k, _)| *k == kind)
                        {
                            return ctor_fn(ctx, span, call_expr_id, args, dest).map(Some);
                        }
                    }
                    return lower_class_constructor(ctx, span, type_name, def, args, dest)
                        .map(Some);
                }
            }
        }
    }
    Ok(None)
}

/// Lower a direct function call (global function, lambda, or generic instantiation).
fn lower_direct_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Record the watermark before lowering the callee expression so we can
    // detect and drop any managed temp it creates (e.g. the closure struct
    // returned by `make_counter(...)` in `make_counter(...)()` chains).
    let func_watermark = ctx.body.local_decls.len();
    let mut func_op = lower_expression(ctx, func, None)?;

    // Mangle generic function names.
    if let ExpressionKind::Identifier(func_name, _) = &func.node {
        if let Some(generic_args) = ctx.type_checker.call_generic_mappings.get(&call_expr_id) {
            let mangled = mangle_generic_name(func_name, generic_args);
            func_op = Operand::Constant(Box::new(crate::mir::Constant {
                span: func.span,
                ty: crate::ast::types::Type::new(TypeKind::Identifier, func.span),
                literal: crate::ast::literal::Literal::Identifier(mangled),
            }));
        }
    }

    // Resolve parameter types for argument coercion.
    let is_generic_call = ctx
        .type_checker
        .call_generic_mappings
        .contains_key(&call_expr_id);
    let func_ty = ctx.type_checker.get_type(func.id);
    let param_types = if is_generic_call {
        None
    } else if let Some(ty) = func_ty {
        if let TypeKind::Function(func_data) = &ty.kind {
            Some(func_data.params.clone())
        } else {
            None
        }
    } else {
        None
    };

    let arg_watermark = ctx.body.local_decls.len();
    let mut arg_ops = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        let mut op = lower_expression(ctx, arg, None)?;

        // Apply parameter type coercion.
        if let Some(params) = &param_types {
            if i < params.len() {
                let target_ty = super::resolve_type(ctx.type_checker, &params[i].typ);
                let op_ty = op.ty(&ctx.body).clone();
                if op_ty.kind != target_ty.kind {
                    let temp = ctx.push_temp(target_ty.clone(), arg.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            Place::new(temp),
                            coerce_rvalue(op, &op_ty, &target_ty),
                        ),
                        span: arg.span,
                    });
                    op = Operand::Copy(Place::new(temp));
                }
            }
        }

        // Ensure managed arguments are passed as Copy to trigger IncRef.
        let op = match op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        arg_ops.push(op);
    }

    // Fill in default values for missing arguments.
    if let Some(params) = &param_types {
        for param in params.iter().skip(args.len()) {
            if let Some(default_expr) = &param.default_value {
                let default_op = lower_expression(ctx, default_expr, None)?;
                arg_ops.push(default_op);
            }
        }
    }

    // Implicit Allocator Injection.
    let is_runtime_fn = if let ExpressionKind::Identifier(name, _) = &func.node {
        name.starts_with("miri_")
    } else {
        false
    };
    let is_indirect_call = !matches!(
        func_op,
        Operand::Constant(ref c) if matches!(c.literal, crate::ast::literal::Literal::Identifier(_))
    );

    if !is_runtime_fn && !is_indirect_call {
        if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
            let already_has_alloc = arg_ops.iter().any(|op| {
                if let Operand::Copy(p) | Operand::Move(p) = op {
                    p.local == alloc_local
                } else {
                    false
                }
            });
            if !already_has_alloc {
                arg_ops.push(Operand::Copy(Place::new(alloc_local)));
            }
        }
    }

    // Determine return destination.
    let mut return_ty = Type::new(TypeKind::Void, *span);
    if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
        return_ty = ty.clone();
    }

    let (destination, op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let target_bb = ctx.new_basic_block();
    let func_op_for_drop = func_op.clone();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: arg_ops.clone(),
            destination: destination.clone(),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);

    // Release managed temporaries created while lowering the call arguments.
    let dest_local = destination.local;
    for arg_op in &arg_ops {
        if let Operand::Copy(place) | Operand::Move(place) = arg_op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, *span);
            }
        }
    }

    // For indirect calls (closure calls), also drop any managed temp that was
    // created while lowering the callee expression.  This handles chains like
    // `make_counter(...)()` where `make_counter(...)` returns a closure into a
    // temp that is used as the callee and never otherwise dropped.
    if is_indirect_call {
        if let Operand::Copy(place) | Operand::Move(place) = &func_op_for_drop {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, func_watermark, *span);
            }
        }
    }

    Ok(op)
}
