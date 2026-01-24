// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory::func;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::Program;
use crate::cli::args::CpuBackend;
use crate::codegen::Backend;
use crate::error::compiler::CompilerError;
use crate::lexer::Lexer;
use crate::mir;
use crate::parser::Parser;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::type_checker::TypeChecker;

fn has_functions(program: &Program) -> bool {
    program
        .body
        .iter()
        .any(|s| matches!(&s.node, StatementKind::FunctionDeclaration(..)))
}

fn is_wrappable_stmt(stmt: &Statement) -> bool {
    matches!(
        &stmt.node,
        StatementKind::Expression(_)
            | StatementKind::Variable(..)
            | StatementKind::If(..)
            | StatementKind::While(..)
            | StatementKind::For(..)
            | StatementKind::Block(..)
            | StatementKind::Return(..)
            | StatementKind::Break
            | StatementKind::Continue
    )
}

/// Wraps a script-style program (no function declarations) in a synthetic `main` function.
/// Skips programs that already contain functions or non-wrappable type definitions.
fn wrap_script_in_main(program: &mut Program) {
    if has_functions(program) {
        return;
    }

    if program.body.is_empty() {
        let body = crate::ast::factory::block(vec![]);
        let main_fn = func("main").build(body);
        program.body = vec![main_fn];
        return;
    }

    let all_wrappable = program.body.iter().all(is_wrappable_stmt);
    if !all_wrappable {
        return;
    }

    let body_stmts = program.body.clone();
    let body = crate::ast::factory::block(body_stmts);
    let main_fn = func("main").build(body);
    program.body = vec![main_fn];
}

/// The result of running the frontend pipeline (parsing + type checking).
#[derive(Debug)]
pub struct PipelineResult {
    /// The parsed abstract syntax tree.
    pub ast: Program,
    /// The type checker state after analysis (contains inferred types and warnings).
    pub type_checker: TypeChecker,
}

/// Options controlling the build process.
#[derive(Debug, Default)]
pub struct BuildOptions {
    /// Output path for the compiled artifact. If `None`, a temp directory is used.
    pub out_path: Option<PathBuf>,
    /// Whether to build in release mode (enables optimizations).
    pub release: bool,
    /// Optimization level (0-3).
    pub opt_level: u8,
    /// Which CPU backend to use for code generation.
    pub cpu_backend: CpuBackend,
}

