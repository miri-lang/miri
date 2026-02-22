// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type compatibility checking for the type checker.
//!
//! This module handles determining whether types are compatible for
//! assignments, function calls, and operations. It includes support for:
//! - Structural type equality
//! - Nullable type compatibility
//! - Numeric type widening
//! - Subtyping (inheritance, interfaces, mixins)
//! - Generic type constraints

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};

impl TypeChecker {
    /// Checks if two types are compatible for assignment or operation.
    ///
    /// This function handles:
    /// - Exact type equality
    /// - Nullable type compatibility (`T` is compatible with `T?`, `None` is compatible with `T?`)
    /// - Numeric type compatibility (literals, widening)
    /// - Inheritance/Interface implementation (via `is_subtype`)
    /// - Generic type constraints
    pub(crate) fn are_compatible(&self, t1: &Type, t2: &Type, context: &Context) -> bool {
        // Fast path: exact equality
        if t1 == t2 {
            return true;
        }

        // Handle nullable types
        if let Some(result) = self.check_nullable_compatibility(t1, t2, context) {
            return result;
        }

        // Handle numeric compatibility
        if let Some(result) = self.check_numeric_compatibility(t1, t2) {
            return result;
        }

        // TypeKind::String and Custom("String") are the same type
        if self.is_string_type(t1) && self.is_string_type(t2) {
            return true;
        }

        // Handle custom types (inheritance, interfaces)
        if let Some(result) = self.check_custom_type_compatibility(t1, t2, context) {
            return result;
        }

        // Handle collection types
        if let Some(result) = self.check_collection_compatibility(t1, t2, context) {
            return result;
        }

        // Handle function types
        if let Some(result) = self.check_function_compatibility(t1, t2, context) {
            return result;
        }

        // Handle generic types
        if let Some(result) = self.check_generic_compatibility(t1, t2, context) {
            return result;
        }

        // Default: structural equality
        t1 == t2
    }

    /// Checks nullable type compatibility.
    fn check_nullable_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        if let TypeKind::Nullable(inner) = &t1.kind {
            // Nullable(T) accepts T or None
            if let TypeKind::Nullable(inner2) = &t2.kind {
                if matches!(inner2.kind, TypeKind::Void) {
                    return Some(true); // None is compatible with any nullable
                }
                return Some(self.are_compatible(inner, inner2, context));
            }
            // Also accepts non-nullable T
            return Some(self.are_compatible(inner, t2, context));
        }

        // Non-nullable type cannot accept nullable
        if let TypeKind::Nullable(_) = &t2.kind {
            return Some(false);
        }

