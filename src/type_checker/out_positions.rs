// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which arguments of each call bind an `out` parameter.
//!
//! A place handed to an `out` parameter is written by the call whatever its
//! callee does, so a parallel pass that hands one a buffer element writes
//! that buffer: the element's index must be one per thread and the buffer is
//! bound for writing. The type checker records each such argument as it
//! checks the call against the callee's declared parameters
//! (`Parameter::is_out`, the declaration MIR lowering reads the same answer
//! off), and both the checker's write checks and kernel lowering read the
//! record through [`OutArgumentQuery`]. A named argument is recorded as the
//! value it names.
//!
//! The record fails closed: a call it has not seen checked answers every
//! argument as bound to an `out` parameter, so a reader that runs too early
//! sees a write too many, never one too few.

use std::collections::HashSet;

use crate::ast::expression::Expression;
use crate::ast::gpu_writes::OutArgumentQuery;

/// The arguments bound to an `out` parameter, each keyed by the id of the
/// call it is written in and its own expression id, and the calls whose
/// arguments are all recorded.
#[derive(Debug, Default)]
pub struct OutArguments {
    bound: HashSet<(usize, usize)>,
    checked: HashSet<usize>,
}

impl OutArguments {
    /// Records that the argument `argument_id`, written in the call
    /// `call_id`, binds an `out` parameter.
    pub fn record(&mut self, call_id: usize, argument_id: usize) {
        self.bound.insert((call_id, argument_id));
    }

    /// Records that the call `call_id` is checked: each of its arguments
    /// bound to an `out` parameter is recorded.
    pub fn mark_checked(&mut self, call_id: usize) {
        self.checked.insert(call_id);
    }

    /// Whether the argument `argument_id`, written in the call `call_id`,
    /// is recorded as binding an `out` parameter; `None` while the call is
    /// not checked, which the record answers no argument of. A reader that
    /// must agree with the record, rather than fail closed, asks this.
    pub fn recorded_binding(&self, call_id: usize, argument_id: usize) -> Option<bool> {
        (self.checked.contains(&call_id)).then(|| self.bound.contains(&(call_id, argument_id)))
    }
}

impl OutArgumentQuery for OutArguments {
    fn binds_out(&self, call: &Expression, argument: &Expression) -> bool {
        !self.checked.contains(&call.id) || self.bound.contains(&(call.id, argument.id))
    }
}

/// The answer for a walk that runs before any call in what it walks is
/// checked and judges only the writes the syntax spells: a store or an
/// atomic builtin. No argument binds an `out` parameter.
pub struct SpelledWritesOnly;

impl OutArgumentQuery for SpelledWritesOnly {
    fn binds_out(&self, _call: &Expression, _argument: &Expression) -> bool {
        false
    }
}
