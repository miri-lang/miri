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
use crate::ast::type_arguments::{written_argument, TypeArgKind, ValueArg};
use crate::ast::types::is_trait_self;
use crate::ast::types::{
    Type, TypeKind, ACCELERABLE_TRAIT_NAME, INDEXED_TRAIT_NAME, ITERABLE_TRAIT_NAME,
    ITERATOR_METHOD_NAME, POSITIONAL_METHOD_NAMES, STRING_TYPE_NAME,
};
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{
    class_ancestry, class_method_declaration, declared_class_has_body, inherited_trait_default,
    trait_default_among, ClassDefinition, Context, FieldInfo, MethodInfo, SymbolInfo,
    TypeDefinition,
};
use crate::type_checker::instantiation_requirements::pins_of;
use crate::type_checker::statements::declarations::generic_method_rules::spelled_signature;
use crate::type_checker::statements::declarations::method_names::DeclaredMethods;
use crate::type_checker::statements::declarations::FunctionDeclarationInfo;
use crate::type_checker::statements::runtime_effects::RuntimeDeclaration;
use crate::type_checker::trait_self::{mentions_self, pin_self};
use crate::type_checker::utils::permits_accelerable;
use crate::type_checker::TypeChecker;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The body a class method declares, or `None` for an abstract method, which
/// declares none or an empty one.
pub(crate) fn method_body(decl: &FunctionDeclarationData) -> Option<&Statement> {
    decl.body.as_deref().filter(|body| {
        !matches!(&body.node, StatementKind::Empty)
            && !matches!(&body.node, StatementKind::Block(stmts) if stmts.is_empty())
    })
}

/// A class whose definition — fields, method signatures, hierarchy — is
/// registered, with its validations and method bodies still to check.
/// Defining every class of a module before checking any body lets a body
/// written above a class read its fields and construct it, while the
/// validations keep their place in source order: they compare the class
/// against its traits, which are defined only when the body pass reaches them.
pub(crate) struct ClassBodies<'a> {
    name: String,
    class_data: &'a ClassData,
    base_class_name: Option<String>,
    base_direct_args: Option<Vec<Type>>,
    trait_names: Vec<String>,
    trait_direct_args: HashMap<String, Vec<Type>>,
    fields: Vec<(String, FieldInfo)>,
    methods: BTreeMap<String, MethodInfo>,
    method_statements: Vec<&'a Statement>,
}

impl TypeChecker {
    /// Defines a class and checks its method bodies in one go.
    pub(crate) fn check_class(
        &mut self,
        class_data: &ClassData,
        context: &mut Context,
        span: Span,
    ) {
        if let Some(bodies) = self.define_class(class_data, context, span) {
            self.check_class_bodies(&bodies, context);
        }
    }

