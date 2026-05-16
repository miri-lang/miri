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

        // Check for duplicate type definitions. The cross-module pre-pass may
        // have inserted a partial placeholder so forward references resolve;
        // recognize it via `pre_registered_types` and let the full check below
        // overwrite it. Anything else is a real duplicate.
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = matches!(existing, TypeDefinition::Class(_))
                && self.pre_registered_types.contains(&name);

            if !is_placeholder {
                self.report_error(format!("Type '{}' is already defined", name), span);
                return;
            }
        }
        self.pre_registered_types.remove(&name);

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

        // Enter class scope and bring the class's own generics into scope BEFORE
        // resolving `implements`/`extends` args, so that nested references to a
        // class generic (`implements Iterable<List<T>>`) resolve as Generic
        // rather than failing with "Unknown type: T".
        context.enter_scope();
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Capture generic args from `extends Base<X, Y, ...>`. Resolved in the
        // class's own generic-param scope so that `extends Base<T>` (where T is
        // this class's generic) survives as `Generic("T")` and gets substituted
        // later by descendants that pin the chain. Mirrors `trait_direct_args`
        // for `implements Trait<...>`.
        let base_direct_args: Option<Vec<Type>> = base_class.as_ref().and_then(|be| {
            if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) = &be.node {
                Some(
                    args.iter()
                        .map(|arg| self.resolve_type_expression(arg, context))
                        .collect(),
                )
            } else {
                None
            }
        });

        // Validate traits exist, are visible, and are actually traits.
        // For each `implements Trait<X>`, we also capture the generic args at the
        // implements site so that trait-method signatures can be compared after
        // substituting the trait's generic params with the chosen concrete types.
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
                // Capture generic args from `implements Trait<X, Y, ...>` so that
                // signature checks can substitute the trait's generic params with
                // the chosen concrete types. The parser produces fully-parsed
                // type expressions for each arg, so nested generics survive
                // intact.
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
                StatementKind::IntrinsicFunctionDeclaration(
                    name,
                    generics,
                    params,
                    return_type_expr,
                    visibility,
                ) => {
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
                        SymbolInfo::new_intrinsic(
                            func_type,
                            visibility.clone(),
                            self.current_module.clone(),
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
            // Walk up the inheritance chain to find parent methods.
            // Track visited classes to short-circuit on circular inheritance —
            // the pre-pass registers mutually-extending shells in
            // global_type_definitions, so without this guard A↔B loops forever.
            //
            // For each ancestor, build a substitution map from its generic params
            // to the concrete args declared at the corresponding `extends ...<>`
            // site. The map composes through the chain: at level N, args declared
            // at level N-1's `extends` are themselves substituted by N-1's map
            // before being installed for level N. Mirrors the trait-hierarchy
            // walk that composes `parent_trait_args` via `compose_parent_substitution`.
            let mut visited = std::collections::HashSet::new();
            visited.insert(name.as_str());
            let mut current_base: Option<&str> = Some(base_name);
            let mut current_args: Option<Vec<Type>> = base_direct_args.clone();
            let mut current_subst: HashMap<String, Type> = HashMap::new();
            while let Some(class_name) = current_base {
                if !visited.insert(class_name) {
                    break;
                }
                if let Some(TypeDefinition::Class(base_def)) =
                    self.global_type_definitions.get(class_name)
                {
                    // Build this ancestor's generic-param substitution. Apply the
                    // already-accumulated `current_subst` to each arg so that an
                    // arg referencing a deeper-level generic (`extends Base<T>`
                    // where T is the intermediate class's param) lands as the
                    // concrete type pinned higher in the chain.
                    let ancestor_subst: HashMap<String, Type> =
                        match (&base_def.generics, &current_args) {
                            (Some(gens), Some(args)) if gens.len() == args.len() => gens
                                .iter()
                                .zip(args.iter())
                                .map(|(g, a)| {
                                    (g.name.clone(), self.substitute_type(a, &current_subst))
                                })
                                .collect(),
                            _ => HashMap::new(),
                        };

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
                                    let parent_substituted =
                                        self.substitute_type(parent_type, &ancestor_subst);
                                    if child_type.kind != parent_substituted.kind {
                                        errors.push(format!(
                                            "Method '{}' has incompatible parameter type for '{}' (position {}): expected {}, got {}",
                                            method_name,
                                            child_name,
                                            i + 1,
                                            parent_substituted,
                                            child_type
                                        ));
                                    }
                                }
                            }

                            // Check return type
                            let parent_return_substituted =
                                self.substitute_type(&parent_method.return_type, &ancestor_subst);
                            if child_method.return_type.kind != parent_return_substituted.kind {
                                errors.push(format!(
                                    "Method '{}' has incompatible return type: expected {}, got {}",
                                    method_name,
                                    parent_return_substituted,
                                    child_method.return_type
                                ));
                            }
                        }
                    }
                    // Move to the next ancestor, carrying its own `extends` args
                    // (resolved in ITS scope) forward. The next iteration will
                    // compose them through the now-current substitution.
                    current_base = base_def.base_class.as_deref();
                    current_args = base_def.base_class_args.clone();
                    current_subst = ancestor_subst;
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
                let mut visited = std::collections::HashSet::new();
                visited.insert(name.as_str());
                let mut current_base: Option<&str> = Some(base_name);
                while let Some(check_class) = current_base {
                    if !visited.insert(check_class) {
                        break;
                    }
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
                    let mut visited = std::collections::HashSet::new();
                    visited.insert(name.as_str());
                    let mut current_base: Option<&str> = Some(base_name);

                    while let Some(class_name) = current_base {
                        if !visited.insert(class_name) {
                            break;
                        }
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
            // Collect all methods from trait hierarchy (including parent traits) and
            // compute a per-trait substitution map. The directly-implemented trait
            // takes its args from the `implements Trait<X>` site; parent traits
            // walked via `parent_traits` inherit args from their child by composing
            // substitutions through each `extends Parent<...>` declaration.
            let mut trait_substitutions: HashMap<String, HashMap<String, Type>> = HashMap::new();
            let all_trait_methods: HashMap<String, (MethodInfo, String)> = {
                let mut all_methods = HashMap::new();
                let initial_subst: HashMap<String, Type> = trait_direct_args
                    .get(trait_name)
                    .and_then(|args| {
                        let Some(TypeDefinition::Trait(td)) =
                            self.global_type_definitions.get(trait_name)
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
                    vec![(trait_name.clone(), initial_subst)];
                let mut visited_traits = std::collections::HashSet::new();

                while let Some((current_trait_name, current_subst)) = traits_to_check.pop() {
                    if !visited_traits.insert(current_trait_name.clone()) {
                        continue;
                    }
                    trait_substitutions.insert(current_trait_name.clone(), current_subst.clone());

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

                        // Walk parents, composing the substitution through any
                        // generic args declared at `extends Parent<...>`.
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
            };

            // Collect missing and mismatched methods
            let mut missing_methods: Vec<(String, String)> = Vec::new();
            let mut mismatched_methods: Vec<(String, String, String)> = Vec::new();

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

                    // Substitution for the originating trait, computed during the
                    // hierarchy walk above. Empty if the trait declared no generic
                    // args at this implements/extends site.
                    let substitution: Option<HashMap<String, Type>> = trait_substitutions
                        .get(origin_trait)
                        .filter(|m| !m.is_empty())
                        .cloned();

                    // Substitute trait method's generic params with the concrete
                    // types from the implements clause before comparing.
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
                        // Self in trait resolves to Custom(trait_name, None).
                        // The class type is either Custom(class_name, None), String,
                        // or a generic type parameter (e.g. T in List<T>).
                        // Generic classes (e.g. List<T>) may express Self as
                        // Custom(class_name, Some([T])); match by base name.
                        if *trait_ty == trait_self_kind {
                            return *class_ty == class_type_kind
                                || (name == "String" && *class_ty == TypeKind::String)
                                || matches!(class_ty, TypeKind::Generic(..))
                                || matches!(class_ty, TypeKind::Custom(cn, _) if cn == &name);
                        }
                        false
                    };

                    let kinds_compatible = |trait_ty: &Type, class_ty: &Type| -> bool {
                        let substituted = substitute(trait_ty);
                        // Fast path: after substitution, kinds match structurally.
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
                    let return_match =
                        kinds_compatible(&method_info.return_type, &class_method.return_type);

                    if !params_match || !return_match {
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
                        mismatched_methods.push((
                            method_name.clone(),
                            origin_trait.clone(),
                            expected,
                        ));
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

            // Report errors for signature mismatches. Use `origin_trait` (the
            // trait that actually declares the method) rather than the directly
            // implemented trait, so inherited-method mismatches name the right
            // trait in the diagnostic.
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

        // Create and register class definition BEFORE checking method bodies
        let has_drop = methods
            .get("drop")
            .is_some_and(|m| m.params.len() == 1 && m.params[0].0 == "self");
        let class_def = ClassDefinition {
            name: name.clone(),
            generics: generic_defs,
            base_class: base_class_name.clone(),
            base_class_args: base_direct_args.clone(),
            traits: trait_names.clone(),
            fields,
            methods,
            module: self.current_module.clone(),
            is_abstract,
            has_drop,
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
