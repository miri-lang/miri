// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Operator type checking for the type checker.
//!
//! This module handles type validation for binary and unary operators,
//! ensuring operands have compatible types for the requested operations.

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::types::{Type, TypeKind};
use crate::ast::BinaryOp;
use crate::ast::UnaryOp;

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
            _ => Ok(crate::ast::factory::make_type(TypeKind::Boolean)),
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
        let left_is_float = matches!(left.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64);
        let right_is_int = self.is_integer(right);
        let right_is_float = matches!(right.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64);

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
            TypeKind::List(inner_expr) | TypeKind::Set(inner_expr) => {
                let inner = self.resolve_type_expression(inner_expr, context);
                if self.are_compatible(&inner, left, context) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in collection of {}",
                        left, inner
                    ))
                }
            }
            TypeKind::Map(key_expr, _) => {
                let key = self.resolve_type_expression(key_expr, context);
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
            _ => Ok(expr_type.clone()),
        }
    }

    /// Checks whether a type implements a given trait by looking up its class
    /// definition and inspecting the `traits` list.
    ///
    /// Maps `TypeKind::String` to class `"String"`, `TypeKind::Custom(name, _)` to `name`,
    /// and returns `false` for primitive types.
    fn type_implements_trait(&self, ty: &Type, trait_name: &str) -> bool {
        let class_name = match &ty.kind {
            TypeKind::String => "String",
            TypeKind::Custom(name, _) => name.as_str(),
            _ => return false,
        };

        if let Some(TypeDefinition::Class(class_def)) = self.global_type_definitions.get(class_name)
        {
            class_def.traits.iter().any(|t| t == trait_name)
        } else {
            false
        }
    }
}
