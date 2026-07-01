// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression type inference for the type checker.
//!
//! This module implements type inference for all expression kinds in Miri.
//! The main entry point is [`TypeChecker::infer_expression`], which dispatches
//! to specialized inference methods based on the expression kind.
//!
//! # Supported Expressions
//!
//! ## Literals
//! - Integer, float, string, boolean, and none literals
//!
//! ## Operators
//! - Binary: arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`<`, `>`, `==`, etc.)
//! - Logical: `and`, `or`
//! - Unary: `-`, `+`, `not`, `~`, `await`
//!
//! ## Collections
//! - Lists: `[1, 2, 3]` → `List<int>`
//! - Maps: `{"a": 1}` → `Map<string, int>`
//! - Sets: `{1, 2, 3}` → `Set<int>`
//! - Tuples: `(1, "a")` → `(int, string)`
//! - Ranges: `1..10` → `Range<int>`
//!
//! ## Access
//! - Member access: `obj.field`
//! - Index access: `list[0]`, `map["key"]`
//!
//! ## Functions
//! - Function calls with generic type inference
//! - Lambda expressions with type inference
//! - Method calls on objects
//!
//! ## Control Flow
//! - Conditional expressions: `x if cond else y`
//! - Match expressions with pattern matching
//!
//! ## Types
//! - Struct instantiation: `Point { x: 1, y: 2 }`
//! - Enum variant construction: `Ok(value)`, `Err(error)`
//! - Generic type instantiation

