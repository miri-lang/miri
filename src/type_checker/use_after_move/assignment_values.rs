// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The value of an assignment that stores a resource.
//!
//! An assignment is an expression whose value is its target's: `let y = (x
//! = 3)` binds 3. When the target's type holds a resource, the store keeps
//! it, and the value the assignment gives back would be a second holder of
//! the same block, whose hook would run when that holder lets go rather than
//! when the target does. So reading the value of such an assignment — as an
//! initializer, an operand, an argument, a returned value or the right side
//! of a chained assignment — is refused, with the repair: write the
//! assignment as a statement of its own, then read the target. The value of
//! an assignment that stores a plain value is read as before. In a generic
//! body the refusal waits for the copies that bind the stored type to a
//! resource.

use crate::ast::expression::Expression;
use crate::diagnostics::DiagnosticCode;
use crate::error::type_error::{TypeError, TypeErrorKind};
use crate::type_checker::open_consumption::OpenRefusal;
use crate::type_checker::place_reads::value_spelling;

use super::UseAfterMoveChecker;

impl UseAfterMoveChecker<'_> {
    /// Refuses reading the value of `assignment`, which writes `target`,
    /// when its type, the target's, holds a resource: the target keeps what
    /// the assignment stores, and the value would be a second holder of it.
    /// In a generic body, where the type is an open parameter, it is refused
    /// at each copy reading that parameter as a resource.
    pub(super) fn refuse_reading_a_resource_store(
        &mut self,
        assignment: &Expression,
        target: &Expression,
    ) {
        let error = read_resource_store_error(target);
        if self.is_resource_expr(assignment) {
            self.errors.push(error);
            return;
        }
        let open = (self.open_parameter(assignment)).zip(self.open_body.clone());
        if let Some((parameter, body)) = open {
            let refusal = OpenRefusal {
                error,
                body,
                parameter,
            };
            self.open_refusals.push(refusal);
        }
    }
}

/// The refusal of reading the value of an assignment to `target`.
fn read_resource_store_error(target: &Expression) -> TypeError {
    let place = value_spelling(target);
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnResourceStoreValueRead,
            message: format!(
                "the value of the assignment to `{place}` is read, and `{place}` keeps the resource it stores: a resource has one holder, so an assignment that stores one gives back no value"
            ),
            help: Some(format!(
                "write the assignment to `{place}` as a statement of its own, then read `{place}`"
            )),
            expected: None,
            actual: None,
        },
        span: target.span,
        source_override: None,
        repair: None,
        related: Vec::new(),
    }
}
