// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type checking for enum declarations.

use crate::ast::factory::make_type;
use crate::ast::types::TypeKind;
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::attributes;
use crate::type_checker::context::{
    Context, EnumDefinition, GenericDefinition, MethodInfo, SymbolInfo, TypeDefinition,
};
use crate::type_checker::statements::declarations::FunctionDeclarationInfo;
use crate::type_checker::TypeChecker;
use std::collections::BTreeMap;

/// What the body pass still checks of an enum
/// [`define_enum`](TypeChecker::define_enum) defined: its method bodies,
/// which may read anything the module defines, including types declared
/// below the enum.
pub(crate) struct EnumBodies<'a> {
    name: String,
    generics: Option<&'a [Expression]>,
    method_statements: Vec<&'a Statement>,
}

/// The syntax of one enum declaration.
pub(crate) struct EnumDeclaration<'a> {
    pub name: &'a Expression,
    pub generics: &'a Option<Vec<Expression>>,
    pub variants: &'a [Expression],
    pub methods: &'a [Statement],
    pub attributes: &'a [Attribute],
    pub visibility: &'a MemberVisibility,
}

impl TypeChecker {
    /// Defines an enum and checks its method bodies in one go.
    pub(crate) fn check_enum(&mut self, declaration: EnumDeclaration, context: &mut Context) {
        if let Some(bodies) = self.define_enum(declaration, context) {
            self.check_enum_bodies(&bodies, context);
        }
    }