use crate::ast::factory as ast_factory;
use crate::ast::factory::make_type;
use crate::ast::types::{vec_dim, BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::mir::MathIntrinsic;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::utils::{is_gpu_compatible, is_perceus_managed};
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

impl TypeChecker {
    /// Infers the return type of a function or constructor call.
    ///
    /// Handles positional and named arguments, generic type inference,
    /// struct/class constructors via `Meta` types, and argument validation.
    pub(crate) fn infer_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        span: Span,
        context: &mut Context,
        call_id: usize,
    ) -> Type {
        let func_type = self.infer_expression(func, context);

        // Process arguments
        let mut positional_args = Vec::with_capacity(args.len());
        let mut named_args = HashMap::with_capacity(args.len());

        for arg in args {
            match &arg.node {
                ExpressionKind::NamedArgument(name, value) => {
                    if named_args.contains_key(name) {
                        self.report_error(format!("Duplicate argument '{}'", name), arg.span);
                    } else {
                        let ty = self.infer_expression(value, context);
                        named_args.insert(name.clone(), (value.as_ref(), ty, arg.span));
                    }
                }
                _ => {
                    if !named_args.is_empty() {
                        self.report_error(
                            "Positional arguments cannot follow named arguments".to_string(),
                            arg.span,
                        );
                    }
                    let ty = self.infer_expression(arg, context);
                    positional_args.push((arg, ty));
                }
            }
        }

        if !context.in_gpu_function {
            self.reject_gpu_resident_call_args(func, args, context);
        }

        let callable = matches!(func_type.kind, TypeKind::Function(_) | TypeKind::Meta(_));

        let result_type = self.infer_call_dispatch(
            &func_type,
            func,
            &positional_args,
            named_args,
            span,
            context,
            call_id,
        );

        if context.in_gpu_function && callable {
            self.check_gpu_call_types(func, &positional_args, context);
        }

        result_type
    }

    /// Dispatches to function or constructor call based on the function type.
    #[allow(clippy::too_many_arguments)]
    fn infer_call_dispatch(
        &mut self,
        func_type: &Type,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
        named_args: HashMap<String, (&Expression, Type, Span)>,
        span: Span,
        context: &mut Context,
        call_id: usize,
    ) -> Type {
        // Check for math intrinsics with numeric polymorphism (abs, min, max).
        // These accept both int and float arguments, with return type matching the first arg.
        if let Some(return_type) =
            self.try_infer_polymorphic_math_call(func, positional_args, span, context)
        {
            return return_type;
        }

        // Validate warp shuffle_down compile-time offset BEFORE type inference.
        // This ensures the offset guard is not bypassed by an early return.
        if self.try_validate_warp_shuffle_down(func, positional_args, span) {
            // Validation errors already reported; continue to type inference.
        }

        // Check for warp shuffle_down polymorphism: return type matches first argument.
        if let Some(return_type) = self.try_infer_warp_shuffle_down_call(func, positional_args) {
            return return_type;
        }

        // Check for vector builtin functions (dot, length, normalize, cross, reflect, mix).
        if let Some(return_type) = self.try_infer_vector_builtin_call(func, positional_args, span) {
            return return_type;
        }

        // Check for GPU atomic operations (atomic_add, atomic_sub, etc.).
        if let Some(return_type) = self.try_infer_atomic_builtin_call(func, positional_args, span) {
            return return_type;
        }

        match &func_type.kind {
            TypeKind::Function(func_data) => self.infer_function_call(
                func_data,
                positional_args,
                named_args,
                span,
                context,
                call_id,
            ),
            TypeKind::Meta(inner_type) => {
                self.infer_constructor_call(inner_type, positional_args, named_args, span, context)
            }
            _ if matches!(func_type.kind, TypeKind::Error) => make_type(TypeKind::Error),
            _ => {
                self.report_error(
                    format!("Expression is not callable: {}", func_type),
                    func.span,
                );
                make_type(TypeKind::Error)
            }
        }
    }

    /// Detects polymorphic math intrinsics (abs, min, max) and infers their return type
    /// based on the first argument's numeric type, bypassing the float-only stdlib signature.
    /// Returns `Some(return_type)` if this is a detected intrinsic, `None` otherwise.
    fn try_infer_polymorphic_math_call(
        &mut self,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
        span: Span,
        context: &Context,
    ) -> Option<Type> {
        // Extract the function name from direct identifier or module member access.
        let func_name = match &func.node {
            ExpressionKind::Identifier(name, _) => Some(name.as_str()),
            ExpressionKind::Member(_module_expr, func_expr) => {
                // Handle `M.abs` where M is `use system.math as M`.
                if let ExpressionKind::Identifier(func_name, _) = &func_expr.node {
                    Some(func_name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        }?;

        // Check if this is a polymorphic math intrinsic (only Abs, Min, Max).
        // Bind the intrinsic once; validate arity in exhaustive match below.
        let intrinsic = match MathIntrinsic::from_name(func_name) {
            Some(m @ (MathIntrinsic::Abs | MathIntrinsic::Min | MathIntrinsic::Max)) => m,
            _ => return None,
        };

        // Verify it's from the math module.
        // Deliberate stdlib-independence deviation: the module gate prevents a user-defined `abs`
        // from being treated as the polymorphic intrinsic. This mirrors MIR intercepts at
        // src/mir/lowering/dispatch.rs:323 and expression/call_expr.rs:183.
        let is_from_math = self
            .get_variable_module(func_name)
            .map(|m| m == "system.math")
            .unwrap_or(false);

        if !is_from_math {
            return None;
        }

        // Validate arity before type checking.
        // Note: The guard above (line 191) ensures `intrinsic` is one of Abs, Min, Max,
        // so the _ arm below should never be reached in well-formed code.
        match intrinsic {
            MathIntrinsic::Abs => {
                if positional_args.len() != 1 {
                    self.report_error(
                        format!(
                            "abs expects exactly one argument, but got {}",
                            positional_args.len()
                        ),
                        span,
                    );
                    return Some(make_type(TypeKind::Error));
                }
            }
            MathIntrinsic::Min | MathIntrinsic::Max => {
                if positional_args.len() != 2 {
                    self.report_error(
                        format!(
                            "{} expects exactly two arguments, but got {}",
                            func_name,
                            positional_args.len()
                        ),
                        span,
                    );
                    return Some(make_type(TypeKind::Error));
                }
            }
            // Other intrinsics are filtered out by the guard above.
            _ => return None,
        }

        // All args must be numeric (int or float). Return type matches the first arg.
        let first_arg_type = positional_args.first().map(|(_, ty)| ty)?;

        if !self.is_numeric_type(&first_arg_type.kind) {
            self.report_error(
                format!(
                    "Type '{}' is not numeric: {} only accepts int or float arguments",
                    first_arg_type, func_name
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        // Validate that all arguments are the same numeric type (or numeric-compatible).
        for (arg_expr, arg_type) in positional_args {
            if !self.are_compatible(first_arg_type, arg_type, context) {
                self.report_error(
                    format!(
                        "Type mismatch in {}: expected {}, got {}",
                        func_name, first_arg_type, arg_type
                    ),
                    arg_expr.span,
                );
            }
        }

        // Return type matches the first argument type (numeric polymorphism).
        Some(first_arg_type.clone())
    }

    /// Detects vector builtin functions (dot, length, normalize, cross, reflect, mix)
    /// and infers their return type based on argument types.
    /// Returns `Some(return_type)` if this is a detected builtin, `None` otherwise.
    fn extract_vec_elem_type(&self, vec_args: Option<&Vec<Expression>>) -> Option<Type> {
        vec_args
            .and_then(|args| args.first())
            .and_then(|elem_expr| {
                if let ExpressionKind::Type(ty, _) = &elem_expr.node {
                    Some((**ty).clone())
                } else {
                    None
                }
            })
    }

    fn infer_vec_dot(
        &mut self,
        positional_args: &[(&Expression, Type)],
        first_arg_type: &Type,
        vec_args: Option<&Vec<Expression>>,
        span: Span,
    ) -> Option<Type> {
        if positional_args.len() != 2 {
            self.report_error(
                format!(
                    "dot expects exactly two arguments, but got {}",
                    positional_args.len()
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let elem_type = self.extract_vec_elem_type(vec_args)?;
        if !matches!(elem_type.kind, TypeKind::F32) {
            self.report_error(
                format!(
                    "dot expects vector with f32 elements, got {}",
                    first_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let second_arg_type = positional_args.get(1).map(|(_, ty)| ty)?;
        if second_arg_type != first_arg_type {
            self.report_error(
                format!(
                    "dot expects both vector arguments to have the same type, got {} and {}",
                    first_arg_type, second_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        Some(elem_type)
    }

    fn infer_vec_length(
        &mut self,
        positional_args: &[(&Expression, Type)],
        first_arg_type: &Type,
        vec_args: Option<&Vec<Expression>>,
        span: Span,
    ) -> Option<Type> {
        if positional_args.len() != 1 {
            self.report_error(
                format!(
                    "length expects exactly one argument, but got {}",
                    positional_args.len()
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let elem_type = self.extract_vec_elem_type(vec_args)?;
        if !matches!(elem_type.kind, TypeKind::F32) {
            self.report_error(
                format!(
                    "length expects vector with f32 elements, got {}",
                    first_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        Some(elem_type)
    }

    fn infer_vec_normalize(
        &mut self,
        positional_args: &[(&Expression, Type)],
        first_arg_type: &Type,
        vec_args: Option<&Vec<Expression>>,
        span: Span,
    ) -> Option<Type> {
        if positional_args.len() != 1 {
            self.report_error(
                format!(
                    "normalize expects exactly one argument, but got {}",
                    positional_args.len()
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let elem_type = self.extract_vec_elem_type(vec_args)?;
        if !matches!(elem_type.kind, TypeKind::F32) {
            self.report_error(
                format!(
                    "normalize expects vector with f32 elements, got {}",
                    first_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        Some(first_arg_type.clone())
    }

    fn infer_vec_cross(
        &mut self,
        positional_args: &[(&Expression, Type)],
        first_arg_type: &Type,
        vec_dimension: u8,
        span: Span,
    ) -> Option<Type> {
        if positional_args.len() != 2 {
            self.report_error(
                format!(
                    "cross expects exactly two arguments, but got {}",
                    positional_args.len()
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        if vec_dimension != 3 {
            self.report_error(
                format!("cross expects Vec3 arguments, got {}", first_arg_type),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        Some(first_arg_type.clone())
    }

    fn infer_vec_reflect(
        &mut self,
        positional_args: &[(&Expression, Type)],
        first_arg_type: &Type,
        vec_args: Option<&Vec<Expression>>,
        span: Span,
    ) -> Option<Type> {
        if positional_args.len() != 2 {
            self.report_error(
                format!(
                    "reflect expects exactly two arguments, but got {}",
                    positional_args.len()
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let elem_type = self.extract_vec_elem_type(vec_args)?;
        if !matches!(elem_type.kind, TypeKind::F32) {
            self.report_error(
                format!(
                    "reflect expects vector with f32 elements, got {}",
                    first_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let second_arg_type = positional_args.get(1).map(|(_, ty)| ty)?;
        if second_arg_type != first_arg_type {
            self.report_error(
                format!(
                    "reflect expects both vector arguments to have the same type, got {} and {}",
                    first_arg_type, second_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        Some(first_arg_type.clone())
    }

    fn infer_vec_mix(
        &mut self,
        positional_args: &[(&Expression, Type)],
        first_arg_type: &Type,
        vec_args: Option<&Vec<Expression>>,
        span: Span,
    ) -> Option<Type> {
        if positional_args.len() != 3 {
            self.report_error(
                format!(
                    "mix expects exactly three arguments, but got {}",
                    positional_args.len()
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let elem_type = self.extract_vec_elem_type(vec_args)?;
        if !matches!(elem_type.kind, TypeKind::F32) {
            self.report_error(
                format!(
                    "mix expects vector with f32 elements, got {}",
                    first_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        let second_arg_type = positional_args.get(1).map(|(_, ty)| ty)?;
        if second_arg_type != first_arg_type {
            self.report_error(
                format!(
                    "mix expects both vector arguments to have the same type, got {} and {}",
                    first_arg_type, second_arg_type
                ),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        Some(first_arg_type.clone())
    }

    /// Handles polymorphic typing for `kernel.warp.shuffle_down(value: T, offset: Int) -> T`.
    /// Returns Some(value_type) if this is a shuffle_down call, None otherwise.
    /// The return type equals the value argument's type (int, f32, etc.).
    fn try_infer_warp_shuffle_down_call(
        &mut self,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
    ) -> Option<Type> {
        // Check if func is `kernel.warp.shuffle_down`.
        let ExpressionKind::Member(obj, prop) = &func.node else {
            return None;
        };
        let ExpressionKind::Member(kernel_expr, warp_expr) = &obj.node else {
            return None;
        };
        let ExpressionKind::Identifier(kernel_name, _) = &kernel_expr.node else {
            return None;
        };
        if kernel_name != crate::ast::types::KERNEL_CONTEXT_IDENT
            && kernel_name != crate::ast::types::GPU_CONTEXT_DEPRECATED_IDENT
        {
            return None;
        }
        let ExpressionKind::Identifier(warp_name, _) = &warp_expr.node else {
            return None;
        };
        if warp_name != "warp" {
            return None;
        }
        let ExpressionKind::Identifier(prop_name, _) = &prop.node else {
            return None;
        };
        if prop_name != "shuffle_down" {
            return None;
        }

        // This IS a warp shuffle_down call. Return the type of the first argument.
        // The second argument validation happens in try_validate_warp_shuffle_down.
        if positional_args.is_empty() {
            return Some(make_type(TypeKind::Error));
        }

        Some(positional_args[0].1.clone())
    }

    fn try_infer_vector_builtin_call(
        &mut self,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
        span: Span,
    ) -> Option<Type> {
        // Extract the function name from direct identifier or module member access.
        let func_name = match &func.node {
            ExpressionKind::Identifier(name, _) => Some(name.as_str()),
            ExpressionKind::Member(_module_expr, func_expr) => {
                if let ExpressionKind::Identifier(func_name, _) = &func_expr.node {
                    Some(func_name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        }?;

        // Check if the first argument is a vector type (Vec2, Vec3, Vec4).
        // Vector builtins dispatch on the first argument's type, not by name alone.
        let first_arg_type = positional_args.first().map(|(_, ty)| ty)?;
        let (vec_name, vec_args) = match &first_arg_type.kind {
            TypeKind::Custom(name, args) => (name.as_str(), args.as_ref()),
            _ => return None,
        };

        let vec_dimension = vec_dim(vec_name)?;

        match func_name {
            "dot" => self.infer_vec_dot(positional_args, first_arg_type, vec_args, span),
            "length" => self.infer_vec_length(positional_args, first_arg_type, vec_args, span),
            "normalize" => {
                self.infer_vec_normalize(positional_args, first_arg_type, vec_args, span)
            }
            "cross" => self.infer_vec_cross(positional_args, first_arg_type, vec_dimension, span),
            "reflect" => self.infer_vec_reflect(positional_args, first_arg_type, vec_args, span),
            "mix" => self.infer_vec_mix(positional_args, first_arg_type, vec_args, span),
            _ => None,
        }
    }

    fn try_infer_atomic_builtin_call(
        &mut self,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
        _span: Span,
    ) -> Option<Type> {
        // Extract the function name from direct identifier only (no module member access).
        let func_name = match &func.node {
            ExpressionKind::Identifier(name, _) => Some(name.as_str()),
            _ => None,
        }?;

        // Check if this is an atomic operation.
        match func_name {
            "atomic_add" | "atomic_sub" | "atomic_max" | "atomic_min" | "atomic_and"
            | "atomic_or" | "atomic_xor" | "atomic_exchange" => {
                // atomic_* operations return the type of the value argument (arg 2).
                // The return value is the old value from the buffer element.
                if positional_args.len() >= 3 {
                    Some(positional_args[2].1.clone())
                } else {
                    None
                }
            }
            "atomic_compare_exchange" => {
                // compare_exchange also returns the type of the value argument (arg 2).
                if positional_args.len() >= 4 {
                    Some(positional_args[2].1.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Validates `kernel.warp.shuffle_down(value, offset)` arguments at compile time.
    /// Returns true if this IS a warp shuffle_down call (whether validation passes or fails).
    /// Reports errors for non-literal offsets or offsets outside [0, 128].
    fn try_validate_warp_shuffle_down(
        &mut self,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
        span: Span,
    ) -> bool {
        // Check if func is `kernel.warp.shuffle_down` (Member(Member(...), Identifier)).
        let ExpressionKind::Member(obj, prop) = &func.node else {
            return false;
        };
        let ExpressionKind::Member(kernel_expr, warp_expr) = &obj.node else {
            return false;
        };
        let ExpressionKind::Identifier(kernel_name, _) = &kernel_expr.node else {
            return false;
        };
        if kernel_name != crate::ast::types::KERNEL_CONTEXT_IDENT
            && kernel_name != crate::ast::types::GPU_CONTEXT_DEPRECATED_IDENT
        {
            return false;
        }
        let ExpressionKind::Identifier(warp_name, _) = &warp_expr.node else {
            return false;
        };
        if warp_name != "warp" {
            return false;
        }
        let ExpressionKind::Identifier(prop_name, _) = &prop.node else {
            return false;
        };
        if prop_name != "shuffle_down" {
            return false;
        }

        // This IS a warp shuffle_down call. Validate the offset argument (arg1).
        if positional_args.len() < 2 {
            self.report_error(
                "shuffle_down requires 2 arguments (value, offset)".to_string(),
                span,
            );
            return true;
        }

        let offset_expr = positional_args[1].0;
        let offset_span = offset_expr.span;

        // Offset MUST be a compile-time integer literal.
        match &offset_expr.node {
            ExpressionKind::Literal(crate::ast::literal::Literal::Integer(lit_val)) => {
                let offset_i128 = lit_val.to_i128();
                if !(0..=128).contains(&offset_i128) {
                    self.report_error(
                        format!(
                            "shuffle offset {} exceeds the maximum subgroup size (128)",
                            offset_i128
                        ),
                        offset_span,
                    );
                }
            }
            _ => {
                self.report_error(
                    "shuffle offset must be a compile-time literal".to_string(),
                    offset_span,
                );
            }
        }

        true
    }

    /// Checks GPU compatibility of call arguments and the function itself.
    /// Validates that the callee is GPU-compatible (scalar parameters/return,
    /// no out parameters, no host-only intrinsics in body, no recursion).
    fn check_gpu_call_types(
        &mut self,
        func: &Expression,
        positional_args: &[(&Expression, Type)],
        context: &Context,
    ) {
        // Check argument types
        for (_, arg_type) in positional_args {
            if matches!(arg_type.kind, TypeKind::Error) {
                continue;
            }
            if !is_gpu_compatible(&arg_type.kind) {
                self.report_error(
                    format!(
                        "Type '{}' is not GPU-compatible: only numeric primitives, booleans, and GPU types may cross a call boundary inside a 'gpu fn'",
                        arg_type
                    ),
                    Span::default(),
                );
            }
        }

        // Check callee definition
        self.validate_gpu_callee(func, context);
    }

    /// Validates that a callee function is GPU-compatible.
    /// A GPU-callable function must have:
    /// - Only scalar (GPU-compatible) parameter types and return type (not arrays)
    /// - No `out` parameters
    /// - A body that only calls other GPU-compatible functions or GPU/math intrinsics
    /// - No direct or indirect recursion
    fn validate_gpu_callee(&mut self, func: &Expression, context: &Context) {
        // Extract the function name from the expression.
        let func_name = match &func.node {
            ExpressionKind::Identifier(name, _) => name.as_str(),
            ExpressionKind::Member(_module_expr, func_expr) => {
                // For module.function, extract the function name
                if let ExpressionKind::Identifier(name, _) = &func_expr.node {
                    name.as_str()
                } else {
                    return; // Can't extract name from complex member expression
                }
            }
            _ => return, // Not a simple function call
        };

        // Look up the function definition in the context
        let Some(info) = context.resolve_info(func_name) else {
            return; // Function not found; error already reported elsewhere
        };

        // If it's an intrinsic or builtin, skip validation
        if info.is_intrinsic {
            return;
        }

        // Inspect the function type to validate compatibility
        let TypeKind::Function(func_data) = &info.ty.kind else {
            return; // Not a function type
        };

        // Validate return type is a GPU-compatible scalar. A function with no
        // declared return type returns `void`, which has no WGSL representation.
        let Some(ret_type_expr) = &func_data.return_type else {
            self.report_error(
                format!(
                    "Function '{}' returns 'void' which is not GPU-compatible",
                    func_name
                ),
                func.span,
            );
            return;
        };
        {
            let ret_type = self.resolve_type_expression(ret_type_expr, context);
            if !self.is_gpu_scalar(&ret_type.kind) {
                self.report_error(
                    format!(
                        "Function '{}' returns '{}' which is not GPU-compatible",
                        func_name, ret_type
                    ),
                    func.span,
                );
                return;
            }
        }

        // Validate parameter types and out-parameter constraints
        for param in &func_data.params {
            let param_type = self.resolve_type_expression(&param.typ, context);

            // Reject out parameters in GPU callees
            if param.is_out {
                self.report_error(
                    format!(
                        "Function '{}' has 'out' parameter '{}' which is not GPU-compatible",
                        func_name, param.name
                    ),
                    func.span,
                );
                return;
            }

            // Reject non-scalar parameter types (arrays, lists, etc.)
            if !self.is_gpu_scalar(&param_type.kind) {
                self.report_error(
                    format!(
                        "Function '{}' parameter '{}' has type '{}' which is not GPU-compatible",
                        func_name, param.name, param_type
                    ),
                    func.span,
                );
                return;
            }
        }

        // Analyze the function body for GPU compatibility (host-only calls, recursion).
        // This requires walking the body AST, which is stored in `function_bodies`.
        // Clone the body Rc to avoid borrow checker issues with `&mut self`.
        let body_opt = self.function_bodies.get(func_name).cloned();
        if let Some(body) = body_opt {
            self.validate_gpu_function_body(func_name, &body);
        }
    }

    /// Validates that a function body is GPU-compatible (basic checks).
    /// Detects calls to host-only intrinsics and recursion.
    fn validate_gpu_function_body(
        &mut self,
        func_name: &str,
        body: &std::rc::Rc<crate::ast::Statement>,
    ) {
        // Check for recursion
        let mut visited = std::collections::HashSet::new();
        if self.check_recursion(func_name, &mut visited) {
            self.report_error(
                "recursion is not allowed in GPU code".to_string(),
                Span::default(),
            );
            return;
        }

        // Check for host-only calls in the body
        let mut has_host_calls = false;
        self.scan_for_host_calls(body, &mut has_host_calls);
        if has_host_calls {
            self.report_error(
                "Function calls host-only intrinsic which is not GPU-compatible".to_string(),
                Span::default(),
            );
        }
    }

    /// Checks if a function has direct or indirect recursion.
    fn check_recursion(
        &self,
        func_name: &str,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        if !visited.insert(func_name.to_string()) {
            // Already in the path - recursion detected
            return true;
        }

        let Some(body) = self.function_bodies.get(func_name) else {
            visited.remove(func_name);
            return false;
        };

        // Collect all direct callees
        let mut callees = Vec::new();
        self.collect_callees(body, &mut callees);

        for callee_name in callees {
            if self.check_recursion(&callee_name, visited) {
                return true;
            }
        }

        visited.remove(func_name);
        false
    }

    /// Collects all direct function call names in a statement.
    fn collect_callees(&self, stmt: &crate::ast::Statement, callees: &mut Vec<String>) {
        use crate::ast::StatementKind;

        match &stmt.node {
            StatementKind::Expression(expr) => self.collect_callees_expr(expr, callees),
            StatementKind::Block(stmts) => {
                for s in stmts {
                    self.collect_callees(s, callees);
                }
            }
            StatementKind::If(cond, then_block, else_block, _) => {
                self.collect_callees_expr(cond, callees);
                self.collect_callees(then_block, callees);
                if let Some(eb) = else_block {
                    self.collect_callees(eb, callees);
                }
            }
            StatementKind::While(cond, body, _) => {
                self.collect_callees_expr(cond, callees);
                self.collect_callees(body, callees);
            }
            StatementKind::For(_, iterable, body) => {
                self.collect_callees_expr(iterable, callees);
                self.collect_callees(body, callees);
            }
            StatementKind::Forall { iterable, body, .. } => {
                self.collect_callees_expr(iterable, callees);
                self.collect_callees(body, callees);
            }
            StatementKind::Return(Some(expr)) => self.collect_callees_expr(expr, callees),
            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    if let Some(init) = &decl.initializer {
                        self.collect_callees_expr(init, callees);
                    }
                }
            }
            _ => {}
        }
    }

    /// Recursively collects direct callees from an expression.
    fn collect_callees_expr(&self, expr: &crate::ast::Expression, callees: &mut Vec<String>) {
        use crate::ast::ExpressionKind;

        match &expr.node {
            ExpressionKind::Call(func_expr, args) => {
                if let ExpressionKind::Identifier(name, _) = &func_expr.node {
                    callees.push(name.clone());
                }
                for arg in args {
                    self.collect_callees_expr(arg, callees);
                }
                self.collect_callees_expr(func_expr, callees);
            }
            ExpressionKind::Binary(left, _, right) => {
                self.collect_callees_expr(left, callees);
                self.collect_callees_expr(right, callees);
            }
            ExpressionKind::Logical(left, _, right) => {
                self.collect_callees_expr(left, callees);
                self.collect_callees_expr(right, callees);
            }
            ExpressionKind::Unary(_, expr) => self.collect_callees_expr(expr, callees),
            ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
                self.collect_callees_expr(cond, callees);
                self.collect_callees_expr(then_expr, callees);
                if let Some(ee) = else_expr {
                    self.collect_callees_expr(ee, callees);
                }
            }
            ExpressionKind::Index(expr, index) => {
                self.collect_callees_expr(expr, callees);
                self.collect_callees_expr(index, callees);
            }
            ExpressionKind::Member(expr, _) => self.collect_callees_expr(expr, callees),
            ExpressionKind::List(elements) | ExpressionKind::Array(elements, _) => {
                for e in elements {
                    self.collect_callees_expr(e, callees);
                }
            }
            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.collect_callees_expr(k, callees);
                    self.collect_callees_expr(v, callees);
                }
            }
            ExpressionKind::Tuple(elements) | ExpressionKind::Set(elements) => {
                for e in elements {
                    self.collect_callees_expr(e, callees);
                }
            }
            ExpressionKind::Lambda(boxed) => {
                self.collect_callees(&boxed.body, callees);
            }
            _ => {}
        }
    }

    /// Scans a statement tree for calls to host-only intrinsics.
    fn scan_for_host_calls(&self, stmt: &crate::ast::Statement, found: &mut bool) {
        use crate::ast::StatementKind;

        match &stmt.node {
            StatementKind::Expression(expr) => self.scan_expr_for_host_calls(expr, found),
            StatementKind::Block(stmts) => {
                for s in stmts {
                    self.scan_for_host_calls(s, found);
                }
            }
            StatementKind::If(cond, then_block, else_block, _) => {
                self.scan_expr_for_host_calls(cond, found);
                self.scan_for_host_calls(then_block, found);
                if let Some(eb) = else_block {
                    self.scan_for_host_calls(eb, found);
                }
            }
            StatementKind::While(cond, body, _) => {
                self.scan_expr_for_host_calls(cond, found);
                self.scan_for_host_calls(body, found);
            }
            StatementKind::For(_, iterable, body) => {
                self.scan_expr_for_host_calls(iterable, found);
                self.scan_for_host_calls(body, found);
            }
            StatementKind::Forall { iterable, body, .. } => {
                self.scan_expr_for_host_calls(iterable, found);
                self.scan_for_host_calls(body, found);
            }
            StatementKind::Return(Some(expr)) => self.scan_expr_for_host_calls(expr, found),
            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    if let Some(init) = &decl.initializer {
                        self.scan_expr_for_host_calls(init, found);
                    }
                }
            }
            _ => {}
        }
    }

    /// Recursively scans expressions for host-only calls.
    fn scan_expr_for_host_calls(&self, expr: &crate::ast::Expression, found: &mut bool) {
        use crate::ast::ExpressionKind;

        match &expr.node {
            ExpressionKind::Call(func_expr, args) => {
                if let ExpressionKind::Identifier(name, _) = &func_expr.node {
                    if self.is_host_intrinsic(name) {
                        *found = true;
                        return;
                    }
                }
                for arg in args {
                    self.scan_expr_for_host_calls(arg, found);
                }
                self.scan_expr_for_host_calls(func_expr, found);
            }
            ExpressionKind::Binary(left, _, right) => {
                self.scan_expr_for_host_calls(left, found);
                self.scan_expr_for_host_calls(right, found);
            }
            ExpressionKind::Logical(left, _, right) => {
                self.scan_expr_for_host_calls(left, found);
                self.scan_expr_for_host_calls(right, found);
            }
            ExpressionKind::Unary(_, expr) => self.scan_expr_for_host_calls(expr, found),
            ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
                self.scan_expr_for_host_calls(cond, found);
                self.scan_expr_for_host_calls(then_expr, found);
                if let Some(ee) = else_expr {
                    self.scan_expr_for_host_calls(ee, found);
                }
            }
            ExpressionKind::Index(expr, index) => {
                self.scan_expr_for_host_calls(expr, found);
                self.scan_expr_for_host_calls(index, found);
            }
            ExpressionKind::Member(expr, _) => self.scan_expr_for_host_calls(expr, found),
            ExpressionKind::List(elements) | ExpressionKind::Array(elements, _) => {
                for e in elements {
                    self.scan_expr_for_host_calls(e, found);
                }
            }
            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.scan_expr_for_host_calls(k, found);
                    self.scan_expr_for_host_calls(v, found);
                }
            }
            ExpressionKind::Tuple(elements) | ExpressionKind::Set(elements) => {
                for e in elements {
                    self.scan_expr_for_host_calls(e, found);
                }
            }
            ExpressionKind::Lambda(boxed) => {
                self.scan_for_host_calls(&boxed.body, found);
            }
            _ => {}
        }
    }

    /// Checks if a function name is a host-only intrinsic.
    fn is_host_intrinsic(&self, name: &str) -> bool {
        matches!(
            name,
            "println" | "print" | "printf" | "eprintln" | "eprint" | "input" | "readln"
        )
    }

    /// Checks if a type is a GPU-compatible scalar (not an array or collection).
    /// Scalars are: numeric primitives, booleans, void, error, and generic parameters.
    fn is_gpu_scalar(&self, kind: &TypeKind) -> bool {
        match kind {
            TypeKind::Int
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
            | TypeKind::Boolean
            | TypeKind::Error => true,

            // A `void` value has no WGSL scalar representation, so a
            // void-returning function cannot be emitted as a GPU helper.
            TypeKind::Void => false,

            // Generic parameters and GPU builtins are OK
            TypeKind::Generic(_, _, _) => true,
            TypeKind::Custom(name, _) => {
                // Only allow GPU builtin types, not Array/List/etc.
                name == crate::ast::types::DIM3_TYPE_NAME
                    || name == crate::ast::types::GPU_CONTEXT_TYPE_NAME
                    || name == crate::ast::types::KERNEL_TYPE_NAME
                    || name == crate::ast::types::FRAME_INPUT_TYPE_NAME
            }

            // Arrays, collections, strings, etc. are NOT scalar
            TypeKind::Array(_, _)
            | TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Tuple(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Option(_)
            | TypeKind::Linear(_)
            | TypeKind::Meta(_)
            | TypeKind::RawPtr
            | TypeKind::Identifier
            | TypeKind::Function(_) => false,
        }
    }

    /// Rejects a gpu-resident binding passed as an argument to a host call.
    /// A host function reads its arguments on the host, so a gpu-resident
    /// value would need an implicit readback. To prevent silent readbacks, the
    /// caller must explicitly copy to host first (`let h = g`) and pass the copy.
    /// GPU functions are exempt from this check: they accept gpu-resident arrays directly.
    fn reject_gpu_resident_call_args(
        &mut self,
        func: &Expression,
        args: &[Expression],
        context: &Context,
    ) {
        let callee_name = match &func.node {
            ExpressionKind::Identifier(name, _) => Some(name.as_str()),
            _ => None,
        };

        let is_gpu_fn = callee_name
            .and_then(|name| self.global_scope.get(name))
            .map(|info| info.is_gpu_fn)
            .unwrap_or(false);

        if is_gpu_fn {
            return;
        }

        let callee = match &func.node {
            ExpressionKind::Identifier(name, _) => format!("'{name}'"),
            _ => "this host function".to_string(),
        };
        for arg in args {
            let value = match &arg.node {
                ExpressionKind::NamedArgument(_, inner) => inner.as_ref(),
                _ => arg,
            };
            let Some(name) = self.gpu_resident_identifier(value, context) else {
                continue;
            };
            let name = name.to_string();
            self.report_error_with_help(
                format!("cannot pass gpu-resident '{name}' to host function {callee}"),
                arg.span,
                format!("copy it to host first: 'let h = {name}', then pass 'h'"),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_function_call(
        &mut self,
        func_data: &crate::ast::types::FunctionTypeData,
        positional_args: &[(&Expression, Type)],
        mut named_args: HashMap<String, (&Expression, Type, Span)>,
        span: Span,
        context: &mut Context,
        call_id: usize,
    ) -> Type {
        let mut generic_map = std::collections::HashMap::new();

        if let Some(gens) = &func_data.generics {
            context.enter_scope();
            self.define_generics(gens, context);
        }

        self.validate_function_parameters(
            func_data,
            positional_args,
            &mut named_args,
            &mut generic_map,
            span,
            context,
        );

        let return_type = self.compute_function_return_type(func_data, &generic_map, context);

        if func_data.generics.is_some() {
            context.exit_scope();
            self.store_generic_call_mapping(func_data, &generic_map, call_id);
        }

        return_type
    }

    fn validate_function_parameters(
        &mut self,
        func_data: &crate::ast::types::FunctionTypeData,
        positional_args: &[(&Expression, Type)],
        named_args: &mut HashMap<String, (&Expression, Type, Span)>,
        generic_map: &mut std::collections::HashMap<String, Type>,
        span: Span,
        context: &mut Context,
    ) {
        let mut pos_iter = positional_args.iter();
        let mut seen_out_vars: std::collections::HashSet<String> = std::collections::HashSet::new();

        for param in &func_data.params {
            let param_type = self.resolve_type_expression(&param.typ, context);

            let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                (Some(*expr), Some(ty.clone()))
            } else if let Some((expr, ty, _)) = named_args.remove(&param.name) {
                (Some(expr), Some(ty))
            } else {
                (None, None)
            };

            if let Some(arg_type) = arg_type {
                if func_data.generics.is_some() {
                    self.infer_generic_types(&param_type, &arg_type, generic_map);
                }

                let concrete_param_type = if func_data.generics.is_some() {
                    self.substitute_type(&param_type, generic_map)
                } else {
                    param_type.clone()
                };

                if param.is_out {
                    self.validate_out_parameter(
                        &param.name,
                        arg_expr,
                        &concrete_param_type,
                        &arg_type,
                        span,
                        &mut seen_out_vars,
                        context,
                    );
                } else if !self.are_compatible(&concrete_param_type, &arg_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for argument '{}': expected {}, got {}",
                            param.name, concrete_param_type, arg_type
                        ),
                        arg_expr.map(|e| e.span).unwrap_or(span),
                    );
                }
            } else if param.default_value.is_none() {
                self.report_error(
                    format!("Missing argument for parameter '{}'", param.name),
                    span,
                );
            }
        }

        if pos_iter.next().is_some() {
            self.report_error(
                format!(
                    "Too many positional arguments: expected {}, got {}",
                    func_data.params.len(),
                    positional_args.len()
                ),
                span,
            );
        }

        for (name, (_, _, span)) in named_args.drain() {
            self.report_error(format!("Unknown argument '{}'", name), span);
        }
    }

    fn compute_function_return_type(
        &mut self,
        func_data: &crate::ast::types::FunctionTypeData,
        generic_map: &std::collections::HashMap<String, Type>,
        context: &mut Context,
    ) -> Type {
        if let Some(rt_expr) = &func_data.return_type {
            let rt = self.resolve_type_expression(rt_expr, context);
            if func_data.generics.is_some() {
                self.substitute_type(&rt, generic_map)
            } else {
                rt
            }
        } else {
            ast_factory::make_type(TypeKind::Void)
        }
    }

    fn store_generic_call_mapping(
        &mut self,
        func_data: &crate::ast::types::FunctionTypeData,
        generic_map: &std::collections::HashMap<String, Type>,
        call_id: usize,
    ) {
        if generic_map.is_empty() {
            return;
        }

        let ordered: Vec<(String, crate::ast::types::Type)> =
            if let Some(gens) = &func_data.generics {
                gens.iter()
                    .filter_map(|g| {
                        if let ExpressionKind::GenericType(name_expr, _, _) = &g.node {
                            if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                                generic_map.get(n).map(|t| (n.clone(), t.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
        if !ordered.is_empty() {
            self.call_generic_mappings.insert(call_id, ordered);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_out_parameter(
        &mut self,
        param_name: &str,
        arg_expr: Option<&Expression>,
        param_type: &Type,
        arg_type: &Type,
        span: Span,
        seen_out_vars: &mut std::collections::HashSet<String>,
        context: &Context,
    ) {
        let arg_span = arg_expr.map(|e| e.span).unwrap_or(span);
        self.validate_out_argument_variable(param_name, arg_expr, seen_out_vars, arg_span, context);
        self.validate_out_parameter_type(param_name, param_type, arg_type, arg_expr, span);
    }

    /// Validates that an out parameter argument is a mutable variable.
    fn validate_out_argument_variable(
        &mut self,
        param_name: &str,
        arg_expr: Option<&Expression>,
        seen_out_vars: &mut std::collections::HashSet<String>,
        arg_span: Span,
        context: &Context,
    ) {
        match arg_expr.map(|e| &e.node) {
            Some(ExpressionKind::Identifier(var_name, _)) => {
                if !context.is_mutable(var_name) {
                    self.report_error(
                        format!(
                            "expected mutable variable for 'out' parameter '{}': '{}' is immutable (declare with 'var')",
                            param_name, var_name
                        ),
                        arg_span,
                    );
                }
                if !seen_out_vars.insert(var_name.clone()) {
                    self.report_error(
                        format!(
                            "same variable passed twice as 'out': '{}' appears more than once",
                            var_name
                        ),
                        arg_span,
                    );
                }
            }
            Some(_) => {
                self.report_error(
                    format!(
                        "expected mutable variable for 'out' parameter '{}', but got a non-variable expression",
                        param_name
                    ),
                    arg_span,
                );
            }
            None => {}
        }
    }

    /// Validates the type of an out parameter.
    fn validate_out_parameter_type(
        &mut self,
        param_name: &str,
        param_type: &Type,
        arg_type: &Type,
        arg_expr: Option<&Expression>,
        span: Span,
    ) {
        let exact_match = matches!(param_type.kind, TypeKind::Error)
            || matches!(arg_type.kind, TypeKind::Error)
            || param_type == arg_type;
        if !exact_match {
            self.report_error(
                format!(
                    "Type mismatch for argument '{}': expected {}, got {}",
                    param_name, param_type, arg_type
                ),
                arg_expr.map(|e| e.span).unwrap_or(span),
            );
        }
    }

    fn infer_constructor_call(
        &mut self,
        inner_type: &Type,
        positional_args: &[(&Expression, Type)],
        mut named_args: HashMap<String, (&Expression, Type, Span)>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let TypeKind::Custom(name, type_args) = &inner_type.kind {
            let type_def = self.resolve_visible_type(name, context).cloned();

            if let Some(TypeDefinition::Class(def)) = &type_def {
                if def.is_abstract {
                    self.report_error(
                        format!(
                            "Cannot instantiate abstract class '{}'. Abstract classes cannot be instantiated directly.",
                            name
                        ),
                        span,
                    );
                    return make_type(TypeKind::Error);
                }

                if let Some(ty) =
                    self.try_infer_list_constructor(name, type_args, positional_args, span, context)
                {
                    return ty;
                }

                if let Some(ty) =
                    self.try_infer_map_constructor(name, type_args, positional_args, span, context)
                {
                    return ty;
                }

                if let Some(ty) =
                    self.try_infer_set_constructor(name, type_args, positional_args, span, context)
                {
                    return ty;
                }

                if let Some(ty) = self.try_infer_array_constructor(
                    name,
                    type_args,
                    positional_args,
                    span,
                    context,
                ) {
                    return ty;
                }

                self.validate_class_generics(def, name, type_args, span);

                if def.generics.is_some() {
                    if let Some(args) = type_args {
                        if let Some(resolved) = self.resolve_type_arg_tuple(args, context) {
                            self.record_generic_class_instantiation(name, resolved);
                        }
                    }
                }

                let init_method = self.find_init_method(def);

                if let Some(init_method) = init_method {
                    self.validate_class_init_args(
                        def,
                        &init_method,
                        positional_args,
                        &mut named_args,
                        type_args,
                        span,
                        context,
                    );
                } else {
                    // No init method anywhere in the chain → MIR maps each
                    // constructor argument directly onto a declared field
                    // (`lower_class_constructor`). The type checker must
                    // gate the same field/arg pairing here; otherwise a
                    // layout-incompatible argument (e.g. `List<F32>` flowing
                    // into a `List<float>` field) reaches codegen unchecked
                    // and reads garbage at run time.
                    self.validate_class_field_args(
                        def,
                        positional_args,
                        &mut named_args,
                        type_args,
                        span,
                        context,
                    );
                }

                return make_type(TypeKind::Custom(name.clone(), type_args.clone()));
            }

            if let Some(TypeDefinition::Struct(def)) = type_def {
                return self.infer_struct_constructor(
                    def,
                    name,
                    type_args,
                    positional_args,
                    named_args,
                    span,
                    context,
                );
            }

            // A constructor-shaped call on a `Custom` type with no visible
            // definition is most often a stdlib collection used without its
            // module (e.g. `Array<int, 3>(..)`); surface the import hint.
            if self.report_hidden_type_import_hint(name, span) {
                return make_type(TypeKind::Error);
            }
        }
        self.report_error(format!("Type '{}' is not callable", inner_type), span);
        make_type(TypeKind::Error)
    }

    fn try_infer_list_constructor(
        &mut self,
        name: &str,
        type_args: &Option<Vec<Expression>>,
        positional_args: &[(&Expression, Type)],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        if BuiltinCollectionKind::from_name(name) != Some(BuiltinCollectionKind::List) {
            return None;
        }

        if let Some(args) = type_args {
            if args.len() == 1 {
                let elem_type = self.resolve_type_expression(&args[0], context);
                return Some(make_type(TypeKind::Custom(
                    "List".to_string(),
                    Some(vec![self.create_type_expression(elem_type)]),
                )));
            } else {
                self.report_error(
                    format!(
                        "Class 'List<T>' expects 1 generic argument, got {}",
                        args.len()
                    ),
                    span,
                );
                return Some(make_type(TypeKind::Error));
            }
        }

        if let Some((_, arg_type)) = positional_args.first() {
            let elem_type = match &arg_type.kind {
                TypeKind::Custom(cname, Some(cargs))
                    if (BuiltinCollectionKind::from_name(cname.as_str())
                        == Some(BuiltinCollectionKind::Array)
                        || BuiltinCollectionKind::from_name(cname.as_str())
                            == Some(BuiltinCollectionKind::List))
                        && !cargs.is_empty() =>
                {
                    self.resolve_type_expression(&cargs[0], context)
                }
                _ => {
                    self.report_error(
                        format!(
                            "List(...) expects an array literal argument, got '{}'. Use 'List<T>()' for an empty list or 'List([...])' to convert an array",
                            arg_type
                        ),
                        span,
                    );
                    return Some(make_type(TypeKind::Error));
                }
            };
            return Some(make_type(TypeKind::Custom(
                "List".to_string(),
                Some(vec![self.create_type_expression(elem_type)]),
            )));
        }

        self.report_error(
            "Cannot instantiate generic class 'List<T>' without explicit type arguments"
                .to_string(),
            span,
        );
        Some(make_type(TypeKind::Error))
    }

    fn try_infer_map_constructor(
        &mut self,
        name: &str,
        type_args: &Option<Vec<Expression>>,
        positional_args: &[(&Expression, Type)],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        if BuiltinCollectionKind::from_name(name) != Some(BuiltinCollectionKind::Map) {
            return None;
        }

        if let Some(args) = type_args {
            if args.len() == 2 {
                let k_type = self.resolve_type_expression(&args[0], context);
                let v_type = self.resolve_type_expression(&args[1], context);
                return Some(make_type(TypeKind::Custom(
                    "Map".to_string(),
                    Some(vec![
                        self.create_type_expression(k_type),
                        self.create_type_expression(v_type),
                    ]),
                )));
            } else {
                self.report_error(
                    format!(
                        "Class 'Map<K, V>' expects 2 generic arguments, got {}",
                        args.len()
                    ),
                    span,
                );
                return Some(make_type(TypeKind::Error));
            }
        }

        if let Some((arg_expr, arg_type)) = positional_args.first() {
            if !matches!(&arg_expr.node, ExpressionKind::Map(_)) {
                self.report_error(
                    "Map(...) only accepts a map literal argument like '{\"key\": value}'. Use 'Map<K, V>()' for an empty map".to_string(),
                    span,
                );
                return Some(make_type(TypeKind::Error));
            }
            if let TypeKind::Custom(cname, Some(cargs)) = &arg_type.kind {
                if BuiltinCollectionKind::from_name(cname.as_str())
                    == Some(BuiltinCollectionKind::Map)
                    && cargs.len() == 2
                {
                    return Some(make_type(TypeKind::Custom(
                        "Map".to_string(),
                        Some(cargs.clone()),
                    )));
                }
            }
        }

        self.report_error(
            "Cannot instantiate generic class 'Map<K, V>' without explicit type arguments"
                .to_string(),
            span,
        );
        Some(make_type(TypeKind::Error))
    }

    fn try_infer_set_constructor(
        &mut self,
        name: &str,
        type_args: &Option<Vec<Expression>>,
        positional_args: &[(&Expression, Type)],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        if BuiltinCollectionKind::from_name(name) != Some(BuiltinCollectionKind::Set) {
            return None;
        }

        if let Some(args) = type_args {
            if args.len() == 1 {
                let elem_type = self.resolve_type_expression(&args[0], context);
                return Some(make_type(TypeKind::Custom(
                    "Set".to_string(),
                    Some(vec![self.create_type_expression(elem_type)]),
                )));
            } else {
                self.report_error(
                    format!(
                        "Class 'Set<T>' expects 1 generic argument, got {}",
                        args.len()
                    ),
                    span,
                );
                return Some(make_type(TypeKind::Error));
            }
        }

        if let Some((arg_expr, arg_type)) = positional_args.first() {
            if !matches!(&arg_expr.node, ExpressionKind::Set(_)) {
                self.report_error(
                    "Set(...) only accepts a set literal argument like '{1, 2, 3}'. Use 'Set<T>()' for an empty set".to_string(),
                    span,
                );
                return Some(make_type(TypeKind::Error));
            }
            if let TypeKind::Custom(cname, Some(cargs)) = &arg_type.kind {
                if BuiltinCollectionKind::from_name(cname.as_str())
                    == Some(BuiltinCollectionKind::Set)
                    && !cargs.is_empty()
                {
                    return Some(make_type(TypeKind::Custom(
                        "Set".to_string(),
                        Some(cargs.clone()),
                    )));
                }
            }
        }

        self.report_error(
            "Cannot instantiate generic class 'Set<T>' without explicit type arguments".to_string(),
            span,
        );
        Some(make_type(TypeKind::Error))
    }

    fn validate_class_generics(
        &mut self,
        def: &crate::type_checker::context::ClassDefinition,
        name: &str,
        type_args: &Option<Vec<Expression>>,
        span: Span,
    ) {
        if let Some(generics) = &def.generics {
            let generic_names: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
            let signature = format!("{}<{}>", name, generic_names.join(", "));

            if let Some(args) = type_args {
                if generics.len() != args.len() {
                    self.report_error(
                        format!(
                            "Class '{}' expects {} generic arguments, got {}",
                            signature,
                            generics.len(),
                            args.len()
                        ),
                        span,
                    );
                }
            } else {
                self.report_error(
                    format!(
                        "Cannot instantiate generic class '{}' without explicit type arguments",
                        signature
                    ),
                    span,
                );
            }
        } else if type_args.is_some() {
            self.report_error(
                format!("Class '{}' does not take generic arguments", name),
                span,
            );
        }
    }

    fn find_init_method(
        &self,
        def: &crate::type_checker::context::ClassDefinition,
    ) -> Option<crate::type_checker::context::MethodInfo> {
        if let Some(m) = def.methods.get("init") {
            Some(m.clone())
        } else {
            let mut found = None;
            let mut base = def.base_class.clone();
            while let Some(bname) = base {
                match self.global_type_definitions.get(&bname) {
                    Some(TypeDefinition::Class(b)) => {
                        if let Some(m) = b.methods.get("init") {
                            found = Some(m.clone());
                            break;
                        }
                        base = b.base_class.clone();
                    }
                    _ => break,
                }
            }
            found
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Mirrors `lower_class_constructor`'s positional/named arg mapping for
    /// classes whose declaration has no `init` method: each constructor
    /// argument feeds directly into a declared field (walking the inheritance
    /// chain via `collect_class_fields_all`). Generic class parameters are
    /// substituted from `type_args` so the comparison sees the concrete
    /// element type (e.g. `data: List<T>` becomes `List<float>` when the
    /// instantiation is `Wrap<float>`).
    fn validate_class_field_args(
        &mut self,
        def: &crate::type_checker::context::ClassDefinition,
        positional_args: &[(&Expression, Type)],
        named_args: &mut HashMap<String, (&Expression, Type, Span)>,
        type_args: &Option<Vec<Expression>>,
        span: Span,
        context: &mut Context,
    ) {
        let mut generic_map = HashMap::new();
        if let Some(generics) = &def.generics {
            if let Some(targs) = type_args {
                for (g, ta) in generics.iter().zip(targs.iter()) {
                    let arg_ty = if self.extract_type_from_expression(ta).is_ok() {
                        self.resolve_type_expression(ta, context)
                    } else {
                        crate::type_checker::generics::value_generic_marker_type(ta.clone())
                    };
                    generic_map.insert(g.name.clone(), arg_ty);
                }
            }
        }

        let all_fields: Vec<(String, crate::type_checker::context::FieldInfo)> =
            crate::type_checker::context::collect_class_fields_all(
                def,
                &self.global_type_definitions,
            )
            .into_iter()
            .map(|(n, f)| (n.to_string(), f.clone()))
            .collect();

        let mut pos_iter = positional_args.iter();

        for (field_name, field_info) in &all_fields {
            let concrete_field_type = if generic_map.is_empty() {
                field_info.ty.clone()
            } else {
                self.substitute_type(&field_info.ty, &generic_map)
            };

            let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                (Some(*expr), Some(ty.clone()))
            } else if let Some((expr, ty, _)) = named_args.remove(field_name.as_str()) {
                (Some(expr), Some(ty))
            } else {
                // Field omitted from the constructor: lowering supplies a
                // default value, so don't surface a type-mismatch here.
                continue;
            };

            if let Some(arg_type) = arg_type {
                if matches!(arg_type.kind, TypeKind::Error)
                    || matches!(concrete_field_type.kind, TypeKind::Error)
                {
                    continue;
                }
                if !self.are_compatible(&concrete_field_type, &arg_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for field '{}': expected {}, got {}",
                            field_name, concrete_field_type, arg_type
                        ),
                        arg_expr.map(|e| e.span).unwrap_or(span),
                    );
                }
            }
        }

        if pos_iter.next().is_some() {
            self.report_error(
                format!(
                    "Too many arguments for '{}' constructor: expected {}, got {}",
                    def.name,
                    all_fields.len(),
                    positional_args.len()
                ),
                span,
            );
        }

        for (arg_name, (_, _, arg_span)) in named_args.drain() {
            self.report_error(format!("Unknown argument '{}'", arg_name), arg_span);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_class_init_args(
        &mut self,
        def: &crate::type_checker::context::ClassDefinition,
        init_method: &crate::type_checker::context::MethodInfo,
        positional_args: &[(&Expression, Type)],
        named_args: &mut HashMap<String, (&Expression, Type, Span)>,
        type_args: &Option<Vec<Expression>>,
        span: Span,
        context: &mut Context,
    ) {
        let mut generic_map = HashMap::new();
        if let Some(generics) = &def.generics {
            if let Some(targs) = type_args {
                for (g, ta) in generics.iter().zip(targs.iter()) {
                    let arg_ty = if self.extract_type_from_expression(ta).is_ok() {
                        self.resolve_type_expression(ta, context)
                    } else {
                        crate::type_checker::generics::value_generic_marker_type(ta.clone())
                    };
                    generic_map.insert(g.name.clone(), arg_ty);
                }
            }
        }

        let mut pos_iter = positional_args.iter();

        for (param_name, param_type) in &init_method.params {
            let concrete_param_type = if !generic_map.is_empty() {
                self.substitute_type(param_type, &generic_map)
            } else {
                param_type.clone()
            };

            let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                (Some(*expr), Some(ty.clone()))
            } else if let Some((expr, ty, _)) = named_args.remove(param_name) {
                (Some(expr), Some(ty))
            } else {
                (None, None)
            };

            if let Some(arg_type) = arg_type {
                if !self.are_compatible(&concrete_param_type, &arg_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for argument '{}': expected {}, got {}",
                            param_name, concrete_param_type, arg_type
                        ),
                        arg_expr.map(|e| e.span).unwrap_or(span),
                    );
                }
            } else {
                self.report_error(
                    format!("Missing argument for parameter '{}'", param_name),
                    span,
                );
            }
        }

        if pos_iter.next().is_some() {
            self.report_error(
                format!(
                    "Too many arguments for '{}' constructor: expected {}, got {}",
                    def.name,
                    init_method.params.len(),
                    positional_args.len()
                ),
                span,
            );
        }

        for (arg_name, (_, _, arg_span)) in named_args.drain() {
            self.report_error(format!("Unknown argument '{}'", arg_name), arg_span);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_struct_constructor(
        &mut self,
        def: crate::type_checker::context::StructDefinition,
        name: &str,
        type_args: &Option<Vec<Expression>>,
        positional_args: &[(&Expression, Type)],
        mut named_args: HashMap<String, (&Expression, Type, Span)>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let mut pos_iter = positional_args.iter();
        let mut generic_map = HashMap::new();

        // Seed the generic map from explicit type arguments (`Vec3<u32>(..)`)
        // so they pin the type parameters. `infer_generic_types` only fills
        // *absent* entries, so an explicit width wins over the type a literal
        // argument would otherwise infer (e.g. `u32` over the default `Int`).
        if let (Some(gens), Some(args)) = (&def.generics, type_args) {
            for (g, arg) in gens.iter().zip(args.iter()) {
                if let ExpressionKind::Type(ty, _) = &arg.node {
                    generic_map.insert(g.name.clone(), (**ty).clone());
                }
            }
        }

        for (field_name, field_type, _) in &def.fields {
            let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                (Some(*expr), Some(ty.clone()))
            } else if let Some((expr, ty, _)) = named_args.remove(field_name) {
                (Some(expr), Some(ty))
            } else {
                (None, None)
            };

            if let Some(arg_type) = arg_type {
                if def.generics.is_some() {
                    self.infer_generic_types(field_type, &arg_type, &mut generic_map);
                }

                let concrete_field_type = if def.generics.is_some() {
                    self.substitute_type(field_type, &generic_map)
                } else {
                    field_type.clone()
                };

                if !self.are_compatible(&concrete_field_type, &arg_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for field '{}': expected {}, got {}",
                            field_name, concrete_field_type, arg_type
                        ),
                        arg_expr.map(|e| e.span).unwrap_or(span),
                    );
                }
            } else {
                self.report_error(format!("Missing argument for field '{}'", field_name), span);
            }
        }

        if pos_iter.next().is_some() {
            self.report_error(
                format!(
                    "Too many positional arguments for struct constructor: expected {}, got {}",
                    def.fields.len(),
                    positional_args.len()
                ),
                span,
            );
        }

        for (field_name, (_, _, arg_span)) in named_args {
            self.report_error(format!("Unknown field '{}'", field_name), arg_span);
        }

        let generic_args = def.generics.as_ref().map(|gens| {
            gens.iter()
                .map(|g| {
                    generic_map
                        .get(&g.name)
                        .cloned()
                        .unwrap_or(make_type(TypeKind::Error))
                })
                .map(|t| self.create_type_expression(t))
                .collect()
        });

        make_type(TypeKind::Custom(name.to_string(), generic_args))
    }

    fn try_infer_array_constructor(
        &mut self,
        name: &str,
        type_args: &Option<Vec<Expression>>,
        positional_args: &[(&Expression, Type)],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        if BuiltinCollectionKind::from_name(name) != Some(BuiltinCollectionKind::Array) {
            return None;
        }

        if let Some(args) = type_args {
            if args.len() == 2 {
                let elem_type = self.resolve_type_expression(&args[0], context);
                let size_expr = args[1].clone();

                // Validate that the size expression is a compile-time constant
                if TypeChecker::try_eval_const_int(&size_expr).is_none() {
                    self.report_error(
                        "Array<T, N>() requires a compile-time constant size; use integer literals or simple arithmetic like '4 * 4'".to_string(),
                        size_expr.span,
                    );
                    return Some(make_type(TypeKind::Error));
                }

                // Reject managed element types at type-check time
                if is_perceus_managed(&elem_type.kind, &self.global_type_definitions) {
                    self.report_error(
                        format!(
                            "Array<T, N>() is not yet supported for managed element type '{}'; use an array literal",
                            elem_type
                        ),
                        args[0].span,
                    );
                    return Some(make_type(TypeKind::Error));
                }

                return Some(make_type(TypeKind::Custom(
                    "Array".to_string(),
                    Some(vec![self.create_type_expression(elem_type), size_expr]),
                )));
            } else {
                self.report_error(
                    format!(
                        "Class 'Array<T, N>' expects 2 generic arguments, got {}",
                        args.len()
                    ),
                    span,
                );
                return Some(make_type(TypeKind::Error));
            }
        }

        // Array() with no arguments but explicit type args should have been caught above.
        // If we get here with positional args, it's an error.
        if !positional_args.is_empty() {
            self.report_error(
                "Array(...) with a positional argument is not supported. Use Array<T, N>() with explicit type arguments for a sized array, or use an array literal like [1, 2, 3]".to_string(),
                span,
            );
            return Some(make_type(TypeKind::Error));
        }

        self.report_error(
            "Cannot instantiate generic class 'Array<T, N>' without explicit type arguments"
                .to_string(),
            span,
        );
        Some(make_type(TypeKind::Error))
    }
}
