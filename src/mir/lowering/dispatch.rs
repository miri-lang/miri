// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method dispatch lowering — name mangling, inheritance resolution, `lower_call`.

use crate::ast::expression::Expression;
use crate::ast::types::{STRING_TYPE_NAME, TUPLE_TYPE_NAME};
use crate::ast::{BuiltinCollectionKind, ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    GpuLaunchArgs, Local, MathIntrinsic, Operand, Place, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::{cow_fn, rt};
use crate::type_checker::context::{
    class_needs_vtable, vtable_slot_index, MethodInfo, TypeDefinition,
};

use super::constructors::{lower_class_constructor, lower_struct_constructor, COLLECTION_CTORS};
use super::helpers::{coerce_rvalue, gpu_math_return_type};
use super::{lower_expression, LoweringContext};

/// Context for lowering a collection intrinsic method (push/get/index).
struct CollectionIntrinsicCall<'a> {
    span: &'a Span,
    call_expr_id: usize,
    obj: &'a Expression,
    obj_ty: &'a Type,
    method_name: &'a str,
    args: &'a [Expression],
}

/// Aggregated result of analyzing GPU function arguments for a kernel launch.
struct ThreadedGpuFnArgs {
    kernel_op: Operand,
    kernel_name: String,
    buffer_args: Vec<Operand>,
    arg_handles: Vec<Option<crate::mir::body::DeviceHandleId>>,
    arg_read_only: Vec<bool>,
    arg_int_narrow: Vec<bool>,
    scalar_args: Vec<Operand>,
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
        TypeKind::String => STRING_TYPE_NAME.to_string(),
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

    // Handle enum-typed receiver: look up the method directly on the enum definition.
    if let Some(TypeDefinition::Enum(enum_def)) = type_defs.get(class_name) {
        if let Some(method_info) = enum_def.methods.get(method_name) {
            return Some((class_name.to_string(), method_info.clone()));
        }
        return None;
    }

    // Is the original caller itself abstract?  If it is, the "concrete caller" rule
    // does not apply — we use the normal defining-class name.
    let caller_is_abstract = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(cd)) if cd.is_abstract
    );
    resolve_via_class_chain(type_defs, class_name, method_name, caller_is_abstract)
}

/// Walk the class's inheritance chain (and each class's traits) for `method_name`.
fn resolve_via_class_chain(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
    caller_is_abstract: bool,
) -> Option<(String, MethodInfo)> {
    let mut current = class_name.to_string();
    loop {
        let (base, traits) = match type_defs.get(&current) {
            Some(TypeDefinition::Class(class_def)) => {
                if let Some(method_info) = class_def.methods.get(method_name) {
                    // A concrete caller finding the method in an abstract ancestor
                    // uses the caller's name so dispatch lands on the per-concrete copy.
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
        if let Some(found) = resolve_via_class_traits(
            type_defs,
            &traits,
            method_name,
            class_name,
            caller_is_abstract,
        ) {
            return Some(found);
        }
        match base {
            Some(b) => current = b,
            None => return None,
        }
    }
}

/// Scan a class's directly-implemented traits for a default `method_name`. The
/// concrete-caller / abstract-definer rule mirrors the class-chain case.
fn resolve_via_class_traits(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    traits: &[String],
    method_name: &str,
    class_name: &str,
    caller_is_abstract: bool,
) -> Option<(String, MethodInfo)> {
    for trait_name in traits {
        if let Some((defining_trait, info)) =
            resolve_trait_default_method(type_defs, trait_name, method_name)
        {
            let defining = if caller_is_abstract {
                defining_trait
            } else {
                class_name.to_string()
            };
            return Some((defining, info));
        }
    }
    None
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
    let ExpressionKind::Identifier(alias_name, _) = &obj_expr.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(func_name, _) = &method_expr.node else {
        return Ok(None);
    };
    let Some(module_path) = ctx
        .type_checker
        .module_aliases
        .get(alias_name.as_str())
        .cloned()
    else {
        return Ok(None);
    };

    if module_path == "system.math" {
        if let Some(intrinsic) = MathIntrinsic::from_name(func_name.as_str()) {
            return lower_math_intrinsic_call(ctx, span, call_expr_id, intrinsic, args, dest)
                .map(Some);
        }
    }
    lower_aliased_function_call(ctx, span, call_expr_id, func_name, args, dest)
}

/// Lower a `system.math` intrinsic call to a `MathIntrinsic` rvalue.
fn lower_math_intrinsic_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    intrinsic: MathIntrinsic,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let arg_ops = lower_plain_args(ctx, args)?;
    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let return_ty = gpu_math_return_type(ctx, args, return_ty, *span);
    let (target, ret_op) = call_destination(ctx, return_ty, dest, *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(target, Rvalue::MathIntrinsic(intrinsic, arg_ops)),
        span: *span,
    });
    Ok(ret_op)
}

/// Lower a direct call to a function reached through a module alias.
fn lower_aliased_function_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func_name: &str,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let mangled = match ctx.type_checker.call_generic_mappings.get(&call_expr_id) {
        Some(generic_args) => mangle_generic_name(func_name, generic_args),
        None => func_name.to_string(),
    };
    let func_op = runtime_fn_operand(&mangled, *span);

    let mut arg_ops = lower_plain_args(ctx, args)?;
    push_allocator_arg(ctx, &mut arg_ops);

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, result_op) = call_destination(ctx, return_ty, dest, *span);

    emit_call_terminator(ctx, func_op, arg_ops, Vec::new(), destination, *span);
    Ok(Some(result_op))
}

