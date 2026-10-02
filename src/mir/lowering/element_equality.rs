// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bodies a set or map matches struct elements and keys through, and the
//! hashes it places them by.
//!
//! An equality is `(a S, b S) -> bool`, lowered from the structural walk
//! `a == b` lowers to, so a container agrees with the comparison the program
//! would write by hand. A hash is `(a S) -> int`, lowered from the walk
//! `a.hash()` lowers to, so two elements the equality calls equal hash alike.
//! Every parameter is borrowed from the container, which keeps owning its
//! elements.

use super::context::LoweringContext;
use super::expression::structural_equality::emit_structural_equality;
use super::expression::structural_hash::emit_structural_hash;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::body::{Body, ExecutionModel, LocalDecl};
use crate::mir::{Local, Operand, Place, Rvalue, StatementKind};
use crate::type_checker::TypeChecker;

/// Lower the equality a container of `element` values matches them through.
pub fn lower_element_equality(
    type_checker: &TypeChecker,
    element: &Type,
    span: Span,
) -> Result<Body, LoweringError> {
    let body = Body::new(2, span, ExecutionModel::Cpu);
    let mut ctx = LoweringContext::new(body, type_checker, false);
    ctx.body
        .new_local(LocalDecl::new(Type::new(TypeKind::Boolean, span), span));
    let left = ctx.push_param("a".to_string(), element.clone(), span);
    let right = ctx.push_param("b".to_string(), element.clone(), span);
    // The runtime calls this with the two elements alone, so a method the
    // comparison calls (an enum's own `equals`) is handed no allocator.
    super::bind_null_allocator(&mut ctx, span);
    let verdict = emit_structural_equality(
        &mut ctx,
        span,
        &element.kind,
        Operand::Copy(Place::new(left)),
        Operand::Copy(Place::new(right)),
        true,
    )?;
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(Local(0)),
            Rvalue::Use(Operand::Copy(Place::new(verdict))),
        ),
        span,
    });
    let (body, _lambdas) = super::finalize_body(&mut ctx, span)?;
    Ok(body)
}

/// Lower the hash a container of `element` values places them by.
pub fn lower_element_hash(
    type_checker: &TypeChecker,
    element: &Type,
    span: Span,
) -> Result<Body, LoweringError> {
    let body = Body::new(1, span, ExecutionModel::Cpu);
    let mut ctx = LoweringContext::new(body, type_checker, false);
    ctx.body
        .new_local(LocalDecl::new(Type::new(TypeKind::Int, span), span));
    let value = ctx.push_param("a".to_string(), element.clone(), span);
    // The runtime calls this with the element alone, so a `hash` method it
    // calls is handed no allocator.
    super::bind_null_allocator(&mut ctx, span);
    let hash = emit_structural_hash(
        &mut ctx,
        span,
        &element.kind,
        Operand::Copy(Place::new(value)),
    )?;
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(Local(0)),
            Rvalue::Use(Operand::Copy(Place::new(hash))),
        ),
        span,
    });
    let (body, _lambdas) = super::finalize_body(&mut ctx, span)?;
    Ok(body)
}