/// Orchestrates the full compilation pipeline from source to executable.
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

    /// Run the frontend (lexer, parser, type checker) on source code.
    pub fn frontend(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let ast = parser.parse().map_err(CompilerError::Parser)?;

        let mut type_checker = crate::type_checker::TypeChecker::new();
        type_checker
            .check(&ast)
            .map_err(CompilerError::TypeErrors)?;

        Ok(PipelineResult { ast, type_checker })
    }

    /// Run the frontend with script-mode wrapping: simple programs without function
    /// declarations are wrapped in a synthetic `main`.
    pub fn frontend_script(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let mut ast = parser.parse().map_err(CompilerError::Parser)?;

        wrap_script_in_main(&mut ast);

        let mut type_checker = crate::type_checker::TypeChecker::new();
        type_checker
            .check(&ast)
            .map_err(CompilerError::TypeErrors)?;

        for warning in &type_checker.warnings {
            eprintln!(
                "{}",
                crate::error::format::format_diagnostic_full(source, warning)
            );
        }

        Ok(PipelineResult { ast, type_checker })
    }

    /// Interpret the source code directly without compilation.
    pub fn interpret(&self, source: &str) -> Result<crate::interpreter::Value, CompilerError> {
        let pipeline_result = self.frontend_script(source)?;
        let mir_bodies = self.lower_to_mir(&pipeline_result, false)?;

        let mut interpreter = crate::interpreter::Interpreter::new();
        interpreter.load_functions(mir_bodies);

        interpreter
            .run_main()
            .map_err(|e| CompilerError::Runtime(e.to_string()))
    }

    /// Compile and execute the source, returning the process exit code.
    pub fn run(&self, source: &str) -> Result<i32, CompilerError> {
        let temp_dir = std::env::temp_dir().join(format!("miri_run_{}", std::process::id()));
        fs::create_dir_all(&temp_dir)?;

        let executable_path = temp_dir.join("program");

        let build_opts = BuildOptions {
            out_path: Some(executable_path.clone()),
            release: false,
            opt_level: 0,
            cpu_backend: CpuBackend::Cranelift,
        };

        self.build(source, &build_opts)?;

        let output = Command::new(&executable_path)
            .output()
            .map_err(|e| CompilerError::Codegen(format!("Failed to execute program: {}", e)))?;

        if !output.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(output.status.code().unwrap_or(-1))
    }

    /// Compile source code to a native executable, returning the artifact path.
    pub fn build(&self, source: &str, opts: &BuildOptions) -> Result<PathBuf, CompilerError> {
        let pipeline_result = self.frontend_script(source)?;
        let mir_bodies = self.lower_to_mir(&pipeline_result, opts.release)?;

        let object_bytes = match opts.cpu_backend {
            CpuBackend::Cranelift => {
                #[cfg(feature = "cranelift")]
                {
                    use crate::codegen::CraneliftBackend;
                    let backend = CraneliftBackend::new()
                        .map_err(|e| CompilerError::Codegen(e.to_string()))?;

                    let bodies_ref: Vec<(&str, &mir::Body)> = mir_bodies
                        .iter()
                        .map(|(name, body)| (name.as_str(), body))
                        .collect();

                    let artifact = backend
                        .compile(&bodies_ref, &Default::default())
                        .map_err(|e| CompilerError::Codegen(e.to_string()))?;

                    artifact.bytes
                }
                #[cfg(not(feature = "cranelift"))]
                {
                    return Err(CompilerError::Codegen(
                        "Cranelift backend not enabled. Recompile with --features cranelift"
                            .to_string(),
                    ));
                }
            }
            CpuBackend::Llvm => {
                use crate::codegen::LlvmBackend;
                let backend = LlvmBackend;

                let bodies_ref: Vec<(&str, &mir::Body)> = mir_bodies
                    .iter()
                    .map(|(name, body)| (name.as_str(), body))
                    .collect();

                let artifact = backend
                    .compile(&bodies_ref, &Default::default())
                    .map_err(|e| CompilerError::Codegen(e.to_string()))?;

                artifact.bytes
            }
        };

        let (work_dir, out_path) = if let Some(out) = opts.out_path.clone() {
            let work_dir = out
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            (work_dir, out)
        } else {
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let unique_dir = std::env::temp_dir().join("miri_build").join(format!(
                "{}_{}",
                timestamp,
                std::process::id()
            ));
            let out = unique_dir.join("a.out");
            (unique_dir, out)
        };

        fs::create_dir_all(&work_dir)?;

        let object_path = work_dir.join("output.o");
        fs::write(&object_path, &object_bytes)?;
        self.link_executable(&object_path, &out_path)?;

        Ok(out_path)
    }

    fn lower_to_mir(
        &self,
        result: &PipelineResult,
        is_release: bool,
    ) -> Result<Vec<(String, mir::Body)>, CompilerError> {
        let mut bodies = Vec::new();

        for stmt in &result.ast.body {
            if let StatementKind::FunctionDeclaration(name, _, _, _, _, _) = &stmt.node {
                let body =
                    mir::lowering::lower_function(stmt, &result.type_checker, is_release, true)
                        .map_err(|e| {
                            CompilerError::Codegen(format!("MIR lowering failed: {}", e))
                        })?;
                bodies.push((name.clone(), body));
            }
        }

        if bodies.is_empty() {
            return Err(CompilerError::Codegen(
                "No functions found to compile".to_string(),
            ));
        }

        Ok(bodies)
    }

    /// Get MIR as a string for debugging purposes.
    pub fn get_mir(&self, source: &str) -> Result<String, CompilerError> {
        let pipeline_result = self.frontend_script(source)?;
        let mir_bodies = self.lower_to_mir(&pipeline_result, false)?;

        let mut output = String::new();
        for (name, body) in &mir_bodies {
            output.push_str(&format!("=== MIR for {} ===\n{}\n\n", name, body));
        }
        Ok(output)
    }

    /// Link an object file to an executable using the system linker.
    fn link_executable(
        &self,
        object_path: &PathBuf,
        output_path: &PathBuf,
    ) -> Result<(), CompilerError> {
        // Try to use cc (the system C compiler) for linking
        let status = Command::new("cc")
            .arg(object_path)
            .arg("-o")
            .arg(output_path)
            .status()
            .map_err(|e| CompilerError::Codegen(format!("Failed to run linker: {}", e)))?;

        if !status.success() {
            return Err(CompilerError::Codegen(format!(
                "Linker failed with exit code: {:?}",
                status.code()
            )));
        }

        Ok(())
    }
}
