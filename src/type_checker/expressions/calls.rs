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
use crate::type_checker::context::{Context, TypeDefinition};
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
                        named_args.insert(name.clone(), (value, ty, arg.span));
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

        match &func_type.kind {
            TypeKind::Function(func_data) => {
                let mut generic_map = std::collections::HashMap::new();

                if let Some(gens) = &func_data.generics {
                    context.enter_scope();
                    self.define_generics(gens, context);
                }

                let mut pos_iter = positional_args.iter();
                let mut seen_out_vars: std::collections::HashSet<String> =
                    std::collections::HashSet::new();

                for param in &func_data.params {
                    let param_type = self.resolve_type_expression(&param.typ, context);

                    let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                        (Some(*expr), Some(ty.clone()))
                    } else if let Some((expr, ty, _)) = named_args.remove(&param.name) {
                        (Some(&**expr), Some(ty))
                    } else {
                        (None, None)
                    };

                    if let Some(arg_type) = arg_type {
                        if func_data.generics.is_some() {
                            self.infer_generic_types(&param_type, &arg_type, &mut generic_map);
                        }

                        let concrete_param_type = if func_data.generics.is_some() {
                            self.substitute_type(&param_type, &generic_map)
                        } else {
                            param_type.clone()
                        };

                        if param.is_out {
                            let arg_span = arg_expr.map(|e| e.span).unwrap_or(span);
                            match arg_expr.map(|e| &e.node) {
                                Some(ExpressionKind::Identifier(var_name, _)) => {
                                    if !context.is_mutable(var_name) {
                                        self.report_error(
                                            format!(
                                                "expected mutable variable for 'out' parameter '{}': '{}' is immutable (declare with 'var')",
                                                param.name, var_name
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
                                            param.name
                                        ),
                                        arg_span,
                                    );
                                }
                                None => {}
                            }

                            let exact_match = matches!(concrete_param_type.kind, TypeKind::Error)
                                || matches!(arg_type.kind, TypeKind::Error)
                                || concrete_param_type == arg_type;
                            if !exact_match {
                                self.report_error(
                                    format!(
                                        "Type mismatch for argument '{}': expected {}, got {}",
                                        param.name, concrete_param_type, arg_type
                                    ),
                                    arg_expr.map(|e| e.span).unwrap_or(span),
                                );
                            }
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

                for (name, (_, _, span)) in named_args {
                    self.report_error(format!("Unknown argument '{}'", name), span);
                }

                let return_type = if let Some(rt_expr) = &func_data.return_type {
                    let rt = self.resolve_type_expression(rt_expr, context);
                    if func_data.generics.is_some() {
                        self.substitute_type(&rt, &generic_map)
                    } else {
                        rt
                    }
                } else {
                    ast_factory::make_type(TypeKind::Void)
                };

                if func_data.generics.is_some() {
                    context.exit_scope();

                    // Store the call-site generic mapping so MIR lowering can mangle the name.
                    if !generic_map.is_empty() {
                        let ordered: Vec<(String, crate::ast::types::Type)> = if let Some(gens) =
                            &func_data.generics
                        {
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
                }

                // GPU kernels cannot call host functions.
                if context.in_gpu_function {
                    if let ExpressionKind::Identifier(name, _) = &func.node {
                        if name == "print" {
                            self.report_error(
                                "Host function 'print' cannot be called from a GPU kernel"
                                    .to_string(),
                                span,
                            );
                        }
                    }
                }

                return_type
            }
            TypeKind::Meta(inner_type) => {
                if let TypeKind::Custom(name, type_args) = &inner_type.kind {
                    let type_def = self.resolve_visible_type(name, context).cloned();

                    // Check for Class constructor
                    if let Some(TypeDefinition::Class(def)) = &type_def {
                        // Prevent instantiation of abstract classes
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

                        // Special handling for built-in collection constructors: always return
                        // the canonical TypeKind (List/Map/Set) rather than Custom("List", ...),
                        // eliminating the dual-representation problem at the source.
                        if BuiltinCollectionKind::from_name(name)
                            == Some(BuiltinCollectionKind::List)
                        {
                            // Check explicit template args
                            if let Some(args) = type_args {
                                if args.len() == 1 {
                                    let elem_type = self.resolve_type_expression(&args[0], context);
                                    return make_type(TypeKind::Custom(
                                        "List".to_string(),
                                        Some(vec![self.create_type_expression(elem_type)]),
                                    ));
                                } else {
                                    self.report_error(
                                        format!(
                                            "Class 'List<T>' expects 1 generic argument, got {}",
                                            args.len()
                                        ),
                                        span,
                                    );
                                    return make_type(TypeKind::Error);
                                }
                            }

                            // Element type must come from an Array or List literal.
                            // Other arg shapes (Set, scalar, struct, etc.) used to type-check
                            // here and crash at runtime because lowering treats the operand as
                            // raw element data; reject them explicitly instead.
                            if let Some((_, arg_type)) = positional_args.first() {
                                let elem_type = match &arg_type.kind {
                                    TypeKind::Custom(cname, Some(cargs))
                                        if (BuiltinCollectionKind::from_name(cname.as_str())
                                            == Some(BuiltinCollectionKind::Array)
                                            || BuiltinCollectionKind::from_name(
                                                cname.as_str(),
                                            ) == Some(BuiltinCollectionKind::List))
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
                                        return make_type(TypeKind::Error);
                                    }
                                };
                                return make_type(TypeKind::Custom(
                                    "List".to_string(),
                                    Some(vec![self.create_type_expression(elem_type)]),
                                ));
                            }

                            self.report_error(
                                "Cannot instantiate generic class 'List<T>' without explicit type arguments".to_string(),
                                span,
                            );
                            return make_type(TypeKind::Error);
                        }

                        if BuiltinCollectionKind::from_name(name)
                            == Some(BuiltinCollectionKind::Map)
                        {
                            if let Some(args) = type_args {
                                if args.len() == 2 {
                                    let k_type = self.resolve_type_expression(&args[0], context);
                                    let v_type = self.resolve_type_expression(&args[1], context);
                                    return make_type(TypeKind::Custom(
                                        "Map".to_string(),
                                        Some(vec![
                                            self.create_type_expression(k_type),
                                            self.create_type_expression(v_type),
                                        ]),
                                    ));
                                } else {
                                    self.report_error(
                                        format!(
                                            "Class 'Map<K, V>' expects 2 generic arguments, got {}",
                                            args.len()
                                        ),
                                        span,
                                    );
                                    return make_type(TypeKind::Error);
                                }
                            }
                            // Map(<map-literal>) populates the map from the literal.
                            // Lowering delegates to the map-literal lowering, so the arg
                            // must be a literal expression — accepting an arbitrary value
                            // of Map type would silently produce an empty map.
                            if let Some((arg_expr, arg_type)) = positional_args.first() {
                                if !matches!(&arg_expr.node, ExpressionKind::Map(_)) {
                                    self.report_error(
                                        "Map(...) only accepts a map literal argument like '{\"key\": value}'. Use 'Map<K, V>()' for an empty map".to_string(),
                                        span,
                                    );
                                    return make_type(TypeKind::Error);
                                }
                                if let TypeKind::Custom(cname, Some(cargs)) = &arg_type.kind {
                                    if BuiltinCollectionKind::from_name(cname.as_str())
                                        == Some(BuiltinCollectionKind::Map)
                                        && cargs.len() == 2
                                    {
                                        return make_type(TypeKind::Custom(
                                            "Map".to_string(),
                                            Some(cargs.clone()),
                                        ));
                                    }
                                }
                            }
                            self.report_error(
                                "Cannot instantiate generic class 'Map<K, V>' without explicit type arguments".to_string(),
                                span,
                            );
                            return make_type(TypeKind::Error);
                        }

                        if BuiltinCollectionKind::from_name(name)
                            == Some(BuiltinCollectionKind::Set)
                        {
                            if let Some(args) = type_args {
                                if args.len() == 1 {
                                    let elem_type = self.resolve_type_expression(&args[0], context);
                                    return make_type(TypeKind::Custom(
                                        "Set".to_string(),
                                        Some(vec![self.create_type_expression(elem_type)]),
                                    ));
                                } else {
                                    self.report_error(
                                        format!(
                                            "Class 'Set<T>' expects 1 generic argument, got {}",
                                            args.len()
                                        ),
                                        span,
                                    );
                                    return make_type(TypeKind::Error);
                                }
                            }
                            // Set(<set-literal>) populates the set from the literal.
                            // Lowering delegates to the set-literal lowering, so the arg
                            // must be a literal expression — accepting an arbitrary value
                            // of Set type would silently produce an empty set.
                            if let Some((arg_expr, arg_type)) = positional_args.first() {
                                if !matches!(&arg_expr.node, ExpressionKind::Set(_)) {
                                    self.report_error(
                                        "Set(...) only accepts a set literal argument like '{1, 2, 3}'. Use 'Set<T>()' for an empty set".to_string(),
                                        span,
                                    );
                                    return make_type(TypeKind::Error);
                                }
                                if let TypeKind::Custom(cname, Some(cargs)) = &arg_type.kind {
                                    if BuiltinCollectionKind::from_name(cname.as_str())
                                        == Some(BuiltinCollectionKind::Set)
                                        && !cargs.is_empty()
                                    {
                                        return make_type(TypeKind::Custom(
                                            "Set".to_string(),
                                            Some(cargs.clone()),
                                        ));
                                    }
                                }
                            }
                            self.report_error(
                                "Cannot instantiate generic class 'Set<T>' without explicit type arguments".to_string(),
                                span,
                            );
                            return make_type(TypeKind::Error);
                        }

                        // Validate generic constraints for class
                        if let Some(generics) = &def.generics {
                            let generic_names: Vec<String> =
                                generics.iter().map(|g| g.name.clone()).collect();
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
                                    format!("Cannot instantiate generic class '{}' without explicit type arguments", signature),
                                    span,
                                );
                            }
                        } else if type_args.is_some() {
                            self.report_error(
                                format!("Class '{}' does not take generic arguments", name),
                                span,
                            );
                        }

                        // Find the applicable `init`: the class's own, or the first one
                        // found walking up the base_class inheritance chain.
                        // Clone everything we need before any &mut self calls.
                        let init_method: Option<crate::type_checker::context::MethodInfo> =
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
                            };

                        if let Some(init_method) = init_method {
                            // Build a generic substitution map from the instantiation's type args.
                            let mut generic_map = HashMap::new();
                            if let Some(generics) = &def.generics {
                                if let Some(targs) = type_args {
                                    for (g, ta) in generics.iter().zip(targs.iter()) {
                                        let concrete = self.resolve_type_expression(ta, context);
                                        generic_map.insert(g.name.clone(), concrete);
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

                                let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next()
                                {
                                    (Some(*expr), Some(ty.clone()))
                                } else if let Some((expr, ty, _)) = named_args.remove(param_name) {
                                    (Some(&**expr), Some(ty))
                                } else {
                                    (None, None)
                                };

                                if let Some(arg_type) = arg_type {
                                    if !self.are_compatible(
                                        &concrete_param_type,
                                        &arg_type,
                                        context,
                                    ) {
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
                                        name,
                                        init_method.params.len(),
                                        positional_args.len()
                                    ),
                                    span,
                                );
                            }

                            for (arg_name, (_, _, arg_span)) in named_args {
                                self.report_error(
                                    format!("Unknown argument '{}'", arg_name),
                                    arg_span,
                                );
                            }
                        }

                        return make_type(TypeKind::Custom(name.clone(), type_args.clone()));
                    }

                    if let Some(TypeDefinition::Struct(def)) = type_def {
                        let mut pos_iter = positional_args.iter();
                        let mut generic_map = HashMap::new();

                        for (field_name, field_type, _) in &def.fields {
                            let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                                (Some(*expr), Some(ty.clone()))
                            } else if let Some((expr, ty, _)) = named_args.remove(field_name) {
                                (Some(&**expr), Some(ty))
                            } else {
                                (None, None)
                            };

                            if let Some(arg_type) = arg_type {
                                if def.generics.is_some() {
                                    self.infer_generic_types(
                                        field_type,
                                        &arg_type,
                                        &mut generic_map,
                                    );
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
                                self.report_error(
                                    format!("Missing argument for field '{}'", field_name),
                                    span,
                                );
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

                        for (name, (_, _, span)) in named_args {
                            self.report_error(format!("Unknown field '{}'", name), span);
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

                        return make_type(TypeKind::Custom(name.clone(), generic_args));
                    }
                }
                self.report_error(format!("Type '{}' is not callable", inner_type), span);
                make_type(TypeKind::Error)
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
}
