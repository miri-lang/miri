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
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{
    ClassDefinition, Context, FieldInfo, MethodInfo, SymbolInfo, TypeDefinition,
};
use crate::type_checker::statements::declarations::FunctionDeclarationInfo;
use crate::type_checker::TypeChecker;
use std::collections::{BTreeMap, HashMap};

impl TypeChecker {
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
            Ok(n) => n.to_string(),
            Err(_) => {
                self.report_error("Invalid class name".to_string(), name_expr.span);
                return;
            }
        };

        // Check for duplicate type definitions
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = match existing {
                TypeDefinition::Class(def) => def.fields.is_empty() && def.methods.is_empty(),
                _ => false,
            };

            if !is_placeholder {
                self.report_error(format!("Type '{}' is already defined", name), span);
                return;
            }
        }

        // Process generics
        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));

        // Validate base class exists and is a class
        let base_class_name = if let Some(base_expr) = base_class {
            match self.extract_type_name(base_expr) {
                Ok(base_name) => {
                    if !self.is_type_visible(base_name) {
                        self.report_error(
                            format!("Base class '{}' is not defined", base_name),
                            base_expr.span,
                        );
                    } else if let Some(def) = self.global_type_definitions.get(base_name) {
                        if !matches!(def, TypeDefinition::Class(_)) {
                            let kind = match def {
                                TypeDefinition::Trait(_) => "a trait",
                                TypeDefinition::Enum(_) => "an enum",
                                TypeDefinition::Struct(_) => "a struct",
                                TypeDefinition::Alias(_) => "a type alias",
                                TypeDefinition::Generic(_) => "a generic type",
                                TypeDefinition::Class(_) => unreachable!(),
                            };
                            self.report_error_with_help(
                                format!("'{}' is not a class", base_name),
                                base_expr.span,
                                format!(
                                    "'{}' is {} — only classes can be used with 'extends'",
                                    base_name, kind
                                ),
                            );
                        }
                    }
                    Some(base_name.to_string())
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
            visited.insert(name.as_str());
            let mut current: &str = base_name;
            loop {
                if visited.contains(current) {
                    self.report_error(
                        format!(
                            "Circular inheritance detected: class '{}' eventually extends itself",
                            name
                        ),
                        span,
                    );
                    break;
                }
                visited.insert(current);
                // Get the base class of current
                if let Some(relation) = self.hierarchy.get(current) {
                    if let Some(ref next_base) = relation.extends {
                        current = next_base;
                    } else {
                        break; // No more base classes
                    }
                } else {
                    break; // Class not in hierarchy yet (could be defined later)
                }
            }
        }

        // Validate traits exist, are visible, and are actually traits
        let mut trait_names = Vec::with_capacity(traits.len());
        for trait_expr in traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.is_type_visible(trait_name) {
                    self.report_error(
                        format!("Trait '{}' is not defined", trait_name),
                        trait_expr.span,
                    );
                } else if let Some(def) = self.global_type_definitions.get(trait_name) {
                    if !matches!(def, TypeDefinition::Trait(_)) {
                        let kind = match def {
                            TypeDefinition::Class(_) => "a class",
                            TypeDefinition::Enum(_) => "an enum",
                            TypeDefinition::Struct(_) => "a struct",
                            TypeDefinition::Alias(_) => "a type alias",
                            TypeDefinition::Generic(_) => "a generic type",
                            TypeDefinition::Trait(_) => unreachable!(),
                        };
                        self.report_error_with_help(
                            format!("'{}' is not a trait", trait_name),
                            trait_expr.span,
                            format!(
                                "'{}' is {} — only traits can be used with 'implements'",
                                trait_name, kind
                            ),
                        );
                    }
                }
                trait_names.push(trait_name.to_string());
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
        let mut fields: Vec<(String, FieldInfo)> = Vec::with_capacity(body.len());
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();
        // Store method info for second pass body checking
        let mut method_statements: Vec<&Statement> = Vec::with_capacity(body.len());

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

                        fields.push((
                            decl.name.clone(),
                            FieldInfo {
                                ty: field_type,
                                mutable: is_mutable,
                                visibility: vis.clone(),
                            },
                        ));
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
                        name_expr.span,
                    );
                }
            }
        }

        // Validate: method overrides must have compatible signatures
        let override_errors: Vec<String> = if let Some(ref base_name) = base_class_name {
            let mut errors = Vec::new();
            // Walk up the inheritance chain to find parent methods
            let mut current_base: Option<&str> = Some(base_name);
            while let Some(class_name) = current_base {
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
                    current_base = base_def.base_class.as_deref();
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
            self.report_error(error, name_expr.span);
        }

        // Validate: child class init must call super.init() when parent has accessible init
        if let Some(ref base_name) = base_class_name {
            // Check if parent has an accessible init method
            let parent_has_init = {
                let mut has_init = false;
                let mut current_base: Option<&str> = Some(base_name);
                while let Some(check_class) = current_base {
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
                        current_base = base_def.base_class.as_deref();
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
                            name_expr.span,
                        );
                    }
                }
            }
        }

        // Validate: non-abstract classes must implement all abstract methods from inheritance chain
        if !is_abstract {
            if let Some(ref base_name) = base_class_name {
                // Collect all abstract methods from the entire inheritance chain
                let missing_errors: Vec<String> = {
                    let mut errors = Vec::new();
                    let mut current_base: Option<&str> = Some(base_name);

                    while let Some(class_name) = current_base {
                        if let Some(TypeDefinition::Class(base_def)) =
                            self.global_type_definitions.get(class_name)
                        {
                            for (method_name, method_info) in &base_def.methods {
                                if method_info.is_abstract && !methods.contains_key(method_name) {
                                    errors.push(format!(
                                        "Class '{}' must implement abstract method '{}' from class '{}'",
                                        name, method_name, class_name
                                    ));
                                }
                            }
                            // Move to the next ancestor
                            current_base = base_def.base_class.as_deref();
                        } else {
                            break;
                        }
                    }
                    errors
                };

                // Report errors for missing methods
                for error in missing_errors {
                    self.report_error(error, name_expr.span);
                }
            }
        }

        // Validate: classes must implement all required trait methods (including parent traits)
        for trait_name in &trait_names {
            // Collect all methods from trait hierarchy (including parent traits)
            let all_trait_methods: HashMap<String, (MethodInfo, String)> = {
                let mut all_methods = HashMap::new();
                let mut traits_to_check: Vec<&str> = vec![trait_name];
                let mut visited_traits = std::collections::HashSet::new();

                while let Some(current_trait_name) = traits_to_check.pop() {
                    if visited_traits.contains(current_trait_name) {
                        continue;
                    }
                    visited_traits.insert(current_trait_name);

                    if let Some(TypeDefinition::Trait(trait_def)) =
                        self.global_type_definitions.get(current_trait_name)
                    {
                        // Add methods from this trait
                        for (method_name, method_info) in &trait_def.methods {
                            // Don't overwrite if already added (child trait methods take precedence)
                            if !all_methods.contains_key(method_name) {
                                all_methods.insert(
                                    method_name.clone(),
                                    (method_info.clone(), current_trait_name.to_string()),
                                );
                            }
                        }

                        // Add parent traits to check
                        for parent_trait in &trait_def.parent_traits {
                            traits_to_check.push(parent_trait);
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
                        // The class type is either Custom(class_name, None), String,
                        // or a generic type parameter (e.g. T in List<T>).
                        if *trait_ty == trait_self_kind {
                            return *class_ty == class_type_kind
                                || (name == "String" && *class_ty == TypeKind::String)
                                || matches!(class_ty, TypeKind::Generic(..));
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
                    name_expr.span,
                );
            }

            // Report errors for signature mismatches
            for (method_name, expected_sig) in mismatched_methods {
                self.report_error(
                    format!(
                        "Method '{}' in class '{}' does not match trait '{}' signature: expected {}",
                        method_name, name, trait_name, expected_sig
                    ),
                    name_expr.span,
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

        // scopes.len() == 2 because we're in [base_scope, class_scope]
        if context.scopes.len() == 2 {
            self.register_type_definition(name.clone(), TypeDefinition::Class(class_def.clone()));
        }
        // Register class type definition so self.* lookups work (move, no clone)
        context.define_type(name.clone(), TypeDefinition::Class(class_def));

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
            name,
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
}
