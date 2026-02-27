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
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

#[derive(Debug, PartialEq)]
pub(crate) enum ReturnStatus {
    None,
    Implicit,
    Explicit,
}

pub(crate) fn check_returns(stmt: &Statement) -> ReturnStatus {
    match &stmt.node {
        StatementKind::Return(_) => ReturnStatus::Explicit,
        StatementKind::While(_, _, WhileStatementType::Forever) => ReturnStatus::Explicit,
        StatementKind::While(_, _, WhileStatementType::While)
        | StatementKind::While(_, _, WhileStatementType::Until)
        | StatementKind::While(_, _, WhileStatementType::DoWhile)
        | StatementKind::While(_, _, WhileStatementType::DoUntil) => ReturnStatus::None,
        StatementKind::Expression(_) => ReturnStatus::Implicit,
        StatementKind::Block(stmts) => {
            for (i, s) in stmts.iter().enumerate() {
                let status = check_returns(s);
                if status == ReturnStatus::Explicit {
                    return ReturnStatus::Explicit;
                }
                if i == stmts.len() - 1 && status == ReturnStatus::Implicit {
                    return ReturnStatus::Implicit;
                }
            }
            ReturnStatus::None
        }
        StatementKind::If(_, then_block, else_block, _) => {
            if let Some(else_stmt) = else_block {
                let then_status = check_returns(then_block);
                let else_status = check_returns(else_stmt);

                match (then_status, else_status) {
                    (ReturnStatus::Explicit, ReturnStatus::Explicit) => ReturnStatus::Explicit,
                    (ReturnStatus::None, _) | (_, ReturnStatus::None) => ReturnStatus::None,
                    _ => ReturnStatus::Implicit,
                }
            } else {
                ReturnStatus::None
            }
        }
        // All other statement kinds (Variable, For, Break, Continue, FunctionDeclaration,
        // Struct, Enum, Class, Trait, Type, RuntimeFunctionDeclaration, Use, Empty)
        // do not contribute to return status analysis.
        StatementKind::Variable(_, _)
        | StatementKind::For(_, _, _)
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Struct(_, _, _, _)
        | StatementKind::Enum(_, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::Type(_, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::Use(_, _)
        | StatementKind::Empty => ReturnStatus::None,
    }
}

impl TypeChecker {
    pub(crate) fn resolve_implicit_return_type(&self, stmt: &Statement) -> Option<Type> {
        match &stmt.node {
            StatementKind::Expression(expr) => self.get_type(expr.id).cloned(),
            StatementKind::Block(stmts) => {
                if let Some(last) = stmts.last() {
                    self.resolve_implicit_return_type(last)
                } else {
                    None
                }
            }
            StatementKind::If(_, then_block, else_block, _) => {
                let t1 = self.resolve_implicit_return_type(then_block);
                let t2 = if let Some(else_stmt) = else_block {
                    self.resolve_implicit_return_type(else_stmt)
                } else {
                    None
                };

                match (t1, t2) {
                    (Some(a), Some(_)) => Some(a), // Assume compatible
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn register_implicit_main_return(
        &mut self,
        name: &str,
        expr_type: Type,
        context: &mut Context,
    ) {
        if context.return_types.pop().is_some() {
            let last = expr_type.clone();
            context.return_types.push(last);
        }

        // Update global symbol as well
        if let Some(info) = self.global_scope.get_mut(name) {
            if let TypeKind::Function(func_data) = &info.ty.kind {
                let type_expr = crate::ast::factory::type_expr_non_null(expr_type.clone());
                self.types.insert(type_expr.id, expr_type.clone());

                info.ty = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: func_data.generics.clone(),
                    params: func_data.params.clone(),
                    return_type: Some(Box::new(type_expr)),
                })));
            }
        }
    }
}
