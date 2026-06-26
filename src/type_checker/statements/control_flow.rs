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

use crate::ast::captures::collect_free_identifiers_excluding;
use crate::ast::factory::make_type;
use crate::ast::statement;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::utils::is_residency_gated_buffer;
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

    pub(crate) fn check_forall(
        &mut self,
        device: &AcceleratorTarget,
        vars: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
        stmt_span: Span,
    ) {
        if let Some(accum_name) = detect_reduction(body, vars, context) {
            self.report_error(
                format!(
                    "loop-carried accumulator '{}' makes 'forall' iterations order-dependent; \
                    'forall' requires independent iterations (reductions are not yet supported)",
                    accum_name
                ),
                stmt_span,
            );
            return;
        }

        let target = resolve_forall_device(body, *device, vars, context);

        match target {
            ForallTarget::Gpu => self.check_gpu_for(vars, iterable, body, context, stmt_span),
            ForallTarget::Cpu => self.check_forall_cpu(vars, iterable, body, context),
            ForallTarget::GpuRequiredButAbsent => {
                self.check_gpu_for_captures(vars, body, context);
                self.report_error(
                    "'gpu forall' requires at least one gpu-resident buffer; none found (annotate data with 'gpu let')"
                        .to_string(),
                    stmt_span,
                );
            }
        }
    }

    fn check_forall_cpu(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        match decls.len() {
            1 => self.check_forall_cpu_1d(decls, iterable, body, context),
            2 => self.check_forall_cpu_2d(decls, iterable, body, context),
            3 => self.check_forall_cpu_3d(decls, iterable, body, context),
            _ => self.report_error(
                format!(
                    "forall: expected 1, 2, or 3 loop variables, got {}",
                    decls.len()
                ),
                iterable.span,
            ),
        }
    }

    fn check_forall_cpu_1d(
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

    fn check_forall_cpu_2d(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        let ExpressionKind::Tuple(ranges) = &iterable.node else {
            self.report_error(
                "2D forall requires two comma-separated ranges".to_string(),
                iterable.span,
            );
            return;
        };

        if ranges.len() != 2 {
            self.report_error(
                "2D forall requires exactly two ranges".to_string(),
                iterable.span,
            );
            return;
        }

        if !self.validate_cpu_forall_ranges(ranges, iterable.span, context) {
            return;
        }

        context.enter_scope();
        context.enter_loop();
        self.bind_cpu_forall_int_variables(decls, context);
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

    fn check_forall_cpu_3d(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        let ExpressionKind::Tuple(ranges) = &iterable.node else {
            self.report_error(
                "3D forall requires three comma-separated ranges".to_string(),
                iterable.span,
            );
            return;
        };

        if ranges.len() != 3 {
            self.report_error(
                "3D forall requires exactly three ranges".to_string(),
                iterable.span,
            );
            return;
        }

        if !self.validate_cpu_forall_ranges(ranges, iterable.span, context) {
            return;
        }

        context.enter_scope();
        context.enter_loop();
        self.bind_cpu_forall_int_variables(decls, context);
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

    fn validate_cpu_forall_ranges(
        &mut self,
        ranges: &[Expression],
        _span: Span,
        context: &mut Context,
    ) -> bool {
        for (i, range_expr) in ranges.iter().enumerate() {
            let ExpressionKind::Range(start, Some(end), _) = &range_expr.node else {
                self.report_error(
                    format!(
                        "forall dimension {}: range must be a bounded numeric range like '0..n'",
                        i
                    ),
                    range_expr.span,
                );
                return false;
            };

            let start_type = self.infer_expression(start, context);
            if !matches!(start_type.kind, TypeKind::Int) {
                self.report_error(
                    format!(
                        "forall dimension {}: range start must be Int, got {}",
                        i, start_type
                    ),
                    start.span,
                );
                return false;
            }

            let end_type = self.infer_expression(end, context);
            if !matches!(end_type.kind, TypeKind::Int) {
                self.report_error(
                    format!(
                        "forall dimension {}: range end must be Int, got {}",
                        i, end_type
                    ),
                    end.span,
                );
                return false;
            }
        }

        true
    }

    fn bind_cpu_forall_int_variables(
        &mut self,
        decls: &[VariableDeclaration],
        context: &mut Context,
    ) {
        for decl in decls {
            let var_type = Type::new(TypeKind::Int, Span::default());
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
    }

    fn check_gpu_for_3d(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
        _stmt_span: Span,
    ) {
        let ExpressionKind::Tuple(ranges) = &iterable.node else {
            self.report_error(
                "3D gpu forall requires three comma-separated ranges".to_string(),
                iterable.span,
            );
            return;
        };

        if ranges.len() != 3 {
            self.report_error(
                "3D gpu forall requires exactly three ranges".to_string(),
                iterable.span,
            );
            return;
        }

        if !self.validate_gpu_for_ranges_nd(ranges, 3, iterable.span, context) {
            return;
        }

        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        self.bind_gpu_for_int_variables(decls, context);
        self.check_statement(body, context);
        self.check_gpu_for_captures(decls, body, context);

        self.finalize_gpu_for_scope(context, outer_in_gpu);
    }

    /// Type-checks a `forall <ident> in <range>` statement (1D) or
    /// `forall x, y in <range1>, <range2>` statement (2D).
    ///
    /// Restrictions enforced beyond `check_for`:
    /// - The iterable must be a numeric range (`a..b` or `a..=b`).
    /// - For 1D: single loop variable; for 2D: exactly two loop variables.
    /// - The range start must be an integer literal.
    /// - The range end may be a runtime Int expression.
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
            3 => self.check_gpu_for_3d(decls, iterable, body, context, stmt_span),
            _ => {
                self.report_error(
                    "gpu forall requires 1, 2, or 3 loop variables".to_string(),
                    stmt_span,
                );
            }
        }
    }

    fn validate_gpu_for_ranges_nd(
        &mut self,
        ranges: &[Expression],
        _dims: usize,
        _iterable_span: Span,
        context: &mut Context,
    ) -> bool {
        let dim_names = ["x", "y", "z"];

        for (i, range_expr) in ranges.iter().enumerate() {
            let ExpressionKind::Range(start, Some(end), range_type) = &range_expr.node else {
                self.report_error(
                    "'gpu forall' requires a bounded numeric range like 'a..b' or 'a..=b'"
                        .to_string(),
                    range_expr.span,
                );
                return false;
            };
            if !matches!(
                range_type,
                RangeExpressionType::Exclusive | RangeExpressionType::Inclusive
            ) {
                self.report_error(
                    "'gpu forall' requires a bounded numeric range like 'a..b' or 'a..=b'"
                        .to_string(),
                    range_expr.span,
                );
                return false;
            }
            if !is_int_literal(start) {
                self.report_error(
                    format!(
                        "'gpu forall' range {} start must be an Int literal",
                        dim_names[i]
                    ),
                    range_expr.span,
                );
                return false;
            }

            let end_type = self.infer_expression(end, context);
            if !matches!(end_type.kind, TypeKind::Int) {
                self.report_error(
                    format!(
                        "'gpu forall' range {} end must be Int, got {}",
                        dim_names[i], end_type.kind
                    ),
                    end.span,
                );
                return false;
            }
        }

        true
    }

    fn bind_gpu_for_int_variables(&mut self, decls: &[VariableDeclaration], context: &mut Context) {
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
    }

    fn finalize_gpu_for_scope(&mut self, context: &mut Context, outer_in_gpu: bool) {
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
                "'gpu forall' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                iterable.span,
            );
            return;
        };
        if !matches!(
            range_type,
            RangeExpressionType::Exclusive | RangeExpressionType::Inclusive
        ) {
            self.report_error(
                "'gpu forall' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                iterable.span,
            );
            return;
        }
        if !is_int_literal(start) {
            self.report_error(
                "'gpu forall' range start must be an Int literal".to_string(),
                iterable.span,
            );
            return;
        }

        let end_type = self.infer_expression(end, context);
        if !matches!(end_type.kind, TypeKind::Int) {
            self.report_error(
                format!("'gpu forall' range end must be Int, got {}", end_type.kind),
                end.span,
            );
            return;
        }

        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        let iterable_type = self.infer_expression(iterable, context);
        let element_type = self.get_iterable_element_type(&iterable_type, iterable.span);
        self.bind_loop_variables(decls, &element_type, &iterable_type, iterable.span, context);
        self.check_statement(body, context);
        self.check_gpu_for_captures(decls, body, context);

        self.finalize_gpu_for_scope(context, outer_in_gpu);
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
                "2D forall requires two comma-separated ranges".to_string(),
                iterable.span,
            );
            return;
        };

        if ranges.len() != 2 {
            self.report_error(
                "2D forall requires exactly two ranges".to_string(),
                iterable.span,
            );
            return;
        }

        if !self.validate_gpu_for_ranges_nd(ranges, 2, iterable.span, context) {
            return;
        }

        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        self.bind_gpu_for_int_variables(decls, context);
        self.check_statement(body, context);
        self.check_gpu_for_captures(decls, body, context);

        self.finalize_gpu_for_scope(context, outer_in_gpu);
    }

    pub(crate) fn check_gpu_frame(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
        stmt_span: Span,
    ) {
        // `gpu frame` must have exactly 1 loop variable (enforced in parser)
        if decls.len() != 1 {
            self.report_error(
                "gpu frame requires exactly 1 loop variable".to_string(),
                stmt_span,
            );
            return;
        }

        let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
            self.report_error(
                "'gpu frame' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                iterable.span,
            );
            return;
        };
        if !matches!(
            range_type,
            RangeExpressionType::Exclusive | RangeExpressionType::Inclusive
        ) {
            self.report_error(
                "'gpu frame' requires a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
                iterable.span,
            );
            return;
        }
        if !is_int_literal(start) {
            self.report_error(
                "'gpu frame' requires Int-literal range start".to_string(),
                iterable.span,
            );
            return;
        }

        // End can be a runtime Int expression.
        let end_type = self.infer_expression(end, context);
        if !matches!(end_type.kind, TypeKind::Int) {
            self.report_error(
                format!("'gpu frame' range end must be Int, got {}", end_type.kind),
                end.span,
            );
            return;
        }

        // Enter scope and type-check body
        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        let int_type = make_type(TypeKind::Int);
        let is_mutable = matches!(decls[0].declaration_type, VariableDeclarationType::Mutable);
        context.define(
            decls[0].name.clone(),
            SymbolInfo::new(
                int_type,
                is_mutable,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
                None,
            ),
        );

        // Bind the per-frame input context, readable only inside this body.
        context.define(
            FRAME_INPUT_IDENT.to_string(),
            SymbolInfo::new(
                make_type(TypeKind::Custom(FRAME_INPUT_TYPE_NAME.to_string(), None)),
                false,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
                None,
            ),
        );

        // Type-check the body
        self.check_statement(body, context);

        // Validate ping-pong buffers: exactly 1 read-only and 1 read-write gpu capture
        self.check_gpu_frame_buffers(decls, body, context, stmt_span);

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

    pub(crate) fn check_gpu_frame_block(
        &mut self,
        block: &Statement,
        context: &mut Context,
        stmt_span: Span,
    ) {
        // Extract statements from the block
        let stmts = match &block.node {
            StatementKind::Block(stmts) => stmts,
            _ => {
                self.report_error(
                    "gpu frame block body must be a block statement".to_string(),
                    block.span,
                );
                return;
            }
        };

        if stmts.is_empty() {
            self.report_error(
                "'gpu frame' block must contain at least one 'gpu forall' pass".to_string(),
                stmt_span,
            );
            return;
        }

        // Flatten the block into its ordered passes, expanding any literal-count
        // `for _ in 0..k` repeat. Malformed children are reported here.
        let passes = match crate::mir::lowering::gpu_frame::flatten_frame_passes(stmts) {
            Ok(passes) => passes,
            Err((msg, sp)) => {
                self.report_error(msg, sp);
                return;
            }
        };

        // Enter GPU scope
        context.enter_scope();
        let outer_in_gpu = context.in_gpu_function;
        context.in_gpu_function = true;
        context.gpu_for_depth += 1;

        // Bind the per-frame input context, readable in all passes
        context.define(
            FRAME_INPUT_IDENT.to_string(),
            SymbolInfo::new(
                make_type(TypeKind::Custom(FRAME_INPUT_TYPE_NAME.to_string(), None)),
                false,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
                None,
            ),
        );

        // Type-check each pass and apply per-pass buffer read/write disjointness validation.
        for pass in passes {
            if let StatementKind::Forall {
                vars: decls,
                iterable,
                body,
                ..
            } = &pass.node
            {
                // Apply per-pass semantic buffer validation.
                self.check_gpu_frame_buffers(decls, body, context, pass.span);
                // Then type-check the pass.
                self.check_gpu_for(decls, iterable, body, context, pass.span);
            }
        }

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

    fn check_gpu_frame_buffers(
        &mut self,
        loop_decls: &[VariableDeclaration],
        body: &Statement,
        context: &Context,
        stmt_span: Span,
    ) {
        // Buffer-level semantic validation: per-pass read/write disjointness.
        // For a single forall pass: reject iff read_set ∩ write_set ≠ ∅ (race),
        // or write_set is empty. Multiple disjoint writes are now LEGAL.
        let loop_var_name = &loop_decls[0].name;
        let (read_set, write_set) = collect_pass_buffer_sets(body, loop_var_name, context);

        if write_set.is_empty() {
            self.report_error(
                "'gpu frame' pass must write at least one gpu buffer".to_string(),
                stmt_span,
            );
            return;
        }

        // Check for data races: buffers in both read_set and write_set.
        let mut race_buffers: Vec<_> = read_set.iter().filter(|b| write_set.contains(*b)).collect();
        if !race_buffers.is_empty() {
            race_buffers.sort();
            self.report_error(
                format!(
                    "'gpu frame' pass creates a data race: buffer '{}' is both read and written in the same pass (use a separate ping-pong buffer)",
                    race_buffers[0]
                ),
                stmt_span,
            );
        }

        // No check for number of write buffers anymore; disjoint writes are legal.
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
                "'break' is not supported inside a 'gpu forall' body: the GPU dispatch is not an iterative loop, so loop-control statements have no meaning at the kernel level"
            } else {
                "Break statement outside of loop"
            };
            self.report_error(msg.to_string(), span);
        }
    }

    pub(crate) fn check_continue(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            let msg = if context.gpu_for_depth > 0 {
                "'continue' is not supported inside a 'gpu forall' body: the GPU dispatch is not an iterative loop, so loop-control statements have no meaning at the kernel level"
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

/// Collects gpu buffer read and write sets for a single pass (forall body).
/// Used for semantic buffer-level disjointness validation.
///
/// Returns (read_set, write_set) where each is a set of gpu buffer names.
/// A buffer in both sets indicates a potential data race.
fn collect_pass_buffer_sets(
    body: &Statement,
    loop_var_name: &str,
    context: &Context,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::HashSet;

    let written = collect_written_names_in_stmt(body);
    // Atomic buffers are read-modify-write race-free by construction, so they
    // are counted as writes only and exempt from the read/write disjointness
    // rule (which exists to make non-atomic ping-pong safe).
    let atomic_written = collect_atomic_written_names_in_stmt(body);
    let mut read_set = HashSet::new();
    let mut write_set = HashSet::new();

    // Collect all captured identifiers, excluding the loop variable.
    let mut bound = HashSet::new();
    bound.insert(loop_var_name.to_string());
    let captured = collect_free_identifiers_excluding(body, &bound);

    for name in captured {
        if name == loop_var_name {
            continue;
        }

        // Check if this variable is visible in the outer context.
        let Some(info) = context.resolve_info(&name) else {
            continue;
        };

        // Only gpu-resident buffers are counted.
        if !is_gpu_buffer_type(&info.ty.kind) {
            continue;
        }

        if atomic_written.contains(&name) {
            // Race-free atomic target: write only, never a conflicting read.
            write_set.insert(name);
            continue;
        }

        let is_written = written.contains(&name);
        let is_read = is_identifier_read_in_stmt(body, &name, loop_var_name);

        if is_written {
            write_set.insert(name.clone());
        }
        if is_read {
            read_set.insert(name);
        }
    }

    (read_set, write_set)
}

/// Helper: collects all variable names that are written to in a statement.
fn collect_written_names_in_stmt(stmt: &Statement) -> std::collections::HashSet<String> {
    let mut written = std::collections::HashSet::new();
    visit_written_stmt(stmt, &mut written);
    written
}

/// Collects the names of buffers mutated by an atomic builtin in a pass body.
fn collect_atomic_written_names_in_stmt(stmt: &Statement) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    visit_atomic_written_stmt(stmt, &mut names);
    names
}

fn visit_atomic_written_stmt(stmt: &Statement, names: &mut std::collections::HashSet<String>) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                visit_atomic_written_stmt(s, names);
            }
        }
        StatementKind::Expression(expr) => visit_atomic_written_expr(expr, names),
        StatementKind::If(_, then_branch, else_branch, _) => {
            visit_atomic_written_stmt(then_branch, names);
            if let Some(eb) = else_branch {
                visit_atomic_written_stmt(eb, names);
            }
        }
        StatementKind::While(_, body, _) => visit_atomic_written_stmt(body, names),
        StatementKind::For(_, _, body) | StatementKind::GpuFrame(_, _, body) => {
            visit_atomic_written_stmt(body, names);
        }
        StatementKind::Forall { body, .. } => visit_atomic_written_stmt(body, names),
        StatementKind::GpuFrameBlock(block) => visit_atomic_written_stmt(block, names),
        _ => {}
    }
}

