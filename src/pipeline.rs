// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

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

/// Check if the program has any function declarations
fn has_functions(program: &Program) -> bool {
    program
        .body
        .iter()
        .any(|s| matches!(&s.node, StatementKind::FunctionDeclaration(..)))
}

/// Check if a statement can be wrapped in a function (simple script content)
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

/// Wrap a script (without functions) in a synthetic main function.
/// The last expression becomes the return value.
/// Only wraps simple scripts - not type definitions.
/// Also wraps empty programs to create a valid entry point.
fn wrap_script_in_main(program: &mut Program) {
    // Don't wrap if already has functions
    if has_functions(program) {
        return;
    }

    // Handle empty program - create empty main
    if program.body.is_empty() {
        let body = crate::ast::factory::block(vec![]);
        let main_fn = func("main").build(body);
        program.body = vec![main_fn];
        return;
    }

    // Don't wrap if any statement is a type definition
    let all_wrappable = program.body.iter().all(is_wrappable_stmt);
    if !all_wrappable {
        return;
    }

    // Use statements as-is for the function body
    // Don't add explicit return - let MIR lowering handle implicit returns
    let body_stmts = program.body.clone();

    // Create function body as a block
    let body = crate::ast::factory::block(body_stmts);

    // Create: fn main() { ... } - no explicit return type, let type checker infer
    let main_fn = func("main").build(body);

    // Replace program body with just the main function
    program.body = vec![main_fn];
}

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
    pub cpu_backend: CpuBackend,
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

        Ok(PipelineResult { ast, type_checker })
    }

    /// Frontend for scripts: wraps simple programs without functions in a main.
    pub fn frontend_script(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let mut ast = parser.parse().map_err(CompilerError::Parser)?;

        // Wrap scripts without functions in a synthetic main
        wrap_script_in_main(&mut ast);

        let mut type_checker = crate::type_checker::TypeChecker::new();
        type_checker
            .check(&ast)
            .map_err(CompilerError::TypeErrors)?;

        // Print any warnings
        for warning in &type_checker.warnings {
            eprintln!(
                "{}",
                crate::error::format_diagnostic(
                    source,
                    &warning.span,
                    &warning.message,
                    "warning",
                    warning.help.as_deref()
                )
            );
        }

        Ok(PipelineResult { ast, type_checker })
    }

    /// Interpret the source code directly without compilation.
    pub fn interpret(&self, source: &str) -> Result<crate::interpreter::Value, CompilerError> {
        let pipeline_result = self.frontend_script(source)?;
        let mir_bodies = self.lower_to_mir(&pipeline_result)?;

        let mut interpreter = crate::interpreter::Interpreter::new();
        interpreter.load_functions(mir_bodies);

        interpreter
            .run_main()
            .map_err(|e| CompilerError::Runtime(e.to_string()))
    }

    pub fn run(&self, source: &str) -> Result<i32, CompilerError> {
        // Build the program to a temporary location
        // Use a process-unique temporary directory to avoid collisions
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

        // Execute the compiled program
        let output = Command::new(&executable_path)
            .output()
            .map_err(|e| CompilerError::Codegen(format!("Failed to execute program: {}", e)))?;

        // Print stdout and stderr
        if !output.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }

        // Return the exit code
        Ok(output.status.code().unwrap_or(-1))
    }

    pub fn build(&self, source: &str, opts: &BuildOptions) -> Result<PathBuf, CompilerError> {
        // Use frontend_script to wrap scripts without functions in a main
        let pipeline_result = self.frontend_script(source)?;

        // Lower AST to MIR
        let mir_bodies = self.lower_to_mir(&pipeline_result)?;

        // Compile via selected backend
        let object_bytes = match opts.cpu_backend {
            CpuBackend::Cranelift => {
                #[cfg(feature = "cranelift")]
                {
                    use crate::codegen::CraneliftBackend;
                    let backend = CraneliftBackend::new()
                        .map_err(|e| CompilerError::Codegen(e.to_string()))?;

                    // Convert to the format expected by compile
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

                // Convert to the format expected by compile
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

        // Determine output paths
        let (work_dir, out_path) = if let Some(out) = opts.out_path.clone() {
            // User specified output - place object file in the same directory as the output
            // This avoids race conditions on shared directories like target/debug
            let work_dir = out
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            (work_dir, out)
        } else {
            // No output specified - use unique temp directory
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

        // Write object file
        let object_path = work_dir.join("output.o");
        fs::write(&object_path, &object_bytes)?;

        // Link to executable
        self.link_executable(&object_path, &out_path)?;

        Ok(out_path)
    }

    /// Lower AST functions to MIR bodies.
    fn lower_to_mir(
        &self,
        result: &PipelineResult,
    ) -> Result<Vec<(String, mir::Body)>, CompilerError> {
        let mut bodies = Vec::new();

        for stmt in &result.ast.body {
            if let StatementKind::FunctionDeclaration(name, _, _, _, _, _) = &stmt.node {
                let body = mir::lowering::lower_function(stmt, &result.type_checker)
                    .map_err(|e| CompilerError::Codegen(format!("MIR lowering failed: {}", e)))?;
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
        let mir_bodies = self.lower_to_mir(&pipeline_result)?;

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
