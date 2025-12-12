// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;

pub struct TypeChecker {
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {}
    }

    pub fn check(&self, _exp: &Expression) -> Result<(), String> {
        // Type checking logic would go here
        Ok(())
    }
}