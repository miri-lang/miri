// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement type checking for the type checker.
//!
//! This module implements type checking for all statement kinds in Miri.
//! The main entry point is [`TypeChecker::check_statement`], which validates
//! statements and registers type information in the context.
//!
//! # Supported Statements
//!
//! ## Declarations
//! - Variable declarations: `let x = 1`, `var y: int = 2`
//! - Function declarations with generics and return type validation
//! - Struct, enum, class, and trait definitions
//! - Type aliases
//!
//! ## Control Flow
//! - If/else statements with condition type checking
//! - While loops (including forever loops)
//! - For loops with iterator type inference
//! - Match statements with exhaustiveness checking
//! - Return statements with type compatibility validation
//!
//! ## Expressions
//! - Expression statements (side effects)
//! - Assignment validation
//!
//! ## Type Definitions
//! - Structs with fields and generic parameters
//! - Enums with variants and associated values
//! - Classes with fields, methods, and inheritance
//! - Traits with method signatures
//!
//! # Return Type Analysis
//!
//! The module includes return status analysis (`check_returns`) to determine:
//! - Whether all code paths return a value
//! - Implicit vs explicit returns
//! - Return type compatibility

use crate::ast::factory::make_type;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_block(&mut self, stmts: &[Statement], context: &mut Context) {
        context.enter_scope();
        for s in stmts {
            self.check_statement(s, context);
        }

        // Check for unconsumed linear variables
        let unconsumed = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }

        context.exit_scope();
    }

    pub(crate) fn check_if(
        &mut self,
        cond: &Expression,
        then_block: &Statement,
        else_block: &Option<Box<Statement>>,
        context: &mut Context,
    ) {
        let cond_type = self.infer_expression(cond, context);
        if !matches!(cond_type.kind, TypeKind::Boolean) {
            self.report_error(
                format!("If condition must be a boolean, got {}", cond_type),
                cond.span,
            );
        }

        // Snapshot state before branching
        let start_state = context.snapshot_linear_state();

        // Check Then block
        context.enter_scope();
        self.check_statement(then_block, context);
        // Check for unconsumed locals in then block
        let unconsumed_then = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed_then {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }
        context.exit_scope();

        let then_state = context.snapshot_linear_state();

        // Restore state for Else block
        context.restore_linear_state(start_state);

        if let Some(else_stmt) = else_block {
            context.enter_scope();
            self.check_statement(else_stmt, context);
            let unconsumed_else = context.get_unconsumed_linear_vars();
            for (name, span) in unconsumed_else {
                self.report_error(
                    format!("Linear variable '{}' must be consumed exactly once", name),
                    span,
                );
            }
            context.exit_scope();
        }

        let else_state = context.snapshot_linear_state();

        // Merge and Validate
        // For a linear variable defined outside implementation of the blocks:
        // If it was consumed in one branch, it must be consumed in the other.
        for (scope_idx, scope) in then_state.iter().enumerate() {
            if scope_idx >= else_state.len() {
                break;
            }
            let else_scope = &else_state[scope_idx];

            for (name, consumed_then) in scope {
                // Find corresponding var in else_scope
                if let Some((_, consumed_else)) = else_scope.iter().find(|(n, _)| n == name) {
                    if *consumed_then != *consumed_else {
                        // We found a mismatch.
                        // We need a span to report the error.
                        // Ideally we point to the if statement or the variable.
                        // We don't have the variable span handy easily here.
                        // Use 'cond.span' as a proxy for the if statement.
                        self.report_error(
                             format!(
                                 "Linear variable '{}' is consumed in one branch but not the other. Linear variables must be consistently consumed.",
                                 name
                             ),
                             cond.span,
                         );
                    }
                }
            }
        }

        // Finalize state: If consistent, set to consumed (which is true in both).
        context.restore_linear_state(then_state);
    }

    pub(crate) fn check_while(
        &mut self,
        cond: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        let cond_type = self.infer_expression(cond, context);
        if !matches!(cond_type.kind, TypeKind::Boolean) {
            self.report_error(
                format!("While condition must be a boolean, got {}", cond_type),
                cond.span,
            );
        }
        context.enter_scope();
        context.enter_loop();
        self.check_statement(body, context);

        let unconsumed = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }

        context.exit_loop();
        context.exit_scope();
    }

    pub(crate) fn check_for(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        let iterable_type = self.infer_expression(iterable, context);
        let element_type = self.get_iterable_element_type(&iterable_type, iterable.span);

        context.enter_scope();
        context.enter_loop();

        self.bind_loop_variables(decls, &element_type, &iterable_type, iterable.span, context);

        self.check_statement(body, context);

        let unconsumed = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }

        context.exit_loop();
        context.exit_scope();
    }

    pub(crate) fn bind_loop_variables(
        &mut self,
        decls: &[VariableDeclaration],
        element_type: &Type,
        iterable_type: &Type,
        span: Span,
        context: &mut Context,
    ) {
        if decls.len() == 1 {
            let decl = &decls[0];
            let var_type = if let Some(type_expr) = &decl.typ {
                let declared_type = self.resolve_type_expression(type_expr, context);
                if !self.are_compatible(&declared_type, element_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for loop variable '{}': expected {}, got {}",
                            decl.name, declared_type, element_type
                        ),
                        type_expr.span,
                    );
                }
                declared_type
            } else {
                element_type.clone()
            };
            let is_mutable = match decl.declaration_type {
                VariableDeclarationType::Mutable => true,
                VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
            };
            context.define(
                decl.name.clone(),
                SymbolInfo::new(
                    var_type,
                    is_mutable,
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );
        } else if decls.len() == 2 {
            if let TypeKind::Tuple(exprs) = &element_type.kind {
                if exprs.len() == 2 {
                    let key_type = self
                        .extract_type_from_expression(&exprs[0])
                        .unwrap_or(make_type(TypeKind::Error));
                    let val_type = self
                        .extract_type_from_expression(&exprs[1])
                        .unwrap_or(make_type(TypeKind::Error));

                    let is_mutable_0 = match decls[0].declaration_type {
                        VariableDeclarationType::Mutable => true,
                        VariableDeclarationType::Immutable | VariableDeclarationType::Constant => {
                            false
                        }
                    };
                    let is_mutable_1 = match decls[1].declaration_type {
                        VariableDeclarationType::Mutable => true,
                        VariableDeclarationType::Immutable | VariableDeclarationType::Constant => {
                            false
                        }
                    };

                    context.define(
                        decls[0].name.clone(),
                        SymbolInfo::new(
                            key_type,
                            is_mutable_0,
                            false,
                            MemberVisibility::Public,
                            self.current_module.clone(),
                            None,
                        ),
                    );
                    context.define(
                        decls[1].name.clone(),
                        SymbolInfo::new(
                            val_type,
                            is_mutable_1,
                            false,
                            MemberVisibility::Public,
                            self.current_module.clone(),
                            None,
                        ),
                    );
                } else {
                    self.report_error(
                        "Destructuring mismatch: expected tuple of size 2".to_string(),
                        span,
                    );
                }
            } else if matches!(&iterable_type.kind, TypeKind::Custom(_name, _) if iterable_type.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map))
            {
                // For Map iterables, `for k, v in map` means: k = key, v = value.
                let val_type = match &iterable_type.kind {
                    TypeKind::Map(_, _) => {
                        unreachable!("collection types are normalized to Custom before this point")
                    }
                    TypeKind::Custom(name, Some(args))
                        if BuiltinCollectionKind::from_name(name)
                            == Some(BuiltinCollectionKind::Map)
                            && args.len() == 2 =>
                    {
                        self.extract_type_from_expression(&args[1])
                            .unwrap_or_else(|_| make_type(TypeKind::Error))
                    }
                    _ => make_type(TypeKind::Error),
                };

                let is_mutable_0 = match decls[0].declaration_type {
                    VariableDeclarationType::Mutable => true,
                    VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
                };
                let is_mutable_1 = match decls[1].declaration_type {
                    VariableDeclarationType::Mutable => true,
                    VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
                };

                context.define(
                    decls[0].name.clone(),
                    SymbolInfo::new(
                        element_type.clone(),
                        is_mutable_0,
                        false,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                        None,
                    ),
                );
                context.define(
                    decls[1].name.clone(),
                    SymbolInfo::new(
                        val_type,
                        is_mutable_1,
                        false,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                        None,
                    ),
                );
            } else {
                // For non-tuple iterables (List, Array, String), the pattern
                // `for x, idx in list` means: x = element, idx = loop index (int).
                let is_mutable_0 = match decls[0].declaration_type {
                    VariableDeclarationType::Mutable => true,
                    VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
                };
                let is_mutable_1 = match decls[1].declaration_type {
                    VariableDeclarationType::Mutable => true,
                    VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
                };

                context.define(
                    decls[0].name.clone(),
                    SymbolInfo::new(
                        element_type.clone(),
                        is_mutable_0,
                        false,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                        None,
                    ),
                );
                context.define(
                    decls[1].name.clone(),
                    SymbolInfo::new(
                        make_type(TypeKind::Int),
                        is_mutable_1,
                        false,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                        None,
                    ),
                );
            }
        } else {
            self.report_error("Invalid number of loop variables".to_string(), span);
        }
    }

    pub(crate) fn check_break(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            self.report_error("Break statement outside of loop".to_string(), span);
        }
    }

    pub(crate) fn check_continue(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            self.report_error("Continue statement outside of loop".to_string(), span);
        }
    }

    pub(crate) fn check_return(
        &mut self,
        expr_opt: &Option<Box<Expression>>,
        context: &mut Context,
        span: Span,
    ) {
        let (actual_return_type, return_span) = if let Some(expr) = expr_opt {
            (self.infer_expression(expr, context), expr.span)
        } else {
            (make_type(TypeKind::Void), span)
        };

        // Check if we are inferring return types for the current function
        if let Some(Some(inferred_types)) = context.inferred_return_types.last_mut() {
            inferred_types.push((actual_return_type, return_span));
            return;
        }

        let expected_return_type = context
            .return_types
            .last()
            .unwrap_or(&make_type(TypeKind::Void))
            .clone();

        if !self.are_compatible(&expected_return_type, &actual_return_type, context) {
            self.report_error(
                format!(
                    "Invalid return type: expected {}, got {}",
                    expected_return_type, actual_return_type
                ),
                return_span,
            );
        }
    }
}
