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

use super::{check_returns, ReturnStatus};
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{
    ClassDefinition, Context, EnumDefinition, FieldInfo, GenericDefinition, MethodInfo,
    StructDefinition, SymbolInfo, TraitDefinition, TypeDefinition,
};
use crate::type_checker::TypeChecker;
use std::collections::{BTreeMap, HashMap};

pub(crate) struct FunctionDeclarationInfo<'a> {
    pub name: &'a str,
    pub generics: &'a Option<Vec<Expression>>,
    pub params: &'a [Parameter],
    pub return_type: &'a Option<Box<Expression>>,
    pub body: Option<&'a Statement>, // None for abstract functions
    pub properties: &'a FunctionProperties,
}

impl TypeChecker {
    /// Type-checks a function declaration.
    ///
    /// Registers the function in the appropriate scope, validates parameter types,
    /// guards, return type, and checks the function body for type correctness.
    /// Handles GPU functions, async functions, and implicit return type inference.
    pub(crate) fn check_function_declaration(
        &mut self,
        info: FunctionDeclarationInfo,
        context: &mut Context,
    ) {
        let FunctionDeclarationInfo {
            name,
            generics,
            params,
            return_type: return_type_expr,
            body,
            properties,
        } = info;

        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: generics.clone(),
            params: params.to_vec(),
            return_type: return_type_expr.clone(),
        })));

        // Don't let imported non-generic functions shadow built-in generic ones
        // (e.g. system.io's print(String) should not override the built-in print<T>)
        let is_shadowing_builtin_generic = if let Some(existing) = self.global_scope.get(name) {
            existing.module == "std"
                && matches!(&existing.ty.kind, TypeKind::Function(fd) if fd.generics.as_ref().is_some_and(|g| !g.is_empty()))
                && self.current_module != "Main"
        } else {
            false
        };

        if !is_shadowing_builtin_generic {
            if context.scopes.len() == 1 {
                self.global_scope.insert(
                    name.to_string(),
                    SymbolInfo::new(
                        func_type.clone(),
                        false,
                        false,
                        properties.visibility.clone(),
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
                    properties.visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            ); // Functions are immutable
        }

        context.enter_scope();

        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        let return_type = if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr, context)
        } else {
            make_type(TypeKind::Void)
        };

        context.return_types.push(return_type.clone());
        context.inferred_return_types.push(None);

        // Reset loop depth for function body as it's a new context
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        // If this is 'main' with implicit return type, we might infer it from the body
        let infer_main_return = name == "main" && return_type_expr.is_none();

        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);

            if let Some(default_val) = &param.default_value {
                let default_val_type = self.infer_expression(default_val, context);
                if !self.are_compatible(&param_type, &default_val_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for default value: expected {}, got {}",
                            param_type, default_val_type
                        ),
                        default_val.span,
                    );
                }
            }

            context.define(
                param.name.clone(),
                SymbolInfo::new(
                    param_type,
                    false,
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );
            // Parameters are immutable by default

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

                    let left =
                        crate::ast::factory::identifier_with_span(&param.name, param.typ.span);
                    let guard_type = self.infer_binary(&left, &bin_op, right, guard.span, context);

                    if !matches!(guard_type.kind, TypeKind::Boolean) {
                        self.report_error(
                            format!("Type mismatch: guard must be boolean, got {}", guard_type),
                            guard.span,
                        );
                    }
                }
            }
        }

        // Handle GPU functions
        let previous_in_gpu = context.in_gpu_function;
        if properties.is_gpu {
            context.in_gpu_function = true;

            // Enforce NO explicit return type in source code
            if let Some(rt_expr) = return_type_expr {
                self.report_error(
                    "GPU functions must not have an explicit return type".to_string(),
                    rt_expr.span,
                );
            }

            // Implicitly set return type to Kernel
            // Note: The `func_type` symbol stored in global_scope above was created using `return_type_expr`.
            // We need to update that symbol to return `Kernel` so that calls to it are typed correctly.
            let kernel_return_type = make_type(TypeKind::Custom("Kernel".to_string(), None));

            if let Some(info) = self.global_scope.get_mut(name) {
                if let TypeKind::Function(func) = &info.ty.kind {
                    info.ty = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                        generics: func.generics.clone(),
                        params: func.params.clone(),
                        return_type: Some(Box::new(crate::ast::factory::type_expr_non_null(
                            kernel_return_type.clone(),
                        ))),
                    })));
                }
            }
            context.update_symbol_type(
                name,
                make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: generics.clone(),
                    params: params.to_vec(),
                    return_type: Some(Box::new(crate::ast::factory::type_expr_non_null(
                        kernel_return_type.clone(),
                    ))),
                }))),
            );

            // Inject 'gpu_context' object (type GpuContext)
            let gpu_context_type = make_type(TypeKind::Custom("GpuContext".to_string(), None));
            context.define(
                "gpu_context".to_string(),
                SymbolInfo::new(
                    gpu_context_type,
                    false, // Immutable
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        // Track function context for await validation
        let previous_in_function = context.in_function;
        let previous_in_async = context.in_async_function;
        context.in_function = true;
        context.in_async_function = properties.is_async;

        // Only check function body if it exists (abstract functions have no body)
        if let Some(body) = body {
            match &body.node {
                StatementKind::Block(stmts) => {
                    // Note: Do not enter a new scope here - the function body shares the scope with parameters.

                    // First, check all statements normally
                    for stmt in stmts.iter() {
                        self.check_statement(stmt, context);
                    }

                    // For implicit return inference, find the last meaningful statement
                    // (skip trailing empty blocks which can be created by trailing whitespace)
                    if infer_main_return {
                        // Find the last non-empty statement that could provide a return value
                        let last_meaningful_stmt = stmts.iter().rev().find(|stmt| {
                            !matches!(&stmt.node, StatementKind::Block(inner) if inner.is_empty())
                        });

                        if let Some(stmt) = last_meaningful_stmt {
                            if let Some(expr_type) = self.resolve_implicit_return_type(stmt) {
                                self.register_implicit_main_return(name, expr_type, context);
                            }
                        }
                    } else if !matches!(return_type.kind, TypeKind::Void) {
                        // For non-main functions with explicit return type, check the last expression
                        if let Some(last_stmt) = stmts.last() {
                            if let StatementKind::Expression(expr) = &last_stmt.node {
                                let expr_type = self.infer_expression(expr, context);
                                if !self.are_compatible(&return_type, &expr_type, context) {
                                    self.report_error(
                                        format!(
                                            "Invalid return type: expected {}, got {}",
                                            return_type, expr_type
                                        ),
                                        expr.span,
                                    );
                                }
                            }
                        }
                    }
                }
                StatementKind::Expression(expr) => {
                    let expr_type = self.infer_expression(expr, context);

                    if !infer_main_return
                        && !matches!(return_type.kind, TypeKind::Void)
                        && !self.are_compatible(&return_type, &expr_type, context)
                    {
                        self.report_error(
                            format!(
                                "Invalid return type: expected {}, got {}",
                                return_type, expr_type
                            ),
                            expr.span,
                        );
                    }

                    if infer_main_return {
                        // Implicit return for single-expression main
                        self.register_implicit_main_return(name, expr_type, context);
                    }
                }
                _ => {
                    self.check_statement(body, context);
                }
            }

            if !matches!(return_type.kind, TypeKind::Void) {
                let status = check_returns(body);
                if status == ReturnStatus::None {
                    self.report_error("Missing return statement".to_string(), body.span);
                }
            }
        }

        context.in_gpu_function = previous_in_gpu;
        context.in_function = previous_in_function;
        context.in_async_function = previous_in_async;
        context.loop_depth = old_loop_depth;
        context.exit_scope();
        context.return_types.pop();
        context.inferred_return_types.pop();
    }

    pub(crate) fn check_struct(
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
            self.report_error("Invalid struct name".to_string(), name_expr.span);
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
                        field_name_expr.span,
                    );
                }
            } else {
                self.report_error("Invalid struct field definition".to_string(), field.span);
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
        let struct_type = make_type(TypeKind::Custom(name.clone(), None)); // TODO: Handle generics

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(struct_type.clone()))),
                    false,
                    false,
                    visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name,
            SymbolInfo::new(
                make_type(TypeKind::Meta(Box::new(struct_type))),
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }

    pub(crate) fn check_enum(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        variants: &[Expression],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid enum name".to_string(), name_expr.span);
            return;
        };

        // Handle generics
        let mut generic_defs = None;
        if let Some(gens) = generics {
            context.enter_scope();
            self.define_generics(gens, context);

            let mut defs = Vec::new();
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint, kind) = &gen.node {
                    if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                        let constraint_type = constraint
                            .as_ref()
                            .map(|c| self.resolve_type_expression(c, context));
                        defs.push(GenericDefinition {
                            name: n.clone(),
                            constraint: constraint_type,
                            kind: kind.clone(),
                        });
                    }
                }
            }
            generic_defs = Some(defs);
        }

        let mut variant_map = BTreeMap::new();
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
                        variant_name_expr.span,
                    );
                }
            } else {
                self.report_error("Invalid enum variant definition".to_string(), variant.span);
            }
        }

        let enum_def = EnumDefinition {
            variants: variant_map,
            generics: generic_defs.clone(),
            module: self.current_module.clone(),
        };

        if generics.is_some() {
            context.exit_scope();
        }

        context.define_type(name.clone(), TypeDefinition::Enum(enum_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Enum(enum_def));
        }

        // Define enum type symbol
        let enum_type = if let Some(defs) = generic_defs {
            let args = defs
                .iter()
                .map(|g| {
                    crate::ast::factory::type_expr_non_null(make_type(TypeKind::Custom(
                        g.name.clone(),
                        None,
                    )))
                })
                .collect();
            make_type(TypeKind::Custom(name.clone(), Some(args)))
        } else {
            make_type(TypeKind::Custom(name.clone(), None))
        };

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(enum_type.clone()))),
                    false,
                    false,
                    visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name,
            SymbolInfo::new(
                make_type(TypeKind::Meta(Box::new(enum_type))),
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_class(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        base_class: &Option<Box<Expression>>,
        traits: &[Expression],
        body: &[Statement],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
        is_abstract: bool,
    ) {
        // Extract class name
        let name = match self.extract_type_name(name_expr) {
            Ok(n) => n,
            Err(_) => {
                self.report_error("Invalid class name".to_string(), name_expr.span);
                return;
            }
        };

        // Check for duplicate type definitions
        if self.global_type_definitions.contains_key(&name) {
            self.report_error(format!("Type '{}' is already defined", name), span);
            return;
        }

        // Process generics
        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));

        // Validate base class exists
        let base_class_name = if let Some(base_expr) = base_class {
            match self.extract_type_name(base_expr) {
                Ok(base_name) => {
                    // Check base class exists
                    if !self.global_type_definitions.contains_key(&base_name) {
                        self.report_error(
                            format!("Base class '{}' is not defined", base_name),
                            base_expr.span,
                        );
                    }
                    Some(base_name)
                }
                Err(_) => {
                    self.report_error("Invalid base class name".to_string(), base_expr.span);
                    None
                }
            }
        } else {
            None
        };

        // Check for circular inheritance
        if let Some(ref base_name) = base_class_name {
            let mut visited = std::collections::HashSet::new();
            visited.insert(name.clone());
            let mut current = base_name.clone();
            loop {
                if visited.contains(&current) {
                    self.report_error(
                        format!(
                            "Circular inheritance detected: class '{}' eventually extends itself",
                            name
                        ),
                        span,
                    );
                    break;
                }
                visited.insert(current.clone());
                // Get the base class of current
                if let Some(relation) = self.hierarchy.get(&current) {
                    if let Some(ref next_base) = relation.extends {
                        current = next_base.clone();
                    } else {
                        break; // No more base classes
                    }
                } else {
                    break; // Class not in hierarchy yet (could be defined later)
                }
            }
        }

        // Validate traits exist
        let mut trait_names = Vec::new();
        for trait_expr in traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.global_type_definitions.contains_key(&trait_name) {
                    self.report_error(
                        format!("Trait '{}' is not defined", trait_name),
                        trait_expr.span,
                    );
                }
                trait_names.push(trait_name);
            }
        }

        // Register class in hierarchy for is_subtype checks (protected visibility, etc.)
        {
            let entry = self.hierarchy.entry(name.clone()).or_default();
            if let Some(ref base_name) = base_class_name {
                entry.extends = Some(base_name.clone());
            }
            for trait_name in &trait_names {
                entry.implements.push(trait_name.clone());
            }
        }

        // Enter class scope
        context.enter_scope();

        // Define generics in scope
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Set class context for self/super resolution
        let class_type = make_type(TypeKind::Custom(name.clone(), None));
        context.enter_class(name.clone(), base_class_name.clone(), class_type);

        // PASS 1: Collect fields and method signatures (without checking bodies)
        let mut fields: BTreeMap<String, FieldInfo> = BTreeMap::new();
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();
        // Store method info for second pass body checking
        let mut method_statements: Vec<&Statement> = Vec::new();

        for stmt in body {
            match &stmt.node {
                StatementKind::Variable(decls, vis) => {
                    for decl in decls {
                        let field_type = if let Some(type_expr) = &decl.typ {
                            self.resolve_type_expression(type_expr, context)
                        } else if let Some(init) = &decl.initializer {
                            self.infer_expression(init, context)
                        } else {
                            self.report_error(
                                format!("Cannot infer type for field '{}'", decl.name),
                                stmt.span,
                            );
                            make_type(TypeKind::Error)
                        };

                        let is_mutable = match decl.declaration_type {
                            VariableDeclarationType::Mutable => true,
                            VariableDeclarationType::Immutable
                            | VariableDeclarationType::Constant => false,
                        };

                        fields.insert(
                            decl.name.clone(),
                            FieldInfo {
                                ty: field_type,
                                mutable: is_mutable,
                                visibility: vis.clone(),
                            },
                        );
                    }
                }
                StatementKind::FunctionDeclaration(decl) => {
                    // Collect method signature only (don't check body yet)
                    let return_ty = if let Some(rt_expr) = &decl.return_type {
                        self.resolve_type_expression(rt_expr, context)
                    } else {
                        make_type(TypeKind::Void)
                    };

                    let param_types: Vec<(String, Type)> = decl
                        .params
                        .iter()
                        .map(|p| {
                            (
                                p.name.clone(),
                                self.resolve_type_expression(&p.typ, context),
                            )
                        })
                        .collect();

                    // Method is abstract if it has no body OR has an empty body
                    let method_is_abstract = decl.body.as_ref().is_none_or(|body| {
                        matches!(&body.node, StatementKind::Empty)
                            || matches!(&body.node, StatementKind::Block(stmts) if stmts.is_empty())
                    });

                    methods.insert(
                        decl.name.clone(),
                        MethodInfo {
                            params: param_types,
                            return_type: return_ty,
                            visibility: decl.properties.visibility.clone(),
                            is_constructor: decl.name == "init",
                            is_abstract: method_is_abstract,
                        },
                    );

                    // Save for second pass
                    method_statements.push(stmt);
                }
                StatementKind::RuntimeFunctionDeclaration(
                    _runtime,
                    rt_name,
                    params,
                    return_type_expr,
                ) => {
                    // Runtime functions inside a class are extern bindings used
                    // by the class methods. Register them in scope so calls
                    // type-check, and also in the global scope for codegen.
                    let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                        generics: None,
                        params: params.to_vec(),
                        return_type: return_type_expr.clone(),
                    })));

                    self.global_scope.insert(
                        rt_name.to_string(),
                        SymbolInfo::new(
                            func_type.clone(),
                            false,
                            false,
                            MemberVisibility::Private,
                            self.current_module.clone(),
                            None,
                        ),
                    );

                    context.define(
                        rt_name.to_string(),
                        SymbolInfo::new(
                            func_type,
                            false,
                            false,
                            MemberVisibility::Private,
                            self.current_module.clone(),
                            None,
                        ),
                    );
                }
                StatementKind::Empty => {}
                _ => {
                    self.report_error(
                        "Only field and method declarations are allowed in class body".to_string(),
                        stmt.span,
                    );
                }
            }
        }

        // Validate: non-abstract classes cannot have abstract methods
        if !is_abstract {
            for (method_name, method_info) in &methods {
                if method_info.is_abstract {
                    self.report_error(
                        format!(
                            "Non-abstract class '{}' cannot have abstract method '{}'",
                            name, method_name
                        ),
                        span,
                    );
                }
            }
        }

        // Validate: method overrides must have compatible signatures
        let override_errors: Vec<String> = if let Some(ref base_name) = base_class_name {
            let mut errors = Vec::new();
            // Walk up the inheritance chain to find parent methods
            let mut current_base = Some(base_name.clone());
            while let Some(ref class_name) = current_base {
                if let Some(TypeDefinition::Class(base_def)) =
                    self.global_type_definitions.get(class_name)
                {
                    for (method_name, child_method) in &methods {
                        // Skip constructor (init) - constructors can have different signatures
                        if method_name == "init" {
                            continue;
                        }
                        if let Some(parent_method) = base_def.methods.get(method_name) {
                            // Check parameter count
                            if child_method.params.len() != parent_method.params.len() {
                                errors.push(format!(
                                    "Method '{}' has incompatible parameter count: parent has {} parameters, child has {}",
                                    method_name,
                                    parent_method.params.len(),
                                    child_method.params.len()
                                ));
                            } else {
                                // Check parameter types
                                for (i, ((child_name, child_type), (_, parent_type))) in
                                    child_method
                                        .params
                                        .iter()
                                        .zip(parent_method.params.iter())
                                        .enumerate()
                                {
                                    if child_type.kind != parent_type.kind {
                                        errors.push(format!(
                                            "Method '{}' has incompatible parameter type for '{}' (position {}): expected {}, got {}",
                                            method_name,
                                            child_name,
                                            i + 1,
                                            parent_type,
                                            child_type
                                        ));
                                    }
                                }
                            }

                            // Check return type
                            if child_method.return_type.kind != parent_method.return_type.kind {
                                errors.push(format!(
                                    "Method '{}' has incompatible return type: expected {}, got {}",
                                    method_name,
                                    parent_method.return_type,
                                    child_method.return_type
                                ));
                            }
                        }
                    }
                    // Move to the next ancestor
                    current_base = base_def.base_class.clone();
                } else {
                    break;
                }
            }
            errors
        } else {
            Vec::new()
        };

        // Report override errors
        for error in override_errors {
            self.report_error(error, span);
        }

        // Validate: child class init must call super.init() when parent has accessible init
        if let Some(ref base_name) = base_class_name {
            // Check if parent has an accessible init method
            let parent_has_init = {
                let mut has_init = false;
                let mut current_base = Some(base_name.clone());
                while let Some(ref check_class) = current_base {
                    if let Some(TypeDefinition::Class(base_def)) =
                        self.global_type_definitions.get(check_class)
                    {
                        if let Some(init_method) = base_def.methods.get("init") {
                            // Parent's init must be accessible (public or protected)
                            if matches!(
                                init_method.visibility,
                                MemberVisibility::Public | MemberVisibility::Protected
                            ) {
                                has_init = true;
                                break;
                            }
                        }
                        current_base = base_def.base_class.clone();
                    } else {
                        break;
                    }
                }
                has_init
            };

            // If parent has init and child has init, check for super.init() call
            if parent_has_init {
                if let Some(child_init) = methods.get("init") {
                    // We need to check if super.init() is called in the init body
                    // Look through method_statements to find the init body
                    let mut found_super_init = false;
                    for stmt in &method_statements {
                        if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                            if decl.name == "init" {
                                if let Some(method_body) = &decl.body {
                                    found_super_init = self.contains_super_init_call(method_body);
                                }
                                break;
                            }
                        }
                    }

                    if !found_super_init && !child_init.is_abstract {
                        self.report_error(
                            format!(
                                "Constructor 'init' in class '{}' must call super.init() because parent class '{}' has a constructor",
                                name, base_name
                            ),
                            span,
                        );
                    }
                }
            }
        }

        // Validate: non-abstract classes must implement all abstract methods from inheritance chain
        if !is_abstract {
            if let Some(ref base_name) = base_class_name {
                // Collect all abstract methods from the entire inheritance chain
                let missing_methods: Vec<(String, String)> = {
                    let mut missing = Vec::new();
                    let mut current_base = Some(base_name.clone());

                    while let Some(ref class_name) = current_base {
                        if let Some(TypeDefinition::Class(base_def)) =
                            self.global_type_definitions.get(class_name)
                        {
                            for (method_name, method_info) in &base_def.methods {
                                if method_info.is_abstract && !methods.contains_key(method_name) {
                                    missing.push((method_name.clone(), class_name.clone()));
                                }
                            }
                            // Move to the next ancestor
                            current_base = base_def.base_class.clone();
                        } else {
                            break;
                        }
                    }
                    missing
                };

                // Report errors for missing methods
                for (method_name, origin_class) in missing_methods {
                    self.report_error(
                        format!(
                            "Class '{}' must implement abstract method '{}' from class '{}'",
                            name, method_name, origin_class
                        ),
                        span,
                    );
                }
            }
        }

        // Validate: classes must implement all required trait methods (including parent traits)
        for trait_name in &trait_names {
            // Collect all methods from trait hierarchy (including parent traits)
            let all_trait_methods: HashMap<String, (MethodInfo, String)> = {
                let mut all_methods = HashMap::new();
                let mut traits_to_check = vec![trait_name.clone()];
                let mut visited_traits = std::collections::HashSet::new();

                while let Some(current_trait_name) = traits_to_check.pop() {
                    if visited_traits.contains(&current_trait_name) {
                        continue;
                    }
                    visited_traits.insert(current_trait_name.clone());

                    if let Some(TypeDefinition::Trait(trait_def)) =
                        self.global_type_definitions.get(&current_trait_name)
                    {
                        // Add methods from this trait
                        for (method_name, method_info) in &trait_def.methods {
                            // Don't overwrite if already added (child trait methods take precedence)
                            if !all_methods.contains_key(method_name) {
                                all_methods.insert(
                                    method_name.clone(),
                                    (method_info.clone(), current_trait_name.clone()),
                                );
                            }
                        }

                        // Add parent traits to check
                        for parent_trait in &trait_def.parent_traits {
                            traits_to_check.push(parent_trait.clone());
                        }
                    }
                }
                all_methods
            };

            // Collect missing and mismatched methods
            let mut missing_methods: Vec<(String, String)> = Vec::new();
            let mut mismatched_methods: Vec<(String, String)> = Vec::new();

            for (method_name, (method_info, origin_trait)) in &all_trait_methods {
                // Check if method is required (abstract, no default implementation)
                if method_info.is_abstract && !methods.contains_key(method_name) {
                    missing_methods.push((method_name.clone(), origin_trait.clone()));
                }

                // Check signature compatibility if method exists
                if let Some(class_method) = methods.get(method_name) {
                    // When checking trait compliance, the trait's own type (from Self)
                    // should match the implementing class type. For example,
                    // trait Equatable defines `fn equals(other Self) bool` which
                    // resolves Self to Custom("Equatable", None), but the class
                    // String implements `fn equals(other String) bool`.
                    let class_type_kind = TypeKind::Custom(name.clone(), None);
                    let trait_self_kind = TypeKind::Custom(origin_trait.clone(), None);

                    let types_match = |trait_ty: &TypeKind, class_ty: &TypeKind| -> bool {
                        if trait_ty == class_ty {
                            return true;
                        }
                        // Self in trait resolves to Custom(trait_name, None).
                        // The class type is either Custom(class_name, None) or String.
                        if *trait_ty == trait_self_kind {
                            return *class_ty == class_type_kind
                                || (name == "String" && *class_ty == TypeKind::String);
                        }
                        false
                    };

                    let params_match = method_info.params.len() == class_method.params.len()
                        && method_info
                            .params
                            .iter()
                            .zip(class_method.params.iter())
                            .all(|((_, t1), (_, t2))| types_match(&t1.kind, &t2.kind));
                    let return_match = types_match(
                        &method_info.return_type.kind,
                        &class_method.return_type.kind,
                    );

                    if !params_match || !return_match {
                        let expected = format!(
                            "fn {}({}) -> {:?}",
                            method_name,
                            method_info
                                .params
                                .iter()
                                .map(|(n, t)| format!("{}: {:?}", n, t.kind))
                                .collect::<Vec<_>>()
                                .join(", "),
                            method_info.return_type.kind
                        );
                        mismatched_methods.push((method_name.clone(), expected));
                    }
                }
            }

            // Report errors for missing methods
            for (method_name, origin_trait) in missing_methods {
                self.report_error(
                    format!(
                        "Class '{}' must implement method '{}' from trait '{}'",
                        name, method_name, origin_trait
                    ),
                    span,
                );
            }

            // Report errors for signature mismatches
            for (method_name, expected_sig) in mismatched_methods {
                self.report_error(
                    format!(
                        "Method '{}' in class '{}' does not match trait '{}' signature: expected {}",
                        method_name, name, trait_name, expected_sig
                    ),
                    span,
                );
            }
        }

        // Create and register class definition BEFORE checking method bodies
        let class_def = ClassDefinition {
            name: name.clone(),
            generics: generic_defs,
            base_class: base_class_name.clone(),
            traits: trait_names.clone(),
            fields,
            methods,
            module: self.current_module.clone(),
            is_abstract,
        };

        // Register class type definition so self.* lookups work
        context.define_type(name.clone(), TypeDefinition::Class(class_def.clone()));
        // scopes.len() == 2 because we're in [base_scope, class_scope]
        if context.scopes.len() == 2 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Class(class_def.clone()));
        }

        // Define class type symbol (as a constructor/type)
        let class_type_meta = make_type(TypeKind::Meta(Box::new(make_type(TypeKind::Custom(
            name.clone(),
            None,
        )))));

        // scopes.len() == 2 because we're in [base_scope, class_scope]
        if context.scopes.len() == 2 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    class_type_meta.clone(),
                    false,
                    false,
                    visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name.clone(),
            SymbolInfo::new(
                class_type_meta,
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );

        // PASS 2: Check method bodies (now class is registered)
        // Skip abstract methods (no body) as they don't need body checking
        for stmt in method_statements {
            if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                // Skip abstract methods (those with no body or empty body)
                let is_abstract = decl.body.as_ref().is_none_or(|body| {
                    matches!(&body.node, StatementKind::Empty)
                        || matches!(&body.node, StatementKind::Block(stmts) if stmts.is_empty())
                });
                if is_abstract {
                    continue;
                }

                self.check_function_declaration(
                    FunctionDeclarationInfo {
                        name: &decl.name,
                        generics: &decl.generics,
                        params: &decl.params,
                        return_type: &decl.return_type,
                        body: decl.body.as_ref().map(|b| b.as_ref()),
                        properties: &decl.properties,
                    },
                    context,
                );
            }
        }

        // Exit class context
        context.exit_class();
        context.exit_scope();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_trait(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        parent_traits: &[Expression],
        body: &[Statement],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        // Extract trait name
        let name = match self.extract_type_name(name_expr) {
            Ok(n) => n,
            Err(_) => {
                self.report_error("Invalid trait name".to_string(), name_expr.span);
                return;
            }
        };

        // Check for duplicate type definitions
        if self.global_type_definitions.contains_key(&name) {
            self.report_error(format!("Type '{}' is already defined", name), span);
            return;
        }

        // Process generics
        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));

        // Validate parent traits exist
        let mut parent_trait_names = Vec::new();
        for trait_expr in parent_traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.global_type_definitions.contains_key(&trait_name) {
                    self.report_error(
                        format!("Parent trait '{}' is not defined", trait_name),
                        trait_expr.span,
                    );
                }
                parent_trait_names.push(trait_name);
            }
        }

        // Enter trait scope
        context.enter_scope();

        // Set trait context so `Self` resolves inside method signatures
        let trait_type = make_type(TypeKind::Custom(name.clone(), None));
        context.enter_class(name.clone(), None, trait_type);

        // Define generics in scope
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Process trait body to collect methods
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();

        for stmt in body {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) => {
                    // Check the function declaration
                    self.check_function_declaration(
                        FunctionDeclarationInfo {
                            name: &decl.name,
                            generics: &decl.generics,
                            params: &decl.params,
                            return_type: &decl.return_type,
                            body: decl.body.as_ref().map(|b| b.as_ref()),
                            properties: &decl.properties,
                        },
                        context,
                    );

                    // Collect method info
                    let return_ty = if let Some(rt_expr) = &decl.return_type {
                        self.resolve_type_expression(rt_expr, context)
                    } else {
                        make_type(TypeKind::Void)
                    };

                    let param_types: Vec<(String, Type)> = decl
                        .params
                        .iter()
                        .map(|p| {
                            (
                                p.name.clone(),
                                self.resolve_type_expression(&p.typ, context),
                            )
                        })
                        .collect();

                    // Trait methods are abstract if they have no body
                    let method_is_abstract = decl.body.is_none();

                    methods.insert(
                        decl.name.clone(),
                        MethodInfo {
                            params: param_types,
                            return_type: return_ty,
                            visibility: decl.properties.visibility.clone(),
                            is_constructor: false,
                            is_abstract: method_is_abstract,
                        },
                    );
                }
                _ => {
                    self.report_error(
                        "Only method declarations are allowed in trait body".to_string(),
                        stmt.span,
                    );
                }
            }
        }

        context.exit_class();
        context.exit_scope();

        // Create trait definition
        let trait_def = TraitDefinition {
            name: name.clone(),
            generics: generic_defs,
            parent_traits: parent_trait_names,
            methods,
            module: self.current_module.clone(),
        };

        // Register trait type definition
        context.define_type(name.clone(), TypeDefinition::Trait(trait_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Trait(trait_def));
        }

        // Define trait type symbol
        let trait_type = make_type(TypeKind::Custom(name.clone(), None));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(trait_type.clone()))),
                    false,
                    false,
                    visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name,
            SymbolInfo::new(
                make_type(TypeKind::Meta(Box::new(trait_type))),
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }

    pub(crate) fn extract_generic_definitions(
        &mut self,
        generics: &[Expression],
        context: &mut Context,
    ) -> Vec<GenericDefinition> {
        let mut result = Vec::new();
        for gen_expr in generics {
            if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen_expr.node {
                if let Ok(gen_name) = self.extract_type_name(name_expr) {
                    let constraint = constraint_expr
                        .as_ref()
                        .map(|c| self.resolve_type_expression(c, context));
                    result.push(GenericDefinition {
                        name: gen_name,
                        constraint,
                        kind: kind.clone(),
                    });
                }
            }
        }
        result
    }
}