/// Lower call arguments with plain expression lowering (no coercion).
fn lower_plain_args(
    ctx: &mut LoweringContext,
    args: &[Expression],
) -> Result<Vec<Operand>, LoweringError> {
    let mut arg_ops = Vec::with_capacity(args.len());
    for arg in args {
        arg_ops.push(lower_expression(ctx, arg, None)?);
    }
    Ok(arg_ops)
}

/// Append the implicit `allocator` argument unless it is already present.
fn push_allocator_arg(ctx: &LoweringContext, arg_ops: &mut Vec<Operand>) {
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        let already_has_alloc = arg_ops
            .iter()
            .any(|op| matches!(op, Operand::Copy(p) | Operand::Move(p) if p.local == alloc_local));
        if !already_has_alloc {
            arg_ops.push(Operand::Copy(Place::new(alloc_local)));
        }
    }
}

/// Thread GPU fn call arguments into the GpuLaunch terminator.
/// Resolve kernel operand and name from a gpu fn callee expression.
fn resolve_kernel_operand(
    ctx: &LoweringContext,
    callee: &Expression,
    span: Span,
) -> Result<(Operand, String), LoweringError> {
    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu fn must be called by name".to_string(),
            span,
        ));
    };

    let kernel_name = match ctx.type_checker.call_generic_mappings.get(&callee.id) {
        Some(generic_args) => mangle_generic_name(func_name, generic_args),
        None => func_name.clone(),
    };

    let kernel_op = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(kernel_name.clone()),
    }));

    Ok((kernel_op, kernel_name))
}

/// Process buffer arguments and metadata for a GPU function call.
#[allow(clippy::type_complexity)]
fn process_gpu_buffer_args(
    ctx: &mut LoweringContext,
    func_name: &str,
    call_args: &[Expression],
    span: Span,
) -> Result<
    (
        Vec<Operand>,
        Vec<Option<crate::mir::body::DeviceHandleId>>,
        Vec<bool>,
        Vec<bool>,
    ),
    LoweringError,
> {
    let out_params = ctx
        .type_checker
        .function_out_params
        .get(func_name)
        .cloned()
        .unwrap_or_default();

    let mut buffer_args = Vec::new();
    let mut arg_handles = Vec::new();
    let mut arg_read_only = Vec::new();
    let mut arg_int_narrow = Vec::new();

    for (arg_idx, arg) in call_args.iter().enumerate() {
        let arg_ty = ctx
            .type_checker
            .get_type(arg.id)
            .cloned()
            .unwrap_or_else(|| Type::new(TypeKind::Void, span));
        let arg_op = lower_expression(ctx, arg, None)?;

        if is_gpu_buffer_type(&arg_ty.kind) {
            if let Operand::Copy(place) | Operand::Move(place) = &arg_op {
                let local_decl = &ctx.body.local_decls[place.local.0];

                // Enforce that buffer arguments to gpu fn are gpu-resident.
                // Bare gpu fn calls are deferred handles (no dispatch yet), so this only
                // applies when the kernel is actually launched.
                if !matches!(
                    local_decl.residency,
                    crate::mir::body::BindingResidency::Gpu
                ) {
                    let buffer_name = local_decl.name.as_deref().unwrap_or("argument");
                    return Err(LoweringError::custom(
                        format!("cannot pass host-resident array '{}' to gpu function", buffer_name),
                        span,
                        Some(format!(
                            "mark the binding as gpu-resident: 'gpu let {} = ...' or 'gpu var {} = ...'",
                            buffer_name, buffer_name
                        )),
                    ));
                }

                let handle = local_decl.device_handle;
                arg_handles.push(handle);
                buffer_args.push(arg_op.clone());

                // GPU buffers are read-only unless the parameter is marked as `out`.
                // Out parameters have write access (read_write in WGSL).
                arg_read_only.push(!out_params.get(arg_idx).copied().unwrap_or(false));
                arg_int_narrow.push(needs_int_narrowing(&arg_ty));
            } else {
                return Err(LoweringError::unsupported_expression(
                    "gpu fn buffer args must be places".to_string(),
                    span,
                ));
            }
        }
    }

    Ok((buffer_args, arg_handles, arg_read_only, arg_int_narrow))
}

/// Analyze GPU function arguments for a kernel launch, producing operands and metadata.
fn thread_gpu_fn_args(
    ctx: &mut LoweringContext,
    callee: &Expression,
    call_args: &[Expression],
    span: Span,
) -> Result<ThreadedGpuFnArgs, LoweringError> {
    let (kernel_op, kernel_name) = resolve_kernel_operand(ctx, callee, span)?;

    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu fn must be called by name".to_string(),
            span,
        ));
    };

    let (buffer_args, arg_handles, arg_read_only, arg_int_narrow) =
        process_gpu_buffer_args(ctx, func_name, call_args, span)?;

    Ok(ThreadedGpuFnArgs {
        kernel_op,
        kernel_name,
        buffer_args,
        arg_handles,
        arg_read_only,
        arg_int_narrow,
        scalar_args: Vec::new(),
    })
}

