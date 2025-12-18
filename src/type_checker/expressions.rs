// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::*;
use crate::syntax_error::Span;

impl TypeChecker {
    pub(crate) fn infer_expression(&mut self, expr: &Expression, context: &mut Context) -> Type {
        let ty = match &expr.node {
            ExpressionKind::Literal(lit) => self.infer_literal(lit),
            ExpressionKind::Binary(left, op, right) => {
                self.infer_binary(left, op, right, expr.span.clone(), context)
            }
            ExpressionKind::Logical(left, op, right) => {
                self.infer_logical(left, op, right, expr.span.clone(), context)
            }
            ExpressionKind::Unary(op, operand) => {
                self.infer_unary(op, operand, expr.span.clone(), context)
            }
            ExpressionKind::Identifier(name, _) => {
                self.infer_identifier(name, expr.span.clone(), context)
            }
            ExpressionKind::Assignment(lhs, _, rhs) => {
                self.infer_assignment(lhs, rhs, expr.span.clone(), context)
            }
            ExpressionKind::Call(func, args) => {
                self.infer_call(func, args, expr.span.clone(), context)
            }
            ExpressionKind::Range(start, end, kind) => {
                self.infer_range(start, end, kind, expr.span.clone(), context)
            }
            ExpressionKind::List(elements) => self.infer_list(elements, expr.span.clone(), context),
            ExpressionKind::Map(entries) => self.infer_map(entries, expr.span.clone(), context),
            ExpressionKind::Set(elements) => self.infer_set(elements, expr.span.clone(), context),
            ExpressionKind::Tuple(elements) => {
                self.infer_tuple(elements, expr.span.clone(), context)
            }
            ExpressionKind::Index(obj, index) => {
                self.infer_index(obj, index, expr.span.clone(), context)
            }
            ExpressionKind::Member(obj, prop) => {
                self.infer_member(obj, prop, expr.span.clone(), context)
            }
            ExpressionKind::Match(subject, branches) => {
                self.infer_match(subject, branches, expr.span.clone(), context)
            }
            ExpressionKind::Conditional(then_expr, cond_expr, else_expr, _) => {
                self.infer_conditional(then_expr, cond_expr, else_expr, expr.span.clone(), context)
            }
            ExpressionKind::FormattedString(parts) => self.infer_formatted_string(parts, context),
            ExpressionKind::Lambda(generics, params, return_type, body, properties) => {
                self.infer_lambda(generics, params, return_type, body, properties, context)
            }
            _ => Type::Int, // Default fallback for unimplemented expressions
        };

        self.types.insert(expr.id, ty.clone());
        ty
    }

