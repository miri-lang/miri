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

use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_integer_list_literal(
        &self,
        elements: &[Expression],
        target_type: &Type,
    ) -> bool {
        let target_size = match self.get_integer_size(target_type) {
            Some(s) => s,
            None => return false,
        };

        for element in elements {
            if let ExpressionKind::Literal(Literal::Integer(int_val)) = &element.node {
                if !self.integer_fits(int_val, target_size, target_type) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    pub(crate) fn integer_fits(&self, val: &IntegerLiteral, size: u8, target_type: &Type) -> bool {
        let is_target_unsigned = matches!(
            target_type.kind,
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
        );

        match val {
            IntegerLiteral::U128(v) => {
                if is_target_unsigned {
                    let max = match size {
                        8 => u8::MAX as u128,
                        16 => u16::MAX as u128,
                        32 => u32::MAX as u128,
                        64 => u64::MAX as u128,
                        128 => u128::MAX,
                        _ => return false,
                    };
                    *v <= max
                } else {
                    let max = match size {
                        8 => i8::MAX as u128,
                        16 => i16::MAX as u128,
                        32 => i32::MAX as u128,
                        64 => i64::MAX as u128,
                        128 => i128::MAX as u128,
                        _ => return false,
                    };
                    *v <= max
                }
            }
            _ => {
                let val_i128 = match val {
                    IntegerLiteral::I8(v) => *v as i128,
                    IntegerLiteral::I16(v) => *v as i128,
                    IntegerLiteral::I32(v) => *v as i128,
                    IntegerLiteral::I64(v) => *v as i128,
                    IntegerLiteral::I128(v) => *v,
                    IntegerLiteral::U8(v) => *v as i128,
                    IntegerLiteral::U16(v) => *v as i128,
                    IntegerLiteral::U32(v) => *v as i128,
                    IntegerLiteral::U64(v) => *v as i128,
                    _ => unreachable!(),
                };

                if is_target_unsigned {
                    if val_i128 < 0 {
                        return false;
                    }
                    let max = match size {
                        8 => u8::MAX as i128,
                        16 => u16::MAX as i128,
                        32 => u32::MAX as i128,
                        64 => u64::MAX as i128,
                        128 => i128::MAX,
                        _ => return false,
                    };
                    if size == 128 {
                        return true;
                    }
                    val_i128 <= max
                } else {
                    let (min, max) = match size {
                        8 => (i8::MIN as i128, i8::MAX as i128),
                        16 => (i16::MIN as i128, i16::MAX as i128),
                        32 => (i32::MIN as i128, i32::MAX as i128),
                        64 => (i64::MIN as i128, i64::MAX as i128),
                        128 => (i128::MIN, i128::MAX),
                        _ => return false,
                    };
                    val_i128 >= min && val_i128 <= max
                }
            }
        }
    }

    /// Checks if a statement (typically a method body) contains a call to super.init()
    pub(crate) fn contains_super_init_call(&self, stmt: &Statement) -> bool {
        match &stmt.node {
            StatementKind::Block(stmts) => stmts.iter().any(|s| self.contains_super_init_call(s)),
            StatementKind::Expression(expr) => self.expression_contains_super_init(expr),
            StatementKind::Return(opt_expr) => opt_expr
                .as_ref()
                .is_some_and(|e| self.expression_contains_super_init(e)),
            StatementKind::If(cond, then_branch, else_branch, _) => {
                self.expression_contains_super_init(cond)
                    || self.contains_super_init_call(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.contains_super_init_call(e))
            }
            StatementKind::While(cond, body, _) => {
                self.expression_contains_super_init(cond) || self.contains_super_init_call(body)
            }
            StatementKind::For(_, iter, body) => {
                self.expression_contains_super_init(iter) || self.contains_super_init_call(body)
            }
            StatementKind::Variable(decls, _) => decls.iter().any(|d| {
                d.initializer
                    .as_ref()
                    .is_some_and(|e| self.expression_contains_super_init(e))
            }),
            _ => false,
        }
    }

    /// Checks if an expression contains a call to super.init()
    #[allow(clippy::only_used_in_recursion)]
    pub(crate) fn expression_contains_super_init(&self, expr: &Expression) -> bool {
        match &expr.node {
            ExpressionKind::Call(callee, _args) => {
                // Check if callee is super.init
                if let ExpressionKind::Member(obj, prop) = &callee.node {
                    if matches!(obj.node, ExpressionKind::Super) {
                        if let ExpressionKind::Identifier(name, _) = &prop.node {
                            if name == "init" {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            // Binary is (left, op, right)
            ExpressionKind::Binary(left, _op, right)
            | ExpressionKind::Logical(left, _op, right) => {
                self.expression_contains_super_init(left)
                    || self.expression_contains_super_init(right)
            }
            ExpressionKind::Unary(_, operand) => self.expression_contains_super_init(operand),
            ExpressionKind::Member(obj, _) => self.expression_contains_super_init(obj),
            _ => false,
        }
    }
}
