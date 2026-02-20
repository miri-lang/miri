// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Utility functions for the type checker.
//!
//! This module provides helper functions for:
//! - Type predicates (is_numeric, is_integer)
//! - Visibility checking
//! - Type expression manipulation
//! - Error reporting

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;

impl TypeChecker {
    // ==================== Error Type Helper ====================

    /// Creates an error type. Use this when type checking fails.
    #[inline]
    pub(crate) fn error_type() -> Type {
        make_type(TypeKind::Error)
    }

    // ==================== Type Predicates ====================

    /// Checks if a type is numeric (any integer or float type).
    pub(crate) fn is_numeric(&self, t: &Type) -> bool {
        matches!(
            t.kind,
            TypeKind::Int
                | TypeKind::Float
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
                | TypeKind::F32
                | TypeKind::F64
        )
    }

    /// Checks if a type is an integer type.
    pub(crate) fn is_integer(&self, t: &Type) -> bool {
        matches!(
            t.kind,
            TypeKind::Int
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
        )
    }

    /// Returns the bit size of an integer type, or None if not an integer.
    pub(crate) fn get_integer_size(&self, t: &Type) -> Option<u8> {
        match &t.kind {
            TypeKind::I8 | TypeKind::U8 => Some(8),
            TypeKind::I16 | TypeKind::U16 => Some(16),
            TypeKind::I32 | TypeKind::U32 => Some(32),
            TypeKind::I64 | TypeKind::U64 => Some(64),
            TypeKind::I128 | TypeKind::U128 => Some(128),
            TypeKind::Int => Some(128), // Treat literal Int as max size for compatibility
            _ => None,
        }
    }

    // ==================== Visibility Checking ====================

    /// Checks if a symbol with the given visibility is accessible from the current module.
    pub(crate) fn check_visibility(&self, visibility: &MemberVisibility, module: &str) -> bool {
        match visibility {
            MemberVisibility::Public => true,
            MemberVisibility::Private => module == self.current_module,
            MemberVisibility::Protected => {
                module == self.current_module || self.is_subtype(&self.current_module, module)
            }
        }
    }

    /// Checks if a class member can be accessed from the current context.
    ///
    /// - `public`: always accessible
    /// - `private`: only accessible from within the same class
    /// - `protected`: accessible from same class or subclasses
    pub(crate) fn check_member_visibility(
        &self,
        visibility: &MemberVisibility,
        member_class: &str,
        current_class: Option<&str>,
    ) -> bool {
        match visibility {
            MemberVisibility::Public => true,
            MemberVisibility::Private => current_class == Some(member_class),
            MemberVisibility::Protected => {
                if let Some(curr) = current_class {
                    curr == member_class || self.is_subtype(curr, member_class)
                } else {
                    false
                }
            }
        }
    }

    // ==================== Type Expression Helpers ====================

    /// Creates a type expression from a Type.
    pub(crate) fn create_type_expression(&self, ty: Type) -> Expression {
        IdNode::new(0, ExpressionKind::Type(Box::new(ty), false), 0..0)
    }

    /// Extracts the element type from an iterable type.
    ///
    /// Supports: List<T>, Set<T>, Map<K,V>, String, Range<T>
    pub(crate) fn get_iterable_element_type(&mut self, ty: &Type, span: Span) -> Type {
        match &ty.kind {
            TypeKind::List(inner) => self
                .extract_type_from_expression(inner)
                .unwrap_or_else(|_| Self::error_type()),
            TypeKind::String => make_type(TypeKind::String),
            TypeKind::Set(inner) => self
                .extract_type_from_expression(inner)
                .unwrap_or_else(|_| Self::error_type()),
            TypeKind::Map(key, val) => make_type(TypeKind::Tuple(vec![*key.clone(), *val.clone()])),
            TypeKind::Custom(name, args) if name == "Range" => {
                if let Some(args) = args {
                    if let Some(arg) = args.first() {
                        return self
                            .extract_type_from_expression(arg)
                            .unwrap_or_else(|_| Self::error_type());
                    }
                }
                Self::error_type()
            }
            TypeKind::Error => Self::error_type(),
            _ => {
                self.report_error(format!("Type {} is not iterable", ty), span);
                Self::error_type()
            }
        }
    }

    // ==================== Name and Type Extraction ====================