fn visit_atomic_written_expr(expr: &Expression, names: &mut std::collections::HashSet<String>) {
    if let ExpressionKind::Call(func, args) = &expr.node {
        if let Some(name) = atomic_builtin_buffer_name(func, args) {
            names.insert(name);
        }
    }
}

fn visit_written_stmt(stmt: &Statement, written: &mut std::collections::HashSet<String>) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                visit_written_stmt(s, written);
            }
        }
        StatementKind::Expression(expr) => visit_written_expr(expr, written),
        StatementKind::Variable(_, _) => {}
        StatementKind::Return(_) => {}
        StatementKind::If(_, then_branch, else_branch, _) => {
            visit_written_stmt(then_branch, written);
            if let Some(eb) = else_branch {
                visit_written_stmt(eb, written);
            }
        }
        StatementKind::While(_, body, _) => visit_written_stmt(body, written),
        StatementKind::For(_, _, body) | StatementKind::GpuFrame(_, _, body) => {
            visit_written_stmt(body, written);
        }
        StatementKind::Forall { body, .. } => {
            visit_written_stmt(body, written);
        }
        StatementKind::GpuFrameBlock(block) => {
            visit_written_stmt(block, written);
        }
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
    }
}

fn visit_written_expr(expr: &Expression, written: &mut std::collections::HashSet<String>) {
    match &expr.node {
        ExpressionKind::Assignment(lhs, _, rhs) => {
            extract_written_lhs(lhs, written);
            visit_written_expr(rhs, written);
        }
        // An atomic builtin (`atomic_add(buf, ..)`) writes its buffer argument.
        ExpressionKind::Call(func, args) => {
            if let Some(name) = atomic_builtin_buffer_name(func, args) {
                written.insert(name);
            }
        }
        _ => {}
    }
}

