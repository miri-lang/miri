// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::Program;
use crate::codegen::CodeGen;
use crate::error::compiler::CompilerError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use inkwell::context::Context;
use std::fs;
use std::path::PathBuf;

use crate::type_checker::TypeChecker;

#[derive(Debug)]
pub struct PipelineResult {
    pub ast: Program,
    pub type_checker: TypeChecker,
}

#[derive(Debug, Default)]
pub struct BuildOptions {
    pub out_path: Option<PathBuf>,
    pub release: bool,
    pub opt_level: u8,
}

pub struct Pipeline {}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn frontend(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let ast = parser.parse().map_err(CompilerError::Parser)?;

        let mut type_checker = crate::type_checker::TypeChecker::new();
        type_checker
            .check(&ast)
            .map_err(CompilerError::TypeErrors)?;

        // TODO: Hook for Lowering/IR Generation
        // let ir = lowerer.lower(typed_ast)?;

        Ok(PipelineResult { ast, type_checker })
    }

    pub fn run(&self, source: &str) -> Result<i32, CompilerError> {
        let pipeline_result = self.frontend(source)?;

        // For now, just print a minimal summary.
        // In the future, this will execute the code.
        println!(
            "AST generated with {} statements.",
            pipeline_result.ast.body.len()
        );

        // TODO: Hook for Code Generation and Execution
        // codegen.execute(ir)?;

        Ok(0)
    }

    pub fn build(&self, source: &str, opts: &BuildOptions) -> Result<PathBuf, CompilerError> {
        let pipeline_result = self.frontend(source)?;

        let target_dir = if opts.release { "release" } else { "debug" };
        let default_out_dir = PathBuf::from("target").join(target_dir);
        let out_path = opts
            .out_path
            .clone()
            .unwrap_or_else(|| default_out_dir.join("a.miribin"));

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let artifact_content = format!(
            "// Miri Build Artifact\n// Optimization Level: {}\n// Release: {}\n\nAST Summary: {} statements\n",
            opts.opt_level,
            opts.release,
            pipeline_result.ast.body.len()
        );

        fs::write(&out_path, artifact_content)?;

        let context = Context::create();
        let codegen = CodeGen::new(&context, "main", &pipeline_result.type_checker);
        codegen
            .generate(&pipeline_result.ast)
            .map_err(CompilerError::Codegen)?;

        // Verify the module
        if let Err(e) = codegen.module.verify() {
            return Err(CompilerError::Codegen(e.to_string()));
        }

        // For now, just print the IR to stderr to verify it works
        codegen.module.print_to_stderr();

        Ok(out_path)
    }
}
