// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::place::Local;
use crate::mir::{BasicBlock, BasicBlockData, Body, LocalDecl, Terminator};
use std::collections::HashMap;

pub struct LoweringContext<'a> {
    pub body: Body,
    pub variable_map: HashMap<String, Local>, // Map variable names to locals
    pub current_block: BasicBlock,
    pub type_checker: &'a crate::type_checker::TypeChecker,
}

impl<'a> LoweringContext<'a> {
    pub fn new(body: Body, type_checker: &'a crate::type_checker::TypeChecker) -> Self {
        let mut ctx = Self {
            body,
            variable_map: HashMap::new(),
            current_block: BasicBlock(0),
            type_checker,
        };
        // Create the first basic block
        ctx.body.basic_blocks.push(BasicBlockData::new(None));
        ctx
    }

    pub fn push_local(&mut self, name: String, ty: Type, span: Span) -> Local {
        let mut decl = LocalDecl::new(ty, span);
        decl.name = Some(name.clone());
        let local = self.body.new_local(decl);
        self.variable_map.insert(name, local);
        local
    }

    pub fn push_temp(&mut self, ty: Type, span: Span) -> Local {
        let decl = LocalDecl::new(ty, span);
        self.body.new_local(decl)
    }

    pub fn push_statement(&mut self, statement: crate::mir::Statement) {
        let block = &mut self.body.basic_blocks[self.current_block.0];
        block.statements.push(statement);
    }

    pub fn set_terminator(&mut self, terminator: Terminator) {
        let block = &mut self.body.basic_blocks[self.current_block.0];
        block.terminator = Some(terminator);
    }
}
