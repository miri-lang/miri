// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::context::{
    Context, EnumDefinition, GenericDefinition, StructDefinition, TypeDefinition,
};
use super::TypeChecker;
use crate::ast::*;
use crate::error::syntax::Span;
use std::collections::HashMap;

pub(crate) struct FunctionDeclarationInfo<'a> {
    pub name: &'a str,
    pub generics: &'a Option<Vec<Expression>>,
    pub params: &'a [Parameter],
    pub return_type: &'a Option<Box<Expression>>,
    pub body: &'a Statement,
    pub properties: &'a FunctionProperties,
}

impl TypeChecker {
    pub(crate) fn check_statement(&mut self, statement: &Statement, context: &mut Context) {
        match statement {
            Statement::Variable(decls, vis) => self.check_variable_declaration(decls, vis, context),
            Statement::Expression(expr) => {
                self.infer_expression(expr, context);
            }
            Statement::Block(stmts) => self.check_block(stmts, context),
            Statement::If(cond, then_block, else_block, _) => {
                self.check_if(cond, then_block, else_block, context)
            }
            Statement::While(cond, body, _) => self.check_while(cond, body, context),
            Statement::For(decls, iterable, body) => self.check_for(decls, iterable, body, context),
            Statement::Break => self.check_break(context),
            Statement::Continue => self.check_continue(context),
            Statement::Return(expr) => self.check_return(expr, context),
            Statement::FunctionDeclaration(name, generics, params, return_type, body, props) => {
                self.check_function_declaration(
                    FunctionDeclarationInfo {
                        name,
                        generics,
                        params,
                        return_type,
                        body,
                        properties: props,
                    },
                    context,
                )
            }
            Statement::Struct(name, generics, fields, vis) => {
                self.check_struct(name, generics, fields, vis, context)
            }
            Statement::Enum(name, variants, vis) => self.check_enum(name, variants, vis, context),
            Statement::Extends(expr) => self.check_extends_statement(expr, context),
            Statement::Implements(exprs) => self.check_implements_statement(exprs, context),
            Statement::Includes(exprs) => self.check_includes_statement(exprs, context),
            Statement::Type(exprs, visibility) => {
                self.check_type_statement(exprs, visibility, context)
            }
            _ => {}
        }
    }

    // --- Statement Checkers ---

    fn check_extends_statement(&mut self, expr: &Expression, _context: &mut Context) {
        if let Ok(parent) = self.extract_type_name(expr) {
            self.hierarchy
                .entry(self.current_module.clone())
                .or_default()
                .extends = Some(parent);
        }
    }

    fn check_implements_statement(&mut self, exprs: &[Expression], _context: &mut Context) {
        for expr in exprs {
            if let Ok(interface) = self.extract_type_name(expr) {
                self.hierarchy
                    .entry(self.current_module.clone())
                    .or_default()
                    .implements
                    .push(interface);
            }
        }
    }

    fn check_includes_statement(&mut self, exprs: &[Expression], _context: &mut Context) {
        for expr in exprs {
            if let Ok(mixin) = self.extract_type_name(expr) {
                self.hierarchy
                    .entry(self.current_module.clone())
                    .or_default()
                    .includes
                    .push(mixin);
            }
        }
    }

    fn check_type_statement(
        &mut self,
        exprs: &[Expression],
        _visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        for expr in exprs {
            if let ExpressionKind::TypeDeclaration(name_expr, _generics, kind, target_expr) =
                &expr.node
            {
                if let Ok(name) = self.extract_name(name_expr) {
                    // Handle "type F is map<string, int>"
                    if *kind == TypeDeclarationKind::Is {
                        if let Some(target) = target_expr {
                            let target_type = self.resolve_type_expression(target, context);
                            self.global_type_definitions
                                .insert(name.clone(), TypeDefinition::Alias(target_type));
                        }
                    } else if let Some(target) = target_expr {
                        if let Ok(target_name) = self.extract_type_name(target) {
                            // Register type if not exists (as empty struct/interface)
                            if !self.global_type_definitions.contains_key(&name) {
                                self.global_type_definitions.insert(
                                    name.clone(),
                                    TypeDefinition::Struct(StructDefinition {
                                        fields: vec![],
                                        generics: None,
                                        module: self.current_module.clone(),
                                    }),
                                );
                            }

                            let entry = self.hierarchy.entry(name.clone()).or_default();
                            match kind {
                                TypeDeclarationKind::Extends => entry.extends = Some(target_name),
                                TypeDeclarationKind::Implements => {
                                    entry.implements.push(target_name)
                                }
                                TypeDeclarationKind::Includes => entry.includes.push(target_name),
                                _ => {}
                            }
                        }
                    } else {
                        // "type A" - opaque type / interface
                        self.global_type_definitions.insert(
                            name.clone(),
                            TypeDefinition::Struct(StructDefinition {
                                fields: vec![],
                                generics: None,
                                module: self.current_module.clone(),
                            }),
                        );
                    }
                }
            }
        }
    }