fn is_gpu_buffer_type(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) | TypeKind::List(_) => true,
        TypeKind::Custom(n, _) => is_collection_type(n),
        _ => false,
    }
}

fn is_collection_type(name: &str) -> bool {
    matches!(
        BuiltinCollectionKind::from_name(name),
        Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
    )
}

fn needs_int_narrowing(ty: &Type) -> bool {
    use super::forall_gpu::needs_int_narrowing as check_narrowing;
    check_narrowing(ty)
}

/// Try to extract Dim3(x, y, z) as [x, y, z] from a compile-time literal.
/// Returns None if the expression is not a Dim3 literal or is not compile-time constant.
fn try_extract_dim3_literal(expr: &Expression) -> Option<[u32; 3]> {
    use crate::ast::expression::ExpressionKind;

    match &expr.node {
        ExpressionKind::Call(func, args) => {
            if let ExpressionKind::Identifier(name, _) = &func.node {
                if name == "Dim3" && args.len() == 3 {
                    let x = extract_u32_literal(&args[0])?;
                    let y = extract_u32_literal(&args[1])?;
                    let z = extract_u32_literal(&args[2])?;
                    return Some([x, y, z]);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_u32_literal(expr: &Expression) -> Option<u32> {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::literal::Literal;

    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(int_lit)) => {
            use crate::ast::literal::IntegerLiteral;
            match int_lit {
                IntegerLiteral::I8(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::I16(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::I32(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::I64(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::U8(v) => Some(*v as u32),
                IntegerLiteral::U16(v) => Some(*v as u32),
                IntegerLiteral::U32(v) => Some(*v),
                IntegerLiteral::U64(v) if *v <= u32::MAX as u64 => Some(*v as u32),
                _ => None,
            }
        }
        _ => None,
    }
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
    let ExpressionKind::Identifier(name, _) = &prop.node else {
        return Ok(None);
    };
    if name != "launch" || !receiver_is_kernel(ctx, obj) {
        return Ok(None);
    }

    if args.len() != 2 {
        return Err(LoweringError::invalid_gpu_launch_args(2, args.len(), *span));
    }
    let grid_op = lower_expression(ctx, &args[0], None)?;
    let block_op = lower_expression(ctx, &args[1], None)?;

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, op) = call_destination(ctx, return_ty, dest, *span);
    let target_bb = ctx.new_basic_block();

    let (
        kernel_op,
        kernel_name,
        call_args,
        arg_handles,
        arg_read_only,
        arg_int_narrow,
        scalar_args,
    ) = if let ExpressionKind::Call(callee, call_args) = &obj.node {
        let gpu_args = thread_gpu_fn_args(ctx, callee, call_args, *span)?;
        (
            gpu_args.kernel_op,
            Some(gpu_args.kernel_name),
            gpu_args.buffer_args,
            gpu_args.arg_handles,
            gpu_args.arg_read_only,
            gpu_args.arg_int_narrow,
            gpu_args.scalar_args,
        )
    } else {
        (
            lower_expression(ctx, obj, None)?,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    };

    if let Some(ref kernel_name) = kernel_name {
        let workgroup_size = try_extract_dim3_literal(&args[1]).ok_or_else(|| {
            LoweringError::custom(
                "gpu fn launch block size must be a compile-time literal Dim3".to_string(),
                *span,
                Some("use a compile-time literal, e.g., block: Dim3(16, 16, 1)".to_string()),
            )
        })?;

        // Validate that all dimensions are > 0.
        if workgroup_size.contains(&0) {
            return Err(LoweringError::custom(
                "gpu fn launch block dimensions must all be >0".to_string(),
                *span,
                Some("each dimension must be at least 1".to_string()),
            ));
        }

        ctx.body
            .kernel_workgroups
            .push((kernel_name.clone(), workgroup_size));
    }

    let launch_args = GpuLaunchArgs::new(call_args, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| LoweringError::custom(e.to_string(), *span, None))?;

    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: grid_op,
            block: block_op,
            launch_args,
            scalar_args,
            uniform_bound_x: None,
            uniform_bound_y: None,
            uniform_bound_z: None,
            destination,
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    Ok(Some(op))
}

/// True when `obj` has the GPU `Kernel` type.
fn receiver_is_kernel(ctx: &LoweringContext, obj: &Expression) -> bool {
    ctx.type_checker
        .get_type(obj.id)
        .map(|ty| matches!(&ty.kind, TypeKind::Custom(n, _) if n == "Kernel"))
        .unwrap_or(false)
}

/// Emit a virtual method call through a vtable slot.
#[allow(clippy::too_many_arguments)]
fn emit_virtual_method_call(
    ctx: &mut LoweringContext,
    vtable_slot: usize,
    self_op: Operand,
    user_args: &[Expression],
    method_info: &MethodInfo,
    destination: &Place,
    op: &Operand,
    obj_temp_local: Option<Local>,
    obj_watermark: usize,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let mut call_args = vec![self_op];
    let arg_watermark = ctx.body.local_decls.len();
    for arg in user_args {
        call_args.push(lower_expression(ctx, arg, None)?);
    }
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }

    let out_args = build_method_out_args(method_info, user_args.len(), call_args.len());
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::VirtualCall {
            vtable_slot,
            args: call_args.clone(),
            out_args,
            destination: destination.clone(),
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(local) = obj_temp_local {
        ctx.emit_temp_drop(local, obj_watermark, span);
    }
    emit_closure_arg_drops(ctx, &call_args[1..], arg_watermark, span);
    Ok(Some(op.clone()))
}

/// Emit a static method call (direct function call).
#[allow(clippy::too_many_arguments)]
fn emit_static_method_call(
    ctx: &mut LoweringContext,
    defining_class: &str,
    method_name: &str,
    self_op: Operand,
    user_args: &[Expression],
    method_info: &MethodInfo,
    destination: &Place,
    op: &Operand,
    obj_temp_local: Option<Local>,
    obj_watermark: usize,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let mut mangled_name = String::with_capacity(defining_class.len() + 1 + method_name.len());
    mangled_name.push_str(defining_class);
    mangled_name.push('_');
    mangled_name.push_str(method_name);
    let mut call_args = vec![self_op];
    let arg_watermark = ctx.body.local_decls.len();
    for arg in user_args {
        call_args.push(lower_expression(ctx, arg, None)?);
    }
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }

    let func_op = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(mangled_name),
    }));

    let out_args = build_method_out_args(method_info, user_args.len(), call_args.len());
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args.clone(),
            out_args,
            destination: destination.clone(),
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(local) = obj_temp_local {
        ctx.emit_temp_drop(local, obj_watermark, span);
    }
    emit_closure_arg_drops(ctx, &call_args[1..], arg_watermark, span);
    Ok(Some(op.clone()))
}

/// Resolve the receiver type override for inherited methods in abstract classes.
fn resolve_receiver_override(
    ctx: &LoweringContext,
    raw_obj_ty: &Type,
    obj: &Expression,
) -> Option<Type> {
    if let TypeKind::Custom(name, _) = &raw_obj_ty.kind {
        let needs_override = matches!(
            ctx.type_checker.global_type_definitions.get(name.as_str()),
            Some(TypeDefinition::Class(cd)) if cd.is_abstract
        ) || matches!(
            ctx.type_checker.global_type_definitions.get(name.as_str()),
            Some(TypeDefinition::Trait(_))
        );
        if needs_override {
            if let ExpressionKind::Identifier(var_name, _) = &obj.node {
                if let Some(&local) = ctx.variable_map.get(var_name.as_str()) {
                    return Some(ctx.body.local_decls[local.0].ty.clone());
                }
            }
        }
    }
    None
}

/// Extract the class name from a type, handling builtins and custom types.
fn extract_class_name(obj_ty: &Type) -> Option<String> {
    match &obj_ty.kind {
        TypeKind::String => Some(STRING_TYPE_NAME.to_string()),
        TypeKind::Tuple(_) => Some(TUPLE_TYPE_NAME.to_string()),
        TypeKind::Custom(name, _) => Some(name.clone()),
        k => k.as_builtin_collection().map(|b| b.name().to_string()),
    }
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
    let Some((obj_ty, class_name, method_name)) = resolve_method_receiver(ctx, obj, method_expr)
    else {
        return Ok(None);
    };

    // 1. Try specialized collection optimizations (direct index-reads / runtime
    // intrinsic calls) to avoid monomorphization conflicts and aid RC analysis.
    let call = CollectionIntrinsicCall {
        span,
        call_expr_id,
        obj,
        obj_ty: &obj_ty,
        method_name: &method_name,
        args,
    };
    if let Some(op) = try_lower_collection_intrinsic(ctx, call, dest.clone())? {
        return Ok(Some(op));
    }

    // 2. Regular inherited method resolution and dispatch.
    let Some((defining_class, method_info)) = resolve_inherited_method(
        &ctx.type_checker.global_type_definitions,
        &class_name,
        &method_name,
    ) else {
        return Ok(None);
    };

    emit_resolved_method_call(
        ctx,
        ResolvedMethod {
            span,
            obj,
            obj_ty: &obj_ty,
            class_name: &class_name,
            method_name: &method_name,
            defining_class: &defining_class,
            method_info: &method_info,
            args,
        },
        dest,
    )
}

/// Resolve a method call's receiver type (applying abstract/trait overrides),
/// class name, and method name. Returns owned values to avoid borrowing `ctx`.
fn resolve_method_receiver(
    ctx: &LoweringContext,
    obj: &Expression,
    method_expr: &Expression,
) -> Option<(Type, String, String)> {
    let raw_obj_ty = ctx.type_checker.get_type(obj.id)?.clone();
    let obj_ty = resolve_receiver_override(ctx, &raw_obj_ty, obj).unwrap_or(raw_obj_ty);
    let class_name = extract_class_name(&obj_ty)?;
    let method_name = match &method_expr.node {
        ExpressionKind::Identifier(name, _) => name.clone(),
        _ => return None,
    };
    Some((obj_ty, class_name, method_name))
}

/// A method call whose receiver type and target method have been resolved.
struct ResolvedMethod<'a> {
    span: &'a Span,
    obj: &'a Expression,
    obj_ty: &'a Type,
    class_name: &'a str,
    method_name: &'a str,
    defining_class: &'a str,
    method_info: &'a MethodInfo,
    args: &'a [Expression],
}

/// Emit a resolved user-method call via virtual (vtable) or static dispatch.
fn emit_resolved_method_call(
    ctx: &mut LoweringContext,
    m: ResolvedMethod,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let return_ty = m.method_info.return_type.clone();
    let obj_watermark = ctx.body.local_decls.len();
    let (self_op, obj_temp_local) =
        prepare_method_self(ctx, m.obj, m.obj_ty, m.method_name, *m.span)?;
    let (destination, op) = call_destination(ctx, return_ty, dest, *m.span);

    if should_use_virtual_dispatch(ctx, m.obj, m.class_name) {
        if let Some(slot) = vtable_slot_index(
            m.class_name,
            m.method_name,
            &ctx.type_checker.global_type_definitions,
        ) {
            return emit_virtual_method_call(
                ctx,
                slot,
                self_op,
                m.args,
                m.method_info,
                &destination,
                &op,
                obj_temp_local,
                obj_watermark,
                *m.span,
            );
        }
    }
    emit_static_method_call(
        ctx,
        m.defining_class,
        m.method_name,
        self_op,
        m.args,
        m.method_info,
        &destination,
        &op,
        obj_temp_local,
        obj_watermark,
        *m.span,
    )
}

/// Lower the receiver, apply a CoW check for mutating collection methods, and
/// return the self operand plus the receiver temp local (for Perceus drops).
fn prepare_method_self(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    method_name: &str,
    span: Span,
) -> Result<(Operand, Option<Local>), LoweringError> {
    let self_op = lower_method_receiver(ctx, obj)?;
    let self_op = match obj_ty
        .kind
        .as_builtin_collection()
        .filter(|k| k.mutates_method(method_name))
        .and_then(cow_fn)
    {
        Some(cow) => emit_cow_check(ctx, self_op, obj_ty, cow, span),
        None => self_op,
    };
    let obj_temp_local = if let Operand::Copy(ref p) = self_op {
        Some(p.local)
    } else {
        None
    };
    Ok((self_op, obj_temp_local))
}

/// Lower a method receiver, resolving `super` to the `self` binding.
fn lower_method_receiver(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Operand, LoweringError> {
    if matches!(&obj.node, ExpressionKind::Super) {
        if let Some(&self_local) = ctx.variable_map.get("self") {
            return Ok(Operand::Copy(Place::new(self_local)));
        }
    }
    lower_expression(ctx, obj, None)
}

/// True when the receiver's static type requires vtable (virtual) dispatch:
/// an abstract class with a vtable, or a trait-typed receiver. `super` calls
/// always dispatch statically.
fn should_use_virtual_dispatch(ctx: &LoweringContext, obj: &Expression, class_name: &str) -> bool {
    if matches!(&obj.node, ExpressionKind::Super) {
        return false;
    }
    let defs = &ctx.type_checker.global_type_definitions;
    let abstract_with_vtable = class_needs_vtable(class_name, defs)
        && matches!(defs.get(class_name), Some(TypeDefinition::Class(cd)) if cd.is_abstract);
    let is_trait = matches!(defs.get(class_name), Some(TypeDefinition::Trait(_)));
    abstract_with_vtable || is_trait
}

/// Build the `out_args` flag list for a method call's argument vector.
///
/// Method dispatch builds args as `[self, ...user_args, alloc?]`. The receiver
/// and the implicit allocator are never `out`; positional user args map 1:1
/// to `method_info.is_param_out(i)`. Length always matches `total_call_args`
/// so downstream code can rely on `out_args.len() == args.len()`.
fn build_method_out_args(
    method_info: &MethodInfo,
    user_arg_count: usize,
    total_call_args: usize,
) -> Vec<bool> {
    let mut flags = vec![false; total_call_args];
    for i in 0..user_arg_count {
        if method_info.is_param_out(i) {
            flags[1 + i] = true;
        }
    }
    flags
}

/// Release temporary closure arguments after a method call.
///
/// Closures passed as arguments are borrowed by the callee (called but not stored).
/// The caller is responsible for freeing them after the call completes.
/// Only locals created above `watermark` and with a `Function` type are released.
fn emit_closure_arg_drops(
    ctx: &mut LoweringContext,
    args: &[Operand],
    watermark: usize,
    span: Span,
) {
    for op in args {
        if let Operand::Copy(p) | Operand::Move(p) = op {
            let local = p.local;
            if local.0 >= watermark {
                let is_closure =
                    matches!(ctx.body.local_decls[local.0].ty.kind, TypeKind::Function(_));
                if is_closure {
                    ctx.emit_temp_drop(local, watermark, span);
                }
            }
        }
    }
}

/// Emit a Copy-on-Write check before a mutation operation on a collection local.
///
/// If the receiver is a simple local variable (`Move` with no projection), emits a call to
/// `cow_fn_name` that returns either the same pointer (RC ≤ 1 → no copy) or a fresh exclusive
/// clone (RC > 1 → clone + decrement old RC). The result is stored back into the receiver local
/// so the subsequent mutation operates on an exclusively-owned collection.
///
/// `Assign` (not `Reassign`) is used for the write-back so Perceus does not DecRef the old
/// value; `Move` is used for the cow_result so Perceus does not IncRef it. No `StorageDead` is
/// emitted for the cow_result temp — its ownership is transferred to self_local.
fn emit_cow_check(
    ctx: &mut LoweringContext,
    obj_op: Operand,
    obj_ty: &Type,
    cow_fn_name: &str,
    span: Span,
) -> Operand {
    let self_local = match &obj_op {
        Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => return obj_op,
    };
    let cow_result = ctx.push_temp(obj_ty.clone(), span);
    let cow_fn = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(cow_fn_name.to_string()),
    }));
    let cow_target = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: cow_fn,
            args: vec![Operand::Move(Place::new(self_local))],
            out_args: Vec::new(),
            destination: Place::new(cow_result),
            target: Some(cow_target),
        },
        span,
    ));
    ctx.set_current_block(cow_target);
    // Write the (possibly-new) pointer back into the receiver local.
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(self_local),
            Rvalue::Use(Operand::Move(Place::new(cow_result))),
        ),
        span,
    });
    Operand::Move(Place::new(self_local))
}

