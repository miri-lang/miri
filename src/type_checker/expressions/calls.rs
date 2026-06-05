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
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::mir::MathIntrinsic;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::utils::is_gpu_compatible;
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
            self.check_gpu_call_types(&positional_args);
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

    /// Checks GPU compatibility of call arguments.
    fn check_gpu_call_types(&mut self, positional_args: &[(&Expression, Type)]) {
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
    }

    /// Rejects a gpu-resident binding passed as an argument to a host call
    /// (GPU_DRAFT §6.4). A host function reads its arguments on the host, so a
    /// gpu-resident value would need an implicit readback — the design forbids
    /// any implicit fence outside cross-residency assignment. The reader must
    /// copy to host first (`let h = g`) and pass the copy.
    fn reject_gpu_resident_call_args(
        &mut self,
        func: &Expression,
        args: &[Expression],
        context: &Context,
    ) {
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

                self.validate_class_generics(def, name, type_args, span);

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
                    positional_args,
                    named_args,
                    span,
                    context,
                );
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

    fn infer_struct_constructor(
        &mut self,
        def: crate::type_checker::context::StructDefinition,
        name: &str,
        positional_args: &[(&Expression, Type)],
        mut named_args: HashMap<String, (&Expression, Type, Span)>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let mut pos_iter = positional_args.iter();
        let mut generic_map = HashMap::new();

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
}