    /// Extracts a name from an identifier expression.
    pub(crate) fn extract_name(&self, expr: &Expression) -> Result<String, String> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => Ok(name.clone()),
            _ => Err("Expected identifier".to_string()),
        }
    }

    /// Extracts a type name from an expression (identifier or type expression).
    pub(crate) fn extract_type_name(&self, expr: &Expression) -> Result<String, String> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => Ok(name.clone()),
            ExpressionKind::Type(ty, _) => match &ty.kind {
                TypeKind::Custom(name, _) => Ok(name.clone()),
                _ => Err("Expected custom type".to_string()),
            },
            _ => Err("Expected type identifier".to_string()),
        }
    }

    /// Extracts a Type from a type expression.
    pub(crate) fn extract_type_from_expression(&self, expr: &Expression) -> Result<Type, String> {
        match &expr.node {
            ExpressionKind::Type(t, is_nullable) => {
                if *is_nullable {
                    Ok(make_type(TypeKind::Nullable(t.clone())))
                } else {
                    Ok(*t.clone())
                }
            }
            _ => Err("Expected type expression".to_string()),
        }
    }

    // ==================== Type Resolution ====================

    /// Resolves a type expression to a concrete Type.
    ///
    /// Handles:
    /// - Built-in collection types (List, Set, Map, Range)
    /// - Nullable types
    /// - Custom types with generic arguments
    /// - Type aliases
    /// - Generic type parameters
    pub(crate) fn resolve_type_expression(&mut self, expr: &Expression, context: &Context) -> Type {
        match self.extract_type_from_expression(expr) {
            Ok(t) => self.resolve_type_kind(t, expr, context),
            Err(msg) => {
                self.report_error(msg, expr.span.clone());
                Self::error_type()
            }
        }
    }

    /// Resolves a Type based on its kind.
    fn resolve_type_kind(&mut self, t: Type, expr: &Expression, context: &Context) -> Type {
        match t.kind {
            TypeKind::List(inner) => {
                let resolved_inner = self.resolve_type_expression(&inner, context);
                make_type(TypeKind::List(Box::new(
                    self.create_type_expression(resolved_inner),
                )))
            }
            TypeKind::Set(inner) => {
                let resolved_inner = self.resolve_type_expression(&inner, context);
                if let TypeKind::Nullable(_) = resolved_inner.kind {
                    self.report_error(
                        "Set elements cannot be nullable".to_string(),
                        inner.span.clone(),
                    );
                }
                make_type(TypeKind::Set(Box::new(
                    self.create_type_expression(resolved_inner),
                )))
            }
            TypeKind::Map(k, v) => {
                let rk = self.resolve_type_expression(&k, context);
                if let TypeKind::Nullable(_) = rk.kind {
                    self.report_error("Map keys cannot be nullable".to_string(), k.span.clone());
                }
                let rv = self.resolve_type_expression(&v, context);
                make_type(TypeKind::Map(
                    Box::new(self.create_type_expression(rk)),
                    Box::new(self.create_type_expression(rv)),
                ))
            }
            TypeKind::Nullable(inner) => {
                let inner_expr = self.create_type_expression(*inner);
                let resolved_inner = self.resolve_type_expression(&inner_expr, context);
                make_type(TypeKind::Nullable(Box::new(resolved_inner)))
            }
            TypeKind::Custom(name, args) => self.resolve_custom_type(&name, args, expr, context),
            _ => make_type(t.kind),
        }
    }

    /// Resolves a custom type (user-defined or built-in generic type).
    fn resolve_custom_type(
        &mut self,
        name: &str,
        args: Option<Vec<Expression>>,
        expr: &Expression,
        context: &Context,
    ) -> Type {
        // Handle built-in generic type aliases
        if let Some(resolved) = self.resolve_builtin_type_alias(name, &args, context) {
            return resolved;
        }

        // Resolve generic arguments recursively
        let resolved_args = args.map(|args_vec| {
            args_vec
                .iter()
                .map(|arg| {
                    let resolved_type = self.resolve_type_expression(arg, context);
                    self.create_type_expression(resolved_type)
                })
                .collect()
        });

        // Look up type definition
        let def = context
            .resolve_type_definition(name)
            .cloned()
            .or_else(|| self.global_type_definitions.get(name).cloned());

        if let Some(def) = def {
            self.validate_and_resolve_type_definition(name, def, resolved_args, expr, context)
        } else {
            self.report_unknown_type(name, expr, context);
            Self::error_type()
        }
    }

    /// Resolves built-in type aliases like Map<K,V>, List<T>, Set<T>, Range<T>.
    fn resolve_builtin_type_alias(
        &mut self,
        name: &str,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        match name {
            "map" => {
                if let Some(args) = args {
                    if args.len() == 2 {
                        let k = self.resolve_type_expression(&args[0], context);
                        if let TypeKind::Nullable(_) = k.kind {
                            self.report_error(
                                "Map keys cannot be nullable".to_string(),
                                args[0].span.clone(),
                            );
                        }
                        let v = self.resolve_type_expression(&args[1], context);
                        return Some(make_type(TypeKind::Map(
                            Box::new(self.create_type_expression(k)),
                            Box::new(self.create_type_expression(v)),
                        )));
                    }
                }
                None
            }
            "list" => {
                if let Some(args) = args {
                    if args.len() == 1 {
                        let t = self.resolve_type_expression(&args[0], context);
                        return Some(make_type(TypeKind::List(Box::new(
                            self.create_type_expression(t),
                        ))));
                    }
                }
                None
            }
            "set" => {
                if let Some(args) = args {
                    if args.len() == 1 {
                        let t = self.resolve_type_expression(&args[0], context);
                        if let TypeKind::Nullable(_) = t.kind {
                            self.report_error(
                                "Set elements cannot be nullable".to_string(),
                                args[0].span.clone(),
                            );
                        }
                        return Some(make_type(TypeKind::Set(Box::new(
                            self.create_type_expression(t),
                        ))));
                    }
                }
                None
            }
            "range" => {
                if let Some(args) = args {
                    if args.len() == 1 {
                        let t = self.resolve_type_expression(&args[0], context);
                        return Some(make_type(TypeKind::Custom(
                            "Range".to_string(),
                            Some(vec![self.create_type_expression(t)]),
                        )));
                    }
                } else {
                    // Default to Range<Int>
                    return Some(make_type(TypeKind::Custom(
                        "Range".to_string(),
                        Some(vec![self.create_type_expression(make_type(TypeKind::Int))]),
                    )));
                }
                None
            }
            "Linear" => {
                if let Some(args) = args {
                    if args.len() == 1 {
                        let t = self.resolve_type_expression(&args[0], context);
                        return Some(make_type(TypeKind::Linear(Box::new(t))));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Validates a type definition and returns the resolved type.
    fn validate_and_resolve_type_definition(
        &mut self,
        name: &str,
        def: TypeDefinition,
        resolved_args: Option<Vec<Expression>>,
        expr: &Expression,
        context: &Context,
    ) -> Type {
        match def {
            TypeDefinition::Struct(struct_def) => {
                self.validate_generics(
                    &resolved_args,
                    &struct_def.generics,
                    context,
                    expr.span.clone(),
                );
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
            TypeDefinition::Enum(enum_def) => {
                self.validate_generics(
                    &resolved_args,
                    &enum_def.generics,
                    context,
                    expr.span.clone(),
                );
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
            TypeDefinition::Generic(gen_def) => {
                if resolved_args.is_some() {
                    self.report_error(
                        "Generic type parameter cannot have generic arguments".to_string(),
                        expr.span.clone(),
                    );
                }
                make_type(TypeKind::Generic(
                    name.to_string(),
                    gen_def.constraint.clone().map(Box::new),
                    gen_def.kind.clone(),
                ))
            }
            TypeDefinition::Alias(alias_def) => {
                self.resolve_type_alias(name, alias_def, resolved_args, expr, context)
            }
            TypeDefinition::Class(class_def) => {
                self.validate_generics(
                    &resolved_args,
                    &class_def.generics,
                    context,
                    expr.span.clone(),
                );
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
            TypeDefinition::Trait(trait_def) => {
                self.validate_generics(
                    &resolved_args,
                    &trait_def.generics,
                    context,
                    expr.span.clone(),
                );
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
        }
    }

    /// Resolves a type alias with generic substitution.
    fn resolve_type_alias(
        &mut self,
        name: &str,
        alias_def: super::context::AliasDefinition,
        resolved_args: Option<Vec<Expression>>,
        expr: &Expression,
        _context: &Context,
    ) -> Type {
        let expected_count = alias_def.generics.as_ref().map_or(0, |g| g.len());
        let provided_count = resolved_args.as_ref().map_or(0, |a| a.len());

        if expected_count != provided_count {
            self.report_generic_count_mismatch(name, expected_count, provided_count, expr);
            return Self::error_type();
        }

        // Substitute generic parameters
        if let Some(gen_defs) = &alias_def.generics {
            let mut mapping = std::collections::HashMap::new();
            if let Some(args) = &resolved_args {
                for (gen_def, arg_expr) in gen_defs.iter().zip(args.iter()) {
                    let arg_type = self
                        .extract_type_from_expression(arg_expr)
                        .unwrap_or_else(|_| Self::error_type());
                    mapping.insert(gen_def.name.clone(), arg_type);
                }
            }
            return self.substitute_type(&alias_def.template, &mapping);
        }

        alias_def.template.clone()
    }

    /// Reports a generic argument count mismatch error.
    fn report_generic_count_mismatch(
        &mut self,
        name: &str,
        expected: usize,
        provided: usize,
        expr: &Expression,
    ) {
        let message = if expected == 0 && provided > 0 {
            format!(
                "Type alias '{}' is not generic but {} type argument(s) were provided",
                name, provided
            )
        } else if provided == 0 && expected > 0 {
            format!(
                "Type alias '{}' requires {} type argument(s)",
                name, expected
            )
        } else {
            format!(
                "Type alias '{}' expects {} type argument(s), got {}",
                name, expected, provided
            )
        };
        self.report_error(message, expr.span.clone());
    }

    /// Reports an unknown type error with suggestions.
    fn report_unknown_type(&mut self, name: &str, expr: &Expression, context: &Context) {
        let mut candidates: Vec<&str> = Vec::new();
        for scope in &context.type_definitions {
            candidates.extend(scope.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.global_type_definitions.keys().map(|s| s.as_str()));
        candidates.extend(["Int", "Float", "String", "Bool", "Void", "Any"]);

        if let Some(suggestion) = find_best_match(name, &candidates) {
            self.report_error_with_help(
                format!("Unknown type: {}", name),
                expr.span.clone(),
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(format!("Unknown type: {}", name), expr.span.clone());
        }
    }

    // ==================== Mutability Checking ====================

    /// Checks if an expression is mutable (can be assigned to).
    #[allow(clippy::only_used_in_recursion)]
    pub(crate) fn is_mutable_expression(&self, expr: &Expression, context: &Context) -> bool {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => {
                // 'self' is considered mutable for assignment purposes
                if name == "self" {
                    return true;
                }
                context.is_mutable(name)
            }
            ExpressionKind::Member(obj, prop) => {
                // For self.field, check field mutability
                if let ExpressionKind::Identifier(name, _) = &obj.node {
                    if name == "self" {
                        if let Some(class_name) = &context.current_class {
                            if let Some(TypeDefinition::Class(def)) =
                                self.global_type_definitions.get(class_name)
                            {
                                if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                                    if let Some(field_info) = def.fields.get(field_name) {
                                        return field_info.mutable;
                                    }
                                }
                            }
                        }
                        return true;
                    }
                }
                self.is_mutable_expression(obj, context)
            }
            ExpressionKind::Index(obj, _) => self.is_mutable_expression(obj, context),
            _ => false,
        }
    }

    // ==================== Error Reporting ====================

    /// Reports a type error.
    pub(crate) fn report_error(&mut self, message: String, span: Span) {
        self.errors.push(TypeError::custom(message, span, None));
    }

    /// Reports a type error with a help message.
    pub(crate) fn report_error_with_help(&mut self, message: String, span: Span, help: String) {
        self.errors
            .push(TypeError::custom(message, span, Some(help)));
    }

    /// Reports a type warning.
    pub(crate) fn report_warning(&mut self, message: String, span: Span) {
        use crate::error::diagnostic::{Diagnostic, Severity};
        self.warnings.push(Diagnostic {
            severity: Severity::Warning,
            code: None,
            title: message.clone(),
            message,
            span: Some(span),
            help: None,
            notes: Vec::new(),
        });
    }
}