/// Lower element_at/get on List, Array, or Tuple.
fn lower_collection_element_access(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    obj_ty: &Type,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op_src = operand_src_local(&obj_op);
    let index_op = lower_expression(ctx, &args[0], None)?;

    let obj_local = store_operand_temp(ctx, move_to_copy(obj_op), obj_ty.clone(), *span);
    let index_local = materialize_index_local(ctx, index_op, args[0].span);

    let mut indexed_place = Place::new(obj_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(index_local));

    let elem_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Int, *span));
    let (destination, op) = call_destination(ctx, elem_ty, dest, *span);

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(destination, Rvalue::Use(Operand::Copy(indexed_place))),
        span: *span,
    });

    ctx.emit_temp_drop(obj_local, obj_watermark, *span);
    if let Some(src_local) = obj_op_src {
        ctx.emit_temp_drop(src_local, obj_watermark, *span);
    }
    Ok(op)
}

/// Materialize an index operand into a bare local, spilling to a temp when it is
/// a projected place or a constant.
fn materialize_index_local(ctx: &mut LoweringContext, index_op: Operand, span: Span) -> Local {
    match index_op {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => store_operand_temp(ctx, index_op, Type::new(TypeKind::Int, span), span),
    }
}

/// Resolve a call's destination place + return operand, using `dest` when given.
fn call_destination(
    ctx: &mut LoweringContext,
    return_ty: Type,
    dest: Option<Place>,
    span: Span,
) -> (Place, Operand) {
    match dest {
        Some(d) => (d.clone(), Operand::Copy(d)),
        None => {
            let temp = ctx.push_temp(return_ty, span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        }
    }
}

/// Lower list.push(item) to miri_rt_list_push.
/// The source local backing a place operand, if any.
fn operand_src_local(op: &Operand) -> Option<Local> {
    match op {
        Operand::Copy(p) | Operand::Move(p) => Some(p.local),
        _ => None,
    }
}

/// Convert a `Move` operand into a `Copy` of the same place.
fn move_to_copy(op: Operand) -> Operand {
    match op {
        Operand::Move(p) => Operand::Copy(p),
        other => other,
    }
}

/// Store an operand into a fresh temp of `ty`, returning the temp local.
fn store_operand_temp(ctx: &mut LoweringContext, op: Operand, ty: Type, span: Span) -> Local {
    let local = ctx.push_temp(ty, span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(local), Rvalue::Use(op)),
        span,
    });
    local
}

