// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::RuntimeKind;
use crate::ast::expression::ExpressionKind;
use crate::ast::factory::{
    func, int_literal_expression, return_statement, type_expr_non_null, type_int,
};
use crate::ast::statement::{AcceleratorTarget, Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::ast::Program;
use crate::codegen::Backend;
use crate::codegen::{BuildTarget, CpuBackend};
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
            | StatementKind::Forall { .. }
            | StatementKind::GpuFrame(..)
            | StatementKind::GpuFrameBlock(..)
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
            | StatementKind::IntrinsicFunctionDeclaration(..)
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

    // Link the "gpu" runtime when the program contains any `forall` /
    // `gpu fn` construct. The Cranelift backend lowers `GpuLaunch` to a
    // call into `miri_gpu_launch_inline`, which lives there.
    if program_uses_gpu(program.body.iter().chain(imported_stmts.iter())) {
        required_runtimes.insert(RuntimeKind::Gpu);
        #[cfg(feature = "cranelift")]
        if let Some(ptr_ty) = ptr_ty {
            imports.push(crate::codegen::cranelift::RuntimeImport {
                name: "miri_gpu_launch_inline".to_string(),
                param_types: vec![ptr_ty],
                return_type: Some(cranelift_codegen::ir::types::I8),
            });
        }
    }

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

/// Walks the AST looking for any GPU construct: `forall` statements or
/// function declarations carrying `is_gpu: true`. Used by
/// `collect_runtime_info` to decide whether to link `libmiri_runtime_gpu`.
pub fn program_uses_gpu<'a, I: IntoIterator<Item = &'a Statement>>(stmts: I) -> bool {
    for stmt in stmts {
        if stmt_uses_gpu(stmt) {
            return true;
        }
    }
    false
}

fn stmt_uses_gpu(stmt: &Statement) -> bool {
    match &stmt.node {
        StatementKind::Forall { device, .. } => {
            matches!(device, AcceleratorTarget::Gpu)
        }
        StatementKind::GpuFrame(_, _, _) => true,
        StatementKind::GpuFrameBlock(block) => stmt_uses_gpu(block),
        StatementKind::FunctionDeclaration(decl) => {
            decl.properties.is_gpu || decl.body.as_ref().is_some_and(|b| stmt_uses_gpu(b))
        }
        StatementKind::Block(stmts) => stmts.iter().any(stmt_uses_gpu),
        StatementKind::If(_, then_branch, else_branch, _) => {
            stmt_uses_gpu(then_branch) || else_branch.as_ref().is_some_and(|s| stmt_uses_gpu(s))
        }
        StatementKind::While(_, body, _) | StatementKind::For(_, _, body) => stmt_uses_gpu(body),
        StatementKind::Class(class_data) => class_data.body.iter().any(stmt_uses_gpu),
        StatementKind::Struct(_, _, _, methods, _)
        | StatementKind::Enum(_, _, _, methods, _, _)
        | StatementKind::Trait(_, _, _, methods, _) => methods.iter().any(stmt_uses_gpu),
        // A `gpu let` / `gpu var` binding may trigger a cross-residency
        // readback that calls into the GPU runtime even when the program has
        // no `forall` / `gpu fn`, so a gpu-resident declaration alone
        // requires linking it.
        StatementKind::Variable(decls, _) => decls
            .iter()
            .any(|d| d.residency == crate::ast::statement::BindingResidency::Gpu),
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::Return(_)
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => false,
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

/// Collects initial data for GPU buffers from compile-time constant
/// array/list literals in gpu let/var declarations.
fn collect_gpu_buffer_initializers(
    program: &Program,
) -> std::collections::HashMap<String, crate::codegen::web_gpu::GpuBufferInit> {
    use std::collections::HashMap;
    let mut inits = HashMap::new();

    fn walk_stmt(
        stmt: &Statement,
        inits: &mut HashMap<String, crate::codegen::web_gpu::GpuBufferInit>,
    ) {
        match &stmt.node {
            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    if decl.residency == crate::ast::statement::BindingResidency::Gpu {
                        if let Some(init) = &decl.initializer {
                            if let Some(data) = extract_const_array_values(init) {
                                // For Array<T, N>() sized constructors, extract explicit length
                                let length = extract_array_size(init);
                                inits.insert(
                                    decl.name.clone(),
                                    crate::codegen::web_gpu::GpuBufferInit {
                                        elem_type: infer_elem_type(init),
                                        values: data,
                                        length,
                                    },
                                );
                            }
                        }
                    }
                }
            }
            StatementKind::Block(stmts) => {
                for s in stmts {
                    walk_stmt(s, inits);
                }
            }
            StatementKind::If(_, then_branch, else_branch, _) => {
                walk_stmt(then_branch, inits);
                if let Some(e) = else_branch {
                    walk_stmt(e, inits);
                }
            }
            StatementKind::While(_, body, _) | StatementKind::For(_, _, body) => {
                walk_stmt(body, inits);
            }
            StatementKind::Forall { body, .. } => {
                walk_stmt(body, inits);
            }
            StatementKind::FunctionDeclaration(decl) => {
                if let Some(body) = &decl.body {
                    walk_stmt(body, inits);
                }
            }
            _ => {}
        }
    }

    walk_stmt(
        &Statement {
            node: StatementKind::Block(program.body.clone()),
            span: crate::error::syntax::Span::new(0, 0),
            id: 0,
        },
        &mut inits,
    );

    inits
}

fn extract_const_array_values(expr: &crate::ast::expression::Expression) -> Option<Vec<f64>> {
    match &expr.node {
        ExpressionKind::Array(elements, _) => {
            let mut values = Vec::new();
            for elem in elements {
                match extract_numeric_literal(elem) {
                    Some(v) => values.push(v),
                    None => return None,
                }
            }
            Some(values)
        }
        ExpressionKind::List(elements) => {
            let mut values = Vec::new();
            for elem in elements {
                match extract_numeric_literal(elem) {
                    Some(v) => values.push(v),
                    None => return None,
                }
            }
            Some(values)
        }
        ExpressionKind::Call(func_expr, args) if args.is_empty() => {
            // Handle Array<T, N>() constructor: return empty vector
            // (length will be determined from the type generic N)
            if is_array_constructor(func_expr) {
                Some(Vec::new())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_array_constructor(expr: &crate::ast::expression::Expression) -> bool {
    if let ExpressionKind::TypeDeclaration(name_expr, Some(generics), _, _) = &expr.node {
        if let ExpressionKind::Identifier(name, _) = &name_expr.node {
            // Array<T, N> has exactly 2 generics
            return name == "Array" && generics.len() == 2;
        }
    }
    false
}

fn extract_numeric_literal(expr: &crate::ast::expression::Expression) -> Option<f64> {
    match &expr.node {
        ExpressionKind::Literal(lit) => match lit {
            crate::ast::literal::Literal::Integer(int_lit) => {
                use crate::ast::literal::IntegerLiteral;
                Some(match int_lit {
                    IntegerLiteral::I8(v) => *v as f64,
                    IntegerLiteral::I16(v) => *v as f64,
                    IntegerLiteral::I32(v) => *v as f64,
                    IntegerLiteral::I64(v) => *v as f64,
                    IntegerLiteral::I128(v) => *v as f64,
                    IntegerLiteral::U8(v) => *v as f64,
                    IntegerLiteral::U16(v) => *v as f64,
                    IntegerLiteral::U32(v) => *v as f64,
                    IntegerLiteral::U64(v) => *v as f64,
                    IntegerLiteral::U128(v) => *v as f64,
                })
            }
            crate::ast::literal::Literal::Float(float_lit) => {
                use crate::ast::literal::FloatLiteral;
                Some(match float_lit {
                    FloatLiteral::F32(v) => f32::from_bits(*v) as f64,
                    FloatLiteral::F64(v) => f64::from_bits(*v),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

fn infer_elem_type(expr: &crate::ast::expression::Expression) -> String {
    match &expr.node {
        ExpressionKind::Array(elements, _) | ExpressionKind::List(elements) => {
            if let Some(elem) = elements.first() {
                infer_elem_type_from_literal(elem)
            } else {
                "i32".to_string()
            }
        }
        ExpressionKind::Call(func_expr, _) if is_array_constructor(func_expr) => {
            // For Array<T, N>(), extract T from the first generic argument
            if let ExpressionKind::TypeDeclaration(_base, Some(generics), _, _) = &func_expr.node {
                if let Some(elem_type_expr) = generics.first() {
                    return match &elem_type_expr.node {
                        ExpressionKind::Identifier(type_name, _) => {
                            use crate::ast::types::wgsl_scalar_name;
                            // Try to map the type name using the shared WGSL scalar mapping.
                            // Parse the type name into a TypeKind for lookup.
                            let kind = match type_name.as_str() {
                                "int" => Some(crate::ast::types::TypeKind::Int),
                                "i8" => Some(crate::ast::types::TypeKind::I8),
                                "i16" => Some(crate::ast::types::TypeKind::I16),
                                "i32" => Some(crate::ast::types::TypeKind::I32),
                                "i64" => Some(crate::ast::types::TypeKind::I64),
                                "u8" => Some(crate::ast::types::TypeKind::U8),
                                "u16" => Some(crate::ast::types::TypeKind::U16),
                                "u32" => Some(crate::ast::types::TypeKind::U32),
                                "u64" => Some(crate::ast::types::TypeKind::U64),
                                "f32" => Some(crate::ast::types::TypeKind::F32),
                                "float" => Some(crate::ast::types::TypeKind::Float),
                                "f64" => Some(crate::ast::types::TypeKind::F64),
                                _ => None,
                            };
                            kind.and_then(|k| wgsl_scalar_name(&k))
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "i32".to_string())
                        }
                        ExpressionKind::Type(inner_ty, _) => {
                            // Extract type name from the Type inside (created by type checker)
                            infer_elem_type_from_type(&inner_ty.kind)
                        }
                        _ => "i32".to_string(),
                    };
                }
            }
            "i32".to_string()
        }
        _ => "i32".to_string(),
    }
}

fn infer_elem_type_from_type(kind: &crate::ast::types::TypeKind) -> String {
    use crate::ast::types::wgsl_scalar_name;
    wgsl_scalar_name(kind).unwrap_or("i32").to_string()
}

fn infer_elem_type_from_literal(elem: &crate::ast::expression::Expression) -> String {
    match &elem.node {
        ExpressionKind::Literal(lit) => match lit {
            crate::ast::literal::Literal::Float(float_lit) => {
                use crate::ast::literal::FloatLiteral;
                match float_lit {
                    FloatLiteral::F32(_) => "f32".to_string(),
                    FloatLiteral::F64(_) => "f64".to_string(),
                }
            }
            crate::ast::literal::Literal::Integer(_) => {
                // Integer literals are int (Miri default), which maps to i32 for browser portability.
                // The host keeps i64; marshalling narrows to i32 for device and widens on readback.
                "i32".to_string()
            }
            _ => "i32".to_string(),
        },
        _ => "i32".to_string(),
    }
}

fn extract_array_size(expr: &crate::ast::expression::Expression) -> Option<usize> {
    if let ExpressionKind::Call(func_expr, _) = &expr.node {
        if let ExpressionKind::TypeDeclaration(_, Some(generics), _, _) = &func_expr.node {
            if generics.len() >= 2 {
                // The size is the second generic argument
                return try_eval_const_size(&generics[1]);
            }
        }
    }
    None
}

fn try_eval_const_size(expr: &crate::ast::expression::Expression) -> Option<usize> {
    // Try to evaluate simple constant expressions (literals and arithmetic)
    match &expr.node {
        ExpressionKind::Literal(lit) => {
            if let crate::ast::literal::Literal::Integer(int_lit) = lit {
                use crate::ast::literal::IntegerLiteral;
                let val = match int_lit {
                    IntegerLiteral::I8(v) => *v as i128,
                    IntegerLiteral::I16(v) => *v as i128,
                    IntegerLiteral::I32(v) => *v as i128,
                    IntegerLiteral::I64(v) => *v as i128,
                    IntegerLiteral::I128(v) => *v,
                    IntegerLiteral::U8(v) => *v as i128,
                    IntegerLiteral::U16(v) => *v as i128,
                    IntegerLiteral::U32(v) => *v as i128,
                    IntegerLiteral::U64(v) => *v as i128,
                    IntegerLiteral::U128(v) => *v as i128,
                };
                if val >= 0 {
                    return Some(val as usize);
                }
            }
            None
        }
        ExpressionKind::Binary(left, op, right) => {
            let l = try_eval_const_size(left)?;
            let r = try_eval_const_size(right)?;
            match op {
                crate::ast::operator::BinaryOp::Add => Some(l + r),
                crate::ast::operator::BinaryOp::Sub => Some(l.saturating_sub(r)),
                crate::ast::operator::BinaryOp::Mul => Some(l * r),
                crate::ast::operator::BinaryOp::Div if r > 0 => Some(l / r),
                _ => None,
            }
        }
        _ => None,
    }
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
    /// Target environment for the produced artifact.
    pub target: BuildTarget,
}

/// Orchestrates the full compilation pipeline from source to executable.
pub struct Pipeline {
    /// Directory of the entry-point source file.  When set, the type checker
    /// uses this to resolve `local.*` module imports.
    source_dir: Option<PathBuf>,
    /// Absolute path of the entry-point source file, used so that *all*
    /// errors (not just those from imported modules) include a file location.
    source_path: Option<String>,
    /// Whether to run the MIR verification pass after RC insertion.
    verify_mir: bool,
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
            verify_mir: false,
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

    /// Configure whether to run the MIR verification pass after RC insertion.
    pub fn with_verify_mir(mut self, verify: bool) -> Self {
        self.verify_mir = verify;
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
            target: BuildTarget::Native,
        };

        self.build(source, &build_opts)?;

        // Ensure canonical paths before executing to prevent traversal or symlink attacks.
        let canonical_temp = temp_dir.path().canonicalize().map_err(|e| {
            CompilerError::Codegen(format!("Failed to canonicalize temp dir: {}", e))
        })?;
        let canonical_executable = executable_path.canonicalize().map_err(|e| {
            CompilerError::Codegen(format!("Failed to canonicalize executable path: {}", e))
        })?;

        if !canonical_executable.starts_with(&canonical_temp) {
            return Err(CompilerError::Codegen(
                "Access Denied: Executable path is outside the temporary directory".to_string(),
            ));
        }

        let output = Command::new(&canonical_executable)
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
        let mut pipeline_result = self.frontend_script(source)?;
        pipeline_result.type_checker.entry_source = Some(std::rc::Rc::from(source));
        pipeline_result.type_checker.entry_source_path = self.source_path().map(std::rc::Rc::from);
        let mir_bodies = self.lower_to_mir(&pipeline_result, opts.release)?;

        match opts.target {
            BuildTarget::Native => {}
            BuildTarget::WebGpu => {
                let gpu_buffer_inits = collect_gpu_buffer_initializers(&pipeline_result.ast);
                return crate::codegen::web_gpu::emit_bundle(
                    &mir_bodies,
                    opts.out_path.as_ref(),
                    Some(source),
                    if gpu_buffer_inits.is_empty() {
                        None
                    } else {
                        Some(&gpu_buffer_inits)
                    },
                );
            }
        }

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

    /// Extract an identifier name from an expression, if it is a simple identifier.
    fn identifier_name(expr: &crate::ast::Expression) -> Option<&str> {
        if let ExpressionKind::Identifier(name, _) = &expr.node {
            Some(name.as_str())
        } else {
            None
        }
    }

    fn mangle_method_name(class_name: &str, method_name: &str) -> String {
        let mut mangled = String::with_capacity(class_name.len() + 1 + method_name.len());
        mangled.push_str(class_name);
        mangled.push('_');
        mangled.push_str(method_name);
        mangled
    }

    fn lower_to_mir(
        &self,
        result: &PipelineResult,
        is_release: bool,
    ) -> Result<Vec<(String, mir::Body)>, CompilerError> {
        let mut bodies = Vec::new();
        let mut lowered_names = std::collections::HashSet::new();

        self.lower_program_bodies(result, is_release, &mut bodies, &mut lowered_names)?;
        self.lower_imported_bodies(result, is_release, &mut bodies, &mut lowered_names)?;
        self.lower_inherited_methods(result, is_release, &mut bodies, &mut lowered_names)?;
        self.lower_trait_default_methods(result, is_release, &mut bodies, &mut lowered_names)?;

        if bodies.is_empty() {
            return Err(CompilerError::Codegen(
                "No functions found to compile".to_string(),
            ));
        }

        self.lower_monomorphized_generics(result, is_release, &mut bodies, &mut lowered_names)?;

        // Clone user functions that are transitively called from GPU kernels into
        // GpuDevice bodies for WGSL emission. Each clone is f32-narrowed for GPU compatibility.
        Self::clone_gpu_device_helpers(&mut bodies)?;

        // Insert Perceus RC operations on all function bodies, exactly once,
        // after all optimization passes have converged.
        for (_name, body) in &mut bodies {
            mir::optimization::insert_rc(body);
        }

        // Elide redundant IncRef/DecRef pairs identified by the RC elision pass.
        // This runs after Perceus to remove pairs that are provably net-zero.
        for (_name, body) in &mut bodies {
            mir::optimization::elide_rc(body);
        }

        // Optional MIR verification pass: check RC invariants after Perceus.
        // Enabled by setting the MIRI_VERIFY_MIR environment variable to any
        // non-empty value, or by configuring it on the Pipeline instance.
        if self.verify_mir || std::env::var("MIRI_VERIFY_MIR").is_ok() {
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

    /// Lower top-level functions and the methods of every class/trait/struct/enum
    /// declared in the user's program AST.
    fn lower_program_bodies(
        &self,
        result: &PipelineResult,
        is_release: bool,
        bodies: &mut Vec<(String, mir::Body)>,
        lowered_names: &mut std::collections::HashSet<String>,
    ) -> Result<(), CompilerError> {
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
                    let Some(class_name) = Self::identifier_name(&class_data.name) else {
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

                            let mangled = Self::mangle_method_name(class_name, &method_decl.name);
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
                    let Some(trait_name) = Self::identifier_name(name_expr) else {
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

                            let mangled = Self::mangle_method_name(trait_name, &method_decl.name);
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
                StatementKind::Struct(name_expr, _generics, _fields, methods, _vis) => {
                    // Compile struct methods with bodies as `StructName_methodName`.
                    // Use lower_function (not lower_class_method) because struct methods
                    // declare `self` explicitly in their param list, so lower_class_method
                    // would double-add it. lower_function treats `self` as a regular param
                    // with synthesized type Custom("Self") which Perceus skips RC-tracking.
                    let Some(struct_name) = Self::identifier_name(name_expr) else {
                        continue;
                    };

                    for method_stmt in methods {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            if method_decl.body.is_none() {
                                continue;
                            }

                            let mangled = Self::mangle_method_name(struct_name, &method_decl.name);
                            if lowered_names.contains(&mangled) {
                                continue;
                            }

                            let (mir_body, lambdas) = mir::lowering::lower_function(
                                method_stmt,
                                &result.type_checker,
                                is_release,
                                true,
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
                StatementKind::Enum(name_expr, _generics, _variants, methods, _vis, _must_use) => {
                    // Compile enum methods as `EnumName_methodName` using lower_class_method.
                    // Enum methods have no explicit `self` in their param list (like class methods).
                    let Some(enum_name) = Self::identifier_name(name_expr) else {
                        continue;
                    };

                    let self_type = crate::ast::types::Type::new(
                        crate::ast::types::TypeKind::Custom(enum_name.to_string(), None),
                        stmt.span,
                    );

                    for method_stmt in methods {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            if method_decl.body.is_none() {
                                continue;
                            }

                            let mangled = Self::mangle_method_name(enum_name, &method_decl.name);
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
        Ok(())
    }

    /// Lower functions and methods imported from stdlib `.mi` modules that were
    /// not already lowered from the program AST.
    fn lower_imported_bodies(
        &self,
        result: &PipelineResult,
        is_release: bool,
        bodies: &mut Vec<(String, mir::Body)>,
        lowered_names: &mut std::collections::HashSet<String>,
    ) -> Result<(), CompilerError> {
        // Lower functions and class methods imported from stdlib modules
        for stmt in &result.type_checker.imported_statements {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) if !lowered_names.contains(&decl.name) => {
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
                    // Extract the class name string
                    let Some(class_name) = Self::identifier_name(&class_data.name) else {
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

                            let mangled = Self::mangle_method_name(class_name, &method_decl.name);
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
                StatementKind::Enum(name_expr, _generics, _variants, methods, _vis, _must_use) => {
                    let Some(enum_name) = Self::identifier_name(name_expr) else {
                        continue;
                    };

                    let self_type =
                        Type::new(TypeKind::Custom(enum_name.to_string(), None), stmt.span);

                    for method_stmt in methods {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            if method_decl.body.is_none() {
                                continue;
                            }

                            let mangled = Self::mangle_method_name(enum_name, &method_decl.name);
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
                    // Compile default (non-abstract) trait methods imported from stdlib
                    // as `TraitName_methodName`, mirroring the in-program AST trait pass.
                    let Some(trait_name) = Self::identifier_name(name_expr) else {
                        continue;
                    };

                    let self_type =
                        Type::new(TypeKind::Custom(trait_name.to_string(), None), stmt.span);

                    for method_stmt in body {
                        if let StatementKind::FunctionDeclaration(method_decl) = &method_stmt.node {
                            if method_decl.body.is_none() {
                                continue;
                            }

                            let mangled = Self::mangle_method_name(trait_name, &method_decl.name);
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
        Ok(())
    }

    /// Re-lower abstract-ancestor methods per concrete class.
    ///
    /// When a concrete class C inherits a non-abstract method from an abstract
    /// ancestor B, `resolve_inherited_method` returns C's name so static dispatch
    /// calls `C_method` instead of `B_method`. `B_method` uses virtual dispatch for
    /// abstract sub-calls, which crashes for objects with no vtable (Array, List).
    /// We re-lower B's method body with self_type = C, naming the result `C_method`,
    /// so calls resolve via static dispatch. Covers user-defined and stdlib classes.
    fn lower_inherited_methods(
        &self,
        result: &PipelineResult,
        is_release: bool,
        bodies: &mut Vec<(String, mir::Body)>,
        lowered_names: &mut std::collections::HashSet<String>,
    ) -> Result<(), CompilerError> {
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
                    let Some(class_name) = Self::identifier_name(&class_data.name) else {
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
                    let Some(class_name) = Self::identifier_name(&class_data.name) else {
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
                                    let mut mangled =
                                        String::with_capacity(class_name.len() + 1 + md.name.len());
                                    mangled.push_str(class_name);
                                    mangled.push('_');
                                    mangled.push_str(&md.name);
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
        Ok(())
    }

    /// Re-lower trait default methods per concrete class.
    ///
    /// Mirrors `lower_inherited_methods` for traits. When a concrete class C
    /// implements a trait T with a non-abstract default method M, we synthesize
    /// `C_M` by re-lowering T.M's body with self_type = C. This lets
    /// `resolve_inherited_method` return C as the defining class (concrete-caller
    /// rule), so calls dispatch statically to `C_M`. Inside C_M, self-calls like
    /// `self.length()` also dispatch statically because self_type is concrete —
    /// avoiding the slot-mismatch issue between per-trait and combined-vtable
    /// method layouts.
    fn lower_trait_default_methods(
        &self,
        result: &PipelineResult,
        is_release: bool,
        bodies: &mut Vec<(String, mir::Body)>,
        lowered_names: &mut std::collections::HashSet<String>,
    ) -> Result<(), CompilerError> {
        {
            use crate::type_checker::context::TypeDefinition;

            // Step 1: collect default-method statements per trait name from both
            // user code and stdlib imports.
            let mut trait_default_methods: std::collections::HashMap<String, Vec<&Statement>> =
                std::collections::HashMap::new();

            let all_stmts = result
                .ast
                .body
                .iter()
                .chain(result.type_checker.imported_statements.iter());

            for stmt in all_stmts {
                if let StatementKind::Trait(name_expr, _gens, _parents, body, _vis) = &stmt.node {
                    let Some(trait_name) = Self::identifier_name(name_expr) else {
                        continue;
                    };
                    let entry = trait_default_methods
                        .entry(trait_name.to_string())
                        .or_default();
                    for method_stmt in body {
                        if let StatementKind::FunctionDeclaration(md) = &method_stmt.node {
                            if md.body.is_some() {
                                entry.push(method_stmt);
                            }
                        }
                    }
                }
            }

            // Step 2: for each concrete class, walk the trait hierarchy of every
            // trait it (or any ancestor class) implements, and emit a `C_M` copy.
            let all_stmts2 = result
                .ast
                .body
                .iter()
                .chain(result.type_checker.imported_statements.iter());

            for stmt in all_stmts2 {
                let class_data = match &stmt.node {
                    StatementKind::Class(cd) => cd,
                    _ => continue,
                };
                let Some(class_name) = Self::identifier_name(&class_data.name) else {
                    continue;
                };
                let cd = match result.type_checker.global_type_definitions.get(class_name) {
                    Some(TypeDefinition::Class(cd)) => cd,
                    _ => continue,
                };
                if cd.is_abstract {
                    continue;
                }

                let self_type =
                    Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);

                // Walk all traits implemented by this class and its ancestors, plus
                // the transitive parent-trait closure, collecting unique trait names.
                let mut trait_names_to_process: Vec<String> = Vec::new();
                let mut visited_traits: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut walk_class = class_name.to_string();
                while let Some(TypeDefinition::Class(walk_cd)) =
                    result.type_checker.global_type_definitions.get(&walk_class)
                {
                    for t_name in &walk_cd.traits {
                        let mut to_check = vec![t_name.clone()];
                        while let Some(t) = to_check.pop() {
                            if !visited_traits.insert(t.clone()) {
                                continue;
                            }
                            if let Some(TypeDefinition::Trait(td)) =
                                result.type_checker.global_type_definitions.get(&t)
                            {
                                to_check.extend(td.parent_traits.iter().cloned());
                            }
                            trait_names_to_process.push(t);
                        }
                    }
                    match &walk_cd.base_class {
                        Some(b) => walk_class = b.clone(),
                        None => break,
                    }
                }

                for t_name in &trait_names_to_process {
                    let method_stmts = match trait_default_methods.get(t_name.as_str()) {
                        Some(ms) => ms,
                        None => continue,
                    };
                    for method_stmt in method_stmts {
                        if let StatementKind::FunctionDeclaration(md) = &method_stmt.node {
                            // Skip if the concrete class (or an ancestor) overrides.
                            let overridden = {
                                let mut current = class_name.to_string();
                                let mut found = false;
                                while let Some(TypeDefinition::Class(c)) =
                                    result.type_checker.global_type_definitions.get(&current)
                                {
                                    if c.methods.contains_key(md.name.as_str()) {
                                        found = true;
                                        break;
                                    }
                                    match &c.base_class {
                                        Some(b) => current = b.clone(),
                                        None => break,
                                    }
                                }
                                found
                            };
                            if overridden {
                                continue;
                            }
                            let mut mangled =
                                String::with_capacity(class_name.len() + 1 + md.name.len());
                            mangled.push_str(class_name);
                            mangled.push('_');
                            mangled.push_str(&md.name);
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
            }
        }
        Ok(())
    }

    /// Monomorphize generic functions: collect every call to a mangled generic name
    /// in the already-lowered bodies, then re-lower the original generic for each
    /// unique instantiation.
    fn lower_monomorphized_generics(
        &self,
        result: &PipelineResult,
        is_release: bool,
        bodies: &mut Vec<(String, mir::Body)>,
        lowered_names: &mut std::collections::HashSet<String>,
    ) -> Result<(), CompilerError> {
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
            for (_, body) in &*bodies {
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
        Ok(())
    }

    /// Get MIR as a string for debugging purposes (pre-RC).
    pub fn get_mir(&self, source: &str) -> Result<String, CompilerError> {
        let mut pipeline_result = self.frontend_script(source)?;
        pipeline_result.type_checker.entry_source = Some(std::rc::Rc::from(source));
        pipeline_result.type_checker.entry_source_path = self.source_path().map(std::rc::Rc::from);
        let mir_bodies = self.lower_to_mir(&pipeline_result, false)?;

        let mut output = String::new();
        for (name, body) in &mir_bodies {
            output.push_str(&format!("=== MIR for {} ===\n{}\n\n", name, body));
        }
        Ok(output)
    }

    /// Lower a source to its full set of MIR bodies using the standard
    /// (non-script) frontend, including the GpuDevice helper-clone pass.
    ///
    /// Intended for GPU test harnesses that need exactly the bodies the real
    /// codegen path emits — the kernel(s) plus every GpuDevice helper reachable
    /// from a kernel — without the script-mode wrapping or RC insertion that
    /// would perturb the emitted WGSL.
    pub fn get_gpu_mir_bodies(
        &self,
        source: &str,
    ) -> Result<Vec<(String, crate::mir::Body)>, CompilerError> {
        let result = self.frontend(source)?;
        self.lower_to_mir(&result, false)
    }

    /// Get MIR bodies after Perceus RC insertion and RC elision, for test inspection.
    ///
    /// Returns the lowered and optimised bodies (including RC operations) so that
    /// tests can verify the RC elision pass removed expected IncRef/DecRef pairs.
    pub fn get_mir_bodies_with_rc(
        &self,
        source: &str,
    ) -> Result<Vec<(String, crate::mir::Body)>, CompilerError> {
        let mut pipeline_result = self.frontend_script(source)?;
        pipeline_result.type_checker.entry_source = Some(std::rc::Rc::from(source));
        pipeline_result.type_checker.entry_source_path = self.source_path().map(std::rc::Rc::from);
        let mut bodies = self.lower_to_mir(&pipeline_result, false)?;
        for (_name, body) in &mut bodies {
            mir::optimization::insert_rc(body);
        }
        for (_name, body) in &mut bodies {
            mir::optimization::elide_rc(body);
        }
        Ok(bodies)
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

        // Link the math library (required for sin, cos, pow, etc. on Linux)
        cmd.arg("-lm");

        // Link required runtime libraries
        for runtime in required_runtimes {
            let lib_dir = runtime_library_dir(runtime)?;
            cmd.arg(format!("-L{}", lib_dir.display()));
            cmd.arg(format!("-l{}", runtime.library_name()));
            for arg in runtime.extra_link_args() {
                cmd.arg(arg);
            }
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

    /// Clone user functions that are transitively called from GPU kernels.
    /// Each clone is marked as GpuDevice and has f32-narrowed types.
    fn clone_gpu_device_helpers(
        bodies: &mut Vec<(String, mir::Body)>,
    ) -> Result<(), CompilerError> {
        let kernel_names: std::collections::HashSet<&str> = bodies
            .iter()
            .filter(|(_, b)| b.execution_model == mir::ExecutionModel::GpuKernel)
            .map(|(n, _)| n.as_str())
            .collect();

        if kernel_names.is_empty() {
            return Ok(());
        }

        let reachable = Self::compute_reachable_callees(bodies, &kernel_names)?;
        let mut helpers_to_add = Vec::new();
        let mut helper_names: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (name, body) in bodies.iter() {
            if reachable.contains(name.as_str())
                && body.execution_model == mir::ExecutionModel::Cpu
                && !kernel_names.contains(name.as_str())
            {
                let mut gpu_body = body.clone();
                gpu_body.execution_model = mir::ExecutionModel::GpuDevice;
                Self::narrow_float_types(&mut gpu_body);
                helper_names.insert(name.clone());
                helpers_to_add.push((name.clone(), gpu_body));
            }
        }

        // A kernel that calls a (now f32-narrowed) GpuDevice helper must not widen
        // the f32 buffer values it passes to the helper's float params. Narrowing
        // the calling kernel's scalar f64 temps to f32 removes that spurious widen.
        // `narrow_type_kind` only touches scalar floats, so explicit `array<f64>`
        // buffers are left intact; f64-buffer kernels that call no helper are
        // unaffected because they never enter this set.
        if !helper_names.is_empty() {
            for idx in 0..bodies.len() {
                if bodies[idx].1.execution_model != mir::ExecutionModel::GpuKernel {
                    continue;
                }
                let one_kernel: std::collections::HashSet<&str> =
                    std::iter::once(bodies[idx].0.as_str()).collect();
                let kernel_reach = Self::compute_reachable_callees(bodies, &one_kernel)?;
                if kernel_reach.iter().any(|n| helper_names.contains(n)) {
                    Self::narrow_float_types(&mut bodies[idx].1);
                }
            }
        }

        bodies.extend(helpers_to_add);
        Ok(())
    }

    /// Compute the set of function names transitively called from the given kernel names.
    fn compute_reachable_callees(
        bodies: &[(String, mir::Body)],
        kernel_names: &std::collections::HashSet<&str>,
    ) -> Result<std::collections::HashSet<String>, CompilerError> {
        use std::collections::VecDeque;

        let name_to_body: std::collections::HashMap<&str, &mir::Body> =
            bodies.iter().map(|(n, b)| (n.as_str(), b)).collect();

        let mut reachable = std::collections::HashSet::new();
        let mut queue = VecDeque::new();

        for kernel_name in kernel_names {
            reachable.insert(kernel_name.to_string());
            queue.push_back(kernel_name.to_string());
        }

        while let Some(current_name) = queue.pop_front() {
            if let Some(body) = name_to_body.get(current_name.as_str()) {
                let callees = Self::extract_callees(body);
                for callee in callees {
                    if !reachable.contains(&callee) {
                        reachable.insert(callee.clone());
                        queue.push_back(callee);
                    }
                }
            }
        }

        Ok(reachable)
    }

    /// Extract function names called by a body (by scanning all Call terminators).
    fn extract_callees(body: &mir::Body) -> Vec<String> {
        use crate::ast::literal::Literal;

        let mut callees = Vec::new();
        for block in &body.basic_blocks {
            if let Some(term) = &block.terminator {
                if let mir::TerminatorKind::Call {
                    func: mir::Operand::Constant(c),
                    ..
                } = &term.kind
                {
                    if let Literal::Identifier(name) = &c.literal {
                        callees.push(name.clone());
                    }
                }
            }
        }
        callees
    }

    /// Narrow all f64 (float) types in the body to f32 for GPU compatibility.
    fn narrow_float_types(body: &mut mir::Body) {
        for local_decl in &mut body.local_decls {
            Self::narrow_type_kind(&mut local_decl.ty.kind);
            local_decl.mir_ty = mir::types::MirType::from_type_kind(&local_decl.ty.kind);
        }
        // Constant operands carry their own type, which drives the WGSL literal
        // width (`2.0lf` for f64). Narrow them too, or an f64 literal survives
        // into an otherwise-f32 helper and trips the SHADER_F64 feature gate.
        for block in &mut body.basic_blocks {
            for stmt in &mut block.statements {
                if let mir::StatementKind::Assign(_, rv) | mir::StatementKind::Reassign(_, rv) =
                    &mut stmt.kind
                {
                    Self::narrow_rvalue_floats(rv);
                }
            }
            if let Some(term) = &mut block.terminator {
                Self::narrow_terminator_floats(&mut term.kind);
            }
        }
    }

    fn narrow_operand_floats(op: &mut mir::Operand) {
        if let mir::Operand::Constant(c) = op {
            Self::narrow_type_kind(&mut c.ty.kind);
        }
    }

    fn narrow_rvalue_floats(rv: &mut mir::Rvalue) {
        match rv {
            mir::Rvalue::Use(o) => Self::narrow_operand_floats(o),
            mir::Rvalue::UnaryOp(_, o) => Self::narrow_operand_floats(o),
            mir::Rvalue::BinaryOp(_, a, b) => {
                Self::narrow_operand_floats(a);
                Self::narrow_operand_floats(b);
            }
            mir::Rvalue::Cast(o, ty) => {
                Self::narrow_operand_floats(o);
                Self::narrow_type_kind(&mut ty.kind);
            }
            mir::Rvalue::MathIntrinsic(_, args) | mir::Rvalue::Aggregate(_, args) => {
                for a in args {
                    Self::narrow_operand_floats(a);
                }
            }
            mir::Rvalue::Phi(pairs) => {
                for (o, _) in pairs {
                    Self::narrow_operand_floats(o);
                }
            }
            mir::Rvalue::Allocate(a, b, c) => {
                Self::narrow_operand_floats(a);
                Self::narrow_operand_floats(b);
                Self::narrow_operand_floats(c);
            }
            mir::Rvalue::Ref(_) | mir::Rvalue::Len(_) | mir::Rvalue::GpuIntrinsic(_) => {}
            mir::Rvalue::AtomicOp {
                buffer,
                index,
                value,
                compare_expected,
                ..
            } => {
                Self::narrow_operand_floats(buffer);
                Self::narrow_operand_floats(index);
                Self::narrow_operand_floats(value);
                if let Some(expected) = compare_expected {
                    Self::narrow_operand_floats(expected);
                }
            }
        }
    }

    fn narrow_terminator_floats(term: &mut mir::TerminatorKind) {
        match term {
            mir::TerminatorKind::SwitchInt { discr, .. } => Self::narrow_operand_floats(discr),
            mir::TerminatorKind::Call { func, args, .. } => {
                Self::narrow_operand_floats(func);
                for a in args {
                    Self::narrow_operand_floats(a);
                }
            }
            mir::TerminatorKind::VirtualCall { args, .. } => {
                for a in args {
                    Self::narrow_operand_floats(a);
                }
            }
            mir::TerminatorKind::Goto { .. }
            | mir::TerminatorKind::Return
            | mir::TerminatorKind::Unreachable
            | mir::TerminatorKind::GpuLaunch { .. } => {}
        }
    }

    /// Recursively narrow TypeKind, converting F64 to F32.
    fn narrow_type_kind(kind: &mut TypeKind) {
        match kind {
            TypeKind::Float => {
                *kind = TypeKind::F32;
            }
            TypeKind::F64 => {
                *kind = TypeKind::F32;
            }
            _ => {}
        }
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