    fn check_variable_declaration(
        &mut self,
        decls: &[VariableDeclaration],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        for decl in decls {
            let inferred_type = self.determine_variable_type(decl, context);
            let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);

            if context.scopes.len() == 1 {
                self.global_scope.insert(
                    decl.name.clone(),
                    super::context::SymbolInfo {
                        ty: inferred_type.clone(),
                        mutable: is_mutable,
                        visibility: visibility.clone(),
                        module: self.current_module.clone(),
                    },
                );
            }

            context.define(
                decl.name.clone(),
                inferred_type,
                is_mutable,
                visibility.clone(),
                self.current_module.clone(),
            );
        }
    }

    fn determine_variable_type(
        &mut self,
        decl: &VariableDeclaration,
        context: &mut Context,
    ) -> Type {
        let inferred_type = if let Some(init) = &decl.initializer {
            self.infer_expression(init, context)
        } else if let Some(type_expr) = &decl.typ {
            self.resolve_type_expression(type_expr, context)
        } else {
            self.report_error(
                format!("Cannot infer type for variable '{}'", decl.name),
                0..0,
            ); // TODO: Span
            Type::Error
        };

        // If both type annotation and initializer exist, check compatibility
        if let (Some(type_expr), Some(init)) = (&decl.typ, &decl.initializer) {
            let declared_type = self.resolve_type_expression(type_expr, context);
            if !self.are_compatible(&declared_type, &inferred_type, context) {
                self.report_error(
                    format!(
                        "Type mismatch for variable '{}': expected {:?}, got {:?}",
                        decl.name, declared_type, inferred_type
                    ),
                    init.span.clone(),
                );
            } else {
                // Check for warning: assigning non-nullable to nullable immutable variable
                if let Type::Nullable(_) = &declared_type {
                    if !matches!(decl.declaration_type, VariableDeclarationType::Mutable) {
                        // If inferred type is NOT nullable (and not None), warn
                        if !matches!(inferred_type, Type::Nullable(_)) {
                            self.report_warning(
                                format!(
                                    "Variable '{}' is immutable but declared as nullable. Consider removing '?' to make it non-nullable.",
                                    decl.name
                                ),
                                type_expr.span.clone(),
                            );
                        }
                    }
                }
            }
            return declared_type;
        }

        inferred_type
    }

    fn check_block(&mut self, stmts: &[Statement], context: &mut Context) {
        context.enter_scope();
        for s in stmts {
            self.check_statement(s, context);
        }
        context.exit_scope();
    }

    fn check_if(
        &mut self,
        cond: &Expression,
        then_block: &Statement,
        else_block: &Option<Box<Statement>>,
        context: &mut Context,
    ) {
        let cond_type = self.infer_expression(cond, context);
        if cond_type != Type::Boolean {
            self.report_error(
                format!("If condition must be a boolean, got {:?}", cond_type),
                cond.span.clone(),
            );
        }
        self.check_statement(then_block, context);
        if let Some(else_stmt) = else_block {
            self.check_statement(else_stmt, context);
        }
    }

    fn check_while(&mut self, cond: &Expression, body: &Statement, context: &mut Context) {
        let cond_type = self.infer_expression(cond, context);
        if cond_type != Type::Boolean {
            self.report_error(
                format!("While condition must be a boolean, got {:?}", cond_type),
                cond.span.clone(),
            );
        }
        context.enter_loop();
        self.check_statement(body, context);
        context.exit_loop();
    }

    fn check_for(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        let iterable_type = self.infer_expression(iterable, context);
        let element_type = self.get_iterable_element_type(&iterable_type, iterable.span.clone());

        context.enter_scope();
        context.enter_loop();

        self.bind_loop_variables(decls, &element_type, iterable.span.clone(), context);

        self.check_statement(body, context);
        context.exit_loop();
        context.exit_scope();
    }

    fn bind_loop_variables(
        &mut self,
        decls: &[VariableDeclaration],
        element_type: &Type,
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
                            "Type mismatch for loop variable '{}': expected {:?}, got {:?}",
                            decl.name, declared_type, element_type
                        ),
                        type_expr.span.clone(),
                    );
                }
                declared_type
            } else {
                element_type.clone()
            };
            let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);
            context.define(
                decl.name.clone(),
                var_type,
                is_mutable,
                MemberVisibility::Public,
                self.current_module.clone(),
            );
        } else if decls.len() == 2 {
            if let Type::Tuple(exprs) = element_type {
                if exprs.len() == 2 {
                    let key_type = self
                        .extract_type_from_expression(&exprs[0])
                        .unwrap_or(Type::Error);
                    let val_type = self
                        .extract_type_from_expression(&exprs[1])
                        .unwrap_or(Type::Error);

                    let is_mutable_0 =
                        matches!(decls[0].declaration_type, VariableDeclarationType::Mutable);
                    let is_mutable_1 =
                        matches!(decls[1].declaration_type, VariableDeclarationType::Mutable);

                    context.define(
                        decls[0].name.clone(),
                        key_type,
                        is_mutable_0,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                    );
                    context.define(
                        decls[1].name.clone(),
                        val_type,
                        is_mutable_1,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                    );
                } else {
                    self.report_error(
                        "Destructuring mismatch: expected tuple of size 2".to_string(),
                        span,
                    );
                }
            } else {
                self.report_error(
                    format!("Expected tuple for destructuring, got {:?}", element_type),
                    span,
                );
            }
        } else {
            self.report_error("Invalid number of loop variables".to_string(), span);
        }
    }

    fn check_break(&mut self, context: &Context) {
        if context.loop_depth == 0 {
            self.report_error("Break statement outside of loop".to_string(), 0..0);
        }
    }

    fn check_continue(&mut self, context: &Context) {
        if context.loop_depth == 0 {
            self.report_error("Continue statement outside of loop".to_string(), 0..0);
        }
    }

    fn check_return(&mut self, expr_opt: &Option<Box<Expression>>, context: &mut Context) {
        let actual_return_type = if let Some(expr) = expr_opt {
            self.infer_expression(expr, context)
        } else {
            Type::Void
        };

        // Check if we are inferring return types for the current function
        if let Some(Some(inferred_types)) = context.inferred_return_types.last_mut() {
            inferred_types.push(actual_return_type);
            return;
        }

        let expected_return_type = context.return_types.last().unwrap_or(&Type::Void).clone();

        if !self.are_compatible(&expected_return_type, &actual_return_type, context) {
            let span = if let Some(expr) = expr_opt {
                expr.span.clone()
            } else {
                0..0 // TODO: Need span for return statement
            };
            self.report_error(
                format!(
                    "Invalid return type: expected {:?}, got {:?}",
                    expected_return_type, actual_return_type
                ),
                span,
            );
        }
    }

    fn check_function_declaration(&mut self, info: FunctionDeclarationInfo, context: &mut Context) {
        let FunctionDeclarationInfo {
            name,
            generics,
            params,
            return_type: return_type_expr,
            body,
            properties,
        } = info;

        let func_type = Type::Function(generics.clone(), params.to_vec(), return_type_expr.clone());

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.to_string(),
                super::context::SymbolInfo {
                    ty: func_type.clone(),
                    mutable: false,
                    visibility: properties.visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name.to_string(),
            func_type,
            false,
            properties.visibility.clone(),
            self.current_module.clone(),
        ); // Functions are immutable

        context.enter_scope();

        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        let return_type = if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr, context)
        } else {
            Type::Void
        };

        context.return_types.push(return_type.clone());
        context.inferred_return_types.push(None);

        // Reset loop depth for function body as it's a new context
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);

            if let Some(default_val) = &param.default_value {
                let default_val_type = self.infer_expression(default_val, context);
                if !self.are_compatible(&param_type, &default_val_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for default value: expected {:?}, got {:?}",
                            param_type, default_val_type
                        ),
                        default_val.span.clone(),
                    );
                }
            }

            context.define(
                param.name.clone(),
                param_type,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
            ); // Parameters are immutable by default

            if let Some(guard) = &param.guard {
                if let ExpressionKind::Guard(op, right) = &guard.node {
                    let bin_op = match op {
                        GuardOp::NotEqual => BinaryOp::NotEqual,
                        GuardOp::LessThan => BinaryOp::LessThan,
                        GuardOp::LessThanEqual => BinaryOp::LessThanEqual,
                        GuardOp::GreaterThan => BinaryOp::GreaterThan,
                        GuardOp::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                        GuardOp::In => BinaryOp::In,
                        GuardOp::NotIn => BinaryOp::In, // Type check is same as In
                        GuardOp::Not => BinaryOp::NotEqual, // Assumption: not is !=
                    };

                    let left = crate::ast::factory::identifier_with_span(
                        &param.name,
                        param.typ.span.clone(),
                    );
                    let guard_type =
                        self.infer_binary(&left, &bin_op, right, guard.span.clone(), context);

                    if guard_type != Type::Boolean {
                        self.report_error(
                            format!("Type mismatch: guard must be boolean, got {:?}", guard_type),
                            guard.span.clone(),
                        );
                    }
                }
            }
        }

        match body {
            Statement::Block(stmts) => {
                context.enter_scope();
                let len = stmts.len();
                for (i, stmt) in stmts.iter().enumerate() {
                    if i == len - 1 {
                        if let Statement::Expression(expr) = stmt {
                            let expr_type = self.infer_expression(expr, context);
                            if return_type != Type::Void
                                && !self.are_compatible(&return_type, &expr_type, context)
                            {
                                self.report_error(
                                    format!(
                                        "Invalid return type: expected {:?}, got {:?}",
                                        return_type, expr_type
                                    ),
                                    expr.span.clone(),
                                );
                            }
                        } else {
                            self.check_statement(stmt, context);
                        }
                    } else {
                        self.check_statement(stmt, context);
                    }
                }
                context.exit_scope();
            }
            Statement::Expression(expr) => {
                let expr_type = self.infer_expression(expr, context);
                if return_type != Type::Void
                    && !self.are_compatible(&return_type, &expr_type, context)
                {
                    self.report_error(
                        format!(
                            "Invalid return type: expected {:?}, got {:?}",
                            return_type, expr_type
                        ),
                        expr.span.clone(),
                    );
                }
            }
            _ => {
                self.check_statement(body, context);
            }
        }

        context.loop_depth = old_loop_depth;
        context.exit_scope();
        context.return_types.pop();
        context.inferred_return_types.pop();
    }

    fn check_struct(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        fields: &[Expression],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid struct name".to_string(), name_expr.span.clone());
            return;
        };

        let mut generic_defs = Vec::new();
        context.enter_scope();
        if let Some(gens) = generics {
            self.define_generics(gens, context);
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen.node {
                    if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                        let constraint_type = constraint_expr
                            .as_ref()
                            .map(|c| self.resolve_type_expression(c, context));
                        generic_defs.push(GenericDefinition {
                            name: n.clone(),
                            constraint: constraint_type,
                            kind: kind.clone(),
                        });
                    }
                }
            }
        }

        let mut fields_vec = Vec::new();
        for field in fields {
            if let ExpressionKind::StructMember(field_name_expr, field_type_expr) = &field.node {
                if let ExpressionKind::Identifier(field_name, _) = &field_name_expr.node {
                    let field_type = self.resolve_type_expression(field_type_expr, context);
                    fields_vec.push((field_name.clone(), field_type, MemberVisibility::Public));
                } else {
                    self.report_error(
                        "Invalid struct field name".to_string(),
                        field_name_expr.span.clone(),
                    );
                }
            } else {
                self.report_error(
                    "Invalid struct field definition".to_string(),
                    field.span.clone(),
                );
            }
        }

        context.exit_scope();

        let struct_def = StructDefinition {
            fields: fields_vec,
            generics: if generic_defs.is_empty() {
                None
            } else {
                Some(generic_defs)
            },
            module: self.current_module.clone(),
        };

        context.define_type(name.clone(), TypeDefinition::Struct(struct_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Struct(struct_def));
        }

        // Define constructor/type symbol
        // The type of the struct name identifier is Meta(Custom(name))
        let struct_type = Type::Custom(name.clone(), None); // TODO: Handle generics

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                super::context::SymbolInfo {
                    ty: Type::Meta(Box::new(struct_type.clone())),
                    mutable: false,
                    visibility: visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name,
            Type::Meta(Box::new(struct_type)),
            false,
            visibility.clone(),
            self.current_module.clone(),
        );
    }

    fn check_enum(
        &mut self,
        name_expr: &Expression,
        variants: &[Expression],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid enum name".to_string(), name_expr.span.clone());
            return;
        };

        let mut variant_map = HashMap::new();
        for variant in variants {
            if let ExpressionKind::EnumValue(variant_name_expr, associated_types) = &variant.node {
                if let ExpressionKind::Identifier(variant_name, _) = &variant_name_expr.node {
                    let mut types = Vec::new();
                    for ty_expr in associated_types {
                        types.push(self.resolve_type_expression(ty_expr, context));
                    }
                    variant_map.insert(variant_name.clone(), types);
                } else {
                    self.report_error(
                        "Invalid enum variant name".to_string(),
                        variant_name_expr.span.clone(),
                    );
                }
            } else {
                self.report_error(
                    "Invalid enum variant definition".to_string(),
                    variant.span.clone(),
                );
            }
        }

        let enum_def = EnumDefinition {
            variants: variant_map,
            module: self.current_module.clone(),
        };

        context.define_type(name.clone(), TypeDefinition::Enum(enum_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Enum(enum_def));
        }

        // Define enum type symbol
        let enum_type = Type::Custom(name.clone(), None);

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                super::context::SymbolInfo {
                    ty: Type::Meta(Box::new(enum_type.clone())),
                    mutable: false,
                    visibility: visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name,
            Type::Meta(Box::new(enum_type)),
            false,
            visibility.clone(),
            self.current_module.clone(),
        );
    }
}
