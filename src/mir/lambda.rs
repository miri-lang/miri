// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lambda/closure support for MIR.
//!
//! Lambdas are lowered to separate MIR bodies and referenced by name.
//! Captured variables are tracked for closure support.

use crate::mir::body::Body;
use crate::mir::place::Local;
use std::collections::HashMap;
use std::rc::Rc;

/// Represents a lowered lambda function.
#[derive(Debug, Clone)]
pub struct LambdaInfo {
    /// The unique name of this lambda (e.g., `__lambda_42`)
    pub name: String,
    /// The MIR body for this lambda
    pub body: Body,
    /// Variables captured from the enclosing scope.
    /// Maps the original variable name to its Local in the enclosing function.
    pub captures: Vec<CapturedVar>,
}

/// A variable captured by a lambda/closure.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedVar {
    pub name: Rc<String>,
    pub lambda_local: Local,
    pub outer_local: Local,
}

/// Registry for lambda bodies collected during lowering.
#[derive(Debug, Default)]
pub struct LambdaRegistry {
    /// Map from lambda name to its info
    pub lambdas: HashMap<String, LambdaInfo>,
}

impl LambdaRegistry {
    pub fn new() -> Self {
        Self {
            lambdas: HashMap::new(),
        }
    }

    pub fn register(&mut self, info: LambdaInfo) {
        self.lambdas.insert(info.name.clone(), info);
    }

    pub fn get(&self, name: &str) -> Option<&LambdaInfo> {
        self.lambdas.get(name)
    }
}