/// If `func`/`args` form an atomic builtin call (`atomic_add(buf, ..)`), returns
/// the identifier name of the buffer argument it mutates.
fn atomic_builtin_buffer_name(func: &Expression, args: &[Expression]) -> Option<String> {
    let ExpressionKind::Identifier(fname, _) = &func.node else {
        return None;
    };
    crate::mir::backend::gpu::GpuAtomicOp::from_builtin_name(fname)?;
    match args.first().map(|a| &a.node) {
        Some(ExpressionKind::Identifier(buf, _)) => Some(buf.clone()),
        _ => None,
    }
}

fn extract_written_lhs(
    lhs: &crate::ast::expression::LeftHandSideExpression,
    written: &mut std::collections::HashSet<String>,
) {
    use crate::ast::expression::LeftHandSideExpression;
    match lhs {
        LeftHandSideExpression::Identifier(expr) => {
            if let ExpressionKind::Identifier(name, _) = &expr.node {
                written.insert(name.clone());
            }
        }
        LeftHandSideExpression::Index(expr) | LeftHandSideExpression::Member(expr) => {
            if let ExpressionKind::Index(base, _) | ExpressionKind::Member(base, _) = &expr.node {
                if let ExpressionKind::Identifier(name, _) = &base.node {
                    written.insert(name.clone());
                }
            }
        }
    }
}