        None
    }

    /// Checks numeric type compatibility including literal widening.
    fn check_numeric_compatibility(&self, t1: &Type, t2: &Type) -> Option<bool> {
        // Int literal compatible with any integer type
        if matches!(t2.kind, TypeKind::Int) && self.is_integer(t1) {
            return Some(true);
        }

        // Float literal compatible with any float type
        if matches!(t2.kind, TypeKind::Float) && matches!(t1.kind, TypeKind::F32 | TypeKind::F64) {
            return Some(true);
        }

        // F32/F64 compatible with Float variable
        if matches!(t1.kind, TypeKind::Float) && matches!(t2.kind, TypeKind::F32 | TypeKind::F64) {
            return Some(true);
        }

        // Integer widening: smaller to larger
        if self.is_integer(t1) && self.is_integer(t2) {
            if let (Some(s1), Some(s2)) = (self.get_integer_size(t1), self.get_integer_size(t2)) {
                if s1 >= s2 {
                    return Some(true);
                }
            }
        }

        // Float widening: F32 to F64
        if matches!(t1.kind, TypeKind::F64) && matches!(t2.kind, TypeKind::F32) {
            return Some(true);
        }

        None
    }

    /// Checks custom type compatibility (inheritance, interfaces).
    fn check_custom_type_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        if let (TypeKind::Custom(n1, args1), TypeKind::Custom(n2, args2)) = (&t1.kind, &t2.kind) {
            if n1 == n2 {
                // Same type name - check generic arguments
                return Some(self.check_generic_args_compatible(args1, args2, context));
            }

            // Check subtyping relationship
            if self.is_subtype(n2, n1) {
                return Some(true);
            }
        }
        None
    }

    /// Checks if generic arguments are compatible.
    fn check_generic_args_compatible(
        &self,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> bool {
        match (args1, args2) {
            (Some(a1), Some(a2)) => {
                if a1.len() != a2.len() {
                    return false;
                }
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    let t1 = self
                        .extract_type_from_expression(arg1)
                        .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                    let t2 = self
                        .extract_type_from_expression(arg2)
                        .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                    if !self.are_compatible(&t1, &t2, context) {
                        return false;
                    }
                }
                true
            }
            (None, None) => true,
            _ => false, // Mismatch in generic args presence
        }
    }

    /// Checks collection type compatibility (List, Set, Map).
    fn check_collection_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        match (&t1.kind, &t2.kind) {
            (TypeKind::List(inner1), TypeKind::List(inner2)) => {
                Some(self.check_inner_type_compatible(inner1, inner2, context))
            }
            (TypeKind::Set(inner1), TypeKind::Set(inner2)) => {
                Some(self.check_inner_type_compatible(inner1, inner2, context))
            }
            (TypeKind::Map(k1, v1), TypeKind::Map(k2, v2)) => {
                if let (Ok(k2_t), Ok(v2_t)) = (
                    self.extract_type_from_expression(k2),
                    self.extract_type_from_expression(v2),
                ) {
                    // Empty map compatible with any map type
                    if matches!(k2_t.kind, TypeKind::Void) && matches!(v2_t.kind, TypeKind::Void) {
                        return Some(true);
                    }
                    if let (Ok(k1_t), Ok(v1_t)) = (
                        self.extract_type_from_expression(k1),
                        self.extract_type_from_expression(v1),
                    ) {
                        return Some(
                            self.are_compatible(&k1_t, &k2_t, context)
                                && self.are_compatible(&v1_t, &v2_t, context),
                        );
                    }
                }
                Some(false)
            }
            (TypeKind::Nullable(inner1), TypeKind::Nullable(inner2)) => {
                if matches!(inner2.kind, TypeKind::Void) {
                    return Some(true);
                }
                Some(self.are_compatible(inner1, inner2, context))
            }
            (TypeKind::Nullable(inner1), _) => Some(self.are_compatible(inner1, t2, context)),
            (TypeKind::Result(ok1, err1), TypeKind::Result(ok2, err2)) => {
                Some(self.check_result_compatible(ok1, err1, ok2, err2, context))
            }
            _ => None,
        }
    }

    /// Checks inner type compatibility for collections.
    fn check_inner_type_compatible(
        &self,
        inner1: &crate::ast::Expression,
        inner2: &crate::ast::Expression,
        context: &Context,
    ) -> bool {
        if let Ok(t2_inner) = self.extract_type_from_expression(inner2) {
            // Empty collection compatible with any element type
            if matches!(t2_inner.kind, TypeKind::Void) {
                return true;
            }
            if let Ok(t1_inner) = self.extract_type_from_expression(inner1) {
                // Special case: Int literal vs specific integer type
                if matches!(t2_inner.kind, TypeKind::Int)
                    && self.is_integer(&t1_inner)
                    && !matches!(t1_inner.kind, TypeKind::Int)
                {
                    return false;
                }
                return self.are_compatible(&t1_inner, &t2_inner, context);
            }
        }
        false
    }

    /// Checks Result type compatibility.
    fn check_result_compatible(
        &self,
        ok1: &crate::ast::Expression,
        err1: &crate::ast::Expression,
        ok2: &crate::ast::Expression,
        err2: &crate::ast::Expression,
        context: &Context,
    ) -> bool {
        if let (Ok(ok1_t), Ok(err1_t), Ok(ok2_t), Ok(err2_t)) = (
            self.extract_type_from_expression(ok1),
            self.extract_type_from_expression(err1),
            self.extract_type_from_expression(ok2),
            self.extract_type_from_expression(err2),
        ) {
            let ok_compatible = matches!(ok2_t.kind, TypeKind::Void)
                || self.are_compatible(&ok1_t, &ok2_t, context);
            let err_compatible = matches!(err2_t.kind, TypeKind::Void)
                || self.are_compatible(&err1_t, &err2_t, context);
            return ok_compatible && err_compatible;
        }
        false
    }

    /// Checks function type compatibility.
    fn check_function_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        if let (TypeKind::Function(gen1, params1, ret1), TypeKind::Function(gen2, params2, ret2)) =
            (&t1.kind, &t2.kind)
        {
            // Check generics count
            let gen1_len = gen1.as_ref().map(|v| v.len()).unwrap_or(0);
            let gen2_len = gen2.as_ref().map(|v| v.len()).unwrap_or(0);
            if gen1_len != gen2_len {
                return Some(false);
            }

            // Check parameters
            if params1.len() != params2.len() {
                return Some(false);
            }

            for (p1, p2) in params1.iter().zip(params2.iter()) {
                let t1 = self
                    .extract_type_from_expression(&p1.typ)
                    .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                let t2 = self
                    .extract_type_from_expression(&p2.typ)
                    .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                if !self.are_compatible(&t1, &t2, context) {
                    return Some(false);
                }
            }

            // Check return type
            let r1 = ret1
                .as_ref()
                .and_then(|r| self.extract_type_from_expression(r).ok())
                .unwrap_or(crate::ast::factory::make_type(TypeKind::Void));
            let r2 = ret2
                .as_ref()
                .and_then(|r| self.extract_type_from_expression(r).ok())
                .unwrap_or(crate::ast::factory::make_type(TypeKind::Void));

            return Some(self.are_compatible(&r1, &r2, context));
        }
        None
    }

    /// Checks generic type compatibility.
    fn check_generic_compatibility(&self, t1: &Type, t2: &Type, context: &Context) -> Option<bool> {
        if let TypeKind::Generic(_, constraint, kind) = &t1.kind {
            if let Some(c) = constraint {
                return Some(self.satisfies_constraint(t2, c, kind, context));
            }
            return Some(true); // Unconstrained generic accepts anything
        }

        if let TypeKind::Generic(_, Some(constraint), kind) = &t2.kind {
            if matches!(kind, TypeDeclarationKind::Extends) {
                return Some(self.are_compatible(t1, constraint, context));
            }
            return Some(false);
        }

        None
    }

    /// Checks if a type is a subtype of another (inheritance, interfaces, mixins).
    pub(crate) fn is_subtype(&self, sub: &str, sup: &str) -> bool {
        if sub == sup {
            return true;
        }

        if let Some(relation) = self.hierarchy.get(sub) {
            // Check extends
            if let Some(parent) = &relation.extends {
                if self.is_subtype(parent, sup) {
                    return true;
                }
            }
            // Check implements
            for interface in &relation.implements {
                if self.is_subtype(interface, sup) {
                    return true;
                }
            }
            // Check includes
            for mixin in &relation.includes {
                if self.is_subtype(mixin, sup) {
                    return true;
                }
            }
        }
        false
    }

    /// Checks if a type satisfies a constraint.
    pub(crate) fn satisfies_constraint(
        &self,
        ty: &Type,
        constraint: &Type,
        kind: &TypeDeclarationKind,
        context: &Context,
    ) -> bool {
        match kind {
            TypeDeclarationKind::Extends => self.are_compatible(constraint, ty, context),
            TypeDeclarationKind::Implements => self.check_implements(ty, constraint, context),
            TypeDeclarationKind::Includes => self.check_includes(ty, constraint, context),
            TypeDeclarationKind::Is => ty == constraint,
            TypeDeclarationKind::None => true,
        }
    }

    /// Checks if a type implements an interface (structural typing for structs).
    pub(crate) fn check_implements(&self, ty: &Type, constraint: &Type, context: &Context) -> bool {
        let (constraint_name, ty_name) = match (&constraint.kind, &ty.kind) {
            (TypeKind::Custom(cn, _), TypeKind::Custom(tn, _)) => (cn.clone(), tn.clone()),
            _ => return false,
        };

        // Check hierarchy first
        if self.is_subtype(&ty_name, &constraint_name) {
            return true;
        }

        // Structural typing for structs
        let constraint_def = context
            .resolve_type_definition(&constraint_name)
            .or_else(|| self.global_type_definitions.get(&constraint_name));

        let ty_def = context
            .resolve_type_definition(&ty_name)
            .or_else(|| self.global_type_definitions.get(&ty_name));

        match (constraint_def, ty_def) {
            (Some(TypeDefinition::Struct(c_def)), Some(TypeDefinition::Struct(t_def))) => {
                // Check that ty has all fields of constraint
                for (c_name, c_type, _) in &c_def.fields {
                    if let Some((_, t_type, _)) =
                        t_def.fields.iter().find(|(t_name, _, _)| t_name == c_name)
                    {
                        if !self.are_compatible(c_type, t_type, context) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Checks if a type includes another (mixin pattern).
    pub(crate) fn check_includes(&self, ty: &Type, constraint: &Type, context: &Context) -> bool {
        let (constraint_name, ty_name) = match (&constraint.kind, &ty.kind) {
            (TypeKind::Custom(cn, _), TypeKind::Custom(tn, _)) => (cn.clone(), tn.clone()),
            _ => return false,
        };

        // Check hierarchy first
        if self.is_subtype(&ty_name, &constraint_name) {
            return true;
        }

        // Structural checking
        let constraint_def = context
            .resolve_type_definition(&constraint_name)
            .or_else(|| self.global_type_definitions.get(&constraint_name));

        let ty_def = context
            .resolve_type_definition(&ty_name)
            .or_else(|| self.global_type_definitions.get(&ty_name));

        match constraint_def {
            Some(TypeDefinition::Class(class_def)) => {
                self.check_class_methods_included(class_def, ty_def, context)
            }
            Some(TypeDefinition::Trait(trait_def)) => {
                self.check_trait_methods_included(trait_def, ty_def, context)
            }
            Some(TypeDefinition::Struct(struct_def)) => {
                self.check_struct_fields_included(struct_def, ty_def, context)
            }
            _ => false,
        }
    }

    /// Checks that a type includes all methods from a class.
    fn check_class_methods_included(
        &self,
        class_def: &super::context::ClassDefinition,
        ty_def: Option<&TypeDefinition>,
        context: &Context,
    ) -> bool {
        let ty_methods = match ty_def {
            Some(TypeDefinition::Class(td)) => &td.methods,
            _ => return false,
        };

        for (method_name, method_info) in &class_def.methods {
            if let Some(ty_method) = ty_methods.get(method_name) {
                if !self.check_method_compatible(ty_method, method_info, context) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Checks that a type includes all methods from a trait.
    fn check_trait_methods_included(
        &self,
        trait_def: &super::context::TraitDefinition,
        ty_def: Option<&TypeDefinition>,
        context: &Context,
    ) -> bool {
        let ty_methods = match ty_def {
            Some(TypeDefinition::Class(td)) => &td.methods,
            _ => return false,
        };

        for (method_name, method_info) in &trait_def.methods {
            if let Some(ty_method) = ty_methods.get(method_name) {
                if !self.check_method_compatible(ty_method, method_info, context) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Checks that a type includes all fields from a struct.
    fn check_struct_fields_included(
        &self,
        struct_def: &super::context::StructDefinition,
        ty_def: Option<&TypeDefinition>,
        context: &Context,
    ) -> bool {
        let ty_fields = match ty_def {
            Some(TypeDefinition::Struct(td)) => &td.fields,
            _ => return false,
        };

        for (c_name, c_type, _) in &struct_def.fields {
            if let Some((_, t_type, _)) = ty_fields.iter().find(|(t_name, _, _)| t_name == c_name) {
                if !self.are_compatible(c_type, t_type, context) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Checks that two method signatures are compatible.
    fn check_method_compatible(
        &self,
        ty_method: &super::context::MethodInfo,
        expected_method: &super::context::MethodInfo,
        context: &Context,
    ) -> bool {
        // Check parameter count
        if ty_method.params.len() != expected_method.params.len() {
            return false;
        }

        // Check parameter types
        for ((_, p_type), (_, c_type)) in ty_method.params.iter().zip(expected_method.params.iter())
        {
            if !self.are_compatible(p_type, c_type, context) {
                return false;
            }
        }

        // Check return type
        self.are_compatible(
            &ty_method.return_type,
            &expected_method.return_type,
            context,
        )
    }

    /// Returns `true` if the type represents a string, either the built-in
    /// `TypeKind::String` or the class `TypeKind::Custom("String", _)`.
    fn is_string_type(&self, ty: &Type) -> bool {
        match &ty.kind {
            TypeKind::String => true,
            TypeKind::Custom(name, _) => name == "String",
            _ => false,
        }
    }
}
