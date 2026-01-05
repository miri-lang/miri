// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::block::BasicBlockData;
use crate::mir::place::Local;
use std::fmt;

/// The body of a function in MIR.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Basic blocks in the CFG.
    pub basic_blocks: Vec<BasicBlockData>,
    /// Declarations of local variables.
    pub local_decls: Vec<LocalDecl>,
    /// The number of arguments the function takes.
    pub arg_count: usize,
    /// The span of the entire function body.
    /// The span of the entire function body.
    pub span: Span,
    /// Whether this function is a GPU kernel/function.
    pub is_gpu: bool,
}

impl Body {
    pub fn new(arg_count: usize, span: Span, is_gpu: bool) -> Self {
        Self {
            basic_blocks: Vec::new(),
            local_decls: Vec::new(),
            arg_count,
            span,
            is_gpu,
        }
    }

    pub fn new_local(&mut self, decl: LocalDecl) -> Local {
        let local = Local(self.local_decls.len());
        self.local_decls.push(decl);
        local
    }
}

/// Declaration of a local variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalDecl {
    pub ty: Type,
    pub span: Span,
    pub name: Option<String>,
    pub is_user_variable: bool,
}

impl LocalDecl {
    pub fn new(ty: Type, span: Span) -> Self {
        Self {
            ty,
            span,
            name: None,
            is_user_variable: false,
        }
    }
}

impl fmt::Display for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, decl) in self.local_decls.iter().enumerate() {
            write!(f, "    let _{}: {};", i, decl.ty)?;
            if let Some(name) = &decl.name {
                write!(f, " // {}", name)?;
            }
            writeln!(f)?;
        }
        writeln!(f)?;

        for (i, block) in self.basic_blocks.iter().enumerate() {
            writeln!(f, "    bb{}: {{", i)?;
            for stmt in &block.statements {
                writeln!(f, "        {};", stmt)?;
            }
            if let Some(terminator) = &block.terminator {
                writeln!(f, "        {};", terminator)?;
            }
            writeln!(f, "    }}")?;
            writeln!(f)?;
        }
        Ok(())
    }
}