/// Helper: determines if a type is a gpu-compatible buffer type.
/// This checks for Array types and List/Map/Set (future gpu buffers).
fn is_gpu_buffer_type(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) => true,
        TypeKind::Custom(name, _) => {
            BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
        }
        _ => false,
    }
}

/// Helper: checks if an identifier is read (appears in any expression) in a statement.
/// Excludes the loop variable.
fn is_identifier_read_in_stmt(stmt: &Statement, name: &str, loop_var: &str) -> bool {
    let mut bound = std::collections::HashSet::new();
    bound.insert(loop_var.to_string());
    is_identifier_read_in_stmt_impl(stmt, name, &bound)
}

fn is_identifier_read_in_stmt_impl(
    stmt: &Statement,
    name: &str,
    bound: &std::collections::HashSet<String>,
) -> bool {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                if is_identifier_read_in_stmt_impl(s, name, bound) {
                    return true;
                }
            }
            false
        }
        StatementKind::Expression(expr) => is_identifier_read_in_expr(expr, name, bound),
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    if is_identifier_read_in_expr(init, name, bound) {
                        return true;
                    }
                }
            }
            false
        }
        StatementKind::Return(Some(e)) => is_identifier_read_in_expr(e, name, bound),
        StatementKind::Return(None) => false,
        StatementKind::If(cond, then_branch, else_branch, _) => {
            if is_identifier_read_in_expr(cond, name, bound) {
                return true;
            }
            if is_identifier_read_in_stmt_impl(then_branch, name, bound) {
                return true;
            }
            if let Some(eb) = else_branch {
                if is_identifier_read_in_stmt_impl(eb, name, bound) {
                    return true;
                }
            }
            false
        }
        StatementKind::While(cond, body, _) => {
            if is_identifier_read_in_expr(cond, name, bound) {
                return true;
            }
            is_identifier_read_in_stmt_impl(body, name, bound)
        }
        StatementKind::For(inner_decls, iter, body)
        | StatementKind::GpuFrame(inner_decls, iter, body) => {
            if is_identifier_read_in_expr(iter, name, bound) {
                return true;
            }
            let mut new_bound = bound.clone();
            for d in inner_decls {
                new_bound.insert(d.name.clone());
            }
            is_identifier_read_in_stmt_impl(body, name, &new_bound)
        }
        StatementKind::Forall {
            vars: inner_decls,
            iterable: iter,
            body,
            ..
        } => {
            if is_identifier_read_in_expr(iter, name, bound) {
                return true;
            }
            let mut new_bound = bound.clone();
            for d in inner_decls {
                new_bound.insert(d.name.clone());
            }
            is_identifier_read_in_stmt_impl(body, name, &new_bound)
        }
        StatementKind::GpuFrameBlock(block) => is_identifier_read_in_stmt_impl(block, name, bound),
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => false,
    }
}

