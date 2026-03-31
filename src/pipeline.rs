// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::RuntimeKind;
use crate::ast::expression::ExpressionKind;
use crate::ast::factory::{
    func, int_literal_expression, return_statement, type_expr_non_null, type_int,
};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::ast::Program;
use crate::cli::args::CpuBackend;
use crate::codegen::Backend;
use crate::error::compiler::CompilerError;
use crate::error::syntax::Span;
use crate::lexer::Lexer;
use crate::mir;
use crate::parser::Parser;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::type_checker::context::TypeDefinition;
use crate::type_checker::TypeChecker;

fn has_main_function(program: &Program) -> bool {
    program.body.iter().any(|s| {
        if let StatementKind::FunctionDeclaration(decl) = &s.node {
            decl.name == "main"
        } else {
            false
        }
    })
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

/// Returns true if the statement should stay at the top level (not wrapped in main).
fn is_top_level_stmt(stmt: &Statement) -> bool {
    matches!(
        &stmt.node,
        StatementKind::Use(..)
            | StatementKind::Class(..)
            | StatementKind::Struct(..)
            | StatementKind::Enum(..)
            | StatementKind::Trait(..)
            | StatementKind::Type(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::FunctionDeclaration(..)
    )
}

/// Information about runtime functions collected from the AST.
struct RuntimeInfo {
    /// Runtime function imports for the codegen backend.
    #[cfg(feature = "cranelift")]
    imports: Vec<crate::codegen::cranelift::RuntimeImport>,
    /// Set of distinct runtimes that need to be linked.
    required_runtimes: HashSet<RuntimeKind>,
}

/// Walk the AST and collect all runtime function declarations.
///
/// Extracts the symbol names, parameter types, and return types so the codegen
/// backend can declare them as external imports, and collects which runtime
/// libraries must be linked.
fn collect_runtime_info(
    program: &Program,
    imported_stmts: &[Statement],
    #[cfg(feature = "cranelift")] ptr_ty: Option<cranelift_codegen::ir::Type>,
) -> RuntimeInfo {
    #[cfg(feature = "cranelift")]
    let mut imports = Vec::new();
    let mut required_runtimes = HashSet::new();

    // Always link the "core" runtime — the codegen emits calls to
    // miri_rt_array_new / miri_rt_list_new for collection literals,
    // which bypass the import system and have no corresponding
    // RuntimeFunctionDeclaration in user code.
    required_runtimes.insert(RuntimeKind::Core);

    let all_stmts = program.body.iter().chain(imported_stmts.iter());

    for stmt in all_stmts {
        match &stmt.node {
            StatementKind::RuntimeFunctionDeclaration(runtime_kind, name, params, return_type) => {
                required_runtimes.insert(runtime_kind.clone());

                #[cfg(feature = "cranelift")]
                if let Some(ptr_ty) = ptr_ty {
                    use crate::codegen::cranelift::translate_type;
                    use crate::type_checker::resolve_type_name;

                    let param_types: Vec<_> = params
                        .iter()
                        .filter_map(|p| {
                            resolve_type_name(&p.typ).map(|t| translate_type(&t, ptr_ty))
                        })
                        .collect();

                    let ret_type = return_type
                        .as_ref()
                        .and_then(|rt| resolve_type_name(rt).map(|t| translate_type(&t, ptr_ty)));

                    imports.push(crate::codegen::cranelift::RuntimeImport {
                        name: name.clone(),
                        param_types,
                        return_type: ret_type,
                    });
                }
            }
            // Walk class bodies to collect required runtimes for linking and imports.
            StatementKind::Class(class_data) => {
                for class_stmt in &class_data.body {
                    if let StatementKind::RuntimeFunctionDeclaration(
                        runtime_kind,
                        name,
                        params,
                        return_type,
                    ) = &class_stmt.node
                    {
                        required_runtimes.insert(runtime_kind.clone());

                        #[cfg(feature = "cranelift")]
                        if let Some(ptr_ty) = ptr_ty {
                            use crate::codegen::cranelift::translate_type;
                            use crate::type_checker::resolve_type_name;

                            let param_types: Vec<_> = params
                                .iter()
                                .filter_map(|p| {
                                    resolve_type_name(&p.typ).map(|t| translate_type(&t, ptr_ty))
                                })
                                .collect();

                            let ret_type = return_type.as_ref().and_then(|rt| {
                                resolve_type_name(rt).map(|t| translate_type(&t, ptr_ty))
                            });

                            imports.push(crate::codegen::cranelift::RuntimeImport {
                                name: name.clone(),
                                param_types,
                                return_type: ret_type,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    RuntimeInfo {
        #[cfg(feature = "cranelift")]
        imports,
        required_runtimes,
    }
}

/// Wraps a script-style program (with or without function declarations) in a synthetic `main` function.
/// Skips programs that already contain a `main` function or non-wrappable type definitions.
///
/// The synthetic main has return type `Int` and appends `return 0` so the process
/// exits cleanly. Without this, the exit code leaks from whatever value the last
/// expression leaves in the return register.
fn wrap_script_in_main(program: &mut Program) {
    if has_main_function(program) {
        return;
    }

    let return_zero = return_statement(Some(Box::new(int_literal_expression(0))));
    let int_ret = type_expr_non_null(type_int());

    if program.body.is_empty() {
        let body = crate::ast::factory::block(vec![return_zero]);
        let main_fn = func("main").return_type(int_ret).build(body);
        program.body = vec![main_fn];
        return;
    }

    // Separate top-level declarations (use, class, struct, etc.) from executable statements.
    let mut top_level = Vec::new();
    let mut body_stmts = Vec::new();

    // First, verify we can wrap the program. If not, return early.
    for stmt in &program.body {
        if !is_top_level_stmt(stmt) && !is_wrappable_stmt(stmt) {
            return;
        }
    }

    let old_body = std::mem::take(&mut program.body);
    for stmt in old_body {
        if is_top_level_stmt(&stmt) {
            top_level.push(stmt);
        } else if is_wrappable_stmt(&stmt) {
            body_stmts.push(stmt);
        }
    }

    body_stmts.push(return_zero);
    let body = crate::ast::factory::block(body_stmts);
    let main_fn = func("main").return_type(int_ret).build(body);
    top_level.push(main_fn);
    program.body = top_level;
}

/// Ensures that a user-defined `main()` function returns `Int` with an
/// implicit `return 0` at the end, so the process exits cleanly.
fn patch_main_return(program: &mut Program) {
    use crate::ast::factory::stmt_with_span;

    let return_zero = return_statement(Some(Box::new(int_literal_expression(0))));
    let int_ret = type_expr_non_null(type_int());

    for stmt in &mut program.body {
        if let StatementKind::FunctionDeclaration(decl) = &mut stmt.node {
            if decl.name == "main" && decl.return_type.is_none() {
                // Set return type to Int
                decl.return_type = Some(Box::new(int_ret));

                // Append `return 0` to the body
                if let Some(body_stmt) = &mut decl.body {
                    match &mut body_stmt.node {
                        StatementKind::Block(stmts) => {
                            stmts.push(return_zero);
                        }
                        _ => {
                            let span = body_stmt.span;
                            let existing = std::mem::replace(
                                body_stmt.as_mut(),
                                stmt_with_span(StatementKind::Empty, Span::new(0, 0)),
                            );
                            **body_stmt = stmt_with_span(
                                StatementKind::Block(vec![existing, return_zero]),
                                span,
                            );
                        }
                    }
                }
                return;
            }
        }
    }
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
pub struct Pipeline {
    /// Directory of the entry-point source file.  When set, the type checker
    /// uses this to resolve `local.*` module imports.
    source_dir: Option<PathBuf>,
    /// Absolute path of the entry-point source file, used so that *all*
    /// errors (not just those from imported modules) include a file location.
    source_path: Option<String>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            source_dir: None,
            source_path: None,
        }
    }

    /// Configure the pipeline with the directory of the source file being
    /// compiled.  This enables `local.*` import resolution.
    pub fn with_source_dir(mut self, dir: PathBuf) -> Self {
        self.source_dir = Some(dir);
        self
    }

    /// Configure the pipeline with the absolute path of the entry-point
    /// source file.  This enables file-path display in all error messages.
    pub fn with_source_path(mut self, path: String) -> Self {
        self.source_path = Some(path);
        self
    }

    /// Returns the entry-point source path, if configured.
    pub fn source_path(&self) -> Option<&str> {
        self.source_path.as_deref()
    }

    /// Build a `TypeChecker` configured with this pipeline's source directory.
    fn make_type_checker(&self) -> TypeChecker {
        match &self.source_dir {
            Some(dir) => TypeChecker::with_source_dir(dir.clone()),
            None => TypeChecker::new(),
        }
    }

    /// Run the frontend (lexer, parser, type checker) on source code.
    pub fn frontend(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let mut ast = parser.parse().map_err(CompilerError::Parser)?;

        crate::ast::normalize::normalize(&mut ast);

        let mut type_checker = self.make_type_checker();
        type_checker
            .check(&ast)
            .map_err(|errors| CompilerError::TypeErrors {
                errors,
                warnings: type_checker.warnings.clone(),
            })?;

        Ok(PipelineResult { ast, type_checker })
    }

    /// Run the frontend with script-mode wrapping: simple programs without function
    /// declarations are wrapped in a synthetic `main`.
    pub fn frontend_script(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let mut ast = parser.parse().map_err(CompilerError::Parser)?;

        crate::ast::normalize::normalize(&mut ast);

        wrap_script_in_main(&mut ast);
        patch_main_return(&mut ast);

        let mut type_checker = self.make_type_checker();
        type_checker
            .check(&ast)
            .map_err(|errors| CompilerError::TypeErrors {
                errors,
                warnings: type_checker.warnings.clone(),
            })?;

        for warning in &type_checker.warnings {
            eprintln!(
                "{}",
                crate::error::format::format_diagnostic(
                    source,
                    warning,
                    self.source_path.as_deref(),
                )
            );
        }

        Ok(PipelineResult { ast, type_checker })
    }

    /// Compile and execute the source, returning the process exit code.
    pub fn run(&self, source: &str) -> Result<i32, CompilerError> {
        let temp_dir = tempfile::tempdir()
            .map_err(|e| CompilerError::Codegen(format!("Failed to create temp dir: {}", e)))?;
        let executable_path = temp_dir.path().join("program");

        let build_opts = BuildOptions {
            out_path: Some(executable_path.clone()),
            release: false,
            opt_level: 0,
            cpu_backend: CpuBackend::Cranelift,
        };

        self.build(source, &build_opts)?;

        // Canonicalize both paths to prevent symlink attacks and path traversal
        let canon_exe = executable_path.canonicalize().map_err(|e| {
            CompilerError::Codegen(format!("Failed to canonicalize executable path: {}", e))
        })?;
        let canon_dir = temp_dir.path().canonicalize().map_err(|e| {
            CompilerError::Codegen(format!("Failed to canonicalize temp dir path: {}", e))
        })?;

        // Containment check
        if !canon_exe.starts_with(&canon_dir) {
            return Err(CompilerError::Codegen(
                "Security violation: executable path escapes temporary directory".to_string(),
            ));
        }

        let output = Command::new(&canon_exe)
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

        let (object_bytes, required_runtimes) = match opts.cpu_backend {
            CpuBackend::Cranelift => {
                #[cfg(feature = "cranelift")]
                {
                    use crate::codegen::CraneliftBackend;
                    let mut backend = CraneliftBackend::new()
                        .map_err(|e| CompilerError::Codegen(e.to_string()))?;
                    backend.set_type_definitions(
                        pipeline_result.type_checker.type_definitions().clone(),
                    );

                    let ptr_ty = backend.pointer_type();
                    let runtime_info = collect_runtime_info(
                        &pipeline_result.ast,
                        &pipeline_result.type_checker.imported_statements,
                        Some(ptr_ty),
                    );
                    backend.set_runtime_imports(runtime_info.imports);

                    let bodies_ref: Vec<(&str, &mir::Body)> = mir_bodies
                        .iter()
                        .map(|(name, body)| (name.as_str(), body))
                        .collect();

                    let artifact = backend
                        .compile(&bodies_ref, &Default::default())
                        .map_err(|e| CompilerError::Codegen(e.to_string()))?;

                    (artifact.bytes, runtime_info.required_runtimes)
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

                let runtime_info = collect_runtime_info(
                    &pipeline_result.ast,
                    &pipeline_result.type_checker.imported_statements,
                    #[cfg(feature = "cranelift")]
                    None,
                );

                let bodies_ref: Vec<(&str, &mir::Body)> = mir_bodies
                    .iter()
                    .map(|(name, body)| (name.as_str(), body))
                    .collect();

                let artifact = backend
                    .compile(&bodies_ref, &Default::default())
                    .map_err(|e| CompilerError::Codegen(e.to_string()))?;

                (artifact.bytes, runtime_info.required_runtimes)
            }
        };

        let (work_dir, out_path) = if let Some(out) = opts.out_path.clone() {
            let work_dir = out
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            (work_dir, out)
        } else {
            let unique_dir = tempfile::Builder::new()
                .prefix("miri_build_")
                .tempdir()
                .map_err(|e| {
                    CompilerError::Codegen(format!("Failed to create build directory: {}", e))
                })?;
            #[allow(deprecated)]
            let unique_dir = unique_dir.into_path();
            let out = unique_dir.join("a.out");
            (unique_dir, out)
        };

        fs::create_dir_all(&work_dir)?;

        // Derive a unique object path from the output path so that parallel
        // builds targeting different output files don't overwrite each other's
        // intermediate `.o` file.
        let object_path = out_path.with_extension("o");
        fs::write(&object_path, &object_bytes)?;
        self.link_executable(&object_path, &out_path, &required_runtimes)?;

        Ok(out_path)
    }

    fn lower_to_mir(
        &self,
        result: &PipelineResult,
        is_release: bool,
    ) -> Result<Vec<(String, mir::Body)>, CompilerError> {
        let mut bodies = Vec::new();
        let mut lowered_names = std::collections::HashSet::new();

        // Lower functions and class methods from the program AST
        for stmt in &result.ast.body {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) => {
                    let (body, lambdas) =
                        mir::lowering::lower_function(stmt, &result.type_checker, is_release, true)
                            .map_err(|e| {
                                CompilerError::Codegen(format!("MIR lowering failed: {}", e))
                            })?;
                    lowered_names.insert(decl.name.clone());
                    bodies.push((decl.name.clone(), body));
                    for lambda in lambdas {
                        lowered_names.insert(lambda.name.clone());
                        bodies.push((lambda.name, lambda.body));
                    }
                }
                StatementKind::Class(class_data) => {
                    let class_name =
                        if let ExpressionKind::Identifier(name, _) = &class_data.name.node {
                            name.as_str()
                        } else {
                            continue;
                        };

                    let self_type =
                        Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);

                    for method_stmt in &class_data.body {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            // Skip abstract methods — they have no body and must not be compiled.
                            if method_decl.body.is_none() {
                                continue;
                            }

                            // Invariant: if the AST says there is a body, the type checker
                            // must not have marked this method as abstract.
                            debug_assert!(
                                !result
                                    .type_checker
                                    .type_definitions()
                                    .get(class_name)
                                    .and_then(|td| {
                                        if let TypeDefinition::Class(def) = td {
                                            def.methods.get(method_decl.name.as_str())
                                        } else {
                                            None
                                        }
                                    })
                                    .map(|m| m.is_abstract)
                                    .unwrap_or(false),
                                "abstract method '{}::{}' must not reach codegen",
                                class_name,
                                method_decl.name
                            );

                            let mut mangled = String::with_capacity(
                                class_name.len() + 1 + method_decl.name.len(),
                            );
                            mangled.push_str(class_name);
                            mangled.push('_');
                            mangled.push_str(&method_decl.name);
                            if lowered_names.contains(&mangled) {
                                continue;
                            }

                            let (mir_body, lambdas) = mir::lowering::lower_class_method(
                                method_stmt,
                                self_type.clone(),
                                &result.type_checker,
                                is_release,
                            )
                            .map_err(|e| {
                                CompilerError::Codegen(format!(
                                    "MIR lowering failed for {}: {}",
                                    mangled, e
                                ))
                            })?;

                            lowered_names.insert(mangled.clone());
                            bodies.push((mangled, mir_body));
                            for lambda in lambdas {
                                lowered_names.insert(lambda.name.clone());
                                bodies.push((lambda.name, lambda.body));
                            }
                        }
                    }
                }
                StatementKind::Trait(name_expr, _generics, _parent_traits, body, _vis) => {
                    // Compile default (non-abstract) trait methods as `TraitName_methodName`.
                    let trait_name = if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                        name.as_str()
                    } else {
                        continue;
                    };

                    let self_type =
                        Type::new(TypeKind::Custom(trait_name.to_string(), None), stmt.span);

                    for method_stmt in body {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            // Only compile methods with a body (default implementations).
                            if method_decl.body.is_none() {
                                continue;
                            }

                            let mut mangled = String::with_capacity(
                                trait_name.len() + 1 + method_decl.name.len(),
                            );
                            mangled.push_str(trait_name);
                            mangled.push('_');
                            mangled.push_str(&method_decl.name);
                            if lowered_names.contains(&mangled) {
                                continue;
                            }

                            let (mir_body, lambdas) = mir::lowering::lower_class_method(
                                method_stmt,
                                self_type.clone(),
                                &result.type_checker,
                                is_release,
                            )
                            .map_err(|e| {
                                CompilerError::Codegen(format!(
                                    "MIR lowering failed for {}: {}",
                                    mangled, e
                                ))
                            })?;

                            lowered_names.insert(mangled.clone());
                            bodies.push((mangled, mir_body));
                            for lambda in lambdas {
                                lowered_names.insert(lambda.name.clone());
                                bodies.push((lambda.name, lambda.body));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Lower functions and class methods imported from stdlib modules
        for stmt in &result.type_checker.imported_statements {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) => {
                    if !lowered_names.contains(&decl.name) {
                        let (body, lambdas) = mir::lowering::lower_function(
                            stmt,
                            &result.type_checker,
                            is_release,
                            true,
                        )
                        .map_err(|e| {
                            CompilerError::Codegen(format!("MIR lowering failed: {}", e))
                        })?;
                        lowered_names.insert(decl.name.clone());
                        bodies.push((decl.name.clone(), body));
                        for lambda in lambdas {
                            lowered_names.insert(lambda.name.clone());
                            bodies.push((lambda.name, lambda.body));
                        }
                    }
                }
                StatementKind::Class(class_data) => {
                    // Extract the class name string
                    let class_name =
                        if let ExpressionKind::Identifier(name, _) = &class_data.name.node {
                            name.as_str()
                        } else {
                            continue;
                        };

                    // Build the `self` type for this class
                    let self_type =
                        Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);

                    // Compile each non-runtime method in the class body
                    for method_stmt in &class_data.body {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            // Skip abstract methods — they have no body and must not be compiled.
                            if method_decl.body.is_none() {
                                continue;
                            }

                            // Invariant: if the AST says there is a body, the type checker
                            // must not have marked this method as abstract.
                            debug_assert!(
                                !result
                                    .type_checker
                                    .type_definitions()
                                    .get(class_name)
                                    .and_then(|td| {
                                        if let TypeDefinition::Class(def) = td {
                                            def.methods.get(method_decl.name.as_str())
                                        } else {
                                            None
                                        }
                                    })
                                    .map(|m| m.is_abstract)
                                    .unwrap_or(false),
                                "abstract method '{}::{}' must not reach codegen",
                                class_name,
                                method_decl.name
                            );

                            let mut mangled = String::with_capacity(
                                class_name.len() + 1 + method_decl.name.len(),
                            );
                            mangled.push_str(class_name);
                            mangled.push('_');
                            mangled.push_str(&method_decl.name);
                            if lowered_names.contains(&mangled) {
                                continue;
                            }

                            let (mir_body, lambdas) = mir::lowering::lower_class_method(
                                method_stmt,
                                self_type.clone(),
                                &result.type_checker,
                                is_release,
                            )
                            .map_err(|e| {
                                CompilerError::Codegen(format!(
                                    "MIR lowering failed for {}: {}",
                                    mangled, e
                                ))
                            })?;

                            lowered_names.insert(mangled.clone());
                            bodies.push((mangled, mir_body));
                            for lambda in lambdas {
                                lowered_names.insert(lambda.name.clone());
                                bodies.push((lambda.name, lambda.body));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // ── Per-concrete-class inherited-method compilation ──────────────────────
        //
        // When a concrete class C (e.g. Array, Car) inherits a non-abstract method
        // from an abstract ancestor B (e.g. Collection, Vehicle), `resolve_inherited_method`
        // returns C's name so that static dispatch calls `C_method` instead of `B_method`.
        // `B_method` internally uses virtual dispatch for abstract sub-calls, which
        // crashes for objects that carry no vtable pointer (Array, List).
        //
        // We therefore re-lower B's method body with self_type = C, naming the
        // result `C_method`.  The compiled code calls `C_length`, `C_element_at`,
        // etc. via *static* dispatch — correct and efficient for every concrete type.
        //
        // This pass covers both user-defined and stdlib classes.
        {
            use crate::type_checker::context::TypeDefinition;

            // Step 1: build abstract_class_methods —
            //   abstract class name → list of (method AST stmt, method name)
            //   for every non-abstract method body in that class.
            let mut abstract_class_methods: std::collections::HashMap<String, Vec<&Statement>> =
                std::collections::HashMap::new();

            let all_stmts = result
                .ast
                .body
                .iter()
                .chain(result.type_checker.imported_statements.iter());

            for stmt in all_stmts {
                if let StatementKind::Class(class_data) = &stmt.node {
                    let class_name = if let ExpressionKind::Identifier(n, _) = &class_data.name.node
                    {
                        n.as_str()
                    } else {
                        continue;
                    };
                    let is_abstract = matches!(
                        result.type_checker.global_type_definitions.get(class_name),
                        Some(TypeDefinition::Class(cd)) if cd.is_abstract
                    );
                    if !is_abstract {
                        continue;
                    }
                    let methods_entry = abstract_class_methods
                        .entry(class_name.to_string())
                        .or_default();
                    for method_stmt in &class_data.body {
                        if let StatementKind::FunctionDeclaration(md) = &method_stmt.node {
                            if md.body.is_some() {
                                // Only collect methods with a concrete body.
                                methods_entry.push(method_stmt);
                            }
                        }
                    }
                }
            }

            // Step 2: for each concrete class, compile inherited abstract-base methods.
            let all_stmts2 = result
                .ast
                .body
                .iter()
                .chain(result.type_checker.imported_statements.iter());

            for stmt in all_stmts2 {
                if let StatementKind::Class(class_data) = &stmt.node {
                    let class_name = if let ExpressionKind::Identifier(n, _) = &class_data.name.node
                    {
                        n.as_str()
                    } else {
                        continue;
                    };
                    let cd = match result.type_checker.global_type_definitions.get(class_name) {
                        Some(TypeDefinition::Class(cd)) => cd,
                        _ => continue,
                    };
                    if cd.is_abstract {
                        continue; // only process concrete classes
                    }

                    let self_type =
                        Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);

                    // Walk up the inheritance chain; stop at the first non-abstract class.
                    let mut base_opt = cd.base_class.clone();
                    while let Some(ref base_name) = base_opt.clone() {
                        let base_cd =
                            match result.type_checker.global_type_definitions.get(base_name) {
                                Some(TypeDefinition::Class(bcd)) => bcd,
                                _ => break,
                            };
                        if !base_cd.is_abstract {
                            break;
                        }

                        if let Some(method_stmts) = abstract_class_methods.get(base_name.as_str()) {
                            for method_stmt in method_stmts.iter() {
                                if let StatementKind::FunctionDeclaration(md) = &method_stmt.node {
                                    // Skip if the concrete class directly overrides this method.
                                    if cd.methods.contains_key(md.name.as_str()) {
                                        continue;
                                    }
                                    let mangled = format!("{}_{}", class_name, md.name);
                                    if lowered_names.contains(&mangled) {
                                        continue;
                                    }
                                    let (mir_body, lambdas) = mir::lowering::lower_class_method(
                                        method_stmt,
                                        self_type.clone(),
                                        &result.type_checker,
                                        is_release,
                                    )
                                    .map_err(|e| {
                                        CompilerError::Codegen(format!(
                                            "MIR lowering failed for {}: {}",
                                            mangled, e
                                        ))
                                    })?;
                                    lowered_names.insert(mangled.clone());
                                    bodies.push((mangled, mir_body));
                                    for lambda in lambdas {
                                        lowered_names.insert(lambda.name.clone());
                                        bodies.push((lambda.name, lambda.body));
                                    }
                                }
                            }
                        }

                        base_opt = base_cd.base_class.clone();
                    }
                }
            }
        }

        if bodies.is_empty() {
            return Err(CompilerError::Codegen(
                "No functions found to compile".to_string(),
            ));
        }

        // Monomorphization pass: collect all calls to mangled generic function names,
        // then re-lower the original generic function for each unique instantiation.
        {
            // Build a map from original function name → AST statement for quick lookup.
            let mut ast_func_map: std::collections::HashMap<&str, &Statement> =
                std::collections::HashMap::new();
            for stmt in &result.ast.body {
                if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                    if decl.generics.is_some() {
                        ast_func_map.insert(decl.name.as_str(), stmt);
                    }
                }
            }
            // Also consider imported generic functions (e.g. from stdlib)
            for stmt in &result.type_checker.imported_statements {
                if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
                    if decl.generics.is_some() {
                        ast_func_map.entry(decl.name.as_str()).or_insert(stmt);
                    }
                }
            }

            // Collect all call-site generic mappings from the type checker.
            // Build: mangled_name → (original_name, substitution_map)
            let mut needed: std::collections::HashMap<
                String,
                (
                    String,
                    std::collections::HashMap<String, crate::ast::types::Type>,
                ),
            > = std::collections::HashMap::new();

            for (call_id, type_args) in &result.type_checker.call_generic_mappings {
                // Find which function this call corresponds to by looking up the call expr
                // in the AST. We search all function bodies for Call exprs with this ID.
                // More direct: we get the function name from the type_args key and scan
                // all call terminators we already lowered.
                let _ = call_id; // used below via body scan

                let subs: std::collections::HashMap<String, crate::ast::types::Type> =
                    type_args.iter().cloned().collect();
                // We'll match this with terminators in the body scan below.
                let _ = subs;
            }

            // Scan all lowered bodies for calls to names containing "__" that look
            // like mangled generics (base__type1__type2…).
            for (_, body) in &bodies {
                for block in &body.basic_blocks {
                    if let Some(term) = &block.terminator {
                        if let mir::TerminatorKind::Call {
                            func: mir::Operand::Constant(c),
                            ..
                        } = &term.kind
                        {
                            if let crate::ast::literal::Literal::Identifier(fname) = &c.literal {
                                if fname.contains("__") && !lowered_names.contains(fname) {
                                    let original =
                                        fname.split("__").next().unwrap_or("").to_string();
                                    for type_args in
                                        result.type_checker.call_generic_mappings.values()
                                    {
                                        let subs: std::collections::HashMap<
                                            String,
                                            crate::ast::types::Type,
                                        > = type_args.iter().cloned().collect();
                                        let candidate =
                                            mir::lowering::control_flow::mangle_generic_name(
                                                &original, type_args,
                                            );
                                        if candidate == *fname {
                                            needed.insert(fname.clone(), (original.clone(), subs));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Lower each needed specialization.
            for (mangled_name, (original_name, subs)) in needed {
                if lowered_names.contains(&mangled_name) {
                    continue;
                }
                if let Some(&ast_stmt) = ast_func_map.get(original_name.as_str()) {
                    let (body, lambdas) = mir::lowering::lower_generic_instantiation(
                        ast_stmt,
                        &result.type_checker,
                        is_release,
                        true,
                        &subs,
                    )
                    .map_err(|e| {
                        CompilerError::Codegen(format!(
                            "MIR lowering failed for {}: {}",
                            mangled_name, e
                        ))
                    })?;
                    lowered_names.insert(mangled_name.clone());
                    bodies.push((mangled_name, body));
                    for lambda in lambdas {
                        lowered_names.insert(lambda.name.clone());
                        bodies.push((lambda.name, lambda.body));
                    }
                }
            }
        }

        // Insert Perceus RC operations on all function bodies, exactly once,
        // after all optimization passes have converged.
        for (_name, body) in &mut bodies {
            mir::optimization::insert_rc(body);
        }

        // Optional MIR verification pass: check RC invariants after Perceus.
        // Enabled by setting the MIRI_VERIFY_MIR environment variable to any
        // non-empty value, or by passing --verify-mir on the CLI (which sets
        // this variable before invoking the pipeline).
        if std::env::var("MIRI_VERIFY_MIR").is_ok() {
            let mut all_violations = Vec::new();
            for (name, body) in &bodies {
                let violations = mir::verify::verify_body(body);
                for v in violations {
                    all_violations.push(format!("  fn {}: {}", name, v));
                }
            }
            if !all_violations.is_empty() {
                return Err(CompilerError::MirVerification(format!(
                    "RC invariant violations detected in {} function(s):\n{}",
                    all_violations.len(),
                    all_violations.join("\n")
                )));
            }
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
    ///
    /// When `required_runtimes` is non-empty, the linker is instructed to
    /// search for and link the corresponding static runtime libraries.
    fn link_executable(
        &self,
        object_path: &PathBuf,
        output_path: &PathBuf,
        required_runtimes: &HashSet<RuntimeKind>,
    ) -> Result<(), CompilerError> {
        let linker_path = resolve_linker()?;
        let linker_path_display = linker_path.display().to_string();
        let mut cmd = Command::new(&linker_path);
        cmd.arg(object_path).arg("-o").arg(output_path);

        // Link required runtime libraries
        for runtime in required_runtimes {
            let lib_dir = runtime_library_dir(runtime)?;
            cmd.arg(format!("-L{}", lib_dir.display()));
            cmd.arg(format!("-l{}", runtime.library_name()));
        }

        let status = cmd.status().map_err(|e| {
            CompilerError::Codegen(format!(
                "Failed to run linker '{}': {}",
                linker_path_display, e
            ))
        })?;

        if !status.success() {
            return Err(CompilerError::Codegen(format!(
                "Linker failed with exit code: {:?}",
                status.code()
            )));
        }

        Ok(())
    }
}

/// Resolve the path to the linker (cc) using absolute paths or environment variables.
/// This prevents unqualified command execution vulnerabilities.
fn resolve_linker() -> Result<PathBuf, CompilerError> {
    if let Ok(cc) = std::env::var("MIRI_CC") {
        return Ok(PathBuf::from(cc));
    }
    if let Ok(cc) = std::env::var("CC") {
        return Ok(PathBuf::from(cc));
    }

    // Default to common absolute paths for 'cc'
    let common_paths = [
        "/usr/bin/cc",
        "/usr/local/bin/cc",
        "/usr/bin/gcc",
        "/usr/bin/clang",
    ];
    for path in common_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(CompilerError::Codegen(
        "Linker 'cc' not found in standard locations. Please set the MIRI_CC or CC environment variable to the absolute path of your linker.".to_string(),
    ))
}

/// Resolve the directory containing the compiled static library for a runtime.
///
/// Searches in order:
/// 1. `MIRI_RUNTIME_DIR` environment variable
/// 2. `src/runtime/<name>/target/release` relative to the compiler binary's
///    ancestor directories (walks up from the binary location).
fn runtime_library_dir(runtime: &RuntimeKind) -> Result<PathBuf, CompilerError> {
    // Check environment variable first
    if let Ok(dir) = std::env::var("MIRI_RUNTIME_DIR") {
        let path = PathBuf::from(dir);
        if path.exists() {
            return Ok(path);
        }
    }

    // Walk up from the compiler binary to find the project root
    if let Ok(exe) = std::env::current_exe() {
        let mut search = exe.as_path();
        while let Some(parent) = search.parent() {
            let release_candidate = parent
                .join("src")
                .join("runtime")
                .join(runtime.name())
                .join("target")
                .join("release");
            if release_candidate.exists() {
                return Ok(release_candidate);
            }
            let debug_candidate = parent
                .join("src")
                .join("runtime")
                .join(runtime.name())
                .join("target")
                .join("debug");
            if debug_candidate.exists() {
                return Ok(debug_candidate);
            }
            search = parent;
        }
    }

    // Fallback: try relative to CWD
    let cwd_candidate = PathBuf::from("src")
        .join("runtime")
        .join(runtime.name())
        .join("target")
        .join("release");
    if cwd_candidate.exists() {
        return Ok(cwd_candidate);
    }

    Err(CompilerError::Codegen(format!(
        "Could not find runtime library '{}'. Set MIRI_RUNTIME_DIR or build the runtime with `cargo build --release`.",
        runtime.library_name()
    )))
}
