// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::*;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;

impl TypeChecker {
    pub(crate) fn check_binary_op_types(
        &self,
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
                if self.are_compatible(left, right, context) {
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
                if matches!(left, Type::Int) && matches!(right, Type::Int) {
                    Ok(Type::Int)
                } else {
                    Err(format!(
                        "Invalid types for bitwise operation: {:?} and {:?}",
                        left, right
                    ))
                }
            }
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

    pub(crate) fn check_visibility(&self, visibility: &MemberVisibility, module: &str) -> bool {
        match visibility {
            MemberVisibility::Public => true,
            MemberVisibility::Private => module == self.current_module,
            MemberVisibility::Protected => {
                module == self.current_module || self.is_subtype(&self.current_module, module)
            }
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

        // Handle inheritance and interfaces
        if let (Type::Custom(n1, _), Type::Custom(n2, _)) = (t1, t2) {
            if self.is_subtype(n2, n1) {
                return true;
            }
        }

        match (t1, t2) {
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
                if let Type::Custom(name, args) = &t {
                    let def = context.resolve_type_definition(name);
                    if let Some(def) = def {
                        match def {
                            TypeDefinition::Struct(struct_def) => {
                                self.validate_generics(
                                    args,
                                    &struct_def.generics,
                                    context,
                                    expr.span.clone(),
                                );
                            }
                            TypeDefinition::Enum(_) => {
                                // TODO: Enum generics
                            }
                            TypeDefinition::Generic(gen_def) => {
                                if args.is_some() {
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
                                if args.is_some() {
                                    // TODO: Handle generic aliases
                                }
                                return alias_type.clone();
                            }
                        }
                    } else {
                        self.report_error(format!("Unknown type: {}", name), expr.span.clone());
                        return Type::Error;
                    }
                }
                t
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
}