/// Build a runtime-function callee constant for `name`.
fn runtime_fn_operand(name: &str, span: Span) -> Operand {
    Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(name.to_string()),
    }))
}

fn lower_list_push(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    item_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let item_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = emit_cow_check(ctx, obj_op, obj_ty, rt::LIST_COW, *span);
    let item_op = lower_expression(ctx, item_arg, None)?;

    let item_op_src = operand_src_local(&item_op);
    let item_copy = move_to_copy(item_op);
    let item_ty = item_copy.ty(&ctx.body).clone();
    let item_local = store_operand_temp(ctx, item_copy, item_ty, item_arg.span);
    let func_op = runtime_fn_operand(rt::LIST_PUSH, *span);
    let target_bb = ctx.new_basic_block();
    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, Operand::Copy(Place::new(item_local))],
            out_args: Vec::new(),
            destination: Place::new(dummy_dest),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(src) = item_op_src {
        ctx.emit_temp_drop(src, item_watermark, item_arg.span);
    }
    Ok(Some(Operand::Copy(Place::new(dummy_dest))))
}

/// Lower list.insert(index, item) to miri_rt_list_insert.
fn lower_list_insert(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    index_arg: &Expression,
    item_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let item_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = emit_cow_check(ctx, obj_op, obj_ty, rt::LIST_COW, *span);
    let index_op = lower_expression(ctx, index_arg, None)?;
    let item_op = lower_expression(ctx, item_arg, None)?;

    let item_op_src = operand_src_local(&item_op);
    let item_copy = move_to_copy(item_op);
    let item_ty = item_copy.ty(&ctx.body).clone();
    let item_local = store_operand_temp(ctx, item_copy, item_ty, item_arg.span);
    let func_op = runtime_fn_operand(rt::LIST_INSERT, *span);
    let target_bb = ctx.new_basic_block();
    let result_temp = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, index_op, Operand::Copy(Place::new(item_local))],
            out_args: Vec::new(),
            destination: Place::new(result_temp),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(src) = item_op_src {
        ctx.emit_temp_drop(src, item_watermark, item_arg.span);
    }
    Ok(Some(Operand::Copy(Place::new(result_temp))))
}

