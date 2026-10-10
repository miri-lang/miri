// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lambda/closure support for MIR.
//!
//! Lambdas are lowered to separate MIR bodies and referenced by name.
//! Captured variables are tracked for closure support.

use crate::mir::body::Body;
use crate::mir::place::Local;
use crate::mir::symbol::Symbol;
use std::rc::Rc;

/// Represents a lowered lambda function.
#[derive(Debug, Clone)]
pub struct LambdaInfo {
    /// What this body is emitted as; its link name is unique per compilation.
    pub symbol: Symbol,
    /// The MIR body for this lambda
    pub body: Body,
    /// Variables captured from the enclosing scope.
    /// Maps the original variable name to its Local in the enclosing function.
    pub captures: Vec<CapturedVar>,
}

/// A variable captured by a lambda/closure.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedVar {
    pub name: Rc<str>,
    pub lambda_local: Local,
    pub outer_local: Local,
}