fn is_identifier_read_in_expr(
    expr: &Expression,
    name: &str,
    bound: &std::collections::HashSet<String>,
) -> bool {
    match &expr.node {
        ExpressionKind::Identifier(ident, _) => ident == name && !bound.contains(name),
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            is_identifier_read_in_expr(lhs, name, bound)
                || is_identifier_read_in_expr(rhs, name, bound)
        }
        ExpressionKind::Range(lhs, Some(rhs), _) => {
            is_identifier_read_in_expr(lhs, name, bound)
                || is_identifier_read_in_expr(rhs, name, bound)
        }
        ExpressionKind::Range(lhs, None, _) => is_identifier_read_in_expr(lhs, name, bound),
        ExpressionKind::Unary(_, e) => is_identifier_read_in_expr(e, name, bound),
        ExpressionKind::Call(func, args) => {
            if is_identifier_read_in_expr(func, name, bound) {
                return true;
            }
            for arg in args {
                if is_identifier_read_in_expr(arg, name, bound) {
                    return true;
                }
            }
            false
        }
        ExpressionKind::Index(base, index) => {
            is_identifier_read_in_expr(base, name, bound)
                || is_identifier_read_in_expr(index, name, bound)
        }
        ExpressionKind::Member(base, _) => is_identifier_read_in_expr(base, name, bound),
        ExpressionKind::Assignment(_, _, rhs) => {
            // Check RHS only. The LHS is being written to, not read.
            // For `a[i] = b[i]`, we only care that `b` is read on the RHS,
            // not that `a` is indexed on the LHS.
            is_identifier_read_in_expr(rhs, name, bound)
        }
        ExpressionKind::Array(exprs, init_expr) => {
            for e in exprs {
                if is_identifier_read_in_expr(e, name, bound) {
                    return true;
                }
            }
            is_identifier_read_in_expr(init_expr, name, bound)
        }
        ExpressionKind::List(exprs) | ExpressionKind::Set(exprs) | ExpressionKind::Tuple(exprs) => {
            for e in exprs {
                if is_identifier_read_in_expr(e, name, bound) {
                    return true;
                }
            }
            false
        }
        ExpressionKind::Map(pairs) => {
            for (k, v) in pairs {
                if is_identifier_read_in_expr(k, name, bound)
                    || is_identifier_read_in_expr(v, name, bound)
                {
                    return true;
                }
            }
            false
        }
        ExpressionKind::Cast(e, _) => is_identifier_read_in_expr(e, name, bound),
        ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
            if is_identifier_read_in_expr(cond, name, bound) {
                return true;
            }
            if is_identifier_read_in_expr(then_expr, name, bound) {
                return true;
            }
            if let Some(e) = else_expr {
                if is_identifier_read_in_expr(e, name, bound) {
                    return true;
                }
            }
            false
        }
        ExpressionKind::Block(_, e) => is_identifier_read_in_expr(e, name, bound),
        ExpressionKind::Match(e, _) => is_identifier_read_in_expr(e, name, bound),
        ExpressionKind::Guard(_, e) => is_identifier_read_in_expr(e, name, bound),
        ExpressionKind::Lambda(_) => false, // Lambdas have their own scope
        ExpressionKind::Literal(_)
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::EnumValue(_, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::FormattedString(_)
        | ExpressionKind::NamedArgument(_, _)
        | ExpressionKind::Super => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForallTarget {
    Cpu,
    Gpu,
    GpuRequiredButAbsent,
}

fn scan_residencies(
    captured: &std::collections::HashSet<String>,
    context: &Context,
) -> (bool, bool) {
    let mut has_gpu = false;
    let mut has_host = false;

    for name in captured {
        let Some(info) = context.resolve_info(name) else {
            continue;
        };

        if matches!(info.ty.kind, TypeKind::Function(_)) {
            continue;
        }

        match info.residency {
            statement::BindingResidency::Host => {
                if !is_scalar_type(&info.ty.kind) {
                    has_host = true;
                }
            }
            statement::BindingResidency::Gpu => {
                has_gpu = true;
            }
        }
    }

    (has_gpu, has_host)
}

fn resolve_forall_device(
    body: &Statement,
    declared: AcceleratorTarget,
    vars: &[VariableDeclaration],
    context: &Context,
) -> ForallTarget {
    let bound: std::collections::HashSet<String> = vars.iter().map(|d| d.name.clone()).collect();
    let captured = collect_free_identifiers_excluding(body, &bound);

    let (has_gpu, _has_host) = scan_residencies(&captured, context);

    if matches!(declared, AcceleratorTarget::Gpu) {
        if has_gpu {
            ForallTarget::Gpu
        } else {
            ForallTarget::GpuRequiredButAbsent
        }
    } else if has_gpu {
        ForallTarget::Gpu
    } else {
        // Host-only data (or no captured data) runs sequentially on the CPU.
        ForallTarget::Cpu
    }
}

fn detect_reduction(
    body: &Statement,
    vars: &[VariableDeclaration],
    context: &Context,
) -> Option<String> {
    let written = collect_written_names_in_stmt(body);

    let bound: std::collections::HashSet<String> = vars.iter().map(|d| d.name.clone()).collect();
    let captured = collect_free_identifiers_excluding(body, &bound);

    for name in written.iter() {
        if !captured.contains(name) {
            continue;
        }

        let Some(info) = context.resolve_info(name) else {
            continue;
        };

        if is_residency_gated_buffer(&info.ty.kind) {
            continue;
        }

        if is_scalar_type(&info.ty.kind) {
            return Some(name.clone());
        }
    }

    None
}

fn is_scalar_type(kind: &TypeKind) -> bool {
    matches!(
        kind,
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Boolean
    )
}