    /// Registers a class's definition, leaving its validations and method
    /// bodies to [`check_class_bodies`](Self::check_class_bodies). `None`
    /// when the class has no valid name to register under.
    pub(crate) fn define_class<'a>(
        &mut self,
        class_data: &'a ClassData,
        context: &mut Context,
        span: Span,
    ) -> Option<ClassBodies<'a>> {
        let ClassData {
            name: name_expr,
            generics,
            base_class,
            traits,
            body,
            visibility,
            is_abstract,
            ..
        } = class_data;
        let name = self.check_class_extract_and_validate_name(name_expr, visibility, span)?;
        self.modules.pre_registered_types.remove(&name);

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
        for clause in base_class.iter().map(Box::as_ref).chain(traits) {
            self.refuse_bare_generic_supertype(&name, clause);
        }

        self.register_class_hierarchy(&name, &base_class_name, &trait_names);

        let class_type = self.type_at_own_parameters(&name, generics.as_deref(), context);
        context.enter_class(name.clone(), base_class_name.clone(), class_type);

        let (fields, methods, method_statements) =
            self.check_class_collect_members(body, context, generic_defs.as_ref());

        self.finalize_class_definition(
            &name,
            generic_defs,
            base_class_name.clone(),
            base_direct_args.clone(),
            trait_names.clone(),
            trait_direct_args.clone(),
            fields.clone(),
            methods.clone(),
            *is_abstract,
            visibility,
            context,
        );

        context.exit_class();
        context.exit_scope();
        Some(ClassBodies {
            name,
            class_data,
            base_class_name,
            base_direct_args,
            trait_names,
            trait_direct_args,
            fields,
            methods,
            method_statements,
        })
    }

    /// Validates a class [`define_class`](Self::define_class) registered
    /// against its traits and base class, then checks its method bodies in
    /// the class's own scope.
    pub(crate) fn check_class_bodies(&mut self, bodies: &ClassBodies, context: &mut Context) {
        let name_expr = &bodies.class_data.name;
        self.check_accelerable_impl(&bodies.name, &bodies.trait_names, &bodies.fields, name_expr);
        self.run_class_validations(
            &bodies.name,
            &bodies.base_class_name,
            &bodies.base_direct_args,
            &bodies.trait_names,
            &bodies.trait_direct_args,
            &bodies.methods,
            &bodies.method_statements,
            name_expr,
            bodies.class_data.is_abstract,
        );

        self.enter_class_scope(bodies, context);
        let class_data = bodies.class_data;
        let clauses = class_data.base_class.iter().map(Box::as_ref);
        self.validate_supertype_clauses(clauses.chain(&class_data.traits), context);
        self.check_typed_field_initializers(bodies, context);
        self.check_class_method_bodies(&bodies.method_statements, MethodPhase::Rest, context);
        self.record_runtime_settled_methods(&bodies.name, &bodies.method_statements);
        context.exit_class();
        context.exit_scope();
    }

    /// Checks the class's `iterator()` body, ahead of the module's other
    /// bodies, in the class's own scope.
    pub(crate) fn check_class_iterator_body(
        &mut self,
        bodies: &ClassBodies,
        context: &mut Context,
    ) {
        self.enter_class_scope(bodies, context);
        self.check_class_method_bodies(&bodies.method_statements, MethodPhase::Iterator, context);
        context.exit_class();
        context.exit_scope();
    }

    /// Enters the scope a class's bodies are checked in: its generic
    /// parameters and the class itself as the receiver's.
    fn enter_class_scope(&mut self, bodies: &ClassBodies, context: &mut Context) {
        let generics = bodies.class_data.generics.as_deref();
        context.enter_scope();
        if let Some(gens) = generics {
            // The definition already reported anything wrong with the bounds.
            let prev = self.suppress_diagnostics;
            self.suppress_diagnostics = true;
            self.define_generics(gens, context);
            self.suppress_diagnostics = prev;
        }
        let class_type = self.type_at_own_parameters(&bodies.name, generics, context);
        context.enter_class(
            bodies.name.clone(),
            bodies.base_class_name.clone(),
            class_type,
        );
    }

    /// Check the initializer of each field that also declares its type against
    /// that type. An untyped field took its type from its initializer when the
    /// class was defined; a typed one is checked here, once every class of the
    /// module is defined, so its initializer can construct a class declared
    /// below it. A field whose type did not resolve was already reported.
    fn check_typed_field_initializers(&mut self, bodies: &ClassBodies, context: &mut Context) {
        for stmt in &bodies.class_data.body {
            let StatementKind::Variable(decls, _) = &stmt.node else {
                continue;
            };
            for decl in decls {
                if decl.typ.is_none() || decl.initializer.is_none() {
                    continue;
                }
                let resolved = bodies
                    .fields
                    .iter()
                    .find(|(name, _)| *name == decl.name)
                    .is_some_and(|(_, field)| !matches!(field.ty.kind, TypeKind::Error));
                if resolved {
                    let outer = std::mem::replace(&mut context.in_field_initializer, true);
                    self.determine_variable_type(decl, context, stmt.span);
                    context.in_field_initializer = outer;
                }
            }
        }
    }

    /// Refuse an `extends` or `implements` clause naming a generic type
    /// without its arguments. The supertype's members are typed by position,
    /// so a clause that leaves the arguments to be matched by name would read
    /// `class B<Y, X> extends A` as `A<Y, X>`, and a bound the supertype
    /// declares on a parameter would never be checked against what fills it.
    fn refuse_bare_generic_supertype(&mut self, class_name: &str, clause: &Expression) {
        if matches!(
            &clause.node,
            ExpressionKind::TypeDeclaration(_, Some(_), _, _)
        ) {
            return;
        }
        let Ok(identity) = self.extract_type_identity(clause) else {
            return;
        };
        let supertype = crate::type_checker::diagnostics::shown_type_name(identity);
        let parameters: Vec<&str> = self
            .generics_of(identity)
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        if parameters.is_empty() {
            return;
        }
        let noun = crate::type_checker::call_instantiation::type_arguments_noun(parameters.len());
        let placeholders = vec!["..."; parameters.len()].join(", ");
        let message = format!(
            "'{supertype}' takes the {noun} `{}`, which '{class_name}' does not write: write \
             the type each one is given — `{supertype}<{placeholders}>` — using a concrete type \
             or a parameter '{class_name}' declares",
            parameters.join("`, `"),
        );
        self.report_error(
            DiagnosticCode::TypGenericArgumentCount,
            message,
            clause.span,
        );
    }

    fn check_class_extract_and_validate_name(
        &mut self,
        name_expr: &Expression,
        visibility: &MemberVisibility,
        span: Span,
    ) -> Option<String> {
        let Some(name) = self.register_declared_identity(name_expr, visibility) else {
            self.report_error(
                DiagnosticCode::TypClassDefinition,
                "Invalid class name".to_string(),
                name_expr.span,
            );
            return None;
        };
        if let Some(existing) = self.type_table.global_type_definitions.get(&name) {
            let is_placeholder = matches!(existing, TypeDefinition::Class(_))
                && self.modules.pre_registered_types.contains(&name);
            if !is_placeholder {
                self.report_error(
                    DiagnosticCode::TypTypeAlreadyDefined,
                    format!(
                        "Type '{}' is already defined",
                        crate::type_checker::diagnostics::shown_type_name(&name)
                    ),
                    span,
                );
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

    /// Checks the arguments of each `extends` or `implements` clause among
    /// `clauses` against the bounds of the class or trait it names.
    ///
    /// Runs in the body pass, in the scope of the declaring type's own
    /// parameters: whether an argument meets a bound depends on the traits
    /// it implements, which are known for every type of the module only once
    /// each one is defined, so a clause may name a type declared below it.
    pub(crate) fn validate_supertype_clauses<'a>(
        &mut self,
        clauses: impl Iterator<Item = &'a Expression>,
        context: &Context,
    ) {
        for clause in clauses {
            if let ExpressionKind::TypeDeclaration(name, Some(args), _, _) = &clause.node {
                self.validate_clause_bounds(name, args, context, clause.span);
            }
        }
    }

    /// Check each argument an `extends` or `implements` clause writes against
    /// the bound the class or trait it names declares on that parameter, as a
    /// construction or an annotation of it is checked: `class B extends A<int>`
    /// for `class A<X implements Named>` is refused, and so is
    /// `class H implements Holder<int>` for `trait Holder<X implements Named>`,
    /// whose defaults would otherwise be compiled at a type the bound excludes.
    fn validate_clause_bounds(
        &mut self,
        supertype_name: &Expression,
        args: &[Expression],
        context: &Context,
        span: Span,
    ) {
        let Ok(name) = self.extract_name(supertype_name) else {
            return;
        };
        let identity = self.written_type_identity(name);
        let generics = match self.type_table.global_type_definitions.get(identity) {
            Some(TypeDefinition::Class(class)) => class.generics.clone(),
            Some(TypeDefinition::Trait(trait_def)) => trait_def.generics.clone(),
            Some(
                TypeDefinition::Struct(_)
                | TypeDefinition::Enum(_)
                | TypeDefinition::Alias(_)
                | TypeDefinition::Generic(_),
            )
            | None => return,
        };
        let Ok(arguments) = args
            .iter()
            .cloned()
            .map(written_argument)
            .collect::<Result<Vec<_>, _>>()
        else {
            return;
        };
        if generics.as_ref().map_or(0, Vec::len) == arguments.len() {
            self.validate_generics(&Some(arguments), &generics, context, span);
        }
    }

    fn register_class_hierarchy(
        &mut self,
        name: &str,
        base_class_name: &Option<String>,
        trait_names: &[String],
    ) {
        let entry = self
            .type_table
            .hierarchy
            .entry(name.to_string())
            .or_default();
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
        self.refuse_bodiless_generic_methods(name, methods, method_statements, name_expr.span);
        if let Some(ref base_name) = base_class_name {
            self.refuse_generic_method_overrides(
                name,
                base_name,
                methods,
                method_statements,
                name_expr.span,
            );
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
        let lineage = ClassLineage {
            name,
            base_name: base_class_name.as_deref(),
            methods,
            traits: trait_names,
        };
        if !is_abstract {
            self.check_class_abstract_methods(&lineage, trait_names, name_expr);
        }
        for trait_name in trait_names {
            self.check_class_trait_methods(
                &lineage,
                trait_name,
                trait_direct_args,
                is_abstract,
                name_expr,
            );
        }
        if let (false, Some(base_name)) = (is_abstract, base_class_name) {
            for (trait_name, args) in self.traits_left_open_above(base_name, base_direct_args) {
                if trait_names.contains(&trait_name) {
                    continue;
                }
                let direct_args = HashMap::from([(trait_name.clone(), args)]);
                self.check_class_trait_methods(
                    &lineage,
                    &trait_name,
                    &direct_args,
                    false,
                    name_expr,
                );
            }
        }
        if let Some(ref base_name) = base_class_name {
            self.check_inherited_self_returns(name, base_name, trait_names, methods, name_expr);
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
        trait_args: HashMap<String, Vec<Type>>,
        fields: Vec<(String, FieldInfo)>,
        methods: BTreeMap<String, MethodInfo>,
        is_abstract: bool,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let class_def = ClassDefinition {
            name: name.to_string(),
            generics: generic_defs,
            base_class: base_class_name,
            base_class_args: base_direct_args,
            traits: trait_names,
            trait_args,
            fields,
            methods,
            module: self.modules.current_module.clone(),
            is_abstract,
            runtime_settled_methods: std::collections::BTreeSet::new(),
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
            self.type_table.global_scope.insert(
                name.to_string(),
                SymbolInfo::new(
                    class_type_meta.clone(),
                    false,
                    false,
                    visibility.clone(),
                    self.modules.current_module.clone(),
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
                self.modules.current_module.clone(),
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
                    let identity = self.written_type_identity(base_name).to_string();
                    if !self.is_type_visible(base_name) {
                        self.report_error(
                            DiagnosticCode::TypClassInheritance,
                            format!("Base class '{}' is not defined", base_name),
                            base_expr.span,
                        );
                    } else if let Some(def) = self.type_table.global_type_definitions.get(&identity)
                    {
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
                                DiagnosticCode::TypClassDefinition,
                                format!("'{}' is not a class", base_name),
                                base_expr.span,
                                format!(
                                    "'{}' is {} — only classes can be used with 'extends'",
                                    base_name, kind
                                ),
                            );
                        }
                    }
                    Some(identity)
                }
                Err(_) => {
                    self.report_error(
                        DiagnosticCode::TypClassInheritance,
                        "Invalid base class name".to_string(),
                        base_expr.span,
                    );
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
                        DiagnosticCode::TypClassInheritance,
                        format!(
                            "Circular inheritance detected: class '{}' eventually extends itself",
                            name
                        ),
                        span,
                    );
                    break;
                }
                visited.insert(current);
                if let Some(relation) = self.type_table.hierarchy.get(current) {
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
    /// Rejects an `implements Accelerable` declaration whose type cannot actually
    /// live on an accelerator: every field must itself be accelerable (or a
    /// generic parameter, whose concrete accelerability is enforced at the
    /// instantiation site). This stops a type from claiming the capability while
    /// holding, say, a `String` or `Map<K, V>` field.
    fn check_accelerable_impl(
        &mut self,
        name: &str,
        trait_names: &[String],
        fields: &[(String, FieldInfo)],
        name_expr: &Expression,
    ) {
        self.check_accelerable_fields(
            name,
            trait_names,
            fields
                .iter()
                .map(|(field_name, field)| (field_name.as_str(), &field.ty)),
            name_expr,
        );
    }

    /// Shared `implements Accelerable` field check for classes and structs: every
    /// field must be accelerable (or a generic parameter, resolved at the
    /// instantiation site). No-op unless the trait list names `Accelerable`.
    pub(crate) fn check_accelerable_fields<'a>(
        &mut self,
        name: &str,
        trait_names: &[String],
        fields: impl Iterator<Item = (&'a str, &'a Type)>,
        name_expr: &Expression,
    ) {
        if !trait_names.iter().any(|t| t == ACCELERABLE_TRAIT_NAME) {
            return;
        }
        for (field_name, field_ty) in fields {
            if permits_accelerable(&field_ty.kind, &self.type_table.global_type_definitions) {
                continue;
            }
            self.report_error(DiagnosticCode::TarGpuTypeNotAccelerable,
                format!(
                    "'{}' implements 'Accelerable' but field '{}' has type '{}', which is not accelerable; every field of an 'Accelerable' type must itself be accelerable.",
                    name, field_name, field_ty
                ),
                name_expr.span,
            );
        }
    }

    pub(crate) fn check_class_traits(
        &mut self,
        traits: &[Expression],
        context: &mut Context,
    ) -> (Vec<String>, HashMap<String, Vec<Type>>) {
        let mut trait_names = Vec::with_capacity(traits.len());
        let mut trait_direct_args: HashMap<String, Vec<Type>> = HashMap::new();
        for trait_expr in traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                let identity = self.written_type_identity(trait_name).to_string();
                if !self.is_type_visible(trait_name) {
                    self.report_error(
                        DiagnosticCode::TypTraitDefinition,
                        format!("Trait '{}' is not defined", trait_name),
                        trait_expr.span,
                    );
                } else if let Some(def) = self.type_table.global_type_definitions.get(&identity) {
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
                            DiagnosticCode::TypTraitDefinition,
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
                    trait_direct_args.insert(identity.clone(), resolved_args);
                }
                trait_names.push(identity);
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
        class_generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
    ) -> (
        Vec<(String, FieldInfo)>,
        BTreeMap<String, MethodInfo>,
        Vec<&'a Statement>,
    ) {
        let mut fields: Vec<(String, FieldInfo)> = Vec::with_capacity(body.len());
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();
        let mut method_statements: Vec<&'a Statement> = Vec::with_capacity(body.len());
        let mut declared = DeclaredMethods::default();

        for stmt in body {
            match &stmt.node {
                StatementKind::Variable(decls, vis) => {
                    self.collect_class_fields(decls, vis, &mut fields, stmt.span, context);
                }
                StatementKind::FunctionDeclaration(decl) => {
                    if self.refuse_redeclared_method(&mut declared, decl, stmt.span, "class") {
                        continue;
                    }
                    self.collect_class_method(
                        decl,
                        &mut methods,
                        stmt,
                        &mut method_statements,
                        context,
                        class_generics,
                    );
                }
                StatementKind::RuntimeFunctionDeclaration(
                    _runtime,
                    rt_name,
                    params,
                    return_type_expr,
                    attributes,
                ) => {
                    self.collect_runtime_function(
                        &RuntimeDeclaration {
                            name: rt_name,
                            params,
                            return_type: return_type_expr.as_deref(),
                            attributes,
                            span: stmt.span,
                        },
                        context,
                    );
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
                        DiagnosticCode::TypClassDefinition,
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
                let outer = std::mem::replace(&mut context.in_field_initializer, true);
                let ty = self.infer_expression(init, context);
                context.in_field_initializer = outer;
                ty
            } else {
                self.report_error(
                    DiagnosticCode::TypTypeInference,
                    format!("Cannot infer type for field '{}'", decl.name),
                    span,
                );
                make_type(TypeKind::Error)
            };

            let is_mutable = match decl.declaration_type {
                VariableDeclarationType::Mutable | VariableDeclarationType::Unmarked => true,
                VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
            };

            fields.push((
                decl.name.clone(),
                FieldInfo {
                    ty: field_type,
                    mutable: is_mutable,
                    visibility: vis.clone(),
                    initializer: decl.initializer.clone(),
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
        class_generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
    ) {
        self.check_drop_hook_shape(decl, stmt, DiagnosticCode::TypClassDefinition);
        self.refuse_generic_device_method(decl, stmt.span);
        self.refuse_generic_construction_method(decl, stmt.span, "class");
        if let (Some(method_generics), Some(class_generics)) = (&decl.generics, class_generics) {
            self.refuse_type_parameters_hiding_owner_parameters(
                method_generics,
                "class",
                class_generics,
            );
        }

        // Validate static method constraints
        if decl.properties.is_static {
            if decl.name == "init" {
                self.report_error(
                    DiagnosticCode::TypBuiltinConstructor,
                    "Static constructors are not supported - constructor cannot be static"
                        .to_string(),
                    stmt.span,
                );
            }
            if decl.properties.is_async {
                self.report_error(
                    DiagnosticCode::TypStaticMethodRestriction,
                    "Static methods cannot be async".to_string(),
                    stmt.span,
                );
            }
            if decl.properties.is_gpu {
                self.report_error(
                    DiagnosticCode::TarGpuCodeRestriction,
                    "Static methods cannot be GPU kernels".to_string(),
                    stmt.span,
                );
            }
            // Reject `self` as a parameter in static methods
            if decl.declares_receiver() {
                self.report_error(
                    DiagnosticCode::TypStaticMethodRestriction,
                    "Static methods cannot have a 'self' parameter".to_string(),
                    stmt.span,
                );
            }
        }

        // Resolve all parameter and return types once, for both validation and MethodInfo building
        let (method_generics, (param_types, return_ty)) =
            self.with_member_generics(decl.generics.as_deref(), context, |checker, context| {
                checker.resolve_class_method_signature(decl, class_generics, context)
            });
        let is_out_flags: Vec<bool> = decl.explicit_params().iter().map(|p| p.is_out).collect();

        // Check if static method references class generic parameters
        if decl.properties.is_static {
            if let Some(class_gens) = class_generics {
                if !class_gens.is_empty() {
                    let class_gen_names: Vec<&str> =
                        class_gens.iter().map(|g| g.name.as_str()).collect();

                    // Check parameter types for generic references (use resolved types from above)
                    for (i, param) in decl.explicit_params().iter().enumerate() {
                        if let Some(gen_name) =
                            self.find_generic_in_resolved_type(&param_types[i].1, &class_gen_names)
                        {
                            self.report_error(
                                DiagnosticCode::TypStaticMethodRestriction,
                                Self::static_method_generic_reference_error(&decl.name, &gen_name),
                                param.typ.span,
                            );
                            return;
                        }
                    }

                    // Check return type for generic references (use resolved type from above)
                    if let Some(gen_name) =
                        self.find_generic_in_resolved_type(&return_ty, &class_gen_names)
                    {
                        if let Some(return_type_expr) = &decl.return_type {
                            self.report_error(
                                DiagnosticCode::TypStaticMethodRestriction,
                                Self::static_method_generic_reference_error(&decl.name, &gen_name),
                                return_type_expr.span,
                            );
                            return;
                        }
                    }
                }
            }
        }

        let method_is_abstract = decl.body.as_ref().is_none_or(|body| {
            matches!(&body.node, StatementKind::Empty)
                || matches!(&body.node, StatementKind::Block(stmts) if stmts.is_empty())
        });

        methods.insert(
            decl.name.clone(),
            MethodInfo {
                generics: method_generics,
                params: param_types,
                is_out_flags,
                return_type: return_ty,
                visibility: decl.properties.visibility.clone(),
                is_constructor: decl.name == "init",
                is_abstract: method_is_abstract,
                is_static: decl.properties.is_static,
                attributes: decl.attributes.clone(),
            },
        );

        method_statements.push(stmt);
    }

    /// The parameter and return types `decl` writes, a class parameter
    /// named inside a function type kept as the class's.
    fn resolve_class_method_signature(
        &mut self,
        decl: &FunctionDeclarationData,
        class_generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
        context: &mut Context,
    ) -> (Vec<(String, Type)>, Type) {
        let class_parameters = class_parameter_types(class_generics);
        let params = decl
            .explicit_params()
            .iter()
            .map(|p| {
                let resolved = self.resolve_type_expression(&p.typ, context);
                (
                    p.name.clone(),
                    self.with_class_parameters_inside_functions(resolved, &class_parameters),
                )
            })
            .collect();
        let return_ty = match &decl.return_type {
            Some(rt_expr) => {
                let resolved = self.resolve_type_expression(rt_expr, context);
                self.with_class_parameters_inside_functions(resolved, &class_parameters)
            }
            None => make_type(TypeKind::Void),
        };
        (params, return_ty)
    }

    /// Register a runtime function declaration in class scope
    fn collect_runtime_function(
        &mut self,
        declaration: &RuntimeDeclaration,
        context: &mut Context,
    ) {
        let rt_name = declaration.name;
        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: declaration.params.to_vec(),
            return_type: declaration.return_type.cloned().map(Box::new),
        })));
        let info = SymbolInfo::new_runtime(func_type, self.modules.current_module.clone());

        self.type_table
            .global_scope
            .insert(rt_name.to_string(), info.clone());
        context.define(rt_name.to_string(), info);
        self.check_runtime_effects(declaration);
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

        self.type_table.global_scope.insert(
            name.to_string(),
            SymbolInfo::new_intrinsic(
                func_type.clone(),
                visibility.clone(),
                self.modules.current_module.clone(),
            ),
        );
        context.define(
            name.to_string(),
            SymbolInfo::new_intrinsic(
                func_type,
                visibility.clone(),
                self.modules.current_module.clone(),
            ),
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
                    DiagnosticCode::TypClassDefinition,
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

        let lineage = ClassLineage {
            name,
            base_name: Some(base_name),
            methods,
            traits: &[],
        };
        let mut current_base_owned: Option<String> = Some(base_name.to_string());
        let mut ancestry: Vec<(String, HashMap<String, Type>)> = Vec::new();
        let mut declared_above: HashSet<String> = HashSet::new();
        while let Some(class_name) = current_base_owned.take() {
            if !visited.insert(class_name.clone()) {
                break;
            }
            let (base_generics, base_methods, base_next_base, base_next_args) =
                match self.type_table.global_type_definitions.get(&class_name) {
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

            self.check_overrides_of_ancestor(&lineage, &base_methods, &ancestor_subst, name_expr);
            declared_above.extend(base_methods.into_keys());
            ancestry.push((class_name, ancestor_subst.clone()));
            current_base_owned = base_next_base;
            current_args = base_next_args;
            current_subst = ancestor_subst;
            if current_base_owned.is_none() {
                break;
            }
        }
        self.check_inherited_default_overrides(&lineage, &ancestry, &declared_above, name_expr);
    }

    /// Check each method the class declares against the one the ancestor
    /// declaring `base_methods` declares under that name, with the ancestor's
    /// parameters read through `ancestor_subst`.
    fn check_overrides_of_ancestor(
        &mut self,
        lineage: &ClassLineage,
        base_methods: &BTreeMap<String, MethodInfo>,
        ancestor_subst: &HashMap<String, Type>,
        name_expr: &Expression,
    ) {
        for (method_name, child_method) in lineage.methods {
            if method_name == "init" {
                continue;
            }
            // A generic method on either side is refused as an override
            // by `refuse_generic_method_overrides`, not compared here.
            if let Some(parent_method) = base_methods
                .get(method_name)
                .filter(|parent| parent.generics.is_empty() && child_method.generics.is_empty())
            {
                self.check_class_method_signature_compat(
                    lineage,
                    method_name,
                    (child_method, parent_method),
                    ancestor_subst,
                    name_expr,
                );
            }
        }
    }

    /// Check each method the class declares that no ancestor class declares
    /// but a trait default an ancestor inherits supplies. The override takes
    /// that default's place in the ancestor's vtable slot, so a call through
    /// the ancestor's type may hand it anything the ancestor's copy accepts:
    /// it is checked against that copy, with `Self` the ancestor, as a method
    /// the ancestor declared itself would be. `ancestry` pairs each ancestor
    /// with what the chain binds its parameters to.
    fn check_inherited_default_overrides(
        &mut self,
        lineage: &ClassLineage,
        ancestry: &[(String, HashMap<String, Type>)],
        declared_above: &HashSet<String>,
        name_expr: &Expression,
    ) {
        let Some(base_name) = lineage.base_name else {
            return;
        };
        for (method_name, child_method) in lineage.methods {
            if method_name == "init"
                || declared_above.contains(method_name)
                || !child_method.generics.is_empty()
            {
                continue;
            }
            let Some((class_level, parent)) = self.inherited_default_copy(base_name, method_name)
            else {
                continue;
            };
            let Some((_, ancestor_subst)) = ancestry
                .iter()
                .find(|(ancestor, _)| *ancestor == class_level)
            else {
                continue;
            };
            self.check_class_method_signature_compat(
                lineage,
                method_name,
                (child_method, &parent),
                ancestor_subst,
                name_expr,
            );
        }
    }

    /// The class whose `implements` list supplies the default `method_name`
    /// a `class_name` receiver inherits, with the default's signature read
    /// at that class: the trait's parameters at what the class binds them to
    /// and `Self` the class at its own parameters.
    fn inherited_default_copy(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<(String, MethodInfo)> {
        let default = inherited_trait_default(
            &self.type_table.global_type_definitions,
            class_name,
            method_name,
        )?;
        if !default.info.generics.is_empty() {
            return None;
        }
        let copy =
            self.member_at_own_parameters(default.class_level, default.trait_name, default.info);
        Some((default.class_level.to_string(), copy))
    }

    /// `method`, which `declaring` declares, as a value of `type_name` at its
    /// own parameters reaches it: `declaring`'s parameters at what the clauses
    /// from `type_name` up to it bind them to and, on a class, a trait's
    /// `Self` the class at its own parameters. A parameter the method declares
    /// itself shadows a pin of the same name.
    pub(crate) fn member_at_own_parameters(
        &self,
        type_name: &str,
        declaring: &str,
        method: &MethodInfo,
    ) -> MethodInfo {
        // The chain is read at the type's own parameters, each bound to
        // itself: an empty substitution would leave a clause such as
        // `implements Sink<U>` unpinned, and the copy would keep the trait's
        // `T` where the class means its `U`.
        let own_parameters: HashMap<String, Type> = (self.generics_of(type_name).iter())
            .map(|generic| {
                let parameter = TypeKind::Generic(generic.name.clone(), generic.bounds.clone());
                (generic.name.clone(), make_type(parameter))
            })
            .collect();
        let supertypes = self.declaring_types_above(type_name, &own_parameters);
        let mut pins = pins_of(&supertypes, declaring).cloned().unwrap_or_default();
        for generic in &method.generics {
            pins.remove(&generic.name);
        }
        if let Some(TypeDefinition::Class(_)) =
            self.type_table.global_type_definitions.get(type_name)
        {
            pin_self(&mut pins, self.class_at_own_parameters(type_name));
        }
        let mut copy = method.clone();
        for (_, param) in &mut copy.params {
            *param = self.substitute_type(param, &pins);
        }
        copy.return_type = self.substitute_type(&copy.return_type, &pins);
        copy
    }

    /// Check signature compatibility for a single method override.
    ///
    /// Parameters must match exactly: a call made through the parent's type may
    /// pass any value the parent accepts. A return may narrow to a class the
    /// parent's return class is extended by, since every caller of the parent
    /// still receives a value of the class it expects.
    fn check_class_method_signature_compat(
        &mut self,
        lineage: &ClassLineage,
        method_name: &str,
        (child_method, parent_method): (&MethodInfo, &MethodInfo),
        ancestor_subst: &HashMap<String, Type>,
        name_expr: &Expression,
    ) {
        if child_method.params.len() != parent_method.params.len() {
            self.report_error(DiagnosticCode::TypClassInheritance,
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
                    self.report_error(DiagnosticCode::TypClassInheritance,
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
                    self.report_error(DiagnosticCode::TypOutParameterMisuse,
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
        if child_method.return_type.kind != parent_return_substituted.kind
            && !self.returns_narrower_class(
                lineage,
                &child_method.return_type,
                &parent_return_substituted,
            )
        {
            self.report_error(
                DiagnosticCode::TypClassInheritance,
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
                    self.report_error(DiagnosticCode::TypBuiltinConstructor,
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
                self.type_table.global_type_definitions.get(check_class)
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

    /// Refuse a concrete class that leaves an abstract method of a class it
    /// extends without a body: one the class declares, one a class between it
    /// and the abstract declaration declares, or a default a trait anywhere in
    /// its chain supplies — the order [`declared_class_has_body`] resolves.
    fn check_class_abstract_methods(
        &mut self,
        lineage: &ClassLineage,
        trait_names: &[String],
        name_expr: &Expression,
    ) {
        let Some(base_name) = lineage.base_name else {
            return;
        };
        let definitions = &self.type_table.global_type_definitions;
        let mut seen = BTreeSet::new();
        let mut refusals = Vec::new();
        for (declaring, class) in class_ancestry(base_name, definitions) {
            for (method_name, method) in &class.methods {
                if !method.is_abstract || !seen.insert(method_name.as_str()) {
                    continue;
                }
                if !declared_class_has_body(
                    definitions,
                    lineage.methods,
                    trait_names,
                    base_name,
                    method_name,
                ) {
                    refusals.push(format!(
                        "Class '{}' must implement abstract method '{}' from class '{}'",
                        crate::type_checker::diagnostics::shown_type_name(lineage.name),
                        method_name,
                        crate::type_checker::diagnostics::shown_type_name(declaring)
                    ));
                }
            }
        }
        for message in refusals {
            self.report_error(DiagnosticCode::TypClassDefinition, message, name_expr.span);
        }
    }

    /// The traits the abstract classes directly above a class implement,
    /// each with its type arguments written in the class's own terms.
    ///
    /// An abstract class may leave a method of a trait it implements without a
    /// body, so its first concrete descendant owes that trait's methods. The
    /// walk stops at the first concrete ancestor, which already supplied every
    /// method of every trait above it.
    fn traits_left_open_above(
        &self,
        base_name: &str,
        base_direct_args: &Option<Vec<Type>>,
    ) -> Vec<(String, Vec<Type>)> {
        let definitions = &self.type_table.global_type_definitions;
        let mut open = Vec::new();
        let mut args = base_direct_args.clone();
        let mut subst: HashMap<String, Type> = HashMap::new();
        for (_, ancestor) in class_ancestry(base_name, definitions) {
            if !ancestor.is_abstract {
                break;
            }
            subst = match (&ancestor.generics, &args) {
                (Some(generics), Some(args)) if generics.len() == args.len() => generics
                    .iter()
                    .zip(args)
                    .map(|(g, a)| (g.name.clone(), self.substitute_type(a, &subst)))
                    .collect(),
                _ => HashMap::new(),
            };
            for trait_name in &ancestor.traits {
                let trait_args = ancestor
                    .trait_args
                    .get(trait_name)
                    .map_or_else(Vec::new, |a| {
                        a.iter().map(|t| self.substitute_type(t, &subst)).collect()
                    });
                open.push((trait_name.clone(), trait_args));
            }
            args = ancestor.base_class_args.clone();
        }
        open
    }

    /// Validate class implements all required trait methods, with its own
    /// declarations or with those it inherits from the classes it extends.
    /// An abstract class (`leaves_bodies_open`) may leave a method without a
    /// body; its concrete descendants owe it.
    ///
    /// An inherited method whose return names `Self` is not compared here: the
    /// body it would inherit builds the ancestor, which
    /// [`TypeChecker::check_inherited_self_returns`] refuses on its own terms.
    fn check_class_trait_methods(
        &mut self,
        lineage: &ClassLineage,
        trait_name: &str,
        trait_direct_args: &HashMap<String, Vec<Type>>,
        leaves_bodies_open: bool,
        name_expr: &Expression,
    ) {
        let name = lineage.name;
        let mut trait_substitutions: HashMap<String, HashMap<String, Type>> = HashMap::new();
        let all_trait_methods = self.collect_trait_methods_resolved(
            trait_name,
            trait_direct_args,
            &mut trait_substitutions,
        );

        let mut missing_methods: Vec<(String, String)> = Vec::new();
        let mut mismatched_methods: Vec<(String, String, String)> = Vec::new();
        let mut open_generic_methods: Vec<(String, String)> = Vec::new();

        for (method_name, (method_info, origin_trait)) in &all_trait_methods {
            let own = lineage.methods.get(method_name);
            let inherited = own
                .is_none()
                .then(|| self.inherited_method(lineage, method_name))
                .flatten();
            let Some(class_method) = own.or(inherited.as_ref()) else {
                let defaulted = trait_default_among(
                    &self.type_table.global_type_definitions,
                    lineage.traits,
                    method_name,
                )
                .is_some();
                if method_info.is_abstract && !leaves_bodies_open && !defaulted {
                    missing_methods.push((method_name.clone(), origin_trait.clone()));
                } else if method_info.is_abstract && !method_info.generics.is_empty() {
                    open_generic_methods.push((method_name.clone(), origin_trait.clone()));
                }
                continue;
            };
            if own.is_none() && mentions_self(&method_info.return_type) {
                continue;
            }
            self.check_class_trait_method_compat(
                method_name,
                method_info,
                class_method,
                lineage,
                origin_trait,
                &trait_substitutions,
                &mut mismatched_methods,
            );
        }

        self.refuse_open_generic_trait_methods(name, &open_generic_methods, name_expr.span);

        self.report_missing_trait_methods(
            lineage,
            missing_methods,
            &trait_substitutions,
            name_expr.span,
        );

        for (method_name, origin_trait, expected_sig) in mismatched_methods {
            self.report_error(
                DiagnosticCode::TypTraitDefinition,
                format!(
                    "Method '{}' in class '{}' does not match trait '{}' signature: expected {}",
                    method_name,
                    crate::type_checker::diagnostics::shown_type_name(name),
                    crate::type_checker::diagnostics::shown_type_name(&origin_trait),
                    expected_sig
                ),
                name_expr.span,
            );
        }
    }

    /// Refuses a class for each trait method it owes and has no body for.
    fn report_missing_trait_methods(
        &mut self,
        lineage: &ClassLineage,
        missing: Vec<(String, String)>,
        trait_substitutions: &HashMap<String, HashMap<String, Type>>,
        span: Span,
    ) {
        for (method_name, origin_trait) in missing {
            let message = format!(
                "Class '{}' must implement method '{}' from trait '{}'",
                crate::type_checker::diagnostics::shown_type_name(lineage.name),
                method_name,
                crate::type_checker::diagnostics::shown_type_name(&origin_trait)
            );
            match self.positional_iterable_repair(lineage, &origin_trait, trait_substitutions) {
                Some(help) => self.report_error_with_help(
                    DiagnosticCode::TypTraitDefinition,
                    message,
                    span,
                    help,
                ),
                None => self.report_error(DiagnosticCode::TypTraitDefinition, message, span),
            }
        }
    }

    /// The repair for a class that owes `Iterable`'s `iterator` while it
    /// reads its elements by position, through `length` and `element_at`: such
    /// a class is an `Indexed` sequence, whose `iterator` is supplied.
    fn positional_iterable_repair(
        &self,
        lineage: &ClassLineage,
        origin_trait: &str,
        trait_substitutions: &HashMap<String, HashMap<String, Type>>,
    ) -> Option<String> {
        let reads_by_position = POSITIONAL_METHOD_NAMES
            .iter()
            .all(|method| lineage.methods.contains_key(*method));
        if origin_trait != ITERABLE_TRAIT_NAME || !reads_by_position {
            return None;
        }
        let element = trait_substitutions
            .get(ITERABLE_TRAIT_NAME)
            .and_then(|substitution| substitution.values().next())
            .map_or_else(|| "T".to_string(), ToString::to_string);
        Some(format!(
            "a class that reads its elements with `length` and `element_at` is read by \
             position: implement `{INDEXED_TRAIT_NAME}<{element}>` in place of \
             `{ITERABLE_TRAIT_NAME}<{element}>`, which supplies its `iterator`"
        ))
    }

    /// Collect all trait methods including parent traits
    pub(super) fn collect_trait_methods_resolved(
        &mut self,
        trait_name: &str,
        trait_direct_args: &HashMap<String, Vec<Type>>,
        trait_substitutions: &mut HashMap<String, HashMap<String, Type>>,
    ) -> HashMap<String, (MethodInfo, String)> {
        let mut all_methods = HashMap::new();
        let initial_subst: HashMap<String, Type> = trait_direct_args
            .get(trait_name)
            .and_then(|args| {
                let Some(TypeDefinition::Trait(td)) =
                    self.type_table.global_type_definitions.get(trait_name)
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

            if let Some(TypeDefinition::Trait(trait_def)) = self
                .type_table
                .global_type_definitions
                .get(&current_trait_name)
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
        lineage: &ClassLineage,
        origin_trait: &str,
        trait_substitutions: &HashMap<String, HashMap<String, Type>>,
        mismatched_methods: &mut Vec<(String, String, String)>,
    ) {
        let class_name = lineage.name;
        // The trait's `Self` is this class at its own parameters, wherever the
        // trait writes it; a bare trait name stays the trait.
        let mut substitution: HashMap<String, Type> = trait_substitutions
            .get(origin_trait)
            .cloned()
            .unwrap_or_default();
        pin_self(&mut substitution, self.class_at_own_parameters(class_name));
        let substitute = |ty: &Type| -> Type { self.substitute_type(ty, &substitution) };
        let kinds_compatible = |trait_ty: &Type, class_ty: &Type| -> bool {
            names_one_type(&substitute(trait_ty).kind, &class_ty.kind)
        };

        // A parameter the trait types `Self` may be declared as a class the
        // implementing class extends: that method accepts every value of the
        // implementing class, which is all the trait asks of it.
        let accepts_self = |trait_ty: &Type, class_ty: &Type| -> bool {
            is_trait_self(&trait_ty.kind)
                && matches!(&class_ty.kind, TypeKind::Custom(ancestor, None)
                if self.declared_class_is_or_extends(
                    class_name, ancestor, class_name, lineage.base_name,
                ))
        };
        // The class's own type parameters are read as the trait's at the
        // same position; a different count or bound matches nothing.
        let renaming = self.trait_parameter_renaming(method_info, class_method, &substitute);
        let rename = |ty: &Type| -> Type {
            match &renaming {
                Some(map) if !map.is_empty() => self.substitute_type(ty, map),
                Some(_) | None => ty.clone(),
            }
        };
        let params_match = renaming.is_some()
            && method_info.params.len() == class_method.params.len()
            && method_info
                .params
                .iter()
                .zip(class_method.params.iter())
                .all(|((_, t1), (_, t2))| {
                    kinds_compatible(t1, &rename(t2)) || accepts_self(t1, t2)
                });
        // `out` is an ABI-affecting modifier (scalar copy-in/copy-out via stack
        // slot). A mismatch between the trait signature and the implementing
        // class would cause a vtable caller and callee to disagree on whether
        // a parameter is a pointer-boxed scalar — silent memory corruption.
        let out_flags_match = (0..method_info.params.len())
            .all(|i| method_info.is_param_out(i) == class_method.is_param_out(i));
        let return_match =
            kinds_compatible(&method_info.return_type, &rename(&class_method.return_type));

        if !params_match || !out_flags_match || !return_match {
            let expected = spelled_signature(method_name, method_info, &substitute);
            mismatched_methods.push((method_name.to_string(), origin_trait.to_string(), expected));
        }
    }

    /// Check method bodies (pass 2)
    fn check_class_method_bodies(
        &mut self,
        method_statements: &[&Statement],
        phase: MethodPhase,
        context: &mut Context,
    ) {
        for stmt in method_statements {
            if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                let is_iterator = decl.name == ITERATOR_METHOD_NAME;
                if method_body(decl).is_none() || is_iterator != (phase == MethodPhase::Iterator) {
                    continue;
                }
                let returning =
                    context
                        .current_class
                        .clone()
                        .filter(|_| is_iterator)
                        .map(
                            |owner| crate::type_checker::iteration_plan::ReturningMethod {
                                owner,
                                method: decl.name.clone(),
                                depth: context.return_types.len() + 1,
                            },
                        );
                let enclosing = std::mem::replace(&mut self.returning_method, returning);
                self.check_function_declaration(
                    FunctionDeclarationInfo {
                        name: &decl.name,
                        generics: &decl.generics,
                        params: decl.explicit_params(),
                        return_type: &decl.return_type,
                        body: decl.body.as_ref().map(|b| b.as_ref()),
                        properties: &decl.properties,
                        span: stmt.span,
                        is_member: true,
                    },
                    context,
                );
                self.returning_method = enclosing;
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
        let Some(TypeDefinition::Trait(parent_def)) =
            self.type_table.global_type_definitions.get(parent_name)
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

    /// Find the first generic parameter name referenced in a resolved type
    fn find_generic_in_resolved_type(&self, ty: &Type, generic_names: &[&str]) -> Option<String> {
        if let TypeKind::Generic(name, _) = &ty.kind {
            if generic_names.contains(&name.as_str()) {
                return Some(name.clone());
            }
        }
        if let TypeKind::Custom(_, Some(args)) = &ty.kind {
            let named = args
                .iter()
                .find_map(|arg| self.find_generic_in_type_expression(arg, generic_names));
            if named.is_some() {
                return named;
            }
        }
        // A parameter nested in a type argument (`List<T>`, `T?`) is as
        // unbound in a static method as a bare one.
        let (components, wrapped) = crate::type_checker::type_identities::type_components(&ty.kind);
        components
            .into_iter()
            .chain(wrapped)
            .find_map(|component| self.find_generic_in_resolved_type(component, generic_names))
    }

    /// Find the first generic parameter name a value argument names.
    fn find_generic_in_type_expression(
        &self,
        expr: &TypeArgument,
        generic_names: &[&str],
    ) -> Option<String> {
        match &expr.node {
            TypeArgKind::Value(ValueArg::Name(name)) if generic_names.contains(&name.as_str()) => {
                Some(name.clone())
            }
            TypeArgKind::Value(
                ValueArg::Name(_)
                | ValueArg::Int(_)
                | ValueArg::Binary(..)
                | ValueArg::OutOfRange { .. },
            )
            | TypeArgKind::Type(..) => None,
        }
    }

    /// Error message for a static method referencing a class generic parameter.
    /// Static methods have no per-instantiation body, so a signature naming a
    /// class generic has no concrete type to specialize the body to. Use a
    /// top-level generic function instead, which gains its own type parameter
    /// at each call site.
    fn static_method_generic_reference_error(method_name: &str, generic_name: &str) -> String {
        format!(
            "Static method '{}' cannot reference class generic parameter '{}'. \
             A static method has no per-instantiation body: only one copy of the method \
             exists regardless of how the class is specialized, so a signature naming a \
             class generic parameter has nothing to link against. \
             Consider using a top-level generic function instead.",
            method_name, generic_name
        )
    }
}

/// The class whose declaration is being checked, before it is registered: its
/// name, the class it extends, and the methods it declares itself.
/// Each of a class's type parameters by name, as the type a use of the name
/// resolves to.
fn class_parameter_types(
    class_generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
) -> HashMap<String, Type> {
    class_generics
        .into_iter()
        .flatten()
        .map(|generic| {
            let parameter = TypeKind::Generic(generic.name.clone(), generic.bounds.clone());
            (generic.name.clone(), make_type(parameter))
        })
        .collect()
}

struct ClassLineage<'a> {
    name: &'a str,
    base_name: Option<&'a str>,
    methods: &'a BTreeMap<String, MethodInfo>,
    /// The traits the class lists itself, whose defaults supply a body for a
    /// method another of them only requires.
    traits: &'a [String],
}

/// Whether a type a trait writes and the type a class implementing it writes
/// in its place are one type. The string type's methods are declared on a
/// class named after it, whose own name inside it spells the built-in type;
/// and a class written with its own value parameter (`Array<T, Size>`) names
/// that parameter as a value where `Self` names it as a parameter.
fn names_one_type(trait_ty: &TypeKind, class_ty: &TypeKind) -> bool {
    let names_string_class =
        |kind: &TypeKind| matches!(kind, TypeKind::Custom(name, None) if name == STRING_TYPE_NAME);
    trait_ty == class_ty
        || (names_string_class(trait_ty) && *class_ty == TypeKind::String)
        || (*trait_ty == TypeKind::String && names_string_class(class_ty))
        || names_one_instantiation(trait_ty, class_ty)
}

/// Whether two named types are one definition at one list of arguments,
/// reading a value parameter written by name as that parameter.
fn names_one_instantiation(first: &TypeKind, second: &TypeKind) -> bool {
    let (
        TypeKind::Custom(first_name, Some(first_args)),
        TypeKind::Custom(second_name, Some(second_args)),
    ) = (first, second)
    else {
        return false;
    };
    first_name == second_name
        && first_args.len() == second_args.len()
        && first_args.iter().zip(second_args).all(|(left, right)| {
            left == right
                || names_value_parameter(left, right)
                || names_value_parameter(right, left)
        })
}

/// Whether `written` names, as a value, the parameter `parameter` stands for.
fn names_value_parameter(written: &TypeArgument, parameter: &TypeArgument) -> bool {
    let TypeArgKind::Value(ValueArg::Name(name)) = &written.node else {
        return false;
    };
    matches!(&parameter.node, TypeArgKind::Type(ty, _)
        if matches!(&ty.kind, TypeKind::Generic(parameter_name, _) if parameter_name == name))
}

impl TypeChecker {
    /// The class `class_name` at its own type parameters, as its body spells
    /// itself: what a trait's `Self` stands for in the class's signatures.
    fn class_at_own_parameters(&self, class_name: &str) -> Type {
        let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(class_name)
        else {
            return make_type(TypeKind::Custom(class_name.to_string(), None));
        };
        let arguments: Vec<_> = (class_def.generics.iter().flatten())
            .map(|generic| {
                let parameter = TypeKind::Generic(generic.name.clone(), generic.bounds.clone());
                self.create_type_argument(make_type(parameter))
            })
            .collect();
        let arguments = (!arguments.is_empty()).then_some(arguments);
        make_type(TypeKind::Custom(class_name.to_string(), arguments))
    }
}

impl TypeChecker {
    /// `ty`, a resolved method signature type, with every name of a class
    /// type parameter written inside a function type it holds read as that
    /// parameter.
    ///
    /// Resolving a function type keeps the types inside it as written, so a
    /// class's `T` inside `fn(List<T>) int` is the name `T` there, where the
    /// trait method it overrides, substituted at the class's arguments, holds
    /// the parameter itself. A method declares no type parameter of a class's
    /// name (see `refuse_shadowed_class_parameters`), so every such name inside
    /// the signature is the class's. Types holding no function type are left
    /// as resolved.
    fn with_class_parameters_inside_functions(
        &self,
        ty: Type,
        class_parameters: &HashMap<String, Type>,
    ) -> Type {
        if class_parameters.is_empty() || !self.holds_function_type(&ty) {
            return ty;
        }
        self.substitute_type(&ty, class_parameters)
    }

    /// Whether `ty` is a function type or holds one among its type arguments,
    /// at any depth.
    fn holds_function_type(&self, ty: &Type) -> bool {
        let holds = |expr: &TypeArgument| {
            self.extract_type_from_argument(expr)
                .is_ok_and(|inner| self.holds_function_type(&inner))
        };
        match &ty.kind {
            TypeKind::Function(_) => true,
            TypeKind::Custom(_, Some(args)) => args.iter().any(holds),
            TypeKind::List(inner) | TypeKind::Set(inner) | TypeKind::Array(inner, _) => {
                holds(inner)
            }
            TypeKind::Map(key, value) | TypeKind::Result(key, value) => holds(key) || holds(value),
            TypeKind::Tuple(elements) => elements.iter().any(holds),
            TypeKind::Option(inner) => self.holds_function_type(inner),
            TypeKind::Custom(_, None)
            | TypeKind::Linear(_)
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
            | TypeKind::Int
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Float
            | TypeKind::Boolean
            | TypeKind::Void
            | TypeKind::String
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Future(_)
            | TypeKind::Generic(_, _)
            | TypeKind::Meta(_)
            | TypeKind::Error
            | TypeKind::OneOf(_) => false,
        }
    }

    /// The declaration of `method_name` the class inherits from the nearest
    /// class it extends that declares one.
    fn inherited_method(&self, lineage: &ClassLineage, method_name: &str) -> Option<MethodInfo> {
        let base_name = lineage.base_name?;
        class_method_declaration(
            base_name,
            method_name,
            &self.type_table.global_type_definitions,
        )
        .map(|(_, method)| method.clone())
    }

    /// True when `child` names a class that is, or extends, the class `parent`
    /// names — both without type arguments, where substitution cannot move them.
    fn returns_narrower_class(&self, lineage: &ClassLineage, child: &Type, parent: &Type) -> bool {
        match (&child.kind, &parent.kind) {
            (TypeKind::Custom(child_class, None), TypeKind::Custom(parent_class, None)) => self
                .declared_class_is_or_extends(
                    child_class,
                    parent_class,
                    lineage.name,
                    lineage.base_name,
                ),
            _ => false,
        }
    }
}

/// Which of a class's method bodies a pass over them checks: its
/// `iterator()`, checked ahead of the module's other bodies, or the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MethodPhase {
    Iterator,
    Rest,
}
