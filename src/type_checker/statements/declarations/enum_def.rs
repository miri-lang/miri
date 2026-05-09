// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type checking for enum declarations.

use crate::ast::factory::make_type;
use crate::ast::types::TypeKind;
use crate::ast::*;
use crate::type_checker::context::{
    Context, EnumDefinition, GenericDefinition, MethodInfo, SymbolInfo, TypeDefinition,
};
use crate::type_checker::statements::declarations::FunctionDeclarationInfo;
use crate::type_checker::TypeChecker;
use std::collections::BTreeMap;

impl TypeChecker {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_enum(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        variants: &[Expression],
        methods: &[Statement],
        must_use: bool,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid enum name".to_string(), name_expr.span);
            return;
        };

        // Check for duplicate type definitions
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = match existing {
                TypeDefinition::Enum(def) => def.variants.is_empty(),
                _ => false,
            };
            if !is_placeholder {
                self.report_error(
                    format!("Type '{}' is already defined", name),
                    name_expr.span,
                );
                return;
            }
        }

        // Enter a scope for generic type parameters
        context.enter_scope();

        let mut generic_defs = Vec::new();
        if let Some(gens) = generics {
            self.define_generics(gens, context);
            for gen in gens {
                if let ExpressionKind::GenericType(gen_name_expr, constraint, kind) = &gen.node {
                    if let ExpressionKind::Identifier(gname, _) = &gen_name_expr.node {
                        let constraint_type = constraint
                            .as_ref()
                            .map(|c| self.resolve_type_expression(c, context));
                        generic_defs.push(GenericDefinition {
                            name: gname.clone(),
                            constraint: constraint_type,
                            kind: kind.clone(),
                        });
                    }
                }
            }
        }

        // Set up class context so `self` resolves correctly in method bodies
        let self_type = make_type(TypeKind::Custom(name.clone(), None));
        context.enter_class(name.clone(), None, self_type.clone());

        // Resolve variants
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
                        "Invalid enum variant name".to_string(),
                        variant_name_expr.span,
                    );
                }
            } else {
                self.report_error("Invalid enum variant definition".to_string(), variant.span);
            }
        }

        // Collect method signatures
        let mut method_map: BTreeMap<String, MethodInfo> = BTreeMap::new();
        let mut method_statements: Vec<&Statement> = Vec::with_capacity(methods.len());
        for method_stmt in methods {
            if let StatementKind::FunctionDeclaration(decl) = &method_stmt.node {
                let mut params = Vec::with_capacity(decl.params.len());
                for param in &decl.params {
                    let param_ty = self.resolve_type_expression(&param.typ, context);
                    params.push((param.name.clone(), param_ty));
                }

                let return_type = if let Some(ret_expr) = &decl.return_type {
                    self.resolve_type_expression(ret_expr, context)
                } else {
                    make_type(TypeKind::Void)
                };

                method_map.insert(
                    decl.name.clone(),
                    MethodInfo {
                        params,
                        return_type,
                        visibility: MemberVisibility::Public,
                        is_constructor: false,
                        is_abstract: false,
                    },
                );
                method_statements.push(method_stmt);
            }
        }

        let generic_defs_opt = if generic_defs.is_empty() {
            None
        } else {
            Some(generic_defs)
        };

        let enum_def = EnumDefinition {
            variants: variant_map,
            generics: generic_defs_opt,
            methods: method_map,
            module: self.current_module.clone(),
            must_use,
        };

        context.define_type(name.clone(), TypeDefinition::Enum(enum_def.clone()));
        if context.scopes.len() == 2 {
            // scopes.len() == 2: base_scope + enum_scope
            self.register_type_definition(name.clone(), TypeDefinition::Enum(enum_def));
        }

        // Define enum type symbol (constructor/type)
        let enum_type_meta = make_type(TypeKind::Meta(Box::new(self_type)));

        if context.scopes.len() == 2 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    enum_type_meta.clone(),
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
                enum_type_meta,
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );

        // PASS 2: Type-check method bodies
        for stmt in method_statements {
            if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                if decl.body.is_none() {
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

        context.exit_class();
        context.exit_scope();
    }
}