    /// Registers an enum's variants and method signatures, leaving its
    /// method bodies to the body pass. `None` when the enum was refused —
    /// an invalid name or a duplicate declaration.
    pub(crate) fn define_enum<'a>(
        &mut self,
        declaration: EnumDeclaration<'a>,
        context: &mut Context,
    ) -> Option<EnumBodies<'a>> {
        let name = self.enum_name(declaration.name, declaration.visibility)?;
        if !self.check_enum_not_duplicate(&name, declaration.name) {
            return None;
        }

        // Enter a scope for generic type parameters
        context.enter_scope();

        let generic_defs = self.resolve_enum_generics(declaration.generics, context);

        // The enum names itself at its own parameters, so a method signature
        // written `Self`, `Holder<T>` or the bare `Holder` all mean the same
        // type and substitute to the receiver's instantiation at a call site.
        let generics = declaration.generics.as_deref();
        let self_type = self.type_at_own_parameters(&name, generics, context);
        context.enter_class(name.clone(), None, self_type);

        let variant_map = self.collect_enum_variants(declaration.variants, context);
        let (method_map, method_statements) =
            self.collect_enum_methods(declaration.methods, &variant_map, context);

        let enum_def = EnumDefinition {
            variants: variant_map,
            generics: (!generic_defs.is_empty()).then_some(generic_defs),
            methods: method_map,
            module: self.modules.current_module.clone(),
            must_use: attributes::has_attribute(declaration.attributes, MUST_USE_ATTRIBUTE),
            non_exhaustive: attributes::has_attribute(
                declaration.attributes,
                NON_EXHAUSTIVE_ATTRIBUTE,
            ),
        };

        // TODO: an enum declared inside a function body is defined only in
        // its own scope, which closes below, so the next statement finds no
        // such type. Either define it in the enclosing scope (and give it a
        // link name that cannot collide with another function's local enum)
        // or refuse local type declarations at the declaration.
        context.define_type(name.clone(), TypeDefinition::Enum(enum_def.clone()));
        if context.scopes.len() == 2 {
            // scopes.len() == 2: base_scope + enum_scope
            self.register_type_definition(name.clone(), TypeDefinition::Enum(enum_def));
        }

        // Define enum type symbol (constructor/type)
        self.register_enum_symbol(&name, declaration.visibility, context);

        context.exit_class();
        context.exit_scope();
        Some(EnumBodies {
            name,
            generics,
            method_statements,
        })
    }

    /// Checks the method bodies of an enum defined earlier, with `self` and
    /// the enum's own type parameters in scope.
    pub(crate) fn check_enum_bodies(&mut self, bodies: &EnumBodies, context: &mut Context) {
        context.enter_scope();
        if let Some(gens) = bodies.generics {
            // The definition already reported anything wrong with the bounds.
            let prev = self.suppress_diagnostics;
            self.suppress_diagnostics = true;
            self.define_generics(gens, context);
            self.suppress_diagnostics = prev;
        }
        let self_type = self.type_at_own_parameters(&bodies.name, bodies.generics, context);
        context.enter_class(bodies.name.clone(), None, self_type);
        self.check_enum_method_bodies(&bodies.method_statements, context);
        context.exit_class();
        context.exit_scope();
    }

    fn enum_name(
        &mut self,
        name_expr: &Expression,
        visibility: &MemberVisibility,
    ) -> Option<String> {
        if let ExpressionKind::Identifier(..) = &name_expr.node {
            return self.register_declared_identity(name_expr, visibility);
        }
        self.report_error(
            DiagnosticCode::TypEnumDefinition,
            "Invalid enum name".to_string(),
            name_expr.span,
        );
        None
    }

    /// Accepts `name` when no type holds it yet or only the enum's own
    /// shell does, and claims the shell so a second declaration of the name
    /// is reported as a duplicate.
    fn check_enum_not_duplicate(&mut self, name: &str, name_expr: &Expression) -> bool {
        if let Some(existing) = self.type_table.global_type_definitions.get(name) {
            let is_placeholder = matches!(existing, TypeDefinition::Enum(_))
                && self.modules.pre_registered_types.contains(name);
            if !is_placeholder {
                self.report_error(
                    DiagnosticCode::TypTypeAlreadyDefined,
                    format!(
                        "Type '{}' is already defined",
                        crate::type_checker::diagnostics::shown_type_name(name)
                    ),
                    name_expr.span,
                );
                return false;
            }
        }
        self.modules.pre_registered_types.remove(name);
        true
    }

    fn resolve_enum_generics(
        &mut self,
        generics: &Option<Vec<Expression>>,
        context: &mut Context,
    ) -> Vec<GenericDefinition> {
        let mut generic_defs = Vec::new();
        if let Some(gens) = generics {
            self.define_generics(gens, context);
            for gen in gens {
                if let ExpressionKind::GenericType(gen_name_expr, constraint, kind) = &gen.node {
                    if let ExpressionKind::Identifier(gname, _) = &gen_name_expr.node {
                        let constraint_type = constraint
                            .as_ref()
                            .map(|c| self.resolve_bound_type(c, context));
                        generic_defs.push(GenericDefinition {
                            name: gname.clone(),
                            constraint: constraint_type,
                            kind: *kind,
                        });
                    }
                }
            }
        }
        generic_defs
    }

    fn collect_enum_variants(
        &mut self,
        variants: &[Expression],
        context: &mut Context,
    ) -> BTreeMap<String, Vec<Type>> {
        let mut variant_map = BTreeMap::new();
        for variant in variants {
            if let ExpressionKind::EnumValue(variant_name_expr, associated_types) = &variant.node {
                if let ExpressionKind::Identifier(variant_name, _) = &variant_name_expr.node {
                    let mut types = Vec::with_capacity(associated_types.len());
                    for ty_expr in associated_types {
                        types.push(self.resolve_type_expression(ty_expr, context));
                    }
                    variant_map.insert(variant_name.clone(), types);
                } else {
                    self.report_error(
                        DiagnosticCode::TypEnumVariant,
                        "Invalid enum variant name".to_string(),
                        variant_name_expr.span,
                    );
                }
            } else {
                self.report_error(
                    DiagnosticCode::TypEnumVariant,
                    "Invalid enum variant definition".to_string(),
                    variant.span,
                );
            }
        }
        variant_map
    }

    fn collect_enum_methods<'a>(
        &mut self,
        methods: &'a [Statement],
        variants: &BTreeMap<String, Vec<Type>>,
        context: &mut Context,
    ) -> (BTreeMap<String, MethodInfo>, Vec<&'a Statement>) {
        let mut method_map: BTreeMap<String, MethodInfo> = BTreeMap::new();
        let mut method_statements: Vec<&Statement> = Vec::with_capacity(methods.len());
        for method_stmt in methods {
            if let StatementKind::FunctionDeclaration(decl) = &method_stmt.node {
                let explicit_params = decl.explicit_params();
                let mut params = Vec::with_capacity(explicit_params.len());
                let mut is_out_flags = Vec::with_capacity(explicit_params.len());
                for param in explicit_params {
                    let param_ty = self.resolve_type_expression(&param.typ, context);
                    params.push((param.name.clone(), param_ty));
                    is_out_flags.push(param.is_out);
                }

                let return_type = if let Some(ret_expr) = &decl.return_type {
                    self.resolve_type_expression(ret_expr, context)
                } else {
                    make_type(TypeKind::Void)
                };

                // Check for collision with variant names if this is a static method
                if decl.properties.is_static && variants.contains_key(&decl.name) {
                    self.report_error(DiagnosticCode::TypEnumVariant,
                        format!(
                            "Static method '{}' has the same name as an enum variant - collision between static method and variant",
                            decl.name
                        ),
                        method_stmt.span,
                    );
                    continue;
                }

                // Validate static method constraints
                if decl.properties.is_static {
                    if decl.properties.is_async {
                        self.report_error(
                            DiagnosticCode::TypStaticMethodRestriction,
                            "Static methods cannot be async".to_string(),
                            method_stmt.span,
                        );
                        continue;
                    }
                    if decl.properties.is_gpu {
                        self.report_error(
                            DiagnosticCode::TarGpuCodeRestriction,
                            "Static methods cannot be GPU kernels".to_string(),
                            method_stmt.span,
                        );
                        continue;
                    }
                    // Reject `self` as a parameter in static methods
                    if decl.declares_receiver() {
                        self.report_error(
                            DiagnosticCode::TypStaticMethodRestriction,
                            "Static methods cannot have a 'self' parameter".to_string(),
                            method_stmt.span,
                        );
                        continue;
                    }
                }

                method_map.insert(
                    decl.name.clone(),
                    MethodInfo {
                        params,
                        is_out_flags,
                        return_type,
                        visibility: decl.properties.visibility.clone(),
                        is_constructor: false,
                        is_abstract: false,
                        is_static: decl.properties.is_static,
                        attributes: decl.attributes.clone(),
                    },
                );
                method_statements.push(method_stmt);
            }
        }
        (method_map, method_statements)
    }

    /// Bind the enum's name to the type itself, so `Holder.One(1)` resolves the
    /// receiver. The name stands for the enum family, not one instantiation, so
    /// it is bound bare even when the enum declares generic parameters.
    fn register_enum_symbol(
        &mut self,
        name: &str,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let enum_type_meta = make_type(TypeKind::Meta(Box::new(make_type(TypeKind::Custom(
            name.to_string(),
            None,
        )))));
        if context.scopes.len() == 2 {
            self.type_table.global_scope.insert(
                name.to_string(),
                SymbolInfo::new(
                    enum_type_meta.clone(),
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
                enum_type_meta,
                false,
                false,
                visibility.clone(),
                self.modules.current_module.clone(),
                None,
            ),
        );
    }

    fn check_enum_method_bodies(
        &mut self,
        method_statements: &[&Statement],
        context: &mut Context,
    ) {
        for stmt in method_statements {
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
