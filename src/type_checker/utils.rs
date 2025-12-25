// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::context::{Context, GenericDefinition, TypeDefinition};
use super::TypeChecker;
use crate::ast::*;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;

impl TypeChecker {
    pub(crate) fn check_binary_op_types(
        &mut self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                if self.is_numeric(left) && self.is_numeric(right) {
                    if self.are_compatible(left, right, context) {
                        Ok(left.clone())
                    } else {
                        Err(format!("Type mismatch: {:?} and {:?} are not compatible for arithmetic operation", left, right))
                    }
                } else if matches!(op, BinaryOp::Add)
                    && matches!(left, Type::String)
                    && matches!(right, Type::String)
                {
                    Ok(Type::String)
                } else if matches!(op, BinaryOp::Mul) {
                    // String multiplication
                    if (matches!(left, Type::String) && matches!(right, Type::Int))
                        || (matches!(left, Type::Int) && matches!(right, Type::String))
                    {
                        Ok(Type::String)
                    } else {
                        Err(format!(
                            "Invalid types for multiplication: {:?} and {:?}",
                            left, right
                        ))
                    }
                } else {
                    Err(format!(
                        "Invalid types for arithmetic operation: {:?} and {:?}",
                        left, right
                    ))
                }
            }
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual => {
                // Allow comparison between any integers
                if self.is_integer(left) && self.is_integer(right) {
                    Ok(Type::Boolean)
                } else if (matches!(left, Type::Float | Type::F32 | Type::F64)
                    && matches!(right, Type::Float | Type::F32 | Type::F64))
                {
                    // Allow comparison between any floats
                    Ok(Type::Boolean)
                } else if self.are_compatible(left, right, context) {
                    Ok(Type::Boolean)
                } else {
                    Err(format!(
                        "Type mismatch: cannot compare {:?} and {:?}",
                        left, right
                    ))
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                if matches!(left, Type::Boolean) && matches!(right, Type::Boolean) {
                    Ok(Type::Boolean)
                } else {
                    Err(format!(
                        "Logical operations require booleans, got {:?} and {:?}",
                        left, right
                    ))
                }
            }
            BinaryOp::BitwiseAnd | BinaryOp::BitwiseOr | BinaryOp::BitwiseXor => {
                if self.is_integer(left) && self.is_integer(right) {
                    if left == right || matches!(right, Type::Int) {
                        Ok(left.clone())
                    } else if matches!(left, Type::Int) && self.are_compatible(right, left, context)
                    {
                        Ok(right.clone())
                    } else {
                        Err(format!(
                            "Type mismatch: {:?} and {:?} are not compatible for bitwise operation",
                            left, right
                        ))
                    }
                } else {
                    Err(format!(
                        "Invalid types for bitwise operation: {:?} and {:?}",
                        left, right
                    ))
                }
            }
            BinaryOp::In => match right {
                Type::List(inner_expr) | Type::Set(inner_expr) => {
                    let inner = self.resolve_type_expression(inner_expr, context);
                    if self.are_compatible(&inner, left, context) {
                        Ok(Type::Boolean)
                    } else {
                        Err(format!(
                            "Type mismatch: cannot check membership of {:?} in collection of {:?}",
                            left, inner
                        ))
                    }
                }
                Type::Map(key_expr, _) => {
                    let key = self.resolve_type_expression(key_expr, context);
                    if self.are_compatible(&key, left, context) {
                        Ok(Type::Boolean)
                    } else {
                        Err(format!(
                            "Type mismatch: cannot check membership of {:?} in map with keys of {:?}",
                            left, key
                        ))
                    }
                }
                Type::Custom(name, Some(args)) if name == "Range" && args.len() == 1 => {
                    let range_type = self.resolve_type_expression(&args[0], context);
                    if self.are_compatible(&range_type, left, context) {
                        Ok(Type::Boolean)
                    } else {
                        Err(format!(
                            "Type mismatch: cannot check membership of {:?} in range of {:?}",
                            left, range_type
                        ))
                    }
                }
                Type::String => {
                    if matches!(left, Type::String) {
                        Ok(Type::Boolean)
                    } else {
                        Err(format!(
                            "Type mismatch: cannot check membership of {:?} in String (expected String)",
                            left
                        ))
                    }
                }
                _ => Err(format!(
                    "Invalid type for 'in' operator: expected collection, got {:?}",
                    right
                )),
            },
            _ => Ok(Type::Boolean),
        }
    }

    pub(crate) fn check_unary_op_types(
        &self,
        op: &UnaryOp,
        expr_type: &Type,
    ) -> Result<Type, String> {
        match op {
            UnaryOp::Negate | UnaryOp::Plus | UnaryOp::Decrement | UnaryOp::Increment => {
                if self.is_numeric(expr_type) {
                    Ok(expr_type.clone())
                } else {
                    Err(format!(
                        "Unary operator requires numeric type, got {:?}",
                        expr_type
                    ))
                }
            }
            UnaryOp::Not => {
                if matches!(expr_type, Type::Boolean) {
                    Ok(Type::Boolean)
                } else {
                    Err(format!("Logical NOT requires boolean, got {:?}", expr_type))
                }
            }
            UnaryOp::Await => {
                if let Type::Future(inner_expr) = expr_type {
                    self.extract_type_from_expression(inner_expr)
                } else {
                    Err(format!("Await requires a Future, got {:?}", expr_type))
                }
            }
            _ => Ok(expr_type.clone()),
        }
    }

    pub(crate) fn is_numeric(&self, t: &Type) -> bool {
        matches!(
            t,
            Type::Int
                | Type::Float
                | Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::I128
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::U128
                | Type::F32
                | Type::F64
        )
    }

    pub(crate) fn is_integer(&self, t: &Type) -> bool {
        matches!(
            t,
            Type::Int
                | Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::I128
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::U128
        )
    }

    pub(crate) fn check_visibility(&self, visibility: &MemberVisibility, module: &str) -> bool {
        match visibility {
            MemberVisibility::Public => true,
            MemberVisibility::Private => module == self.current_module,
            MemberVisibility::Protected => {
                module == self.current_module || self.is_subtype(&self.current_module, module)
            }
        }
    }

    pub(crate) fn get_integer_size(&self, t: &Type) -> Option<u8> {
        match t {
            Type::I8 | Type::U8 => Some(8),
            Type::I16 | Type::U16 => Some(16),
            Type::I32 | Type::U32 => Some(32),
            Type::I64 | Type::U64 => Some(64),
            Type::I128 | Type::U128 => Some(128),
            Type::Int => Some(128), // Treat literal Int as max size for compatibility checks? Or handle separately.
            _ => None,
        }
    }

    pub(crate) fn are_compatible(&self, t1: &Type, t2: &Type, context: &Context) -> bool {
        if t1 == t2 {
            return true;
        }

        // Handle Nullable types
        if let Type::Nullable(inner) = t1 {
            // Nullable(T) accepts T or None (Nullable(Void))
            if let Type::Nullable(inner2) = t2 {
                if let Type::Void = **inner2 {
                    return true; // None is compatible with any nullable
                }
                return self.are_compatible(inner, inner2, context);
            }
            // Also accepts non-nullable T
            return self.are_compatible(inner, t2, context);
        }

        // If t1 is NOT nullable, but t2 IS nullable (and not None), it's incompatible
        // unless t2 is Nullable(Void) (None) which is definitely incompatible with non-nullable t1
        if let Type::Nullable(_) = t2 {
            return false;
        }

        // Allow Type::Int (literals) to be assigned to any integer type
        if matches!(t2, Type::Int) && self.is_integer(t1) {
            return true;
        }

        // Allow Type::Float (literals) to be assigned to any float type
        if matches!(t2, Type::Float) && matches!(t1, Type::F32 | Type::F64) {
            return true;
        }

        // Allow F32/F64 to be assigned to Type::Float (variable)
        if matches!(t1, Type::Float) && matches!(t2, Type::F32 | Type::F64) {
            return true;
        }

        // Integer widening: Allow assigning smaller integer to larger integer
        // t1 is the target type (variable), t2 is the source type (value)
        // e.g. let a: i16 = b (where b is i8). t1=I16, t2=I8.
        if self.is_integer(t1) && self.is_integer(t2) {
            if let (Some(s1), Some(s2)) = (self.get_integer_size(t1), self.get_integer_size(t2)) {
                if s1 >= s2 {
                    // Also check signedness compatibility?
                    // For now, let's assume simple size check as requested "assigning smaller type to a larger".
                    // Strict signedness check might be needed later.
                    // e.g. u8 -> i16 (ok), i8 -> u16 (maybe not ok if negative?)
                    // User said "assigning smaller type to a larger".
                    // Let's stick to size for now.
                    return true;
                }
            }
        }

        // Float widening
        if matches!(t1, Type::F64) && matches!(t2, Type::F32) {
            return true;
        }

        // Handle inheritance and interfaces
        if let (Type::Custom(n1, _), Type::Custom(n2, _)) = (t1, t2) {
            if self.is_subtype(n2, n1) {
                return true;
            }
        }

        match (t1, t2) {
            (Type::List(inner1), Type::List(inner2)) => {
                if let Ok(t2_inner) = self.extract_type_from_expression(inner2) {
                    if matches!(t2_inner, Type::Void) {
                        return true;
                    }
                    if let Ok(t1_inner) = self.extract_type_from_expression(inner1) {
                        // If target is specific integer and source is generic Int (literal),
                        // we return false to force value checking in check_variable_declaration
                        if matches!(t2_inner, Type::Int)
                            && self.is_integer(&t1_inner)
                            && !matches!(t1_inner, Type::Int)
                        {
                            return false;
                        }
                        return self.are_compatible(&t1_inner, &t2_inner, context);
                    }
                }
                false
            }
            (Type::Set(inner1), Type::Set(inner2)) => {
                if let Ok(t2_inner) = self.extract_type_from_expression(inner2) {
                    if matches!(t2_inner, Type::Void) {
                        return true;
                    }
                    if let Ok(t1_inner) = self.extract_type_from_expression(inner1) {
                        return self.are_compatible(&t1_inner, &t2_inner, context);
                    }
                }
                false
            }
            (Type::Map(k1, v1), Type::Map(k2, v2)) => {
                if let (Ok(k2_t), Ok(v2_t)) = (
                    self.extract_type_from_expression(k2),
                    self.extract_type_from_expression(v2),
                ) {
                    if matches!(k2_t, Type::Void) && matches!(v2_t, Type::Void) {
                        return true;
                    }
                    if let (Ok(k1_t), Ok(v1_t)) = (
                        self.extract_type_from_expression(k1),
                        self.extract_type_from_expression(v1),
                    ) {
                        return self.are_compatible(&k1_t, &k2_t, context)
                            && self.are_compatible(&v1_t, &v2_t, context);
                    }
                }
                false
            }
            (Type::Nullable(inner1), Type::Nullable(inner2)) => {
                if matches!(**inner2, Type::Void) {
                    return true;
                }
                self.are_compatible(inner1, inner2, context)
            }
            (Type::Nullable(inner1), t2) => self.are_compatible(inner1, t2, context),
            (Type::Result(ok1, err1), Type::Result(ok2, err2)) => {
                if let (Ok(ok1_t), Ok(err1_t), Ok(ok2_t), Ok(err2_t)) = (
                    self.extract_type_from_expression(ok1),
                    self.extract_type_from_expression(err1),
                    self.extract_type_from_expression(ok2),
                    self.extract_type_from_expression(err2),
                ) {
                    let ok_compatible =
                        matches!(ok2_t, Type::Void) || self.are_compatible(&ok1_t, &ok2_t, context);
                    let err_compatible = matches!(err2_t, Type::Void)
                        || self.are_compatible(&err1_t, &err2_t, context);

                    return ok_compatible && err_compatible;
                }
                false
            }
            (Type::Function(gen1, params1, ret1), Type::Function(gen2, params2, ret2)) => {
                // Check generics count
                if gen1.as_ref().map(|v| v.len()).unwrap_or(0)
                    != gen2.as_ref().map(|v| v.len()).unwrap_or(0)
                {
                    return false;
                }

                // Check parameters
                if params1.len() != params2.len() {
                    return false;
                }

                for (p1, p2) in params1.iter().zip(params2.iter()) {
                    // Parameter types must be compatible (contravariant? strict for now)
                    // Also, we ignore parameter names for compatibility if one of them is empty
                    // (which happens in function types vs function declarations)
                    let t1 = self
                        .extract_type_from_expression(&p1.typ)
                        .unwrap_or(Type::Error);
                    let t2 = self
                        .extract_type_from_expression(&p2.typ)
                        .unwrap_or(Type::Error);

                    if !self.are_compatible(&t1, &t2, context) {
                        return false;
                    }
                }

                // Check return type
                let r1 = if let Some(r) = ret1 {
                    self.extract_type_from_expression(r).unwrap_or(Type::Void)
                } else {
                    Type::Void
                };

                let r2 = if let Some(r) = ret2 {
                    self.extract_type_from_expression(r).unwrap_or(Type::Void)
                } else {
                    Type::Void
                };

                self.are_compatible(&r1, &r2, context)
            }
            (Type::Generic(_, constraint, kind), t2) => {
                if let Some(c) = constraint {
                    self.satisfies_constraint(t2, c, kind, context)
                } else {
                    true
                }
            }
            (t1, Type::Generic(_, Some(constraint), kind)) => match kind {
                TypeDeclarationKind::Extends => self.are_compatible(t1, constraint, context),
                _ => false,
            },
            _ => t1 == t2,
        }
    }

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
            // Check includes (treat as mixin/parent)
            for mixin in &relation.includes {
                if self.is_subtype(mixin, sup) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn create_type_expression(&self, ty: Type) -> Expression {
        IdNode::new(0, ExpressionKind::Type(Box::new(ty), false), 0..0)
    }

    pub(crate) fn get_iterable_element_type(&mut self, ty: &Type, span: Span) -> Type {
        match ty {
            Type::List(inner) => self
                .extract_type_from_expression(inner)
                .unwrap_or(Type::Error),
            Type::String => Type::String,
            Type::Set(inner) => self
                .extract_type_from_expression(inner)
                .unwrap_or(Type::Error),
            Type::Map(key, val) => Type::Tuple(vec![*key.clone(), *val.clone()]),
            Type::Custom(name, args) if name == "Range" => {
                if let Some(args) = args {
                    if let Some(arg) = args.first() {
                        return self
                            .extract_type_from_expression(arg)
                            .unwrap_or(Type::Error);
                    }
                }
                Type::Error
            }
            Type::Error => Type::Error,
            _ => {
                self.report_error(format!("Type {:?} is not iterable", ty), span);
                Type::Error
            }
        }
    }

    pub(crate) fn check_implements(&self, ty: &Type, constraint: &Type, context: &Context) -> bool {
        // Resolve constraint to StructDefinition
        let constraint_def = if let Type::Custom(name, _) = constraint {
            context
                .resolve_type_definition(name)
                .or_else(|| self.global_type_definitions.get(name))
        } else {
            return false;
        };

        let constraint_fields = if let Some(TypeDefinition::Struct(def)) = constraint_def {
            &def.fields
        } else {
            return false; // Constraint must be a struct (interface)
        };

        // Resolve ty to StructDefinition
        let ty_def = if let Type::Custom(name, _) = ty {
            context
                .resolve_type_definition(name)
                .or_else(|| self.global_type_definitions.get(name))
        } else {
            return false; // Only structs can implement interfaces for now
        };

        let ty_fields = if let Some(TypeDefinition::Struct(def)) = ty_def {
            &def.fields
        } else {
            return false;
        };

        // Check if ty has all fields of constraint
        for (c_name, c_type, _) in constraint_fields {
            if let Some((_, t_type, _)) = ty_fields.iter().find(|(t_name, _, _)| t_name == c_name) {
                if !self.are_compatible(c_type, t_type, context) {
                    return false;
                }
            } else {
                return false; // Missing field
            }
        }

        true
    }

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
            TypeDeclarationKind::Includes => true, // TODO
            TypeDeclarationKind::Is => ty == constraint,
            TypeDeclarationKind::None => true,
        }
    }

    pub(crate) fn validate_generics(
        &mut self,
        args: &Option<Vec<Expression>>,
        params: &Option<Vec<crate::type_checker::context::GenericDefinition>>,
        context: &Context,
        span: Span,
    ) {
        let args_len = args.as_ref().map_or(0, |v| v.len());
        let params_len = params.as_ref().map_or(0, |v| v.len());

        if args_len != params_len {
            self.report_error(
                format!(
                    "Generic argument count mismatch: expected {}, got {}",
                    params_len, args_len
                ),
                span,
            );
            return;
        }

        if let (Some(args_vec), Some(params_vec)) = (args, params) {
            for (i, arg_expr) in args_vec.iter().enumerate() {
                let param_def = &params_vec[i];
                let arg_type = self.resolve_type_expression(arg_expr, context);

                if let Some(constraint) = &param_def.constraint {
                    if !self.satisfies_constraint(&arg_type, constraint, &param_def.kind, context) {
                        self.report_error(
                            format!(
                                "Type {:?} does not satisfy constraint {:?} {:?}",
                                arg_type, param_def.kind, constraint
                            ),
                            arg_expr.span.clone(),
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn extract_name(&self, expr: &Expression) -> Result<String, String> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => Ok(name.clone()),
            _ => Err("Expected identifier".to_string()),
        }
    }

    pub(crate) fn extract_type_name(&self, expr: &Expression) -> Result<String, String> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => Ok(name.clone()),
            ExpressionKind::Type(ty, _) => match &**ty {
                Type::Custom(name, _) => Ok(name.clone()),
                _ => Err("Expected custom type".to_string()),
            },
            _ => Err("Expected type identifier".to_string()),
        }
    }

    pub(crate) fn extract_type_from_expression(&self, expr: &Expression) -> Result<Type, String> {
        match &expr.node {
            ExpressionKind::Type(t, is_nullable) => {
                if *is_nullable {
                    Ok(Type::Nullable(t.clone()))
                } else {
                    Ok(*t.clone())
                }
            }
            _ => Err("Expected type expression".to_string()),
        }
    }

    pub(crate) fn resolve_type_expression(&mut self, expr: &Expression, context: &Context) -> Type {
        match self.extract_type_from_expression(expr) {
            Ok(t) => {
                match t {
                    Type::List(inner) => {
                        let resolved_inner = self.resolve_type_expression(&inner, context);
                        Type::List(Box::new(self.create_type_expression(resolved_inner)))
                    }
                    Type::Set(inner) => {
                        let resolved_inner = self.resolve_type_expression(&inner, context);
                        if let Type::Nullable(_) = resolved_inner {
                            self.report_error(
                                "Set elements cannot be nullable".to_string(),
                                inner.span.clone(),
                            );
                        }
                        Type::Set(Box::new(self.create_type_expression(resolved_inner)))
                    }
                    Type::Map(k, v) => {
                        let rk = self.resolve_type_expression(&k, context);
                        if let Type::Nullable(_) = rk {
                            self.report_error(
                                "Map keys cannot be nullable".to_string(),
                                k.span.clone(),
                            );
                        }
                        let rv = self.resolve_type_expression(&v, context);
                        Type::Map(
                            Box::new(self.create_type_expression(rk)),
                            Box::new(self.create_type_expression(rv)),
                        )
                    }
                    Type::Nullable(inner) => {
                        let inner_expr = self.create_type_expression(*inner);
                        let resolved_inner = self.resolve_type_expression(&inner_expr, context);
                        Type::Nullable(Box::new(resolved_inner))
                    }
                    Type::Custom(name, args) => {
                        // Handle built-in generic types
                        if name == "map" {
                            if let Some(args) = &args {
                                if args.len() == 2 {
                                    let k = self.resolve_type_expression(&args[0], context);
                                    if let Type::Nullable(_) = k {
                                        self.report_error(
                                            "Map keys cannot be nullable".to_string(),
                                            args[0].span.clone(),
                                        );
                                    }
                                    let v = self.resolve_type_expression(&args[1], context);
                                    return Type::Map(
                                        Box::new(self.create_type_expression(k)),
                                        Box::new(self.create_type_expression(v)),
                                    );
                                }
                            }
                        } else if name == "list" {
                            if let Some(args) = &args {
                                if args.len() == 1 {
                                    let t = self.resolve_type_expression(&args[0], context);
                                    return Type::List(Box::new(self.create_type_expression(t)));
                                }
                            }
                        } else if name == "set" {
                            if let Some(args) = &args {
                                if args.len() == 1 {
                                    let t = self.resolve_type_expression(&args[0], context);
                                    if let Type::Nullable(_) = t {
                                        self.report_error(
                                            "Set elements cannot be nullable".to_string(),
                                            args[0].span.clone(),
                                        );
                                    }
                                    return Type::Set(Box::new(self.create_type_expression(t)));
                                }
                            }
                        } else if name == "range" {
                            // Support explicit range type: range<int> or just range (defaults to int?)
                            // The user example: let r1 range = 1..n
                            // This implies `range` is a valid type name.
                            // If no args, we might assume range<int> or generic range?
                            // infer_range returns Type::Custom("Range", Some(vec![type_expr]))
                            // So we should map "range" to Type::Custom("Range", ...)
                            // Note: "Range" vs "range". infer_range uses "Range".
                            // Let's normalize to "Range".
                            if let Some(args) = &args {
                                if args.len() == 1 {
                                    let t = self.resolve_type_expression(&args[0], context);
                                    return Type::Custom(
                                        "Range".to_string(),
                                        Some(vec![self.create_type_expression(t)]),
                                    );
                                }
                            } else {
                                // Default to range<int> if no generic provided? Or just Range<Any>?
                                // For now let's default to Range<Int> as it's the most common.
                                return Type::Custom(
                                    "Range".to_string(),
                                    Some(vec![self.create_type_expression(Type::Int)]),
                                );
                            }
                        }

                        // Resolve generic arguments recursively
                        let resolved_args = if let Some(args_vec) = args {
                            let mut resolved = Vec::new();
                            for arg in args_vec {
                                let resolved_type = self.resolve_type_expression(&arg, context);
                                resolved.push(self.create_type_expression(resolved_type));
                            }
                            Some(resolved)
                        } else {
                            None
                        };

                        let def = context.resolve_type_definition(&name);
                        if let Some(def) = def {
                            match def {
                                TypeDefinition::Struct(struct_def) => {
                                    self.validate_generics(
                                        &resolved_args,
                                        &struct_def.generics,
                                        context,
                                        expr.span.clone(),
                                    );
                                }
                                TypeDefinition::Enum(_) => {
                                    // TODO: Enum generics
                                }
                                TypeDefinition::Generic(gen_def) => {
                                    if resolved_args.is_some() {
                                        self.report_error(
                                            "Generic type parameter cannot have generic arguments"
                                                .to_string(),
                                            expr.span.clone(),
                                        );
                                    }
                                    return Type::Generic(
                                        name.clone(),
                                        gen_def.constraint.clone().map(Box::new),
                                        gen_def.kind.clone(),
                                    );
                                }
                                TypeDefinition::Alias(alias_type) => {
                                    if resolved_args.is_some() {
                                        // TODO: Handle generic aliases
                                    }
                                    return alias_type.clone();
                                }
                            }
                        } else {
                            self.report_error(format!("Unknown type: {}", name), expr.span.clone());
                            return Type::Error;
                        }
                        Type::Custom(name, resolved_args)
                    }
                    _ => t,
                }
            }
            Err(msg) => {
                self.report_error(msg, expr.span.clone());
                Type::Error
            }
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    pub(crate) fn is_mutable_expression(&self, expr: &Expression, context: &Context) -> bool {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => context.is_mutable(name),
            ExpressionKind::Member(obj, _) => self.is_mutable_expression(obj, context),
            ExpressionKind::Index(obj, _) => self.is_mutable_expression(obj, context),
            _ => false,
        }
    }

    pub(crate) fn report_error(&mut self, message: String, span: Span) {
        self.errors.push(TypeError::new(message, span));
    }

    pub(crate) fn report_warning(&mut self, message: String, span: Span) {
        self.warnings.push(TypeError::new(message, span));
    }

    pub(crate) fn infer_generic_types(
        &self,
        param_type: &Type,
        arg_type: &Type,
        mapping: &mut std::collections::HashMap<String, Type>,
    ) {
        match (param_type, arg_type) {
            (Type::Generic(name, _, _), _) => {
                if !mapping.contains_key(name) {
                    mapping.insert(name.clone(), arg_type.clone());
                }
            }
            (Type::List(p_inner_expr), Type::List(a_inner_expr)) => {
                if let (Ok(p_inner), Ok(a_inner)) = (
                    self.extract_type_from_expression(p_inner_expr),
                    self.extract_type_from_expression(a_inner_expr),
                ) {
                    self.infer_generic_types(&p_inner, &a_inner, mapping);
                }
            }
            (Type::Map(p_k_expr, p_v_expr), Type::Map(a_k_expr, a_v_expr)) => {
                if let (Ok(p_k), Ok(p_v), Ok(a_k), Ok(a_v)) = (
                    self.extract_type_from_expression(p_k_expr),
                    self.extract_type_from_expression(p_v_expr),
                    self.extract_type_from_expression(a_k_expr),
                    self.extract_type_from_expression(a_v_expr),
                ) {
                    self.infer_generic_types(&p_k, &a_k, mapping);
                    self.infer_generic_types(&p_v, &a_v, mapping);
                }
            }
            (Type::Set(p_inner_expr), Type::Set(a_inner_expr)) => {
                if let (Ok(p_inner), Ok(a_inner)) = (
                    self.extract_type_from_expression(p_inner_expr),
                    self.extract_type_from_expression(a_inner_expr),
                ) {
                    self.infer_generic_types(&p_inner, &a_inner, mapping);
                }
            }
            (Type::Nullable(p_inner), Type::Nullable(a_inner)) => {
                self.infer_generic_types(p_inner, a_inner, mapping);
            }
            (Type::Custom(p_name, p_args), Type::Custom(a_name, a_args)) => {
                if p_name == a_name {
                    if let (Some(p_args), Some(a_args)) = (p_args, a_args) {
                        if p_args.len() == a_args.len() {
                            for (p_arg_expr, a_arg_expr) in p_args.iter().zip(a_args.iter()) {
                                if let (Ok(p_arg), Ok(a_arg)) = (
                                    self.extract_type_from_expression(p_arg_expr),
                                    self.extract_type_from_expression(a_arg_expr),
                                ) {
                                    self.infer_generic_types(&p_arg, &a_arg, mapping);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn substitute_type(
        &self,
        ty: &Type,
        mapping: &std::collections::HashMap<String, Type>,
    ) -> Type {
        match ty {
            Type::Generic(name, _, _) => {
                if let Some(subst) = mapping.get(name) {
                    subst.clone()
                } else {
                    ty.clone()
                }
            }
            Type::Custom(name, args) => {
                if args.is_none() {
                    if let Some(subst) = mapping.get(name) {
                        return subst.clone();
                    }
                }
                // Also substitute in args if present
                let new_args = args.as_ref().map(|args_vec| {
                    args_vec
                        .iter()
                        .map(|arg| {
                            let arg_type = self
                                .extract_type_from_expression(arg)
                                .unwrap_or(Type::Error);
                            let subst_arg = self.substitute_type(&arg_type, mapping);
                            self.create_type_expression(subst_arg)
                        })
                        .collect()
                });
                Type::Custom(name.clone(), new_args)
            }
            Type::List(inner_expr) => {
                if let Ok(inner) = self.extract_type_from_expression(inner_expr) {
                    let subst_inner = self.substitute_type(&inner, mapping);
                    Type::List(Box::new(self.create_type_expression(subst_inner)))
                } else {
                    ty.clone()
                }
            }
            Type::Map(k_expr, v_expr) => {
                if let (Ok(k), Ok(v)) = (
                    self.extract_type_from_expression(k_expr),
                    self.extract_type_from_expression(v_expr),
                ) {
                    Type::Map(
                        Box::new(self.create_type_expression(self.substitute_type(&k, mapping))),
                        Box::new(self.create_type_expression(self.substitute_type(&v, mapping))),
                    )
                } else {
                    ty.clone()
                }
            }
            Type::Set(inner_expr) => {
                if let Ok(inner) = self.extract_type_from_expression(inner_expr) {
                    let subst_inner = self.substitute_type(&inner, mapping);
                    Type::Set(Box::new(self.create_type_expression(subst_inner)))
                } else {
                    ty.clone()
                }
            }
            Type::Nullable(inner) => Type::Nullable(Box::new(self.substitute_type(inner, mapping))),
            Type::Result(ok_expr, err_expr) => {
                if let (Ok(ok), Ok(err)) = (
                    self.extract_type_from_expression(ok_expr),
                    self.extract_type_from_expression(err_expr),
                ) {
                    Type::Result(
                        Box::new(self.create_type_expression(self.substitute_type(&ok, mapping))),
                        Box::new(self.create_type_expression(self.substitute_type(&err, mapping))),
                    )
                } else {
                    ty.clone()
                }
            }
            _ => ty.clone(),
        }
    }

    pub(crate) fn define_generics(&mut self, generics: &[Expression], context: &mut Context) {
        for gen in generics {
            if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen.node {
                let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                    n.clone()
                } else {
                    continue;
                };

                let constraint_type = constraint_expr
                    .as_ref()
                    .map(|c| self.resolve_type_expression(c, context));

                context.define_type(
                    name.clone(),
                    TypeDefinition::Generic(GenericDefinition {
                        name: name.clone(),
                        constraint: constraint_type,
                        kind: kind.clone(),
                    }),
                );
            }
        }
    }
}
