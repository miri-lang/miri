// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Operator type checking for the type checker.
//!
//! This module handles type validation for binary and unary operators,
//! ensuring operands have compatible types for the requested operations.

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::types::{
    vec_dim, BuiltinCollectionKind, Type, TypeKind, EQUALS_METHOD_NAME, STRING_TYPE_NAME,
};
use crate::ast::BinaryOp;
use crate::ast::UnaryOp;

/// Bound on how deeply structural equality may nest before the compiler
/// refuses. The comparison is expanded inline, so an unbounded chain would
/// exhaust the compiler's stack on user input.
const MAX_STRUCTURAL_EQUALITY_DEPTH: usize = 64;

impl TypeChecker {
    /// Checks that binary operation operands have compatible types.
    ///
    /// Returns the result type of the operation, or an error message if
    /// the operands are incompatible.
    pub(crate) fn check_binary_op_types(
        &mut self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                self.check_arithmetic_op(left, op, right, context)
            }
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual => self.check_comparison_op(left, right, context),
            BinaryOp::And | BinaryOp::Or => self.check_logical_op(left, right),
            BinaryOp::BitwiseAnd | BinaryOp::BitwiseOr | BinaryOp::BitwiseXor => {
                self.check_bitwise_op(left, right, context)
            }
            BinaryOp::In => self.check_membership_op(left, right, context),
            BinaryOp::NullCoalesce => self.check_null_coalesce_op(left, right, context),
            BinaryOp::Not | BinaryOp::Range => {
                Ok(crate::ast::factory::make_type(TypeKind::Boolean))
            }
        }
    }

    /// Checks arithmetic operations (+, -, *, /, %).
    fn check_arithmetic_op(
        &mut self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let left_is_int = self.is_integer(left);
        let left_is_float = matches!(
            left.kind,
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
        );
        let right_is_int = self.is_integer(right);
        let right_is_float = matches!(
            right.kind,
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
        );

        // Disallow mixed int/float operations
        if (left_is_int && right_is_float) || (left_is_float && right_is_int) {
            let op_name = match op {
                BinaryOp::Add => "add",
                BinaryOp::Sub => "subtract",
                BinaryOp::Mul => "multiply",
                BinaryOp::Div => "divide",
                BinaryOp::Mod => "modulo",
                _ => "operate on",
            };
            return Err(format!(
                "Type mismatch: cannot {} a float to an integer",
                op_name
            ));
        }

        // Numeric operations
        if self.is_numeric(left) && self.is_numeric(right) {
            if self.are_compatible(left, right, context) {
                return Ok(left.clone());
            }
            return Err(format!(
                "Type mismatch: {} and {} are not compatible for arithmetic operation",
                left, right
            ));
        }

        // Allow arithmetic on same-typed generic parameters (e.g. T + T in a generic method body).
        // Concrete enforcement happens at call sites where T is resolved to a numeric type.
        if matches!(left.kind, TypeKind::Generic(..)) && self.are_compatible(left, right, context) {
            return Ok(left.clone());
        }

        // Trait-based Add: if left implements Addable and types are compatible
        if matches!(op, BinaryOp::Add) && self.type_implements_trait(left, "Addable") {
            if self.are_compatible(left, right, context) {
                return Ok(left.clone());
            }
            return Err(format!(
                "Type mismatch: cannot add {} and {} (both must be the same type)",
                left, right
            ));
        }
        // Trait-based Mul: if left implements Multiplicable and right is int
        if matches!(op, BinaryOp::Mul) && self.type_implements_trait(left, "Multiplicable") {
            if self.is_numeric(right) {
                return Ok(left.clone());
            }
            return Err(format!(
                "Type mismatch: cannot multiply {} by {} (right operand must be an integer)",
                left, right
            ));
        }

        // Vector-scalar broadcast: allow operations like Vec3<f32> * f32
        if let TypeKind::Custom(vec_name, Some(args)) = &left.kind {
            if vec_dim(vec_name).is_some() && self.is_numeric(right) {
                if let Some(first_arg) = args.first() {
                    if let crate::ast::expression::ExpressionKind::Type(elem_type, _) =
                        &first_arg.node
                    {
                        if self.is_numeric(elem_type) {
                            return Ok(left.clone());
                        }
                    }
                }
            }
        }

        // Reverse vector-scalar broadcast: allow operations like f32 * Vec3<f32>
        if let TypeKind::Custom(vec_name, Some(args)) = &right.kind {
            if vec_dim(vec_name).is_some() && self.is_numeric(left) {
                if let Some(first_arg) = args.first() {
                    if let crate::ast::expression::ExpressionKind::Type(elem_type, _) =
                        &first_arg.node
                    {
                        if self.is_numeric(elem_type) {
                            return Ok(right.clone());
                        }
                    }
                }
            }
        }

        Err(format!(
            "Invalid types for arithmetic operation: {} and {}",
            left, right
        ))
    }

    /// Checks comparison operations (==, !=, <, <=, >, >=).
    fn check_comparison_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let bool_type = || crate::ast::factory::make_type(TypeKind::Boolean);

        // Allow comparison between any integers
        if self.is_integer(left) && self.is_integer(right) {
            return Ok(bool_type());
        }

        // Allow comparison between any floats
        if matches!(left.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64)
            && matches!(right.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64)
        {
            return Ok(bool_type());
        }

        // Allow comparison between compatible types
        if self.are_compatible(left, right, context) {
            self.check_type_structurally_comparable(left)?;
            return Ok(bool_type());
        }

        // Trait-based Equatable: if left implements Equatable
        if self.type_implements_trait(left, "Equatable")
            && self.are_compatible(left, right, context)
        {
            return Ok(bool_type());
        }

        Err(format!(
            "Type mismatch: cannot compare {} and {}",
            left, right
        ))
    }

    /// Checks logical operations (&&, ||).
    fn check_logical_op(&self, left: &Type, right: &Type) -> Result<Type, String> {
        if matches!(left.kind, TypeKind::Boolean) && matches!(right.kind, TypeKind::Boolean) {
            Ok(crate::ast::factory::make_type(TypeKind::Boolean))
        } else {
            Err(format!(
                "Logical operations require booleans, got {} and {}",
                left, right
            ))
        }
    }

    /// Checks bitwise operations (&, |, ^).
    fn check_bitwise_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        if !self.is_integer(left) || !self.is_integer(right) {
            return Err(format!(
                "Invalid types for bitwise operation: {} and {}",
                left, right
            ));
        }

        if left == right || matches!(right.kind, TypeKind::Int) {
            return Ok(left.clone());
        }

        if matches!(left.kind, TypeKind::Int) && self.are_compatible(right, left, context) {
            return Ok(right.clone());
        }

        Err(format!(
            "Type mismatch: {} and {} are not compatible for bitwise operation",
            left, right
        ))
    }

    /// Checks membership operation (`in`).
    fn check_membership_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let bool_type = || crate::ast::factory::make_type(TypeKind::Boolean);

        match &right.kind {
            // Canonical collection variants are normalized to Custom before this point.
            TypeKind::List(_) | TypeKind::Set(_) | TypeKind::Map(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name.as_str()),
                    Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Set)
                ) && !args.is_empty() =>
            {
                let inner = self.resolve_type_expression(&args[0], context);
                if self.are_compatible(&inner, left, context) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in collection of {}",
                        left, inner
                    ))
                }
            }
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name.as_str())
                    == Some(BuiltinCollectionKind::Map)
                    && !args.is_empty() =>
            {
                let key = self.resolve_type_expression(&args[0], context);
                if self.are_compatible(&key, left, context) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in map with keys of {}",
                        left, key
                    ))
                }
            }
            TypeKind::Custom(name, Some(args)) if name == "Range" && args.len() == 1 => {
                let range_type = self.resolve_type_expression(&args[0], context);
                if self.are_compatible(&range_type, left, context) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in range of {}",
                        left, range_type
                    ))
                }
            }
            TypeKind::String => {
                if matches!(left.kind, TypeKind::String) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in String (expected String)",
                        left
                    ))
                }
            }
            _ => Err(format!(
                "Invalid type for 'in' operator: expected collection, got {}",
                right
            )),
        }
    }

    /// Checks unary operation operand types.
    ///
    /// Returns the result type of the operation, or an error message if
    /// the operand is incompatible.
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
                        "Unary operator requires numeric type, got {}",
                        expr_type
                    ))
                }
            }
            UnaryOp::Not => {
                if matches!(expr_type.kind, TypeKind::Boolean) {
                    Ok(crate::ast::factory::make_type(TypeKind::Boolean))
                } else {
                    Err(format!("Logical NOT requires boolean, got {}", expr_type))
                }
            }
            UnaryOp::Await => {
                if let TypeKind::Future(inner_expr) = &expr_type.kind {
                    self.extract_type_from_expression(inner_expr)
                } else if let TypeKind::Custom(name, args) = &expr_type.kind {
                    if name == "Future" {
                        if let Some(args) = args {
                            if let Some(arg) = args.first() {
                                return self.extract_type_from_expression(arg);
                            }
                        }
                        return Ok(crate::ast::factory::make_type(TypeKind::Void));
                    }
                    Err(format!("Await requires a Future, got {}", expr_type))
                } else {
                    Err(format!("Await requires a Future, got {}", expr_type))
                }
            }
            UnaryOp::BitwiseNot => {
                if self.is_integer(expr_type) {
                    Ok(expr_type.clone())
                } else {
                    Err(format!(
                        "Bitwise NOT requires integer type, got {}",
                        expr_type
                    ))
                }
            }
        }
    }

    /// Checks null coalescing operation (`??`).
    /// LHS must be `Option<T>`, RHS must be compatible with `T`. Result type is `T`.
    fn check_null_coalesce_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        if let TypeKind::Option(inner) = &left.kind {
            let inner_ty = inner.as_ref().clone();
            if self.are_compatible(&inner_ty, right, context) {
                Ok(inner_ty)
            } else {
                Err(format!(
                    "Type mismatch in '??': Option contains {}, but default value is {}",
                    inner_ty, right
                ))
            }
        } else {
            Err(format!(
                "Left side of '??' must be an Option type, got {}",
                left
            ))
        }
    }

    /// Checks whether a type implements a given trait by looking up its class
    /// definition and inspecting the `traits` list.
    ///
    /// Maps `TypeKind::String` to class `"String"`, `TypeKind::Custom(name, _)` to `name`,
    /// and returns `false` for primitive types.
    fn type_implements_trait(&self, ty: &Type, trait_name: &str) -> bool {
        let class_name = match &ty.kind {
            TypeKind::String => STRING_TYPE_NAME,
            TypeKind::Custom(name, _) => name.as_str(),
            _ => return false,
        };

        if let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(class_name)
        {
            class_def.traits.iter().any(|t| t == trait_name)
        } else {
            false
        }
    }

    /// Rejects a type whose shape cannot be compared structurally.
    ///
    /// Structural equality is expanded inline at lowering, so a type that
    /// contains itself would expand without bound and take the compiler with
    /// it. A type that defines its own `equals` is always accepted: that
    /// method replaces the derived comparison, which is what makes the advice
    /// in these diagnostics work.
    pub(crate) fn check_type_structurally_comparable(&self, ty: &Type) -> Result<(), String> {
        let mut visiting = Vec::new();
        self.check_structural_comparability(&ty.kind, &mut visiting, 0)
    }

    /// Walk a type's payloads, carrying the enclosing type names to detect a
    /// cycle and the nesting depth to bound an acyclic chain. Both are passed
    /// down rather than held in shared state so sibling payloads cannot be
    /// mistaken for nesting.
    fn check_structural_comparability(
        &self,
        kind: &TypeKind,
        visiting: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), String> {
        if depth >= MAX_STRUCTURAL_EQUALITY_DEPTH {
            return Err(
                "Type nesting too deep for structural equality; implement `equals` method instead"
                    .to_string(),
            );
        }

        match kind {
            TypeKind::Option(inner) => {
                self.check_structural_comparability(&inner.kind, visiting, depth + 1)
            }
            TypeKind::Result(ok_expr, err_expr) => {
                for expr in [ok_expr, err_expr] {
                    if let crate::ast::expression::ExpressionKind::Type(ty, _) = &expr.node {
                        self.check_structural_comparability(&ty.kind, visiting, depth + 1)?;
                    }
                }
                Ok(())
            }
            TypeKind::Custom(name, args) => {
                self.check_named_type_comparability(name, args.as_deref(), visiting, depth)
            }
            _ => Ok(()),
        }
    }

    /// Check a named type's payloads or fields, unless it defines `equals`.
    fn check_named_type_comparability(
        &self,
        name: &str,
        args: Option<&[crate::ast::expression::Expression]>,
        visiting: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), String> {
        if self.type_defines_own_equality(name) {
            return Ok(());
        }
        if visiting.iter().any(|seen| seen == name) {
            return Err(format!(
                "Recursive type `{}` cannot use structural equality; implement `equals` method instead",
                name
            ));
        }

        let member_types: Vec<Type> = match self.type_table.global_type_definitions.get(name) {
            Some(TypeDefinition::Enum(enum_def)) => enum_def
                .variants
                .values()
                .flatten()
                .map(|payload_ty| {
                    Type::new(
                        crate::type_checker::generics::substitute_generic_field_kind(
                            &payload_ty.kind,
                            args,
                            enum_def.generics.as_ref(),
                        ),
                        payload_ty.span,
                    )
                })
                .collect(),
            Some(TypeDefinition::Struct(struct_def)) => struct_def
                .fields
                .iter()
                .map(|field| field.1.clone())
                .collect(),
            _ => return Ok(()),
        };

        visiting.push(name.to_string());
        let result = member_types.iter().try_for_each(|member_ty| {
            self.check_structural_comparability(&member_ty.kind, visiting, depth + 1)
        });
        visiting.pop();
        result
    }

    /// True when the named type supplies its own `equals`, which the operator
    /// lowering dispatches to in place of a derived structural comparison.
    fn type_defines_own_equality(&self, name: &str) -> bool {
        match self.type_table.global_type_definitions.get(name) {
            Some(TypeDefinition::Class(class_def)) => {
                class_def.methods.contains_key(EQUALS_METHOD_NAME)
            }
            Some(TypeDefinition::Enum(enum_def)) => {
                enum_def.methods.contains_key(EQUALS_METHOD_NAME)
            }
            _ => false,
        }
    }
}