/// Lower list/array.set(index, value) to a direct indexed assignment.
fn lower_collection_set(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    index_arg: &Expression,
    item_arg: &Expression,
    builtin: Option<BuiltinCollectionKind>,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let obj_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = if builtin == Some(BuiltinCollectionKind::List) {
        emit_cow_check(ctx, obj_op, obj_ty, rt::LIST_COW, *span)
    } else {
        obj_op
    };
    let obj_op_src = operand_src_local(&obj_op);
    let index_op = lower_expression(ctx, index_arg, None)?;
    let item_op = lower_expression(ctx, item_arg, None)?;
    let item_op_src = operand_src_local(&item_op);

    let obj_local = store_operand_temp(ctx, move_to_copy(obj_op), obj_ty.clone(), *span);
    let index_local = materialize_index_local(ctx, index_op, index_arg.span);
    let mut indexed_place = Place::new(obj_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(index_local));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(indexed_place, Rvalue::Use(move_to_copy(item_op))),
        span: *span,
    });

    ctx.emit_temp_drop(obj_local, obj_watermark, *span);
    if let Some(src_local) = obj_op_src {
        ctx.emit_temp_drop(src_local, obj_watermark, *span);
    }
    if let Some(item_src) = item_op_src {
        ctx.emit_temp_drop(item_src, obj_watermark, *span);
    }
    Ok(Some(void_none_operand(*span)))
}

