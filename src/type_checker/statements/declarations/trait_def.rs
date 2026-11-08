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
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{
    trait_is_or_extends, Context, GenericDefinition, MethodInfo, SymbolInfo, TraitDefinition,
    TypeDefinition,
};
use crate::type_checker::statements::declarations::method_names::DeclaredMethods;
use crate::type_checker::statements::declarations::FunctionDeclarationInfo;
use crate::type_checker::trait_self::trait_self_type;
use crate::type_checker::TypeChecker;
use std::collections::BTreeMap;

/// The syntax of one trait declaration.
pub(crate) struct TraitDeclaration<'a> {
    pub name: &'a Expression,
    pub generics: &'a Option<Vec<Expression>>,
    pub parent_traits: &'a [Expression],
    pub body: &'a [Statement],
    pub visibility: &'a MemberVisibility,
    pub span: Span,
}

/// The default method bodies of a trait defined ahead of the body pass,
/// which the body pass still checks.
pub(crate) struct TraitBodies<'a> {
    name: String,
    generics: Option<&'a [Expression]>,
    method_statements: Vec<&'a Statement>,
}

impl TypeChecker {
    /// Defines a trait and checks its default method bodies in one go.
    pub(crate) fn check_trait(&mut self, declaration: TraitDeclaration, context: &mut Context) {
        if let Some(bodies) = self.define_trait(declaration, context) {
            self.check_trait_bodies(&bodies, context);
        }
    }

