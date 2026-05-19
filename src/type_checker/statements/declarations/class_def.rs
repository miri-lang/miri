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
        let Some(name) = self.check_class_extract_and_validate_name(name_expr, span) else {
            return;
        };
        self.pre_registered_types.remove(&name);

        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));
        let base_class_name = self.check_class_base_class(base_class, name_expr);
        self.check_class_circular_inheritance(&name, &base_class_name, span);

        context.enter_scope();
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        let base_direct_args = self.resolve_base_direct_args(base_class, context);
        let (trait_names, trait_direct_args) = self.check_class_traits(traits, context);

        self.register_class_hierarchy(&name, &base_class_name, &trait_names);

        let class_type = make_type(TypeKind::Custom(name.clone(), None));
        context.enter_class(name.clone(), base_class_name.clone(), class_type);

        let (fields, methods, method_statements) = self.check_class_collect_members(body, context);

        self.run_class_validations(
            &name,
            &base_class_name,
            &base_direct_args,
            &trait_names,
            &trait_direct_args,
            &methods,
            &method_statements,
            name_expr,
            is_abstract,
        );

        self.finalize_class_definition(
            &name,
            generic_defs,
            base_class_name,
            base_direct_args,
            trait_names,
            fields,
            methods,
            is_abstract,
            visibility,
            context,
        );

        self.check_class_method_bodies(&method_statements, context);

        context.exit_class();
        context.exit_scope();
    }

    fn check_class_extract_and_validate_name(
        &mut self,
        name_expr: &Expression,
        span: Span,
    ) -> Option<String> {
        let name = match self.extract_type_name(name_expr) {
            Ok(n) => n.to_string(),
            Err(_) => {
                self.report_error("Invalid class name".to_string(), name_expr.span);
                return None;
            }
        };
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = matches!(existing, TypeDefinition::Class(_))
                && self.pre_registered_types.contains(&name);
            if !is_placeholder {
                self.report_error(format!("Type '{}' is already defined", name), span);
                return None;
            }
        }
        Some(name)
    }

    fn resolve_base_direct_args(
        &mut self,
        base_class: &Option<Box<Expression>>,
        context: &mut Context,
    ) -> Option<Vec<Type>> {
        base_class.as_ref().and_then(|be| {
            if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) = &be.node {
                Some(
                    args.iter()
                        .map(|arg| self.resolve_type_expression(arg, context))
                        .collect(),
                )
            } else {
                None
            }
        })
    }

    fn register_class_hierarchy(
        &mut self,
        name: &str,
        base_class_name: &Option<String>,
        trait_names: &[String],
    ) {
        let entry = self.hierarchy.entry(name.to_string()).or_default();
        if let Some(ref base_name) = base_class_name {
            entry.extends = Some(base_name.clone());
        }
        for trait_name in trait_names {
            entry.implements.push(trait_name.clone());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_class_validations(
        &mut self,
        name: &str,
        base_class_name: &Option<String>,
        base_direct_args: &Option<Vec<Type>>,
        trait_names: &[String],
        trait_direct_args: &HashMap<String, Vec<Type>>,
        methods: &BTreeMap<String, MethodInfo>,
        method_statements: &[&Statement],
        name_expr: &Expression,
        is_abstract: bool,
    ) {
        if !is_abstract {
            self.check_class_non_abstract_methods(name, methods, name_expr);
        }
        if let Some(ref base_name) = base_class_name {
            self.check_class_method_overrides(
                name,
                base_name,
                base_direct_args,
                methods,
                name_expr,
            );
        }
        if let Some(ref base_name) = base_class_name {
            self.check_class_super_init(name, base_name, methods, method_statements, name_expr);
        }
        if !is_abstract {
            if let Some(ref base_name) = base_class_name {
                self.check_class_abstract_methods(name, base_name, methods, name_expr);
            }
        }
        for trait_name in trait_names {
            self.check_class_trait_methods(name, trait_name, methods, trait_direct_args, name_expr);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finalize_class_definition(
        &mut self,
        name: &str,
        generic_defs: Option<Vec<crate::type_checker::context::GenericDefinition>>,
        base_class_name: Option<String>,
        base_direct_args: Option<Vec<Type>>,
        trait_names: Vec<String>,
        fields: Vec<(String, FieldInfo)>,
        methods: BTreeMap<String, MethodInfo>,
        is_abstract: bool,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let has_drop = methods
            .get("drop")
            .is_some_and(|m| m.params.len() == 1 && m.params[0].0 == "self");
        let class_def = ClassDefinition {
            name: name.to_string(),
            generics: generic_defs,
            base_class: base_class_name,
            base_class_args: base_direct_args,
            traits: trait_names,
            fields,
            methods,
            module: self.current_module.clone(),
            is_abstract,
            has_drop,
        };

        if context.scopes.len() == 2 {
            self.register_type_definition(
                name.to_string(),
                TypeDefinition::Class(class_def.clone()),
            );
        }
        context.define_type(name.to_string(), TypeDefinition::Class(class_def));

        let class_type_meta = make_type(TypeKind::Meta(Box::new(make_type(TypeKind::Custom(
            name.to_string(),
            None,
        )))));

        if context.scopes.len() == 2 {
            self.global_scope.insert(
                name.to_string(),
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
            name.to_string(),
            SymbolInfo::new(
                class_type_meta,
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }

    /// Check base class validity and extract name
    fn check_class_base_class(
        &mut self,
        base_class: &Option<Box<Expression>>,
        _name_expr: &Expression,
    ) -> Option<String> {
        if let Some(base_expr) = base_class {
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
        }
    }

    /// Check for circular inheritance in the class hierarchy
    fn check_class_circular_inheritance(
        &mut self,
        name: &str,
        base_class_name: &Option<String>,
        span: Span,
    ) {
        if let Some(ref base_name) = base_class_name {
            let mut visited = std::collections::HashSet::new();
            visited.insert(name);
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
                if let Some(relation) = self.hierarchy.get(current) {
                    if let Some(ref next_base) = relation.extends {
                        current = next_base;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    /// Validate traits and extract their names and generic arguments
    fn check_class_traits(
        &mut self,
        traits: &[Expression],
        context: &mut Context,
    ) -> (Vec<String>, HashMap<String, Vec<Type>>) {
        let mut trait_names = Vec::with_capacity(traits.len());
        let mut trait_direct_args: HashMap<String, Vec<Type>> = HashMap::new();
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
                if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) = &trait_expr.node {
                    let resolved_args: Vec<Type> = args
                        .iter()
                        .map(|arg| self.resolve_type_expression(arg, context))
                        .collect();
                    trait_direct_args.insert(trait_name.to_string(), resolved_args);
                }
                trait_names.push(trait_name.to_string());
            }
        }
        (trait_names, trait_direct_args)
    }

    /// Collect fields and method signatures from class body (pass 1)
    #[allow(clippy::type_complexity)]
    fn check_class_collect_members<'a>(
        &mut self,
        body: &'a [Statement],
        context: &mut Context,
    ) -> (
        Vec<(String, FieldInfo)>,
        BTreeMap<String, MethodInfo>,
        Vec<&'a Statement>,
    ) {
        let mut fields: Vec<(String, FieldInfo)> = Vec::with_capacity(body.len());
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();
        let mut method_statements: Vec<&'a Statement> = Vec::with_capacity(body.len());

        for stmt in body {
            match &stmt.node {
                StatementKind::Variable(decls, vis) => {
                    self.collect_class_fields(decls, vis, &mut fields, stmt.span, context);
                }
                StatementKind::FunctionDeclaration(decl) => {
                    self.collect_class_method(
                        decl,
                        &mut methods,
                        stmt,
                        &mut method_statements,
                        context,
                    );
                }
                StatementKind::RuntimeFunctionDeclaration(
                    _runtime,
                    rt_name,
                    params,
                    return_type_expr,
                ) => {
                    self.collect_runtime_function(rt_name, params, return_type_expr, context);
                }
                StatementKind::IntrinsicFunctionDeclaration(
                    name,
                    generics,
                    params,
                    return_type_expr,
                    visibility,
                ) => {
                    self.collect_intrinsic_function(
                        name,
                        generics,
                        params,
                        return_type_expr,
                        visibility,
                        context,
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

        (fields, methods, method_statements)
    }

    /// Collect field declarations from variable statements in class body
    fn collect_class_fields(
        &mut self,
        decls: &[VariableDeclaration],
        vis: &MemberVisibility,
        fields: &mut Vec<(String, FieldInfo)>,
        span: Span,
        context: &mut Context,
    ) {
        for decl in decls {
            let field_type = if let Some(type_expr) = &decl.typ {
                self.resolve_type_expression(type_expr, context)
            } else if let Some(init) = &decl.initializer {
                self.infer_expression(init, context)
            } else {
                self.report_error(format!("Cannot infer type for field '{}'", decl.name), span);
                make_type(TypeKind::Error)
            };

            let is_mutable = match decl.declaration_type {
                VariableDeclarationType::Mutable => true,
                VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
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

    /// Collect method signature and statement from function declaration
    fn collect_class_method<'a>(
        &mut self,
        decl: &FunctionDeclarationData,
        methods: &mut BTreeMap<String, MethodInfo>,
        stmt: &'a Statement,
        method_statements: &mut Vec<&'a Statement>,
        context: &mut Context,
    ) {
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
        let is_out_flags: Vec<bool> = decl.params.iter().map(|p| p.is_out).collect();

        let method_is_abstract = decl.body.as_ref().is_none_or(|body| {
            matches!(&body.node, StatementKind::Empty)
                || matches!(&body.node, StatementKind::Block(stmts) if stmts.is_empty())
        });

        methods.insert(
            decl.name.clone(),
            MethodInfo {
                params: param_types,
                is_out_flags,
                return_type: return_ty,
                visibility: decl.properties.visibility.clone(),
                is_constructor: decl.name == "init",
                is_abstract: method_is_abstract,
            },
        );

        method_statements.push(stmt);
    }

    /// Register a runtime function declaration in class scope
    fn collect_runtime_function(
        &mut self,
        rt_name: &str,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        context: &mut Context,
    ) {
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

    /// Register an intrinsic function declaration in class scope
    fn collect_intrinsic_function(
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

        self.global_scope.insert(
            name.to_string(),
            SymbolInfo::new_intrinsic(
                func_type.clone(),
                visibility.clone(),
                self.current_module.clone(),
            ),
        );
        context.define(
            name.to_string(),
            SymbolInfo::new_intrinsic(func_type, visibility.clone(), self.current_module.clone()),
        );
    }

    /// Validate that non-abstract classes cannot have abstract methods
    fn check_class_non_abstract_methods(
        &mut self,
        name: &str,
        methods: &BTreeMap<String, MethodInfo>,
        name_expr: &Expression,
    ) {
        for (method_name, method_info) in methods {
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

    /// Validate method override signatures
    fn check_class_method_overrides(
        &mut self,
        name: &str,
        base_name: &str,
        base_direct_args: &Option<Vec<Type>>,
        methods: &BTreeMap<String, MethodInfo>,
        name_expr: &Expression,
    ) {
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        visited.insert(name.to_string());
        let mut current_args: Option<Vec<Type>> = base_direct_args.clone();
        let mut current_subst: HashMap<String, Type> = HashMap::new();

        let mut current_base_owned: Option<String> = Some(base_name.to_string());
        while let Some(class_name) = current_base_owned.take() {
            if !visited.insert(class_name.clone()) {
                break;
            }
            let (base_generics, base_methods, base_next_base, base_next_args) =
                match self.global_type_definitions.get(&class_name) {
                    Some(TypeDefinition::Class(base_def)) => (
                        base_def.generics.clone(),
                        base_def.methods.clone(),
                        base_def.base_class.clone(),
                        base_def.base_class_args.clone(),
                    ),
                    _ => break,
                };
            let ancestor_subst: HashMap<String, Type> = match (&base_generics, &current_args) {
                (Some(gens), Some(args)) if gens.len() == args.len() => gens
                    .iter()
                    .zip(args.iter())
                    .map(|(g, a)| (g.name.clone(), self.substitute_type(a, &current_subst)))
                    .collect(),
                _ => HashMap::new(),
            };

            for (method_name, child_method) in methods {
                if method_name == "init" {
                    continue;
                }
                if let Some(parent_method) = base_methods.get(method_name) {
                    self.check_class_method_signature_compat(
                        method_name,
                        child_method,
                        parent_method,
                        &ancestor_subst,
                        name_expr,
                    );
                }
            }

            current_base_owned = base_next_base;
            current_args = base_next_args;
            current_subst = ancestor_subst;
            if current_base_owned.is_none() {
                break;
            }
        }
    }

    /// Check signature compatibility for a single method override
    fn check_class_method_signature_compat(
        &mut self,
        method_name: &str,
        child_method: &MethodInfo,
        parent_method: &MethodInfo,
        ancestor_subst: &HashMap<String, Type>,
        name_expr: &Expression,
    ) {
        if child_method.params.len() != parent_method.params.len() {
            self.report_error(
                format!(
                    "Method '{}' has incompatible parameter count: parent has {} parameters, child has {}",
                    method_name,
                    parent_method.params.len(),
                    child_method.params.len()
                ),
                name_expr.span,
            );
        } else {
            for (i, ((child_name, child_type), (_, parent_type))) in child_method
                .params
                .iter()
                .zip(parent_method.params.iter())
                .enumerate()
            {
                let parent_substituted = self.substitute_type(parent_type, ancestor_subst);
                if child_type.kind != parent_substituted.kind {
                    self.report_error(
                        format!(
                            "Method '{}' has incompatible parameter type for '{}' (position {}): expected {}, got {}",
                            method_name,
                            child_name,
                            i + 1,
                            parent_substituted,
                            child_type
                        ),
                        name_expr.span,
                    );
                }
                // `out` is an ABI-affecting modifier — a mismatch between parent
                // and overriding child would let a vtable caller and callee
                // disagree on whether a parameter is pointer-boxed.
                let parent_out = parent_method.is_param_out(i);
                let child_out = child_method.is_param_out(i);
                if parent_out != child_out {
                    self.report_error(
                        format!(
                            "Method '{}' has incompatible 'out' modifier for parameter '{}' (position {}): parent declares {}, child declares {}",
                            method_name,
                            child_name,
                            i + 1,
                            if parent_out { "out" } else { "no out" },
                            if child_out { "out" } else { "no out" },
                        ),
                        name_expr.span,
                    );
                }
            }
        }

        let parent_return_substituted =
            self.substitute_type(&parent_method.return_type, ancestor_subst);
        if child_method.return_type.kind != parent_return_substituted.kind {
            self.report_error(
                format!(
                    "Method '{}' has incompatible return type: expected {}, got {}",
                    method_name, parent_return_substituted, child_method.return_type
                ),
                name_expr.span,
            );
        }
    }

    /// Validate child class init calls super.init() when parent has accessible init
    fn check_class_super_init(
        &mut self,
        name: &str,
        base_name: &str,
        methods: &BTreeMap<String, MethodInfo>,
        method_statements: &[&Statement],
        name_expr: &Expression,
    ) {
        let parent_has_init = self.check_class_parent_has_init(name, base_name);

        if parent_has_init {
            if let Some(child_init) = methods.get("init") {
                let mut found_super_init = false;
                for stmt in method_statements {
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

    /// Check if parent class has an accessible init method
    fn check_class_parent_has_init(&self, name: &str, base_name: &str) -> bool {
        let mut visited = std::collections::HashSet::new();
        visited.insert(name);
        let mut current_base: Option<&str> = Some(base_name);
        while let Some(check_class) = current_base {
            if !visited.insert(check_class) {
                break;
            }
            if let Some(TypeDefinition::Class(base_def)) =
                self.global_type_definitions.get(check_class)
            {
                if let Some(init_method) = base_def.methods.get("init") {
                    if matches!(
                        init_method.visibility,
                        MemberVisibility::Public | MemberVisibility::Protected
                    ) {
                        return true;
                    }
                }
                current_base = base_def.base_class.as_deref();
            } else {
                break;
            }
        }
        false
    }

    /// Validate non-abstract classes implement all abstract methods
    fn check_class_abstract_methods(
        &mut self,
        name: &str,
        base_name: &str,
        methods: &BTreeMap<String, MethodInfo>,
        name_expr: &Expression,
    ) {
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        visited.insert(name.to_string());
        let mut current_base: Option<String> = Some(base_name.to_string());

        while let Some(class_name) = current_base.take() {
            if !visited.insert(class_name.clone()) {
                break;
            }
            let (abstract_method_names, next_base) =
                match self.global_type_definitions.get(&class_name) {
                    Some(TypeDefinition::Class(base_def)) => {
                        let names: Vec<String> = base_def
                            .methods
                            .iter()
                            .filter(|(_, info)| info.is_abstract)
                            .map(|(n, _)| n.clone())
                            .collect();
                        (names, base_def.base_class.clone())
                    }
                    _ => break,
                };
            for method_name in &abstract_method_names {
                if !methods.contains_key(method_name) {
                    self.report_error(
                        format!(
                            "Class '{}' must implement abstract method '{}' from class '{}'",
                            name, method_name, class_name
                        ),
                        name_expr.span,
                    );
                }
            }
            current_base = next_base;
        }
    }

    /// Validate class implements all required trait methods
    fn check_class_trait_methods(
        &mut self,
        name: &str,
        trait_name: &str,
        methods: &BTreeMap<String, MethodInfo>,
        trait_direct_args: &HashMap<String, Vec<Type>>,
        name_expr: &Expression,
    ) {
        let mut trait_substitutions: HashMap<String, HashMap<String, Type>> = HashMap::new();
        let all_trait_methods = self.collect_trait_methods_resolved(
            trait_name,
            trait_direct_args,
            &mut trait_substitutions,
        );

        let mut missing_methods: Vec<(String, String)> = Vec::new();
        let mut mismatched_methods: Vec<(String, String, String)> = Vec::new();

        for (method_name, (method_info, origin_trait)) in &all_trait_methods {
            if method_info.is_abstract && !methods.contains_key(method_name) {
                missing_methods.push((method_name.clone(), origin_trait.clone()));
            }

            if let Some(class_method) = methods.get(method_name) {
                self.check_class_trait_method_compat(
                    method_name,
                    method_info,
                    class_method,
                    name,
                    origin_trait,
                    &trait_substitutions,
                    &mut mismatched_methods,
                );
            }
        }

        for (method_name, origin_trait) in missing_methods {
            self.report_error(
                format!(
                    "Class '{}' must implement method '{}' from trait '{}'",
                    name, method_name, origin_trait
                ),
                name_expr.span,
            );
        }

        for (method_name, origin_trait, expected_sig) in mismatched_methods {
            self.report_error(
                format!(
                    "Method '{}' in class '{}' does not match trait '{}' signature: expected {}",
                    method_name, name, origin_trait, expected_sig
                ),
                name_expr.span,
            );
        }
    }

    /// Collect all trait methods including parent traits
    fn collect_trait_methods_resolved(
        &mut self,
        trait_name: &str,
        trait_direct_args: &HashMap<String, Vec<Type>>,
        trait_substitutions: &mut HashMap<String, HashMap<String, Type>>,
    ) -> HashMap<String, (MethodInfo, String)> {
        let mut all_methods = HashMap::new();
        let initial_subst: HashMap<String, Type> = trait_direct_args
            .get(trait_name)
            .and_then(|args| {
                let Some(TypeDefinition::Trait(td)) = self.global_type_definitions.get(trait_name)
                else {
                    return None;
                };
                let gens = td.generics.as_ref()?;
                if gens.len() != args.len() {
                    return None;
                }
                Some(
                    gens.iter()
                        .zip(args.iter())
                        .map(|(g, a)| (g.name.clone(), a.clone()))
                        .collect(),
                )
            })
            .unwrap_or_default();

        let mut traits_to_check: Vec<(String, HashMap<String, Type>)> =
            vec![(trait_name.to_string(), initial_subst)];
        let mut visited_traits = std::collections::HashSet::new();

        while let Some((current_trait_name, current_subst)) = traits_to_check.pop() {
            if !visited_traits.insert(current_trait_name.clone()) {
                continue;
            }
            trait_substitutions.insert(current_trait_name.clone(), current_subst.clone());

            if let Some(TypeDefinition::Trait(trait_def)) =
                self.global_type_definitions.get(&current_trait_name)
            {
                for (method_name, method_info) in &trait_def.methods {
                    if !all_methods.contains_key(method_name) {
                        all_methods.insert(
                            method_name.clone(),
                            (method_info.clone(), current_trait_name.clone()),
                        );
                    }
                }

                for parent_name in &trait_def.parent_traits {
                    let parent_subst = self
                        .compose_parent_substitution(
                            parent_name,
                            trait_def.parent_trait_args.get(parent_name),
                            &current_subst,
                        )
                        .unwrap_or_default();
                    traits_to_check.push((parent_name.clone(), parent_subst));
                }
            }
        }
        all_methods
    }

    /// Check trait method signature compatibility
    #[allow(clippy::too_many_arguments)]
    fn check_class_trait_method_compat(
        &self,
        method_name: &str,
        method_info: &MethodInfo,
        class_method: &MethodInfo,
        class_name: &str,
        origin_trait: &str,
        trait_substitutions: &HashMap<String, HashMap<String, Type>>,
        mismatched_methods: &mut Vec<(String, String, String)>,
    ) {
        let class_type_kind = TypeKind::Custom(class_name.to_string(), None);
        let trait_self_kind = TypeKind::Custom(origin_trait.to_string(), None);

        let substitution: Option<HashMap<String, Type>> = trait_substitutions
            .get(origin_trait)
            .filter(|m| !m.is_empty())
            .cloned();

        let substitute = |ty: &Type| -> Type {
            match &substitution {
                Some(map) => self.substitute_type(ty, map),
                None => ty.clone(),
            }
        };

        let types_match = |trait_ty: &TypeKind, class_ty: &TypeKind| -> bool {
            if trait_ty == class_ty {
                return true;
            }
            if *trait_ty == trait_self_kind {
                return *class_ty == class_type_kind
                    || (class_name == "String" && *class_ty == TypeKind::String)
                    || matches!(class_ty, TypeKind::Generic(..))
                    || matches!(class_ty, TypeKind::Custom(cn, _) if cn == class_name);
            }
            false
        };

        let kinds_compatible = |trait_ty: &Type, class_ty: &Type| -> bool {
            let substituted = substitute(trait_ty);
            if substituted.kind == class_ty.kind {
                return true;
            }
            types_match(&substituted.kind, &class_ty.kind)
        };

        let params_match = method_info.params.len() == class_method.params.len()
            && method_info
                .params
                .iter()
                .zip(class_method.params.iter())
                .all(|((_, t1), (_, t2))| kinds_compatible(t1, t2));
        // `out` is an ABI-affecting modifier (scalar copy-in/copy-out via stack
        // slot). A mismatch between the trait signature and the implementing
        // class would cause a vtable caller and callee to disagree on whether
        // a parameter is a pointer-boxed scalar — silent memory corruption.
        let out_flags_match = (0..method_info.params.len())
            .all(|i| method_info.is_param_out(i) == class_method.is_param_out(i));
        let return_match = kinds_compatible(&method_info.return_type, &class_method.return_type);

        if !params_match || !out_flags_match || !return_match {
            let expected = format!(
                "fn {}({}) -> {:?}",
                method_name,
                method_info
                    .params
                    .iter()
                    .map(|(n, t)| format!("{}: {:?}", n, substitute(t).kind))
                    .collect::<Vec<_>>()
                    .join(", "),
                substitute(&method_info.return_type).kind
            );
            mismatched_methods.push((method_name.to_string(), origin_trait.to_string(), expected));
        }
    }

    /// Check method bodies (pass 2)
    fn check_class_method_bodies(
        &mut self,
        method_statements: &[&Statement],
        context: &mut Context,
    ) {
        for stmt in method_statements {
            if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
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
    }

    /// Compose the substitution for a parent trait by combining the child's
    /// current substitution with the args the child supplied at
    /// `extends Parent<...>`.
    ///
    /// The args returned by `parent_args` are expressed in the *child* trait's
    /// generic scope. We substitute through `child_subst` to land them in
    /// concrete types, then bind them to the *parent* trait's generic params.
    fn compose_parent_substitution(
        &self,
        parent_name: &str,
        parent_args: Option<&Vec<Type>>,
        child_subst: &HashMap<String, Type>,
    ) -> Option<HashMap<String, Type>> {
        let parent_args = parent_args?;
        let Some(TypeDefinition::Trait(parent_def)) = self.global_type_definitions.get(parent_name)
        else {
            return None;
        };
        let gens = parent_def.generics.as_ref()?;
        if gens.len() != parent_args.len() {
            return None;
        }
        let mut map = HashMap::new();
        for (g, a) in gens.iter().zip(parent_args.iter()) {
            let substituted = self.substitute_type(a, child_subst);
            map.insert(g.name.clone(), substituted);
        }
        Some(map)
    }
}
