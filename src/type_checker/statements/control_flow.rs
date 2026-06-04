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

        let start_state = context.snapshot_linear_state();
        let then_state = self.check_if_then_branch(then_block, context);
        context.restore_linear_state(start_state);
        let else_state = self.check_if_else_branch(else_block, context);
        self.validate_and_merge_if_branches(&then_state, &else_state, cond.span, context);
    }

    fn check_if_then_branch(
        &mut self,
        then_block: &Statement,
        context: &mut Context,
    ) -> Vec<Vec<(String, bool)>> {
        context.enter_scope();
        self.check_statement(then_block, context);
        let unconsumed_then = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed_then {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }
        context.exit_scope();
        context.snapshot_linear_state()
    }

    fn check_if_else_branch(
        &mut self,
        else_block: &Option<Box<Statement>>,
        context: &mut Context,
    ) -> Vec<Vec<(String, bool)>> {
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
        context.snapshot_linear_state()
    }

    fn validate_and_merge_if_branches(
        &mut self,
        then_state: &[Vec<(String, bool)>],
        else_state: &[Vec<(String, bool)>],
        cond_span: Span,
        context: &mut Context,
    ) {
        for (scope_idx, scope) in then_state.iter().enumerate() {
            if scope_idx >= else_state.len() {
                break;
            }
            let else_scope = &else_state[scope_idx];

            for (name, consumed_then) in scope {
                if let Some((_, consumed_else)) = else_scope.iter().find(|(n, _)| n == name) {
                    if *consumed_then != *consumed_else {
                        self.report_error(
                             format!(
                                 "Linear variable '{}' is consumed in one branch but not the other. Linear variables must be consistently consumed.",
                                 name
                             ),
                             cond_span,
                         );
                    }
                }
            }
        }
        context.restore_linear_state(then_state.to_vec());
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

    /// Type-checks a `gpu for <ident> in <range>` statement (1D) or
    /// `gpu for x, y in <range1>, <range2>` statement (2D, N1).
    ///
    /// Restrictions enforced beyond `check_for`:
    /// - The iterable must be a numeric range (`a..b` or `a..=b`).
    /// - For 1D: single loop variable; for 2D: exactly two loop variables.
    /// - The range start(s) must be an integer literal (variable starts are a follow-up).
    /// - The range end(s) may be a runtime Int expression (F1 feature).
    ///   Non-literal ends are lowered to uniform buffers in the MIR kernel.
    /// - The loop body is checked with `context.in_gpu_function = true`, so
    ///   discarded values and variable types are validated against
    ///   [`is_gpu_compatible`](crate::type_checker::utils::is_gpu_compatible).
    /// - `break` / `continue` in the body's immediate scope are rejected:
    ///   the GPU dispatch is not an iterative loop, so loop-control statements
    ///   have no meaning at the kernel level. Nested CPU `for`/`while` inside
    ///   the body still permit them via their own `enter_loop`.
    pub(crate) fn check_gpu_for(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
        stmt_span: Span,
    ) {
        match decls.len() {
            1 => self.check_gpu_for_1d(decls, iterable, body, context, stmt_span),
            2 => self.check_gpu_for_2d(decls, iterable, body, context, stmt_span),
            _ => {
                self.report_error(
                    "gpu for requires 1 or 2 loop variables".to_string(),
                    stmt_span,
                );
            }
        }
    }

    fn check_gpu_for_1d(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
        _stmt_span: Span,
    ) {
        let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
            self.report_error(
                "'gpu for' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                iterable.span,
            );
            return;
        };
        if !matches!(
            range_type,
            RangeExpressionType::Exclusive | RangeExpressionType::Inclusive
        ) {
            self.report_error(
                "'gpu for' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                iterable.span,
            );
            return;
        }
        if !is_int_literal(start) {
            self.report_error(
                "'gpu for' requires Int-literal range start (variable start is a follow-up)"
                    .to_string(),
                iterable.span,
            );
            return;
        }

        // End can be a runtime Int expression (F1 feature).
        // Type-check it and validate it is Int or gpu-compatible.
        let end_type = self.infer_expression(end, context);
        if !matches!(end_type.kind, TypeKind::Int) {
            self.report_error(
                format!("'gpu for' range end must be Int, got {}", end_type.kind),
                end.span,
            );
            return;
        }

        let iterable_type = self.infer_expression(iterable, context);
        let element_type = self.get_iterable_element_type(&iterable_type, iterable.span);

        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        self.bind_loop_variables(decls, &element_type, &iterable_type, iterable.span, context);
        self.check_statement(body, context);
        self.check_gpu_for_captures(decls, body, context);

        context.gpu_for_depth -= 1;
        context.in_gpu_function = outer_in_gpu;

        let unconsumed = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }

        context.exit_scope();
    }

    fn check_gpu_for_2d(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
        _stmt_span: Span,
    ) {
        let ExpressionKind::Tuple(ranges) = &iterable.node else {
            self.report_error(
                "2D gpu for requires two comma-separated ranges".to_string(),
                iterable.span,
            );
            return;
        };

        if ranges.len() != 2 {
            self.report_error(
                "2D gpu for requires exactly two ranges".to_string(),
                iterable.span,
            );
            return;
        }

        // Type check both ranges
        for (i, range_expr) in ranges.iter().enumerate() {
            let ExpressionKind::Range(start, Some(end), range_type) = &range_expr.node else {
                self.report_error(
                    "'gpu for' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                    range_expr.span,
                );
                return;
            };
            if !matches!(
                range_type,
                RangeExpressionType::Exclusive | RangeExpressionType::Inclusive
            ) {
                self.report_error(
                    "'gpu for' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                    range_expr.span,
                );
                return;
            }
            if !is_int_literal(start) {
                self.report_error(
                    format!(
                        "'gpu for' range {} start must be an Int literal (variable start is a follow-up)",
                        if i == 0 { "x" } else { "y" }
                    ),
                    range_expr.span,
                );
                return;
            }

            // 2D gpu for requires literal bounds (N1a will support variable bounds)
            if !is_int_literal(end) {
                self.report_error(
                    "2D gpu for requires literal bounds (variable bounds are N1a follow-up)"
                        .to_string(),
                    end.span,
                );
                return;
            }

            let end_type = self.infer_expression(end, context);
            if !matches!(end_type.kind, TypeKind::Int) {
                self.report_error(
                    format!(
                        "'gpu for' range {} end must be Int, got {}",
                        if i == 0 { "x" } else { "y" },
                        end_type.kind
                    ),
                    end.span,
                );
                return;
            }
        }

        // Bind both loop variables as Int
        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        let int_type = make_type(TypeKind::Int);
        for decl in decls {
            let var_type = if let Some(type_expr) = &decl.typ {
                let declared_type = self.resolve_type_expression(type_expr, context);
                if !self.are_compatible(&declared_type, &int_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for loop variable '{}': expected Int, got {}",
                            decl.name, declared_type
                        ),
                        type_expr.span,
                    );
                }
                declared_type
            } else {
                int_type.clone()
            };
            let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);
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
        }

        self.check_statement(body, context);
        self.check_gpu_for_captures(decls, body, context);

        context.gpu_for_depth -= 1;
        context.in_gpu_function = outer_in_gpu;

        let unconsumed = context.get_unconsumed_linear_vars();
        for (name, span) in unconsumed {
            self.report_error(
                format!("Linear variable '{}' must be consumed exactly once", name),
                span,
            );
        }

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
        match decls.len() {
            1 => self.bind_single_loop_variable(&decls[0], element_type, context),
            2 => self.bind_pair_loop_variables(decls, element_type, iterable_type, span, context),
            _ => self.report_error("Invalid number of loop variables".to_string(), span),
        }
    }

    fn bind_single_loop_variable(
        &mut self,
        decl: &VariableDeclaration,
        element_type: &Type,
        context: &mut Context,
    ) {
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
        let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);
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
    }

    fn bind_pair_loop_variables(
        &mut self,
        decls: &[VariableDeclaration],
        element_type: &Type,
        iterable_type: &Type,
        span: Span,
        context: &mut Context,
    ) {
        if let TypeKind::Tuple(exprs) = &element_type.kind {
            self.bind_tuple_destructure(decls, exprs, span, context);
        } else if matches!(&iterable_type.kind, TypeKind::Custom(_name, _) if iterable_type.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map))
        {
            self.bind_map_iteration(decls, element_type, iterable_type, context);
        } else {
            self.bind_sequence_with_index(decls, element_type, context);
        }
    }

    fn bind_tuple_destructure(
        &mut self,
        decls: &[VariableDeclaration],
        exprs: &[Expression],
        span: Span,
        context: &mut Context,
    ) {
        if exprs.len() != 2 {
            self.report_error(
                "Destructuring mismatch: expected tuple of size 2".to_string(),
                span,
            );
            return;
        }
        let key_type = self
            .extract_type_from_expression(&exprs[0])
            .unwrap_or(make_type(TypeKind::Error));
        let val_type = self
            .extract_type_from_expression(&exprs[1])
            .unwrap_or(make_type(TypeKind::Error));

        let is_mutable_0 = matches!(decls[0].declaration_type, VariableDeclarationType::Mutable);
        let is_mutable_1 = matches!(decls[1].declaration_type, VariableDeclarationType::Mutable);

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
    }

    fn bind_map_iteration(
        &mut self,
        decls: &[VariableDeclaration],
        element_type: &Type,
        iterable_type: &Type,
        context: &mut Context,
    ) {
        let val_type = match &iterable_type.kind {
            TypeKind::Map(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map)
                    && args.len() == 2 =>
            {
                self.extract_type_from_expression(&args[1])
                    .unwrap_or_else(|_| make_type(TypeKind::Error))
            }
            _ => make_type(TypeKind::Error),
        };

        let is_mutable_0 = matches!(decls[0].declaration_type, VariableDeclarationType::Mutable);
        let is_mutable_1 = matches!(decls[1].declaration_type, VariableDeclarationType::Mutable);

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
    }

    fn bind_sequence_with_index(
        &mut self,
        decls: &[VariableDeclaration],
        element_type: &Type,
        context: &mut Context,
    ) {
        let is_mutable_0 = matches!(decls[0].declaration_type, VariableDeclarationType::Mutable);
        let is_mutable_1 = matches!(decls[1].declaration_type, VariableDeclarationType::Mutable);

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

    pub(crate) fn check_break(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            let msg = if context.gpu_for_depth > 0 {
                "'break' is not supported inside a 'gpu for' body: the GPU dispatch is not an iterative loop, so loop-control statements have no meaning at the kernel level"
            } else {
                "Break statement outside of loop"
            };
            self.report_error(msg.to_string(), span);
        }
    }

    pub(crate) fn check_continue(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            let msg = if context.gpu_for_depth > 0 {
                "'continue' is not supported inside a 'gpu for' body: the GPU dispatch is not an iterative loop, so loop-control statements have no meaning at the kernel level"
            } else {
                "Continue statement outside of loop"
            };
            self.report_error(msg.to_string(), span);
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

fn is_int_literal(expr: &Expression) -> bool {
    matches!(
        &expr.node,
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_))
    )
}
