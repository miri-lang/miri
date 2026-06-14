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
use crate::ast::types::{TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{
    AliasDefinition, Context, GenericDefinition, StructDefinition, SymbolInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;

pub mod control_flow;
pub mod declarations;
pub mod gpu_for_captures;
pub mod helpers;
pub mod imports;
pub mod returns;
pub mod variables;

pub(crate) use declarations::FunctionDeclarationInfo;
pub(crate) use returns::check_returns;
pub(crate) use returns::ReturnStatus;

impl TypeChecker {
    /// Checks a statement for type correctness.
    ///
    /// This method handles variable declarations, control flow, function declarations,
    /// and other statement types.
    pub(crate) fn check_statement(&mut self, statement: &Statement, context: &mut Context) {
        match &statement.node {
            StatementKind::Variable(decls, vis) => {
                self.check_variable_declaration(decls, vis, context, statement.span)
            }
            StatementKind::Expression(expr) => self.check_expr_stmt(expr, context, statement.span),
            StatementKind::Block(stmts) => self.check_block(stmts, context),
            StatementKind::If(cond, then_block, else_block, _) => {
                self.check_if(cond, then_block, else_block, context)
            }
            StatementKind::While(cond, body, _) => self.check_while(cond, body, context),
            StatementKind::For(decls, iterable, body) => {
                self.check_for(decls, iterable, body, context)
            }
            StatementKind::GpuFor(decls, iterable, body) => {
                self.check_gpu_for(decls, iterable, body, context, statement.span)
            }
            StatementKind::GpuFrame(decls, iterable, body) => {
                self.check_gpu_frame(decls, iterable, body, context, statement.span)
            }
            StatementKind::GpuFrameBlock(block) => {
                self.check_gpu_frame_block(block, context, statement.span)
            }
            StatementKind::Break => self.check_break(context, statement.span),
            StatementKind::Continue => self.check_continue(context, statement.span),
            StatementKind::Return(expr) => self.check_return(expr, context, statement.span),
            StatementKind::FunctionDeclaration(decl) => self.check_function_declaration(
                FunctionDeclarationInfo {
                    name: &decl.name,
                    generics: &decl.generics,
                    params: &decl.params,
                    return_type: &decl.return_type,
                    body: decl.body.as_ref().map(|b| b.as_ref()),
                    properties: &decl.properties,
                },
                context,
            ),
            StatementKind::Struct(name, generics, fields, methods, vis) => {
                self.check_struct(name, generics, fields, methods, vis, context)
            }
            StatementKind::Enum(name, generics, variants, methods, vis, must_use) => {
                self.check_enum(name, generics, variants, methods, *must_use, vis, context)
            }
            StatementKind::Class(class_data) => self.check_class(
                &class_data.name,
                &class_data.generics,
                &class_data.base_class,
                &class_data.traits,
                &class_data.body,
                &class_data.visibility,
                context,
                statement.span,
                class_data.is_abstract,
            ),
            StatementKind::Trait(name, generics, parent_traits, body, vis) => self.check_trait(
                name,
                generics,
                parent_traits,
                body,
                vis,
                context,
                statement.span,
            ),
            StatementKind::Type(exprs, visibility) => {
                self.check_type_statement(exprs, visibility, context)
            }
            StatementKind::RuntimeFunctionDeclaration(_runtime, name, params, return_type_expr) => {
                self.check_runtime_fn_decl(name, params, return_type_expr, context)
            }
            StatementKind::IntrinsicFunctionDeclaration(
                name,
                generics,
                params,
                return_type_expr,
                visibility,
            ) => self.check_intrinsic_fn_decl(
                name,
                generics,
                params,
                return_type_expr,
                visibility,
                context,
            ),
            StatementKind::Use(path_expr, alias) => {
                self.check_use(path_expr, alias, context);
            }
            StatementKind::Empty => {}
        }
    }

    fn check_expr_stmt(&mut self, expr: &Expression, context: &mut Context, span: Span) {
        let expr_type = self.infer_expression(expr, context);
        if !context.suppress_must_use {
            if let TypeKind::Custom(type_name, _) = &expr_type.kind {
                if let Some(TypeDefinition::Enum(def)) =
                    self.global_type_definitions.get(type_name.as_str())
                {
                    if def.must_use {
                        self.report_error(
                            format!(
                                "Unused value of type '{}': this value must be used",
                                type_name
                            ),
                            span,
                        );
                    }
                }
            }
        }
        self.check_gpu_discarded_expression(&expr_type, context, span);
    }

    /// Inside a `gpu fn`, an expression-statement whose value is discarded
    /// (e.g. `echo()` where `echo` returns `String`) must still produce a
    /// GPU-compatible type — otherwise the call would have to materialize a
    /// forbidden value at runtime. Variable bindings are validated elsewhere
    /// (see [`Self::check_gpu_variable_type`]).
    fn check_gpu_discarded_expression(&mut self, expr_type: &Type, context: &Context, span: Span) {
        if !context.in_gpu_function {
            return;
        }
        if matches!(expr_type.kind, TypeKind::Error) {
            return;
        }
        if crate::type_checker::utils::is_gpu_compatible(&expr_type.kind) {
            return;
        }
        self.report_error(
            format!(
                "Discarded value of type '{}' is not GPU-compatible: only numeric primitives, booleans, and GPU types may be produced inside a 'gpu fn'",
                expr_type
            ),
            span,
        );
    }

    fn check_runtime_fn_decl(
        &mut self,
        name: &str,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        context: &mut Context,
    ) {
        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: params.to_vec(),
            return_type: return_type_expr.clone(),
        })));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.to_string(),
                SymbolInfo::new(
                    func_type.clone(),
                    false,
                    false,
                    MemberVisibility::Private,
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name.to_string(),
            SymbolInfo::new(
                func_type,
                false,
                false,
                MemberVisibility::Private,
                self.current_module.clone(),
                None,
            ),
        );

        for param in params {
            self.resolve_type_expression(&param.typ, context);
        }

        if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr, context);
        }
    }

    fn check_intrinsic_fn_decl(
        &mut self,
        name: &str,
        generics: &Option<Vec<Expression>>,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: generics.clone(),
            params: params.to_vec(),
            return_type: return_type_expr.clone(),
        })));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.to_string(),
                SymbolInfo::new_intrinsic(
                    func_type.clone(),
                    visibility.clone(),
                    self.current_module.clone(),
                ),
            );
        }

        context.define(
            name.to_string(),
            SymbolInfo::new_intrinsic(func_type, visibility.clone(), self.current_module.clone()),
        );

        if let Some(gens) = generics {
            context.enter_scope();
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint, kind) = &gen.node {
                    if let ExpressionKind::Identifier(gen_name, _) = &name_expr.node {
                        let constraint_ty = constraint
                            .as_ref()
                            .map(|c| self.resolve_type_expression(c, context));
                        context.define_type(
                            gen_name.clone(),
                            TypeDefinition::Generic(GenericDefinition {
                                name: gen_name.clone(),
                                constraint: constraint_ty,
                                kind: *kind,
                            }),
                        );
                    }
                }
            }
        }

        for param in params {
            self.resolve_type_expression(&param.typ, context);
        }

        if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr, context);
        }

        if generics.is_some() {
            context.exit_scope();
        }
    }

    pub(crate) fn check_type_statement(
        &mut self,
        exprs: &[Expression],
        _visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        for expr in exprs {
            if let ExpressionKind::TypeDeclaration(name_expr, generics, kind, target_expr) =
                &expr.node
            {
                if let Ok(name) = self.extract_name(name_expr) {
                    if *kind == TypeDeclarationKind::None && target_expr.is_none() {
                        self.report_error(
                            format!(
                                "Incomplete type declaration '{}'. Use 'is', 'extends', 'implements', or 'includes' to define the type.",
                                name
                            ),
                            expr.span,
                        );
                        continue;
                    }
                    let name = name.to_string();

                    if *kind == TypeDeclarationKind::Is {
                        if let Some(target) = target_expr {
                            self.check_type_alias(&name, generics, target, context);
                        }
                    } else if let Some(target) = target_expr {
                        self.check_type_hierarchy(&name, kind, target, context, expr.span);
                    }
                }
            }
        }
    }

    fn check_type_alias(
        &mut self,
        name: &str,
        generics: &Option<Vec<Expression>>,
        target: &Expression,
        context: &mut Context,
    ) {
        let generic_defs = self.extract_generic_defs(generics, context);

        if let Some(ref gens) = generics {
            context.enter_scope();
            self.define_generics(gens, context);
        }

        let target_type = self.resolve_type_expression(target, context);

        if generics.is_some() {
            context.exit_scope();
        }

        self.register_type_definition(
            name.to_string(),
            TypeDefinition::Alias(AliasDefinition {
                template: target_type,
                generics: generic_defs,
            }),
        );
    }

    fn extract_generic_defs(
        &mut self,
        generics: &Option<Vec<Expression>>,
        context: &mut Context,
    ) -> Option<Vec<GenericDefinition>> {
        if let Some(gens) = generics {
            let mut defs = Vec::with_capacity(gens.len());
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint_expr, gen_kind) = &gen.node
                {
                    let gen_name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                        n.clone()
                    } else {
                        continue;
                    };
                    let constraint_type = constraint_expr
                        .as_ref()
                        .map(|c| self.resolve_type_expression(c, context));
                    defs.push(GenericDefinition {
                        name: gen_name,
                        constraint: constraint_type,
                        kind: *gen_kind,
                    });
                }
            }
            if defs.is_empty() {
                None
            } else {
                Some(defs)
            }
        } else {
            None
        }
    }

    fn check_type_hierarchy(
        &mut self,
        name: &str,
        kind: &TypeDeclarationKind,
        target: &Expression,
        _context: &mut Context,
        span: Span,
    ) {
        if self.is_type_visible(name) {
            self.report_error(
                format!(
                    "Type '{}' is already defined. Cannot use 'type' statement with '{}' on an existing type.",
                    name, kind
                ),
                span,
            );
            return;
        }

        if let Ok(target_name) = self.extract_type_name(target) {
            if !self.is_type_visible(target_name) {
                self.report_error(
                    format!("Unknown type '{}' in type declaration", target_name),
                    target.span,
                );
                return;
            }
            let target_name = target_name.to_string();

            self.register_type_definition(
                name.to_string(),
                TypeDefinition::Struct(StructDefinition {
                    fields: vec![],
                    generics: None,
                    has_drop: false,
                    module: self.current_module.clone(),
                }),
            );

            let entry = self.hierarchy.entry(name.to_string()).or_default();
            match kind {
                TypeDeclarationKind::Extends => entry.extends = Some(target_name),
                TypeDeclarationKind::Implements => entry.implements.push(target_name),
                TypeDeclarationKind::Includes => entry.includes.push(target_name),
                _ => {}
            }
        }
    }
}