/// A `void`-typed `None` constant operand (a unit return value).
fn void_none_operand(span: Span) -> Operand {
    Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Void, span),
        literal: crate::ast::literal::Literal::None,
    }))
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
        method_name,
        args,
    } = call;
    let builtin = obj_ty.kind.as_builtin_collection();
    let is_indexable_collection = matches!(
        builtin,
        Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
    ) || obj_ty.kind.is_tuple();
    if args.len() == 1 && matches!(method_name, "element_at" | "get") && is_indexable_collection {
        return lower_collection_element_access(ctx, span, call_expr_id, obj, obj_ty, args, dest)
            .map(Some);
    }

    if args.len() == 1 && method_name == "push" && builtin == Some(BuiltinCollectionKind::List) {
        return lower_list_push(ctx, obj, obj_ty, &args[0], span);
    }

    if args.len() == 2 && method_name == "insert" && builtin == Some(BuiltinCollectionKind::List) {
        return lower_list_insert(ctx, obj, obj_ty, &args[0], &args[1], span);
    }

    if args.len() == 2
        && method_name == "set"
        && matches!(
            builtin,
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        )
    {
        return lower_collection_set(ctx, obj, obj_ty, &args[0], &args[1], builtin, span);
    }

    // Try GPU reduce on array with 2 args (init, fold). Only a gpu-resident
    // receiver routes to the device reduction; a host receiver falls through to
    // the CPU `Foldable::reduce`. The residency is read from the binding's local
    // WITHOUT lowering `obj` here, so the single lowering happens inside
    // `try_lower_gpu_reduce` (lowering it here too would double-emit a non-trivial
    // receiver expression).
    if args.len() == 2 && method_name == "reduce" && builtin == Some(BuiltinCollectionKind::Array) {
        if let ExpressionKind::Identifier(name, _) = &obj.node {
            let is_gpu_resident = ctx.variable_map.get(name.as_str()).is_some_and(|&local| {
                ctx.body.local_decls[local.0].residency == crate::mir::body::BindingResidency::Gpu
            });
            if is_gpu_resident {
                return super::reduce_gpu::try_lower_gpu_reduce(
                    ctx,
                    obj,
                    obj_ty,
                    &args[0],
                    &args[1],
                    call_expr_id,
                    dest,
                    span,
                );
            }
        }
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
                // Extract concrete type_args from the overall call expression type
                let call_ty = ctx.type_checker.get_type(call_expr_id);
                let type_args = call_ty.and_then(|ty| {
                    if let TypeKind::Custom(_, ta) = &ty.kind {
                        ta.as_ref().map(|v| v.as_slice())
                    } else {
                        None
                    }
                });

                // Struct constructor.
                if let Some(TypeDefinition::Struct(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    return lower_struct_constructor(
                        ctx, span, type_name, def, args, type_args, dest,
                    )
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
    let func_watermark = ctx.body.local_decls.len();
    let mut func_op = lower_expression(ctx, func, None)?;

    apply_generic_mangling(ctx, &func.node, call_expr_id, &mut func_op, func.span);

    let is_generic_call = ctx
        .type_checker
        .call_generic_mappings
        .contains_key(&call_expr_id);
    let param_types = resolve_param_types(ctx, func.id, is_generic_call);

    let arg_watermark = ctx.body.local_decls.len();
    let mut arg_ops = lower_and_coerce_args(ctx, args, &param_types);

    fill_default_args(ctx, &mut arg_ops, &param_types)?;

    inject_allocator_arg(ctx, &func.node, &func_op, &mut arg_ops);

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or(Type::new(TypeKind::Void, *span));
    let (destination, op) = call_destination(ctx, return_ty, dest, *span);

    let is_indirect_call = !matches!(
        func_op,
        Operand::Constant(ref c) if matches!(c.literal, crate::ast::literal::Literal::Identifier(_))
    );
    let func_op_for_drop = func_op.clone();
    let out_args = build_out_args(&param_types, &arg_ops);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops.clone(),
        out_args,
        destination.clone(),
        *span,
    );
    emit_direct_call_drops(ctx, &arg_ops, arg_watermark, destination.local, *span);
    if is_indirect_call {
        if let Operand::Copy(place) | Operand::Move(place) = &func_op_for_drop {
            if place.local != destination.local {
                ctx.emit_temp_drop(place.local, func_watermark, *span);
            }
        }
    }
    Ok(op)
}

/// Emit a `Call` terminator to `destination` and advance to its successor block.
fn emit_call_terminator(
    ctx: &mut LoweringContext,
    func_op: Operand,
    args: Vec<Operand>,
    out_args: Vec<bool>,
    destination: Place,
    span: Span,
) {
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            out_args,
            destination,
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
}

/// Build the per-arg `out` flag list for a direct call.
fn build_out_args(
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
    arg_ops: &[Operand],
) -> Vec<bool> {
    match param_types {
        Some(params) => arg_ops
            .iter()
            .enumerate()
            .map(|(i, _)| params.get(i).is_some_and(|p| p.is_out))
            .collect(),
        None => Vec::new(),
    }
}

/// Drop each freshly-created argument temp (skipping the call destination).
fn emit_direct_call_drops(
    ctx: &mut LoweringContext,
    arg_ops: &[Operand],
    arg_watermark: usize,
    dest_local: Local,
    span: Span,
) {
    for arg_op in arg_ops {
        if let Operand::Copy(place) | Operand::Move(place) = arg_op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, span);
            }
        }
    }
}

