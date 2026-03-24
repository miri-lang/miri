// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method dispatch lowering — name mangling, inheritance resolution, `lower_call`.

use crate::ast::expression::Expression;
use crate::ast::{BuiltinCollectionKind, ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    AggregateKind, Constant, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::{
    class_needs_vtable, vtable_slot_index, MethodInfo, TypeDefinition,
};

use super::constructors::{
    compute_elem_size_from_type, lower_class_constructor, lower_struct_constructor,
};
use super::helpers::coerce_rvalue;
use super::{lower_expression, LoweringContext};

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
        TypeKind::List(_) => "list".to_string(),
        TypeKind::Array(_, _) => "array".to_string(),
        TypeKind::Map(_, _) => "map".to_string(),
        TypeKind::Set(_) => "set".to_string(),
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

    let mut current = class_name.to_string();
    loop {
        let (base, traits) = match type_defs.get(&current) {
            Some(TypeDefinition::Class(class_def)) => {
                if let Some(method_info) = class_def.methods.get(method_name) {
                    return Some((current, method_info.clone()));
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
    let mut to_check = vec![trait_name.to_string()];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name.clone()) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(&t_name) {
            if let Some(method_info) = td.methods.get(method_name) {
                return Some((t_name, method_info.clone()));
            }
            to_check.extend(td.parent_traits.iter().cloned());
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
    let mut to_check = vec![trait_name.to_string()];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name.clone()) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(&t_name) {
            if let Some(method_info) = td.methods.get(method_name) {
                if !method_info.is_abstract {
                    return Some((t_name, method_info.clone()));
                }
            }
            to_check.extend(td.parent_traits.iter().cloned());
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
    // Module alias call: `M.foo(args)` where `M` was declared via `use X as M`.
    // Lower as a direct call to `foo` by emitting a plain function-name operand.
    if let ExpressionKind::Member(obj_expr, method_expr) = &func.node {
        if let ExpressionKind::Identifier(alias_name, _) = &obj_expr.node {
            let is_module_alias = ctx
                .type_checker
                .module_aliases
                .contains_key(alias_name.as_str());
            if is_module_alias {
                if let ExpressionKind::Identifier(func_name, _) = &method_expr.node {
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
                    let mut arg_ops = Vec::with_capacity(args.len());
                    for arg in args {
                        arg_ops.push(lower_expression(ctx, arg, None)?);
                    }
                    // Inject allocator the same way the generic call path does.
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
                    return Ok(result_op);
                }
            }
        }
    }

    // Check for kernel launch: kernel_handle.launch(grid, block)
    if let ExpressionKind::Member(obj, prop) = &func.node {
        if let ExpressionKind::Identifier(name, _) = &prop.node {
            if name == "launch" {
                // Check if the object is of type Kernel
                // We need to resolve the type of 'obj'
                // We can check if TypeChecker says it's Kernel
                // Note: infer_expression puts types in ctx.type_checker.types map by ID.
                if let Some(ty) = ctx.type_checker.get_type(obj.id) {
                    // Check if type name is Kernel
                    if let TypeKind::Custom(type_name, _) = &ty.kind {
                        if type_name == "Kernel" {
                            // This is a GPU kernel launch!
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

                            // GPU launch returns void by default.

                            let mut return_ty = Type::new(TypeKind::Void, *span);
                            if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
                                return_ty = ty.clone();
                            }

                            // Use provided dest or create temp
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

                            return Ok(op);
                        }
                    }
                }
            }
        }
    }

    // Handle method calls on class types (e.g. s.to_upper(), obj.method(args)).
    // Resolves the class definition from the object's type and emits a call to
    // the mangled function `{ClassName}_{method_name}`.
    if let ExpressionKind::Member(obj, method_expr) = &func.node {
        if let Some(obj_ty) = ctx.type_checker.get_type(obj.id) {
            let class_name = match &obj_ty.kind {
                TypeKind::String => Some("String".to_string()),
                TypeKind::Tuple(_) => Some("Tuple".to_string()),
                TypeKind::Custom(name, _) => Some(name.clone()),
                k => k.as_builtin_collection().map(|b| b.name().to_string()),
            };

            if let Some(class_name) = class_name {
                if let ExpressionKind::Identifier(method_name, _) = &method_expr.node {
                    // element_at / get on List, Array, Tuple: emit a direct index-read
                    // at the call site so Perceus sees the concrete element type and
                    // inserts IncRef. Going through the compiled generic method would
                    // lose the concrete type, preventing IncRef of managed elements.
                    if args.len() == 1
                        && matches!(method_name.as_str(), "element_at" | "get")
                        && matches!(class_name.as_str(), "List" | "Array" | "Tuple")
                    {
                        let obj_watermark = ctx.body.local_decls.len();
                        let obj_op = lower_expression(ctx, obj, None)?;
                        let obj_op_copy_src = if let Operand::Copy(ref p) = obj_op {
                            Some(p.local)
                        } else {
                            None
                        };
                        let index_op = lower_expression(ctx, &args[0], None)?;

                        let obj_local = ctx.push_temp(obj_ty.clone(), *span);
                        ctx.push_statement(crate::mir::Statement {
                            kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
                            span: *span,
                        });

                        let index_local = match index_op {
                            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                                p.local
                            }
                            _ => {
                                let temp = ctx.push_temp(
                                    Type::new(TypeKind::Int, args[0].span),
                                    args[0].span,
                                );
                                ctx.push_statement(crate::mir::Statement {
                                    kind: StatementKind::Assign(
                                        Place::new(temp),
                                        Rvalue::Use(index_op),
                                    ),
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
                            kind: StatementKind::Assign(
                                destination,
                                Rvalue::Use(Operand::Copy(indexed_place)),
                            ),
                            span: *span,
                        });

                        if let Some(src_local) = obj_op_copy_src {
                            ctx.emit_temp_drop(obj_local, obj_watermark, *span);
                            ctx.emit_temp_drop(src_local, obj_watermark, *span);
                        }
                        return Ok(op);
                    }

                    // push(item) on List: emit miri_rt_list_push directly.
                    // Going through the compiled List_push method would cause a
                    // monomorphization conflict when List<int> and List<bool> both
                    // try to declare List_push with different T-typed arg signatures.
                    if args.len() == 1 && method_name == "push" && class_name == "List" {
                        let obj_op = lower_expression(ctx, obj, None)?;
                        let item_op = lower_expression(ctx, &args[0], None)?;
                        let item_ty = item_op.ty(&ctx.body).clone();
                        let item_local = ctx.push_temp(item_ty, args[0].span);
                        ctx.push_statement(crate::mir::Statement {
                            kind: StatementKind::Assign(
                                Place::new(item_local),
                                Rvalue::Use(item_op),
                            ),
                            span: args[0].span,
                        });
                        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
                            span: *span,
                            ty: Type::new(TypeKind::Identifier, *span),
                            literal: crate::ast::literal::Literal::Identifier(
                                rt::LIST_PUSH.to_string(),
                            ),
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
                        return Ok(Operand::Copy(Place::new(dummy_dest)));
                    }

                    // insert(index, item) on List: emit miri_rt_list_insert directly.
                    if args.len() == 2 && method_name == "insert" && class_name == "List" {
                        let obj_op = lower_expression(ctx, obj, None)?;
                        let index_op = lower_expression(ctx, &args[0], None)?;
                        let item_op = lower_expression(ctx, &args[1], None)?;
                        let item_ty = item_op.ty(&ctx.body).clone();
                        let item_local = ctx.push_temp(item_ty, args[1].span);
                        ctx.push_statement(crate::mir::Statement {
                            kind: StatementKind::Assign(
                                Place::new(item_local),
                                Rvalue::Use(item_op),
                            ),
                            span: args[1].span,
                        });
                        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
                            span: *span,
                            ty: Type::new(TypeKind::Identifier, *span),
                            literal: crate::ast::literal::Literal::Identifier(
                                rt::LIST_INSERT.to_string(),
                            ),
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
                        return Ok(Operand::Copy(Place::new(result_temp)));
                    }

                    // set(index, value) on List / Array: emit a direct indexed assignment
                    // so the existing codegen handles OOB checking and element RC correctly,
                    // and avoids monomorphization conflicts from the T-typed method parameter.
                    if args.len() == 2
                        && method_name == "set"
                        && matches!(class_name.as_str(), "List" | "Array")
                    {
                        let obj_op = lower_expression(ctx, obj, None)?;
                        let index_op = lower_expression(ctx, &args[0], None)?;
                        let item_op = lower_expression(ctx, &args[1], None)?;
                        let obj_local = ctx.push_temp(obj_ty.clone(), *span);
                        ctx.push_statement(crate::mir::Statement {
                            kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
                            span: *span,
                        });
                        let index_local = match index_op {
                            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                                p.local
                            }
                            _ => {
                                let temp = ctx.push_temp(
                                    Type::new(TypeKind::Int, args[0].span),
                                    args[0].span,
                                );
                                ctx.push_statement(crate::mir::Statement {
                                    kind: StatementKind::Assign(
                                        Place::new(temp),
                                        Rvalue::Use(index_op),
                                    ),
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
                        return Ok(Operand::Constant(Box::new(crate::mir::Constant {
                            span: *span,
                            ty: Type::new(TypeKind::Void, *span),
                            literal: crate::ast::literal::Literal::None,
                        })));
                    }

                    if let Some((defining_class, method_info)) = resolve_inherited_method(
                        &ctx.type_checker.global_type_definitions,
                        &class_name,
                        method_name,
                    ) {
                        let return_ty = method_info.return_type.clone();

                        // For `super.method()`, the receiver must be `self` (the current
                        // instance), not the super constant (which would lower to a null
                        // pointer via Literal::Identifier). The type checker already resolved
                        // obj_ty to the parent class type so `resolve_inherited_method` above
                        // correctly starts its search from the parent — we only need to
                        // substitute the actual self operand here.
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
                        // Track the receiver local so we can emit StorageDead for it
                        // after the call (enabling the Perceus DecRef). This covers both
                        // simple receivers (Copy(local)) and field-projected ones
                        // (Copy(local.field)) — in both cases `p.local` is the outermost
                        // container that may be a new temp created during lowering.
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
                            && (
                                // Abstract class dispatch: receiver is an abstract class with vtable.
                                (class_needs_vtable(
                                    &class_name,
                                    &ctx.type_checker.global_type_definitions,
                                ) && matches!(
                                    ctx.type_checker.global_type_definitions.get(&class_name),
                                    Some(TypeDefinition::Class(cd)) if cd.is_abstract
                                ))
                                // Trait dispatch: receiver is a trait type.
                                || matches!(
                                    ctx.type_checker.global_type_definitions.get(&class_name),
                                    Some(TypeDefinition::Trait(_))
                                )
                            );

                        if use_virtual_dispatch {
                            // Find the vtable slot index in the abstract class's method table.
                            if let Some(slot) = vtable_slot_index(
                                &class_name,
                                method_name,
                                &ctx.type_checker.global_type_definitions,
                            ) {
                                let mut call_args = vec![self_op];
                                for arg in args {
                                    call_args.push(lower_expression(ctx, arg, None)?);
                                }
                                // Inject allocator — compiled class methods accept it as their last arg
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
                                return Ok(op);
                            }
                            // If slot not found, fall through to static dispatch below.
                        }

                        // Static dispatch path (concrete receiver type, super calls, etc.)
                        let mangled_name = format!("{}_{}", defining_class, method_name);
                        let mut call_args = vec![self_op];
                        for arg in args {
                            call_args.push(lower_expression(ctx, arg, None)?);
                        }
                        // Inject allocator — compiled class methods accept it as their last arg
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
                        return Ok(op);
                    }
                }
            }
        }
    }

    // Check for struct constructor call
    // The type checker gives struct names the type Meta(Custom(name, ...))
    if let Some(func_ty) = ctx.type_checker.get_type(func.id) {
        if let TypeKind::Meta(inner) = &func_ty.kind {
            if let TypeKind::Custom(type_name, _) = &inner.kind {
                // Look up struct definition
                if let Some(TypeDefinition::Struct(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    // This is a struct constructor - emit Aggregate instead of Call
                    return lower_struct_constructor(ctx, span, type_name, def, args, dest);
                }
                // Look up class definition
                if let Some(TypeDefinition::Class(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    if BuiltinCollectionKind::from_name(type_name)
                        == Some(BuiltinCollectionKind::List)
                    {
                        let list_ty = if let Some(call_ty) = ctx.type_checker.get_type(call_expr_id)
                        {
                            call_ty.clone()
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

                        let target_bb = ctx.new_basic_block();

                        if args.len() == 1 {
                            let array_op = lower_expression(ctx, &args[0], None)?;

                            // Track the temp array local so we can emit StorageDead after the call
                            let temp_array_local = match &array_op {
                                Operand::Copy(p) | Operand::Move(p) => Some(p.clone()),
                                _ => None,
                            };

                            // Determine array length, element size, and whether
                            // elements are RC-managed (Option, List, Array, etc.)
                            let mut len_val = 0;
                            let mut elem_size = 8;
                            let mut elems_are_managed = false;
                            if let ExpressionKind::Array(elements, _) = &args[0].node {
                                len_val = elements.len() as i64;
                                if !elements.is_empty() {
                                    if let Some(ty) = ctx.type_checker.get_type(elements[0].id) {
                                        elem_size = compute_elem_size_from_type(&ty.kind);
                                        elems_are_managed = ctx.is_perceus_managed(&ty.kind);
                                    }
                                }
                            }

                            let len_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Int, *span),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I64(len_val),
                                ),
                            }));

                            let size_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Int, *span),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I64(elem_size),
                                ),
                            }));

                            // Use the managed-array variant when elements are
                            // heap-allocated so the list IncRefs them before the
                            // source array's element-drop loop releases its refs.
                            let rt_fn_name = if elems_are_managed {
                                rt::LIST_NEW_FROM_MANAGED_ARRAY
                            } else {
                                rt::LIST_NEW_FROM_RAW
                            };
                            let func_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Identifier, *span),
                                literal: crate::ast::literal::Literal::Identifier(
                                    rt_fn_name.to_string(),
                                ),
                            }));

                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Call {
                                    func: func_op,
                                    args: vec![array_op, len_op, size_op],
                                    destination: destination.clone(),
                                    target: Some(target_bb),
                                },
                                *span,
                            ));

                            // The temp array was consumed by miri_rt_list_new_from_raw
                            // (data copied). Emit StorageDead so Perceus inserts DecRef.
                            ctx.set_current_block(target_bb);
                            if let Some(arr_place) = temp_array_local {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: StatementKind::StorageDead(arr_place),
                                    span: *span,
                                });
                            }

                            // Need a new target block since we added statements to the original
                            let final_bb = ctx.new_basic_block();
                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Goto { target: final_bb },
                                *span,
                            ));
                            ctx.set_current_block(final_bb);
                            return Ok(result_op);
                        } else {
                            // Assuming element size is 8 for simplicity, or 0 if it doesn't matter yet
                            let size_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Int, *span),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I64(8),
                                ),
                            }));
                            let func_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Identifier, *span),
                                literal: crate::ast::literal::Literal::Identifier(
                                    rt::LIST_NEW.to_string(),
                                ),
                            }));
                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Call {
                                    func: func_op,
                                    args: vec![size_op],
                                    destination: destination.clone(),
                                    target: Some(target_bb),
                                },
                                *span,
                            ));
                        }

                        ctx.set_current_block(target_bb);
                        return Ok(result_op);
                    }

                    if matches!(
                        BuiltinCollectionKind::from_name(type_name),
                        Some(BuiltinCollectionKind::Map | BuiltinCollectionKind::Set)
                    ) {
                        let return_ty =
                            if let Some(call_ty) = ctx.type_checker.get_type(call_expr_id) {
                                call_ty.clone()
                            } else if BuiltinCollectionKind::from_name(type_name)
                                == Some(BuiltinCollectionKind::Map)
                            {
                                crate::ast::factory::type_map(
                                    crate::ast::factory::type_void(),
                                    crate::ast::factory::type_void(),
                                )
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

                        let aggregate_kind = if BuiltinCollectionKind::from_name(type_name)
                            == Some(BuiltinCollectionKind::Map)
                        {
                            AggregateKind::Map
                        } else {
                            AggregateKind::Set
                        };

                        ctx.push_statement(crate::mir::Statement {
                            kind: StatementKind::Assign(
                                destination,
                                Rvalue::Aggregate(aggregate_kind, vec![]),
                            ),
                            span: *span,
                        });

                        return Ok(result_op);
                    }

                    // This is a class constructor - emit Aggregate instead of Call
                    return lower_class_constructor(ctx, span, type_name, def, args, dest);
                }
            }
        }
    }

    let func_op = lower_expression(ctx, func, None)?;

    // If this call site has generic type substitutions, mangle the function name.
    let func_op = if let ExpressionKind::Identifier(func_name, _) = &func.node {
        if let Some(generic_args) = ctx.type_checker.call_generic_mappings.get(&call_expr_id) {
            let mangled = mangle_generic_name(func_name, generic_args);
            Operand::Constant(Box::new(crate::mir::Constant {
                span: func.span,
                ty: crate::ast::types::Type::new(TypeKind::Identifier, func.span),
                literal: crate::ast::literal::Literal::Identifier(mangled),
            }))
        } else {
            func_op
        }
    } else {
        func_op
    };

    // Try to get function type to check parameters.
    // For generic calls (those with a mangled name), skip parameter coercion: the
    // arguments already have the correct concrete types and coercing them against the
    // unsubstituted generic parameter type (TypeKind::Custom("T", None)) would corrupt
    // the call signature.
    let is_generic_call = ctx
        .type_checker
        .call_generic_mappings
        .contains_key(&call_expr_id);
    let func_ty = ctx.type_checker.get_type(func.id);
    let param_types = if is_generic_call {
        None
    } else if let Some(ty) = func_ty {
        if let TypeKind::Function(func) = &ty.kind {
            Some(func.params.clone())
        } else {
            None
        }
    } else {
        None
    };

    let arg_watermark = ctx.body.local_decls.len();
    let mut arg_ops = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        let op = lower_expression(ctx, arg, None)?;

        let op = if let Some(params) = &param_types {
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
                    Operand::Copy(Place::new(temp))
                } else {
                    op
                }
            } else {
                op
            }
        } else {
            op
        };

        // Ensure managed arguments are passed as Copy so that Perceus inserts
        // an IncRef at the call site. The callee owns the reference and releases
        // it via StorageDead on the parameter in finalize_body. Without this, a
        // Move argument is not IncRef'd, the callee's DecRef brings RC to 0, and
        // the caller's reference becomes dangling.
        let op = match op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };

        arg_ops.push(op);
    }

    // Fill in default values for missing arguments
    if let Some(params) = &param_types {
        for param in params.iter().skip(args.len()) {
            if let Some(default_expr) = &param.default_value {
                // Lower the default value expression
                let default_op = lower_expression(ctx, default_expr, None)?;
                arg_ops.push(default_op);
            }
            // If no default and missing, type checker should have caught this error
        }
    }

    // Implicit Allocator Injection at Call Site.
    // Skip for runtime functions (miri_ prefix) and for indirect calls through
    // function-pointer variables (lambdas) — those bodies have no allocator param.
    let is_runtime_fn = if let ExpressionKind::Identifier(name, _) = &func.node {
        name.starts_with("miri_")
    } else {
        false
    };

    // An indirect call is one where the callee operand resolved to a local
    // variable (function pointer) rather than a named constant identifier.
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

    // Determine return type (void for now, or from type checker)
    let mut return_ty = Type::new(TypeKind::Void, *span);

    // Attempt to resolve return type from TypeChecker using the Call expression ID
    if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
        return_ty = ty.clone();
    }

    // Use provided dest or create temp
    let (destination, op) = if let Some(d) = dest {
        // We might want to verify types match, but we trust caller for DPS optimization
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let target_bb = ctx.new_basic_block();

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

    Ok(op)
}
