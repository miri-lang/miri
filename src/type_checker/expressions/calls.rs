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

                        if !self.are_compatible(&concrete_param_type, &arg_type, context) {
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
                    let type_def = context
                        .resolve_type_definition(name)
                        .cloned()
                        .or_else(|| self.global_type_definitions.get(name).cloned());

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

                        // Special handling for List constructor: List([1,2,3]) → TypeKind::List(int)
                        // This produces a built-in List type so the existing codegen
                        // infrastructure for indexing, for-loops, and bounds checking works.
                        if BuiltinCollectionKind::from_name(name)
                            == Some(BuiltinCollectionKind::List)
                        {
                            // Check explicit template args
                            if let Some(args) = type_args {
                                if args.len() == 1 {
                                    let elem_type = self.resolve_type_expression(&args[0], context);
                                    return make_type(TypeKind::List(Box::new(
                                        self.create_type_expression(elem_type),
                                    )));
                                } else {
                                    self.report_error(
                                        format!(
                                            "Class 'List<T>' expects 1 generic arguments, got {}",
                                            args.len()
                                        ),
                                        span,
                                    );
                                    return make_type(TypeKind::Error);
                                }
                            }

                            // Infer element type from the argument (expected to be an array)
                            if let Some((_, arg_type)) = positional_args.first() {
                                let elem_type = match &arg_type.kind {
                                    TypeKind::Array(inner, _) | TypeKind::List(inner) => {
                                        self.resolve_type_expression(inner, context)
                                    }
                                    _ => arg_type.clone(),
                                };
                                return make_type(TypeKind::List(Box::new(
                                    self.create_type_expression(elem_type),
                                )));
                            }

                            self.report_error(
                                "Cannot instantiate generic class 'List<T>' without explicit type arguments".to_string(),
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

                        // Class constructors are handled via init method
                        // For now, just return the class type
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