    /// Registers a trait's parents and method signatures, leaving its
    /// default method bodies to the body pass. `None` when the trait was
    /// refused — an invalid name or a duplicate declaration.
    pub(crate) fn define_trait<'a>(
        &mut self,
        declaration: TraitDeclaration<'a>,
        context: &mut Context,
    ) -> Option<TraitBodies<'a>> {
        let TraitDeclaration {
            name: name_expr,
            generics,
            parent_traits,
            body,
            visibility,
            span,
        } = declaration;
        let name = self.trait_name(name_expr, visibility, span)?;
        self.modules.pre_registered_types.remove(&name);

        context.enter_scope();
        let trait_type = make_type(TypeKind::Custom(name.clone(), None));
        context.enter_class(name.clone(), None, trait_type.clone());

        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }
        let trait_at_own_parameters =
            self.type_at_own_parameters(&name, generics.as_deref(), context);
        context.set_self_type(trait_self_type(trait_at_own_parameters));
        let parent_trait_names = self.resolve_parent_traits(parent_traits, context);
        self.refuse_circular_trait_inheritance(&name, &parent_trait_names, name_expr.span);
        let parent_trait_args = self.resolve_parent_trait_args(parent_traits, context);
        let (methods, method_statements) =
            self.collect_trait_methods(body, generic_defs.as_deref().unwrap_or_default(), context);

        let trait_def = TraitDefinition {
            name: name.clone(),
            generics: generic_defs,
            parent_traits: parent_trait_names,
            parent_trait_args,
            methods,
            module: self.modules.current_module.clone(),
        };
        self.finalize_trait_definition(&name, &trait_type, trait_def, visibility, context);
        context.exit_class();
        context.exit_scope();
        Some(TraitBodies {
            name,
            generics: generics.as_deref(),
            method_statements,
        })
    }

    /// Checks the default method bodies of a trait
    /// [`define_trait`](Self::define_trait) defined, in a scope where its
    /// type parameters and `Self` are defined again.
    pub(crate) fn check_trait_bodies(&mut self, bodies: &TraitBodies, context: &mut Context) {
        context.enter_scope();
        let trait_type = make_type(TypeKind::Custom(bodies.name.clone(), None));
        context.enter_class(bodies.name.clone(), None, trait_type);
        if let Some(gens) = bodies.generics {
            // The definition already reported anything wrong with the bounds.
            let prev = self.suppress_diagnostics;
            self.suppress_diagnostics = true;
            self.define_generics(gens, context);
            self.suppress_diagnostics = prev;
        }
        let trait_at_own_parameters =
            self.type_at_own_parameters(&bodies.name, bodies.generics, context);
        context.set_self_type(trait_self_type(trait_at_own_parameters));
        self.check_trait_method_bodies(&bodies.method_statements, context);
        context.exit_class();
        context.exit_scope();
    }

    /// The identity `name_expr` declares a trait under, refusing an invalid
    /// name or a type already defined by another declaration.
    fn trait_name(
        &mut self,
        name_expr: &Expression,
        visibility: &MemberVisibility,
        span: Span,
    ) -> Option<String> {
        let Some(name) = self.register_declared_identity(name_expr, visibility) else {
            self.report_error(
                DiagnosticCode::TypTraitDefinition,
                "Invalid trait name".to_string(),
                name_expr.span,
            );
            return None;
        };
        let Some(existing) = self.type_table.global_type_definitions.get(&name) else {
            return Some(name);
        };
        let is_placeholder = matches!(existing, TypeDefinition::Trait(def)
            if def.methods.is_empty() || self.modules.pre_registered_types.contains(&name));
        if is_placeholder {
            return Some(name);
        }
        self.report_error(
            DiagnosticCode::TypTypeAlreadyDefined,
            format!(
                "Type '{}' is already defined",
                crate::type_checker::diagnostics::shown_type_name(&name)
            ),
            span,
        );
        None
    }

    fn finalize_trait_definition(
        &mut self,
        name: &str,
        trait_type: &Type,
        trait_def: TraitDefinition,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        context.define_type(name.to_string(), TypeDefinition::Trait(trait_def.clone()));
        if context.scopes.len() == 2 {
            self.register_type_definition(name.to_string(), TypeDefinition::Trait(trait_def));
        }
        self.register_trait_symbol(name, trait_type, visibility, context);
    }

    /// Refuses a trait among whose parents it is itself, directly or through
    /// other traits. A cycle's requirements have no declaration to start from,
    /// and a class implementing one would owe each trait's methods to the others.
    fn refuse_circular_trait_inheritance(&mut self, name: &str, parents: &[String], span: Span) {
        let definitions = &self.type_table.global_type_definitions;
        if !parents
            .iter()
            .any(|parent| trait_is_or_extends(parent, name, definitions))
        {
            return;
        }
        self.report_error(
            DiagnosticCode::TypTraitDefinition,
            format!(
                "Circular inheritance: trait '{}' eventually extends itself",
                crate::type_checker::diagnostics::shown_type_name(name)
            ),
            span,
        );
    }

    fn resolve_parent_traits(
        &mut self,
        parent_traits: &[Expression],
        _context: &mut Context,
    ) -> Vec<String> {
        let mut parent_trait_names = Vec::with_capacity(parent_traits.len());
        for trait_expr in parent_traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.is_type_visible(trait_name) {
                    self.report_error(
                        DiagnosticCode::TypTraitDefinition,
                        format!("Parent trait '{}' is not defined", trait_name),
                        trait_expr.span,
                    );
                } else if let Some(def) = self
                    .type_table
                    .global_type_definitions
                    .get(self.written_type_identity(trait_name))
                {
                    if !matches!(def, TypeDefinition::Trait(_)) {
                        let kind = match def {
                            TypeDefinition::Class(_) => "a class",
                            TypeDefinition::Enum(_) => "an enum",
                            TypeDefinition::Struct(_) => "a struct",
                            TypeDefinition::Alias(_) => "a type alias",
                            TypeDefinition::Generic(_) => "a generic type",
                            TypeDefinition::Trait(_) => unreachable!(),
                        };
                        self.report_error_with_help(DiagnosticCode::TypTraitDefinition,
                            format!("'{}' is not a trait", trait_name),
                            trait_expr.span,
                            format!(
                                "'{}' is {} — only traits can be used with 'extends' in a trait definition",
                                trait_name, kind
                            ),
                        );
                    }
                }
                parent_trait_names.push(self.written_type_identity(trait_name).to_string());
            }
        }
        parent_trait_names
    }

    fn resolve_parent_trait_args(
        &mut self,
        parent_traits: &[Expression],
        context: &mut Context,
    ) -> BTreeMap<String, Vec<Type>> {
        let mut parent_trait_args: BTreeMap<String, Vec<Type>> = BTreeMap::new();
        for trait_expr in parent_traits {
            if let Ok(parent_name) = self.extract_type_identity(trait_expr).map(String::from) {
                if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) = &trait_expr.node {
                    let resolved_args: Vec<Type> = args
                        .iter()
                        .map(|arg| self.resolve_type_expression(arg, context))
                        .collect();
                    parent_trait_args.insert(parent_name, resolved_args);
                }
            }
        }
        parent_trait_args
    }

    fn collect_trait_methods<'a>(
        &mut self,
        body: &'a [Statement],
        trait_parameters: &[GenericDefinition],
        context: &mut Context,
    ) -> (BTreeMap<String, MethodInfo>, Vec<&'a Statement>) {
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();
        let mut method_statements: Vec<&Statement> = Vec::with_capacity(body.len());
        let mut declared = DeclaredMethods::default();

        for stmt in body {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) => {
                    if self.refuse_redeclared_method(&mut declared, decl, stmt.span, "trait") {
                        continue;
                    }
                    self.check_drop_hook_shape(decl, stmt, DiagnosticCode::TypTraitDefinition);
                    self.refuse_generic_device_method(decl, stmt.span);
                    self.refuse_generic_construction_method(decl, stmt.span, "trait");
                    if let Some(generics) = &decl.generics {
                        self.refuse_type_parameters_hiding_owner_parameters(
                            generics,
                            "trait",
                            trait_parameters,
                        );
                    }

                    // Reject static methods in traits
                    if decl.properties.is_static {
                        self.report_error(
                            DiagnosticCode::TypStaticMethodRestriction,
                            "Static methods are not allowed in trait declarations".to_string(),
                            stmt.span,
                        );
                    }

                    let (method_generics, (param_types, return_ty)) = self.with_member_generics(
                        decl.generics.as_deref(),
                        context,
                        |checker, context| checker.resolve_member_signature(decl, context),
                    );
                    let is_out_flags: Vec<bool> =
                        decl.explicit_params().iter().map(|p| p.is_out).collect();

                    // Trait methods are abstract if they have no body
                    let method_is_abstract = decl.body.is_none();

                    methods.insert(
                        decl.name.clone(),
                        MethodInfo {
                            generics: method_generics,
                            params: param_types,
                            is_out_flags,
                            return_type: return_ty,
                            visibility: decl.properties.visibility.clone(),
                            is_constructor: false,
                            is_abstract: method_is_abstract,
                            is_static: false,
                            attributes: decl.attributes.clone(),
                        },
                    );

                    method_statements.push(stmt);
                }
                _ => {
                    self.report_error(
                        DiagnosticCode::TypTraitDefinition,
                        "Only method declarations are allowed in trait body".to_string(),
                        stmt.span,
                    );
                }
            }
        }
        (methods, method_statements)
    }

    fn register_trait_symbol(
        &mut self,
        name: &str,
        trait_type: &Type,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        if context.scopes.len() == 2 {
            self.type_table.global_scope.insert(
                name.to_string(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(trait_type.clone()))),
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
                make_type(TypeKind::Meta(Box::new(trait_type.clone()))),
                false,
                false,
                visibility.clone(),
                self.modules.current_module.clone(),
                None,
            ),
        );
    }

    fn check_trait_method_bodies(
        &mut self,
        method_statements: &[&Statement],
        context: &mut Context,
    ) {
        for stmt in method_statements.iter().copied() {
            if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                if decl.body.is_none() {
                    continue;
                }
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
            }
        }
    }
}