    fn infer_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Integer(_) => Type::Int,
            Literal::Float(f) => match f {
                FloatLiteral::F32(_) => Type::F32,
                FloatLiteral::F64(_) => Type::F64,
            },
            Literal::Boolean(_) => Type::Boolean,
            Literal::String(_) => Type::String,
            Literal::Symbol(_) => Type::Symbol,
            Literal::Regex(_) => Type::Custom("Regex".into(), None),
            Literal::None => Type::Nullable(Box::new(Type::Void)), // None is compatible with any nullable type, treating as Nullable(Void) for now or special handling
        }
    }

    fn infer_binary(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let left_ty = self.infer_expression(left, context);
        let right_ty = self.infer_expression(right, context);

        match self.check_binary_op_types(&left_ty, op, &right_ty, context) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                Type::Error
            }
        }
    }

    fn infer_logical(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        // Logical ops are binary ops in this AST, but we can treat them similarly
        self.infer_binary(left, op, right, span, context)
    }

    fn infer_unary(
        &mut self,
        op: &UnaryOp,
        operand: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let expr_ty = self.infer_expression(operand, context);
        match self.check_unary_op_types(op, &expr_ty) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                Type::Error
            }
        }
    }

    fn infer_identifier(&mut self, name: &str, span: Span, context: &Context) -> Type {
        let info_opt = context
            .resolve_info(name)
            .or_else(|| self.global_scope.get(name).cloned());

        if let Some(info) = info_opt {
            if !self.check_visibility(&info.visibility, &info.module) {
                self.report_error(format!("Variable '{}' is not visible", name), span);
                return Type::Error;
            }
            info.ty
        } else {
            self.report_error(format!("Undefined variable: {}", name), span);
            Type::Error
        }
    }

    fn infer_assignment(
        &mut self,
        lhs: &LeftHandSideExpression,
        rhs: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let rhs_type = self.infer_expression(rhs, context);
        let lhs_type = match lhs {
            LeftHandSideExpression::Identifier(id_expr) => {
                if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                    if !context.is_mutable(name) {
                        self.report_error(
                            format!("Cannot assign to immutable variable '{}'", name),
                            span.clone(),
                        );
                    }
                    self.infer_identifier(name, id_expr.span.clone(), context)
                } else {
                    self.report_error("Invalid assignment target".to_string(), span.clone());
                    Type::Error
                }
            }
            LeftHandSideExpression::Member(member_expr) => {
                if let ExpressionKind::Member(obj, prop) = &member_expr.node {
                    if !self.is_mutable_expression(obj, context) {
                        self.report_error(
                            "Cannot assign to field of immutable variable".to_string(),
                            span.clone(),
                        );
                    }
                    self.infer_member(obj, prop, member_expr.span.clone(), context)
                } else {
                    Type::Error
                }
            }
            LeftHandSideExpression::Index(index_expr) => {
                if let ExpressionKind::Index(obj, index) = &index_expr.node {
                    if !self.is_mutable_expression(obj, context) {
                        self.report_error(
                            "Cannot assign to element of immutable variable".to_string(),
                            span.clone(),
                        );
                    }
                    self.infer_index(obj, index, index_expr.span.clone(), context)
                } else {
                    Type::Error
                }
            }
        };

        if !self.are_compatible(&lhs_type, &rhs_type, context) {
            self.report_error(
                format!(
                    "Type mismatch in assignment: cannot assign {:?} to {:?}",
                    rhs_type, lhs_type
                ),
                span,
            );
        }
        lhs_type
    }

    fn infer_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let func_type = self.infer_expression(func, context);
        match func_type {
            Type::Function(_, params, return_type_expr) => {
                if args.len() != params.len() {
                    self.report_error(
                        format!(
                            "Incorrect number of arguments: expected {}, got {}",
                            params.len(),
                            args.len()
                        ),
                        span.clone(),
                    );
                }

                for (i, arg) in args.iter().enumerate() {
                    let arg_type = self.infer_expression(arg, context);
                    if i < params.len() {
                        let param_type = self.resolve_type_expression(&params[i].typ, context);
                        if !self.are_compatible(&param_type, &arg_type, context) {
                            self.report_error(
                                format!(
                                    "Type mismatch for argument {}: expected {:?}, got {:?}",
                                    i + 1,
                                    param_type,
                                    arg_type
                                ),
                                arg.span.clone(),
                            );
                        }
                    }
                }

                if let Some(rt_expr) = return_type_expr {
                    self.resolve_type_expression(&rt_expr, context)
                } else {
                    Type::Void
                }
            }
            Type::Meta(inner_type) => {
                if let Type::Custom(name, _) = &*inner_type {
                    if let Some(TypeDefinition::Struct(def)) =
                        context.resolve_type_definition(name).cloned()
                    {
                        if args.len() != def.fields.len() {
                            self.report_error(
                                format!("Incorrect number of arguments for struct constructor: expected {}, got {}", def.fields.len(), args.len()),
                                span.clone()
                            );
                        }

                        for (i, arg) in args.iter().enumerate() {
                            let arg_type = self.infer_expression(arg, context);
                            if i < def.fields.len() {
                                let (_, field_type, _) = &def.fields[i];
                                if !self.are_compatible(field_type, &arg_type, context) {
                                    self.report_error(
                                        format!(
                                            "Type mismatch for field '{}': expected {:?}, got {:?}",
                                            def.fields[i].0, field_type, arg_type
                                        ),
                                        arg.span.clone(),
                                    );
                                }
                            }
                        }
                        return Type::Custom(name.clone(), None);
                    }
                }
                self.report_error(format!("Type '{:?}' is not callable", inner_type), span);
                Type::Error
            }
            Type::Error => Type::Error,
            _ => {
                self.report_error(
                    format!("Expression is not callable: {:?}", func_type),
                    func.span.clone(),
                );
                Type::Error
            }
        }
    }

    fn infer_range(
        &mut self,
        start: &Expression,
        end: &Option<Box<Expression>>,
        kind: &RangeExpressionType,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let start_type = self.infer_expression(start, context);

        if matches!(kind, RangeExpressionType::IterableObject) {
            return start_type;
        }

        if let Some(end_expr) = end {
            let end_type = self.infer_expression(end_expr, context);
            if !self.are_compatible(&start_type, &end_type, context) {
                self.report_error(
                    format!("Range types mismatch: {:?} and {:?}", start_type, end_type),
                    span,
                );
            }
        }

        let type_expr = self.create_type_expression(start_type);
        Type::Custom("Range".to_string(), Some(vec![type_expr]))
    }

    fn infer_list(&mut self, elements: &[Expression], span: Span, context: &mut Context) -> Type {
        if elements.is_empty() {
            return Type::List(Box::new(self.create_type_expression(Type::Void)));
        }

        let first_type = self.infer_expression(&elements[0], context);
        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "List elements must have the same type".to_string(),
                    span.clone(),
                );
                return Type::Error;
            }
        }

        Type::List(Box::new(self.create_type_expression(first_type)))
    }

    fn infer_map(
        &mut self,
        entries: &[(Expression, Expression)],
        span: Span,
        context: &mut Context,
    ) -> Type {
        if entries.is_empty() {
            return Type::Map(
                Box::new(self.create_type_expression(Type::Void)),
                Box::new(self.create_type_expression(Type::Void)),
            );
        }

        let (first_key, first_val) = &entries[0];
        let key_type = self.infer_expression(first_key, context);
        let val_type = self.infer_expression(first_val, context);

        for (key, val) in &entries[1..] {
            let k_type = self.infer_expression(key, context);
            let v_type = self.infer_expression(val, context);

            if !self.are_compatible(&key_type, &k_type, context) {
                self.report_error("Map keys must have the same type".to_string(), span.clone());
                return Type::Error;
            }
            if !self.are_compatible(&val_type, &v_type, context) {
                self.report_error(
                    "Map values must have the same type".to_string(),
                    span.clone(),
                );
                return Type::Error;
            }
        }

        Type::Map(
            Box::new(self.create_type_expression(key_type)),
            Box::new(self.create_type_expression(val_type)),
        )
    }

    fn infer_set(&mut self, elements: &[Expression], span: Span, context: &mut Context) -> Type {
        if elements.is_empty() {
            return Type::Set(Box::new(self.create_type_expression(Type::Void)));
        }

        let first_type = self.infer_expression(&elements[0], context);
        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "Set elements must have the same type".to_string(),
                    span.clone(),
                );
                return Type::Error;
            }
        }

        Type::Set(Box::new(self.create_type_expression(first_type)))
    }

    fn infer_tuple(&mut self, elements: &[Expression], _span: Span, context: &mut Context) -> Type {
        let mut element_types = Vec::new();
        for element in elements {
            let ty = self.infer_expression(element, context);
            element_types.push(self.create_type_expression(ty));
        }
        Type::Tuple(element_types)
    }

    fn infer_index(
        &mut self,
        obj: &Expression,
        index: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let obj_type = self.infer_expression(obj, context);
        let index_type = self.infer_expression(index, context);

        match obj_type {
            Type::List(inner_type_expr) => {
                if index_type != Type::Int {
                    self.report_error(
                        "List index must be an integer".to_string(),
                        index.span.clone(),
                    );
                    return Type::Error;
                }
                self.resolve_type_expression(&inner_type_expr, context)
            }
            Type::Map(key_type_expr, val_type_expr) => {
                let key_type = self.resolve_type_expression(&key_type_expr, context);
                if !self.are_compatible(&key_type, &index_type, context) {
                    self.report_error("Invalid map key type".to_string(), index.span.clone());
                    return Type::Error;
                }
                self.resolve_type_expression(&val_type_expr, context)
            }
            Type::Tuple(element_type_exprs) => {
                // Check if tuple is homogeneous
                let is_homogeneous = if element_type_exprs.is_empty() {
                    true
                } else {
                    let resolved_types: Vec<Type> = element_type_exprs
                        .iter()
                        .map(|t| self.resolve_type_expression(t, context))
                        .collect();

                    let first_type = &resolved_types[0];
                    resolved_types
                        .iter()
                        .all(|t| self.are_compatible(t, first_type, context))
                };

                if is_homogeneous {
                    if index_type != Type::Int {
                        self.report_error(
                            "Tuple index must be an integer".to_string(),
                            index.span.clone(),
                        );
                        return Type::Error;
                    }
                    // If homogeneous, we can return the type of the first element (or any element)
                    if element_type_exprs.is_empty() {
                        // Indexing empty tuple is always out of bounds, but let's handle it gracefully or error
                        self.report_error(
                            "Tuple index out of bounds (empty tuple)".to_string(),
                            span,
                        );
                        return Type::Error;
                    }

                    // If it's a literal, we can still check bounds
                    if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
                        let idx = match val {
                            crate::ast::IntegerLiteral::I8(v) => *v as usize,
                            crate::ast::IntegerLiteral::I16(v) => *v as usize,
                            crate::ast::IntegerLiteral::I32(v) => *v as usize,
                            crate::ast::IntegerLiteral::I64(v) => *v as usize,
                            crate::ast::IntegerLiteral::I128(v) => *v as usize,
                            crate::ast::IntegerLiteral::U8(v) => *v as usize,
                            crate::ast::IntegerLiteral::U16(v) => *v as usize,
                            crate::ast::IntegerLiteral::U32(v) => *v as usize,
                            crate::ast::IntegerLiteral::U64(v) => *v as usize,
                            crate::ast::IntegerLiteral::U128(v) => *v as usize,
                        };
                        if idx >= element_type_exprs.len() {
                            self.report_error("Tuple index out of bounds".to_string(), span);
                            return Type::Error;
                        }
                    }

                    self.resolve_type_expression(&element_type_exprs[0], context)
                } else {
                    // For heterogeneous tuple, index must be a compile-time integer literal
                    if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
                        let idx = match val {
                            crate::ast::IntegerLiteral::I8(v) => *v as usize,
                            crate::ast::IntegerLiteral::I16(v) => *v as usize,
                            crate::ast::IntegerLiteral::I32(v) => *v as usize,
                            crate::ast::IntegerLiteral::I64(v) => *v as usize,
                            crate::ast::IntegerLiteral::I128(v) => *v as usize,
                            crate::ast::IntegerLiteral::U8(v) => *v as usize,
                            crate::ast::IntegerLiteral::U16(v) => *v as usize,
                            crate::ast::IntegerLiteral::U32(v) => *v as usize,
                            crate::ast::IntegerLiteral::U64(v) => *v as usize,
                            crate::ast::IntegerLiteral::U128(v) => *v as usize,
                        };

                        if idx < element_type_exprs.len() {
                            self.resolve_type_expression(&element_type_exprs[idx], context)
                        } else {
                            self.report_error("Tuple index out of bounds".to_string(), span);
                            Type::Error
                        }
                    } else {
                        self.report_error(
                            "Tuple index must be an integer literal for heterogeneous tuples"
                                .to_string(),
                            index.span.clone(),
                        );
                        Type::Error
                    }
                }
            }
            Type::String => {
                if index_type != Type::Int {
                    self.report_error(
                        "String index must be an integer".to_string(),
                        index.span.clone(),
                    );
                    return Type::Error;
                }
                Type::String // Indexing a string returns a string (char)
            }
            Type::Error => Type::Error,
            _ => {
                self.report_error(format!("Type {:?} is not indexable", obj_type), span);
                Type::Error
            }
        }
    }

    fn infer_member(
        &mut self,
        obj: &Expression,
        prop: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let obj_type = self.infer_expression(obj, context);

        let prop_name = if let ExpressionKind::Identifier(name, _) = &prop.node {
            name
        } else {
            self.report_error(
                "Member property must be an identifier".to_string(),
                prop.span.clone(),
            );
            return Type::Error;
        };

        match obj_type {
            Type::Custom(name, _) => {
                // Instance member access (Struct field)
                // We need to clone the definition to avoid borrowing issues with context
                let def_opt = context
                    .resolve_type_definition(&name)
                    .cloned()
                    .or_else(|| self.global_type_definitions.get(&name).cloned());

                if let Some(TypeDefinition::Struct(def)) = def_opt {
                    if let Some((_, field_type, visibility)) =
                        def.fields.iter().find(|(n, _, _)| n == prop_name)
                    {
                        if !self.check_visibility(visibility, &def.module) {
                            self.report_error(
                                format!("Field '{}' is not visible", prop_name),
                                span,
                            );
                            return Type::Error;
                        }
                        field_type.clone()
                    } else {
                        self.report_error(
                            format!("Struct '{}' has no field '{}'", name, prop_name),
                            span,
                        );
                        Type::Error
                    }
                } else {
                    // Could be an enum instance, but enums don't have fields yet (unless methods are added later)
                    self.report_error(format!("Type '{}' does not have members", name), span);
                    Type::Error
                }
            }
            Type::Meta(inner_type) => {
                // Static member access (Enum variant)
                if let Type::Custom(name, _) = *inner_type {
                    let def_opt = context
                        .resolve_type_definition(&name)
                        .cloned()
                        .or_else(|| self.global_type_definitions.get(&name).cloned());

                    if let Some(TypeDefinition::Enum(def)) = def_opt {
                        if let Some(variant_types) = def.variants.get(prop_name) {
                            // If variant has no associated types, it's a value of the Enum type.
                            // If it has associated types, it's a constructor function.
                            if variant_types.is_empty() {
                                Type::Custom(name.clone(), None)
                            } else {
                                // Constructor function: (args) -> EnumType
                                let params: Vec<Parameter> = variant_types
                                    .iter()
                                    .enumerate()
                                    .map(|(i, t)| Parameter {
                                        name: format!("arg{}", i),
                                        typ: Box::new(self.create_type_expression(t.clone())),
                                        guard: None,
                                        default_value: None,
                                    })
                                    .collect();
                                Type::Function(
                                    None,
                                    params,
                                    Some(Box::new(
                                        self.create_type_expression(Type::Custom(
                                            name.clone(),
                                            None,
                                        )),
                                    )),
                                )
                            }
                        } else {
                            self.report_error(
                                format!("Enum '{}' has no variant '{}'", name, prop_name),
                                span,
                            );
                            Type::Error
                        }
                    } else {
                        self.report_error(
                            format!("Type '{}' does not have static members", name),
                            span,
                        );
                        Type::Error
                    }
                } else {
                    self.report_error(
                        format!("Type '{:?}' does not have static members", inner_type),
                        span,
                    );
                    Type::Error
                }
            }
            _ => {
                self.report_error(format!("Type '{:?}' does not have members", obj_type), span);
                Type::Error
            }
        }
    }

    fn infer_match(
        &mut self,
        subject: &Expression,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let subject_type = self.infer_expression(subject, context);

        if branches.is_empty() {
            return Type::Void;
        }

        let mut first_branch_type = None;

        for branch in branches {
            for pattern in &branch.patterns {
                self.check_pattern(pattern, &subject_type, context, span.clone());
            }

            context.enter_scope();
            let body_type = self.infer_statement_type(&branch.body, context);
            context.exit_scope();

            if let Some(first) = &first_branch_type {
                if !self.are_compatible(first, &body_type, context) {
                    self.report_error(
                        format!(
                            "Match branch types mismatch: expected {:?}, got {:?}",
                            first, body_type
                        ),
                        span.clone(),
                    );
                }
            } else {
                first_branch_type = Some(body_type);
            }
        }

        first_branch_type.unwrap_or(Type::Void)
    }

    fn infer_lambda(
        &mut self,
        generics: &Option<Vec<Expression>>,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        body: &Statement,
        _properties: &FunctionProperties,
        context: &mut Context,
    ) -> Type {
        // Determine expected return type
        let expected_return_type = return_type_expr
            .as_ref()
            .map(|rt_expr| self.resolve_type_expression(rt_expr, context));

        if let Some(rt) = &expected_return_type {
            context.return_types.push(rt.clone());
            context.inferred_return_types.push(None);
        } else {
            context.return_types.push(Type::Void); // Placeholder
            context.inferred_return_types.push(Some(Vec::new()));
        }

        context.enter_scope();

        // Reset loop depth for function body as it's a new context
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);
            context.define(
                param.name.clone(),
                param_type,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
            ); // Parameters are immutable by default
        }

        // Check body and infer implicit return type
        let implicit_return_type = match body {
            Statement::Block(stmts) => {
                context.enter_scope();
                let mut last_type = Type::Void;
                for (i, stmt) in stmts.iter().enumerate() {
                    if i == stmts.len() - 1 {
                        if let Statement::Expression(expr) = stmt {
                            last_type = self.infer_expression(expr, context);
                        } else {
                            self.check_statement(stmt, context);
                        }
                    } else {
                        self.check_statement(stmt, context);
                    }
                }
                context.exit_scope();
                last_type
            }
            Statement::Expression(expr) => self.infer_expression(expr, context),
            _ => {
                self.check_statement(body, context);
                Type::Void
            }
        };

        // Finalize return type
        let final_return_type_expr = if let Some(expected) = expected_return_type {
            let is_void_implicit = matches!(implicit_return_type, Type::Void);
            let is_void_expected = matches!(expected, Type::Void);

            if !is_void_expected && is_void_implicit {
                // Check if the last statement was a return statement?
                let ends_with_return = match body {
                    Statement::Block(stmts) => {
                        if let Some(last) = stmts.last() {
                            matches!(last, Statement::Return(_))
                        } else {
                            false
                        }
                    }
                    Statement::Return(_) => true,
                    _ => false,
                };

                if !ends_with_return {
                    self.report_error(
                        format!(
                            "Invalid return type: expected {:?}, got {:?}",
                            expected, implicit_return_type
                        ),
                        0..0, // TODO: Span
                    );
                }
            } else if !self.are_compatible(&expected, &implicit_return_type, context)
                && expected != Type::Void
            {
                self.report_error(
                    format!(
                        "Invalid return type: expected {:?}, got {:?}",
                        expected, implicit_return_type
                    ),
                    0..0, // TODO: Span
                );
            }
            return_type_expr.clone()
        } else {
            // Inference
            let collected_returns = context
                .inferred_return_types
                .pop()
                .expect("Stack should not be empty")
                .expect("Should be Some(Vec) for inference mode");
            context.return_types.pop(); // Pop the placeholder

            let mut candidate = implicit_return_type;

            for ret_ty in collected_returns {
                if candidate == Type::Void {
                    candidate = ret_ty;
                } else if ret_ty != Type::Void {
                    if !self.are_compatible(&candidate, &ret_ty, context) {
                        self.report_error(
                            format!(
                                "Incompatible return types in lambda: {:?} and {:?}",
                                candidate, ret_ty
                            ),
                            0..0,
                        );
                    }
                } else {
                    // candidate is not Void, ret_ty is Void.
                    self.report_error(
                        format!(
                            "Incompatible return types in lambda: {:?} and {:?}",
                            candidate, ret_ty
                        ),
                        0..0,
                    );
                }
            }

            Some(Box::new(self.create_type_expression(candidate)))
        };

        if return_type_expr.is_some() {
            context.return_types.pop();
            context.inferred_return_types.pop();
        }

        context.loop_depth = old_loop_depth;
        context.exit_scope();

        Type::Function(generics.clone(), params.to_vec(), final_return_type_expr)
    }

    fn infer_conditional(
        &mut self,
        then_expr: &Expression,
        cond_expr: &Expression,
        else_expr_opt: &Option<Box<Expression>>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let cond_type = self.infer_expression(cond_expr, context);
        if cond_type != Type::Boolean {
            self.report_error(
                format!(
                    "Conditional condition must be a boolean, got {:?}",
                    cond_type
                ),
                cond_expr.span.clone(),
            );
        }

        let then_type = self.infer_expression(then_expr, context);

        if let Some(else_expr) = else_expr_opt {
            let else_type = self.infer_expression(else_expr, context);
            if !self.are_compatible(&then_type, &else_type, context) {
                self.report_error(
                    format!(
                        "Conditional branches must have the same type: expected {:?}, got {:?}",
                        then_type, else_type
                    ),
                    span,
                );
            }
            then_type
        } else {
            if !self.are_compatible(&then_type, &Type::Void, context) {
                self.report_error(
                    format!(
                        "Conditional expression without else branch must return Void, got {:?}",
                        then_type
                    ),
                    span,
                );
            }
            Type::Void
        }
    }

    fn infer_formatted_string(&mut self, parts: &[Expression], context: &mut Context) -> Type {
        for part in parts {
            self.infer_expression(part, context);
        }
        Type::String
    }

    fn check_pattern(
        &mut self,
        pattern: &Pattern,
        subject_type: &Type,
        context: &mut Context,
        span: Span,
    ) {
        match pattern {
            Pattern::Literal(lit) => {
                let lit_type = self.infer_literal(lit);
                if !self.are_compatible(subject_type, &lit_type, context) {
                    self.report_error(
                        format!(
                            "Pattern type mismatch: expected {:?}, got {:?}",
                            subject_type, lit_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Identifier(name) => {
                // Bind variable
                context.define(
                    name.clone(),
                    subject_type.clone(),
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                ); // Immutable binding by default
            }
            Pattern::Tuple(patterns) => {
                if let Type::Tuple(elem_types) = subject_type {
                    if patterns.len() != elem_types.len() {
                        self.report_error(
                            format!(
                                "Tuple pattern length mismatch: expected {}, got {}",
                                elem_types.len(),
                                patterns.len()
                            ),
                            span.clone(),
                        );
                        return;
                    }

                    // Clone to avoid borrowing issues
                    let elem_types_cloned = elem_types.clone();

                    for (i, pat) in patterns.iter().enumerate() {
                        let elem_type =
                            self.resolve_type_expression(&elem_types_cloned[i], context);
                        self.check_pattern(pat, &elem_type, context, span.clone());
                    }
                } else {
                    self.report_error(
                        format!(
                            "Expected tuple type for tuple pattern, got {:?}",
                            subject_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Regex(_) => {
                if !matches!(subject_type, Type::String) {
                    self.report_error(
                        format!(
                            "Regex pattern requires string subject, got {:?}",
                            subject_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Default => {}
        }
    }

    fn infer_statement_type(&mut self, stmt: &Statement, context: &mut Context) -> Type {
        match stmt {
            Statement::Expression(expr) => self.infer_expression(expr, context),
            Statement::Block(stmts) => {
                context.enter_scope();
                let mut last_type = Type::Void;
                for (i, s) in stmts.iter().enumerate() {
                    if i == stmts.len() - 1 {
                        last_type = self.infer_statement_type(s, context);
                    } else {
                        self.check_statement(s, context);
                    }
                }
                context.exit_scope();
                last_type
            }
            _ => {
                self.check_statement(stmt, context);
                Type::Void
            }
        }
    }
}