fn apply_generic_mangling(
    ctx: &mut LoweringContext,
    func_node: &ExpressionKind,
    call_expr_id: usize,
    func_op: &mut Operand,
    func_span: Span,
) {
    if let ExpressionKind::Identifier(func_name, _) = func_node {
        if let Some(generic_args) = ctx.type_checker.call_generic_mappings.get(&call_expr_id) {
            let mangled = mangle_generic_name(func_name, generic_args);
            *func_op = Operand::Constant(Box::new(crate::mir::Constant {
                span: func_span,
                ty: crate::ast::types::Type::new(TypeKind::Identifier, func_span),
                literal: crate::ast::literal::Literal::Identifier(mangled),
            }));
        }
    }
}

fn resolve_param_types(
    ctx: &LoweringContext,
    func_id: usize,
    is_generic_call: bool,
) -> Option<Vec<crate::ast::common::Parameter>> {
    if is_generic_call {
        return None;
    }
    let func_ty = ctx.type_checker.get_type(func_id)?;
    if let TypeKind::Function(func_data) = &func_ty.kind {
        Some(func_data.params.clone())
    } else {
        None
    }
}

fn lower_and_coerce_args(
    ctx: &mut LoweringContext,
    args: &[Expression],
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
) -> Vec<Operand> {
    let mut arg_ops = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        let mut op = lower_expression(ctx, arg, None).unwrap_or_else(|_| {
            Operand::Constant(Box::new(crate::mir::Constant {
                span: arg.span,
                ty: Type::new(TypeKind::Void, arg.span),
                literal: crate::ast::literal::Literal::None,
            }))
        });

        if let Some(params) = param_types {
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

        let op = match op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        arg_ops.push(op);
    }
    arg_ops
}

fn fill_default_args(
    ctx: &mut LoweringContext,
    arg_ops: &mut Vec<Operand>,
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
) -> Result<(), LoweringError> {
    if let Some(params) = param_types {
        for param in params.iter().skip(arg_ops.len()) {
            if let Some(default_expr) = &param.default_value {
                let default_op = lower_expression(ctx, default_expr, None)?;
                arg_ops.push(default_op);
            }
        }
    }
    Ok(())
}

fn inject_allocator_arg(
    ctx: &mut LoweringContext,
    func_node: &ExpressionKind,
    func_op: &Operand,
    arg_ops: &mut Vec<Operand>,
) {
    let is_runtime_fn = if let ExpressionKind::Identifier(name, _) = func_node {
        name.starts_with("miri_")
    } else {
        false
    };
    let is_indirect_call = !matches!(
        func_op,
        Operand::Constant(ref c) if matches!(c.literal, crate::ast::literal::Literal::Identifier(_))
    );

    if is_runtime_fn || is_indirect_call {
        return;
    }

    let is_math_fn = if let ExpressionKind::Identifier(name, _) = func_node {
        MathIntrinsic::from_name(name.as_str()).is_some()
            && ctx
                .type_checker
                .get_variable_module(name.as_str())
                .map(|m| m == "system.math")
                .unwrap_or(false)
    } else {
        false
    };

    if is_math_fn {
        return;
    }

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
