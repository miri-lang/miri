// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

mod line_map;
mod link;
mod lowering;

use lowering::{complete_generic_instantiation_registry, settled_type_facts};

use crate::ast::common::RuntimeKind;
use crate::ast::factory::{
    func, int_literal_expression, return_statement, type_expr_non_null, type_int,
};
use crate::ast::statement::{
    AcceleratorTarget, ClassData, FunctionDeclarationData, Statement, StatementKind,
};
use crate::ast::types::{Type, TypeKind};
use crate::ast::Program;
use crate::codegen::Backend;
use crate::codegen::{BuildTarget, CpuBackend, FunctionCodeMap};
use crate::diagnostics::codes::DiagnosticCode;
use crate::error::compiler::CompilerError;
use crate::error::diagnostic::Diagnostic;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::lexer::Lexer;
use crate::mir;
use crate::mir::call_graph::direct_callees;
use crate::mir::facts::{ProgramFacts, StaleFact};
use crate::mir::lowering::requests::LoweredBody;
use crate::mir::passes::{self, run_body_pass, PassContext, PassContract, PassManager};
use crate::mir::symbol::{Symbol, SymbolTable, TypeInstance};
use crate::parser::Parser;
#[cfg(feature = "cranelift")]
use crate::runtime_effects::RuntimeEffects;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::mir::lowering::instance_demand::{
    instantiated_method_symbol, ImpliedMethodVerdicts, InstanceMethod, MethodDemand,
};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::instantiation_requirements::pins_of;
use crate::type_checker::{CompiledInstance, DeclaredFunction, ModuleId, TypeChecker};

/// Result of running and capturing a program's output and exit status.
pub struct RunCaptureResult {
    /// The exit code from the process (from status.code() when available).
    pub exit_code: i32,
    /// The signal number if the process was killed by a signal (Unix only).
    pub signal: Option<i32>,
    /// Standard output bytes.
    pub stdout: Vec<u8>,
    /// Standard error bytes.
    pub stderr: Vec<u8>,
    /// The diagnostic code a runtime trap reported, if one did.
    pub trap_code: Option<DiagnosticCode>,
    /// What the run's GPU work cost, when it reached the GPU runtime at all.
    pub gpu: Option<GpuTelemetry>,
}

/// The residency operations a run paid for, as the GPU runtime counted them.
///
/// Absent rather than zeroed for a program that never reached the GPU runtime:
/// "no GPU work" and "GPU work that cost nothing" are different answers, and a
/// reader deciding whether a kernel's results were ever transferred back needs
/// to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GpuTelemetry {
    /// Host buffers copied to the device.
    pub uploads: u64,
    /// Kernel dispatches.
    pub launches: u64,
    /// Device buffers copied back to the host.
    pub readbacks: u64,
}

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

/// Returns true for a `Variable` statement whose bindings are all `const`.
///
/// A module-level `const` is a compile-time value with no initialization order,
/// so in script mode it stays at the top level rather than being demoted into
/// the synthetic `main` body. That keeps it visible to sibling functions — a
/// `const` used as an `Array<T, SIZE>()` size or referenced from a function body
/// declared elsewhere in the file. Mutable/immutable (`var`/`let`) bindings are
/// runtime globals and remain wrapped, preserving their execution order.
fn is_top_level_const_stmt(stmt: &Statement) -> bool {
    let StatementKind::Variable(decls, _) = &stmt.node else {
        return false;
    };
    !decls.is_empty()
        && decls.iter().all(|d| {
            matches!(
                d.declaration_type,
                crate::ast::statement::VariableDeclarationType::Constant
            )
        })
}

/// True when script-mode wrapping would demote this statement into a synthetic
/// `main` body. A program that declares its own `main` skips that wrapping, so
/// such a statement is silently dropped instead of running — which is why the
/// test runner refuses a test file containing one.
pub fn is_script_body_statement(stmt: &Statement) -> bool {
    is_wrappable_stmt(stmt) && !is_top_level_const_stmt(stmt)
}

/// True when running the program would execute something the author wrote.
///
/// A program qualifies by declaring `main`, or by carrying a statement that
/// script-mode wrapping would demote into a synthetic one. A file holding only
/// declarations satisfies neither: the synthetic `main` would contain nothing
/// but its own `return 0`, so building it yields an executable that prints
/// nothing and exits zero — which a caller reading the exit status cannot
/// distinguish from a program that ran and had nothing to say. `miri check`
/// still accepts such a file; validating a module is what `check` is for.
fn has_executable_entry_point(program: &Program) -> bool {
    program.body.iter().any(|stmt| {
        matches!(&stmt.node, StatementKind::FunctionDeclaration(decl) if decl.name == "main")
            || is_script_body_statement(stmt)
    })
}

/// True when the file declares a function the test runner would call.
///
/// Such a file has no `main` by design: a test file holds only declarations,
/// and the runner synthesizes the entry point. Reading it as a program with
/// nothing to run reports a correct artifact as suspect, and proposes an edit
/// — adding `main` — that the runner then refuses.
fn declares_a_test(program: &Program) -> bool {
    program.body.iter().any(|stmt| {
        let StatementKind::FunctionDeclaration(declaration) = &stmt.node else {
            return false;
        };
        declaration
            .attributes
            .iter()
            .any(|attribute| attribute.name == crate::ast::TEST_ATTRIBUTE)
    })
}

/// What `run` and `build` report for a program with nothing to execute.
///
/// A warning rather than an error, because an empty program, a file of `const`
/// declarations and a file of functions alone are all accepted shapes that
/// build and exit zero — behaviour the suite asserts directly. What was missing
/// is that the caller was never told, so a run that could not have produced
/// output looked exactly like one that had nothing to say.
fn nothing_to_run_warning() -> crate::error::diagnostic::Diagnostic {
    use crate::diagnostics::DiagnosticCode;
    use crate::error::diagnostic::DiagnosticBuilder;

    let code = DiagnosticCode::BldNothingToRun;
    DiagnosticBuilder::warning(code.title())
        .code(code.as_str())
        .message(
            "this file has nothing to run: it declares no 'main' and carries no top-level \
             statements",
        )
        .help(
            "add a 'main' function, or check the file instead with 'miri check' if it is a module.",
        )
        .build()
}

/// What `run` and `build` report for a file whose only callable code is tests.
///
/// A note rather than a warning: the file is correct as written, and the only
/// thing worth saying is that the command used does not run what it holds. The
/// executable a build produces from it calls none of its tests, so the report
/// names the command that does.
fn test_file_built_note() -> crate::error::diagnostic::Diagnostic {
    use crate::diagnostics::DiagnosticCode;
    use crate::error::diagnostic::DiagnosticBuilder;

    let code = DiagnosticCode::BldTestFileBuilt;
    DiagnosticBuilder::note(code.title())
        .code(code.as_str())
        .message(
            "this file declares '@test' functions and no other entry point, so the executable \
             built from it runs none of them",
        )
        .help("run the tests it declares with 'miri test' on this file.")
        .build()
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
    required_runtimes: BTreeSet<RuntimeKind>,
}

/// Walk the AST and collect all runtime function declarations.
///
/// Extracts the symbol names, parameter types, and return types so the codegen
/// backend can declare them as external imports, and collects which runtime
/// libraries must be linked.
fn collect_runtime_info(
    program: &Program,
    imported_stmts: &[Statement],
    #[cfg(feature = "cranelift")] declared: &RuntimeEffects,
    #[cfg(feature = "cranelift")] ptr_ty: Option<cranelift_codegen::ir::Type>,
) -> Result<RuntimeInfo, CompilerError> {
    #[cfg(feature = "cranelift")]
    let mut imports = Vec::new();
    let mut required_runtimes = BTreeSet::new();

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
    for declaration in runtime_declarations(all_stmts) {
        required_runtimes.insert(declaration.kind.clone());

        #[cfg(feature = "cranelift")]
        if let Some(ptr_ty) = ptr_ty {
            let Some(effect) = declared.get(declaration.name) else {
                return Err(CompilerError::Lowering(LoweringError::internal(
                    DiagnosticCode::MirValidationFailed,
                    format!("no record states what `{}` does", declaration.name),
                    Span::default(),
                )));
            };
            imports.push(runtime_import(
                declaration.name,
                effect,
                declaration.params,
                declaration.return_type,
                ptr_ty,
            ));
        }
    }

    Ok(RuntimeInfo {
        #[cfg(feature = "cranelift")]
        imports,
        required_runtimes,
    })
}

/// One `runtime` function declaration.
struct RuntimeDeclaration<'s> {
    kind: &'s RuntimeKind,
    name: &'s str,
    params: &'s [crate::ast::common::Parameter],
    return_type: Option<&'s crate::ast::expression::Expression>,
}

/// Every `runtime` function declared among `stmts`, at the top level or in
/// the body of a class.
fn runtime_declarations<'s>(
    stmts: impl Iterator<Item = &'s Statement>,
) -> impl Iterator<Item = RuntimeDeclaration<'s>> {
    stmts
        .flat_map(|stmt| std::iter::once(stmt).chain(class_body(stmt)))
        .filter_map(|stmt| {
            let StatementKind::RuntimeFunctionDeclaration(kind, name, params, return_type, _) =
                &stmt.node
            else {
                return None;
            };
            Some(RuntimeDeclaration {
                kind,
                name,
                params,
                return_type: return_type.as_deref(),
            })
        })
}

/// The statements of `stmt`'s body when it declares a class; none otherwise.
fn class_body(stmt: &Statement) -> &[Statement] {
    if let StatementKind::Class(class_data) = &stmt.node {
        &class_data.body
    } else {
        &[]
    }
}

/// The external declaration of the runtime entry point `name`, from the
/// parameters and return type its Miri declaration spells.
///
/// Each parameter is declared as [`crate::runtime_fns::param_abi`] says it
/// travels — an element as the address and byte count it is passed as, a
/// closure as itself and the address of the trampoline it is called through —
/// and an element result as the storage it is written into; see
/// [`crate::runtime_fns::abi_params`]. A parameter whose type is not written
/// as a type is declared as a pointer, so no later parameter moves out of the
/// position the call passes it in.
#[cfg(feature = "cranelift")]
fn runtime_import(
    name: &str,
    effect: &crate::runtime_effects::RuntimeEffect,
    params: &[crate::ast::common::Parameter],
    return_type: Option<&crate::ast::expression::Expression>,
    ptr_ty: cranelift_codegen::ir::Type,
) -> crate::codegen::cranelift::RuntimeImport {
    use crate::codegen::cranelift::translate_type;
    use crate::type_checker::resolve_type_name;

    let declared: Vec<_> = params
        .iter()
        .map(|p| resolve_type_name(&p.typ).map_or(ptr_ty, |t| translate_type(&t, ptr_ty)))
        .collect();
    crate::codegen::cranelift::RuntimeImport {
        name: name.to_string(),
        param_types: crate::runtime_fns::abi_params(effect, &declared, ptr_ty),
        return_type: return_type
            .filter(|_| !effect.returns_element_value())
            .and_then(|rt| resolve_type_name(rt).map(|t| translate_type(&t, ptr_ty))),
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
        StatementKind::Struct(_, _, _, methods, _, _)
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
fn wrap_script_in_main(program: &mut Program) -> Option<usize> {
    if has_main_function(program) {
        return None;
    }

    let return_zero = return_statement(Some(Box::new(int_literal_expression(0))));
    let int_ret = type_expr_non_null(type_int());

    if program.body.is_empty() {
        let body = crate::ast::factory::block(vec![return_zero]);
        let main_fn = func("main").return_type(int_ret).build(body);
        let id = main_fn.id;
        program.body = vec![main_fn];
        return Some(id);
    }

    // Separate top-level declarations (use, class, struct, etc.) from executable statements.
    let mut top_level = Vec::new();
    let mut body_stmts = Vec::new();

    // First, verify we can wrap the program. If not, return early.
    for stmt in &program.body {
        if !is_top_level_stmt(stmt) && !is_wrappable_stmt(stmt) {
            return None;
        }
    }

    let old_body = std::mem::take(&mut program.body);
    for stmt in old_body {
        if is_top_level_stmt(&stmt) || is_top_level_const_stmt(&stmt) {
            top_level.push(stmt);
        } else if is_wrappable_stmt(&stmt) {
            body_stmts.push(stmt);
        }
    }

    body_stmts.push(return_zero);
    let mut body = crate::ast::factory::block(body_stmts);
    crate::ast::exit_status::bare_returns_exit_zero(&mut body);
    let main_fn = func("main").return_type(int_ret).build(body);
    let id = main_fn.id;
    top_level.push(main_fn);
    program.body = top_level;
    Some(id)
}

/// Ensures that a user-defined `main()` function returns `Int` with an
/// implicit `return 0` at the end, so the process exits cleanly.
///
/// A bare `return` the author wrote in that body is given the same `0`, since
/// it would otherwise be checked against the `Int` installed here.
/// Give the program the entry point it is compiled with.
///
/// Returns the statement declaring the `main` a script was wrapped in, which
/// the type checker reads as the script's top-level code.
fn install_entry_point(program: &mut Program) -> Option<usize> {
    let script_main = wrap_script_in_main(program);
    patch_main_return(program);
    script_main
}

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
                    crate::ast::exit_status::bare_returns_exit_zero(body_stmt);
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

/// The warnings and notes a command raised while compiling a program.
pub type Notices = Vec<Diagnostic>;

/// A compile's outcome with the warnings and notes it raised, returned rather
/// than printed so a caller reporting in JSON can carry them in its envelope.
///
/// A failure in the frontend carries its warnings inside the error, so the
/// notices are empty then; a failure after it keeps the frontend's notices.
#[derive(Debug)]
pub struct Reported<T> {
    pub outcome: Result<T, CompilerError>,
    pub notices: Notices,
}

impl<T> Reported<T> {
    fn without_notices(outcome: Result<T, CompilerError>) -> Self {
        Self {
            outcome,
            notices: Vec::new(),
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
    /// The slots each vtable fills, settled once lowering has reached every
    /// body; empty until then.
    pub vtable_fills: mir::lowering::vtable_demand::VtableFills,
    /// The generic class methods lowering withheld because nothing that runs
    /// reaches them at an instance their obligations fail at; see
    /// `ImpliedMethodVerdicts`.
    pub withheld_methods: HashSet<Symbol>,
    /// The bodies GPU code reaches as device helpers, whose GPU clones a WGSL
    /// module declares and calls under their WGSL names; empty until lowering.
    pub device_helpers: HashSet<Symbol>,
}

/// Every lowered body under the name it is emitted as, and the bodies GPU
/// code reaches as device helpers.
#[derive(Debug)]
pub struct GpuProgram {
    pub bodies: Vec<(String, mir::Body)>,
    pub device_helpers: HashSet<Symbol>,
}

/// The name `run` builds the program it executes under, inside its work
/// directory.
pub const RUN_EXECUTABLE_NAME: &str = "program";

/// Where and how a run builds and starts its program, beyond the codegen
/// mode the pipeline holds.
#[derive(Debug)]
pub struct RunSetup<'d> {
    /// The directory the program is built and run in.
    pub work_dir: &'d Path,
    /// The intermediate forms the build writes beside the program.
    pub emit: Vec<Listing>,
    /// Variables set for the program on top of the ones the compiler sets.
    pub environment: Vec<(&'static str, std::ffi::OsString)>,
}

/// Options controlling the build process.
#[derive(Debug, Default)]
pub struct BuildOptions {
    /// Output path for the compiled artifact. If `None`, a temp directory is used.
    pub out_path: Option<PathBuf>,
    /// Whether to build in release mode: MIR records no local names, and
    /// compiled code carries none of the runtime's checking hooks (the heap
    /// guard, the leak check, the allocation and reference-count counters). The
    /// optimization level is set apart, in `opt_level`; see
    /// [`BuildOptions::resolve_opt_level`] for how a caller derives it.
    pub release: bool,
    /// Optimization level (0-3): 0 emits unoptimized code, 1-3 optimize for
    /// speed.
    pub opt_level: u8,
    /// Whether every function keeps a frame-pointer record, leaf functions
    /// included, so a profiler that walks frame pointers sees every caller.
    pub preserve_frame_pointers: bool,
    /// Which CPU backend to use for code generation.
    pub cpu_backend: CpuBackend,
    /// Target environment for the produced artifact.
    pub target: BuildTarget,
    /// For the `web-gpu` target, also emit a native host binary inside the
    /// bundle directory so the same GPU program can run on the host. Ignored by
    /// the `native` target (which always produces the executable). Off by
    /// default so bundle-only callers do not require a linker.
    pub emit_native_host: bool,
    /// The intermediate forms the native build also writes beside its
    /// executable, each as `<executable>.<form>`.
    pub emit: Vec<Listing>,
}

/// An intermediate form a build can write beside its executable, for a person
/// or a tool reading what the compiler made of a program.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listing {
    /// The MIR of every function, after every MIR pass, reference counting
    /// included.
    Mir,
    /// The Cranelift IR of every function, as Cranelift left it after
    /// optimizing.
    Clif,
    /// The source line each run of every function's machine code was
    /// generated from, for a profiler to read; see [`line_map`].
    Lines,
}

impl Listing {
    /// The extension the listing's file takes after the executable's name.
    fn extension(self) -> &'static str {
        match self {
            Listing::Mir => "mir",
            Listing::Clif => "clif",
            Listing::Lines => "lines",
        }
    }

    /// The forms a build made in `codegen` mode writes when `requested`
    /// were asked for: those, and for a profile build its line map, which
    /// the profiler attributing its samples to lines reads.
    pub fn written_by(mut requested: Vec<Listing>, codegen: CodegenMode) -> Vec<Listing> {
        if codegen.preserve_frame_pointers && !requested.contains(&Listing::Lines) {
            requested.push(Listing::Lines);
        }
        requested
    }
}

/// The object code a CPU backend produced for a program.
struct ObjectCode {
    bytes: Vec<u8>,
    required_runtimes: BTreeSet<RuntimeKind>,
    /// The backend's IR listing, when the build asked for one.
    clif: Option<String>,
    /// Where each function's code came from, when the build asked for its
    /// line map; empty otherwise.
    code_map: Vec<FunctionCodeMap>,
}

/// The MIR of every body, as `Listing::Mir` writes it.
fn mir_listing(bodies: &[(String, mir::Body)]) -> String {
    bodies
        .iter()
        .map(|(name, body)| format!("fn {name}\n{body}\n"))
        .collect()
}

/// Write `text` beside the executable at `out_path` as `<out_path>.<extension>`.
fn write_listing(out_path: &Path, listing: Listing, text: &str) -> Result<(), CompilerError> {
    let mut path = out_path.as_os_str().to_owned();
    path.push(".");
    path.push(listing.extension());
    fs::write(PathBuf::from(path), text)?;
    Ok(())
}

impl BuildOptions {
    /// The optimization level a build runs at: an explicit level wins,
    /// `release` alone means 3, and neither means 0.
    pub fn resolve_opt_level(release: bool, requested: Option<u8>) -> u8 {
        requested.unwrap_or(if release { 3 } else { 0 })
    }
}

/// How a build generates code: the part of [`BuildOptions`] the command
/// line's `--release`, `--profile` and `--opt-level` flags select.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodegenMode {
    /// Release mode, as [`BuildOptions::release`].
    pub release: bool,
    /// Optimization level, as [`BuildOptions::opt_level`].
    pub opt_level: u8,
    /// Frame pointers in every function, as
    /// [`BuildOptions::preserve_frame_pointers`].
    pub preserve_frame_pointers: bool,
}

impl CodegenMode {
    /// The mode the flags select. `profile` builds what `release` builds and
    /// keeps a frame pointer in every function; an explicit optimization level
    /// wins over either.
    pub fn from_flags(release: bool, profile: bool, opt_level: Option<u8>) -> Self {
        let release = release || profile;
        Self {
            release,
            opt_level: BuildOptions::resolve_opt_level(release, opt_level),
            preserve_frame_pointers: profile,
        }
    }
}

/// The linker argument that drops every section nothing in the program reaches.
///
/// A runtime library carries the whole of its standard library, and a program
/// reaches a small part of it. Without this the linker copies the rest into
/// every executable, with its debug information, and that copying is most of
/// what a link costs. Rust links its own executables the same way.
const DEAD_CODE_LINK_ARG: &str = if cfg!(target_os = "macos") {
    "-Wl,-dead_strip"
} else {
    "-Wl,--gc-sections"
};

/// Which build of a runtime's static library a program links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeBuild {
    /// The library every build but a profile build links.
    Standard,
    /// The library built with frame pointers, for a runtime that has one.
    FramePointers,
}

impl RuntimeBuild {
    /// The build a program made with `opts` links.
    fn for_options(opts: &BuildOptions) -> Self {
        if opts.preserve_frame_pointers {
            Self::FramePointers
        } else {
            Self::Standard
        }
    }

    /// The linker arguments a program linking this build adds. A profile
    /// build on Linux exports its symbols, so the sampler's `dladdr`, which
    /// there reads only an executable's exported symbols, names the runtime
    /// and compiled functions a sample lands in; macOS reads the whole
    /// symbol table and needs nothing.
    fn executable_link_args(self) -> &'static [&'static str] {
        match self {
            Self::FramePointers if cfg!(target_os = "linux") => &["-rdynamic"],
            Self::FramePointers | Self::Standard => &[],
        }
    }
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
    /// How `run` generates the code of the program it executes.
    run_codegen: CodegenMode,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// A pass that broke what it declares about the control-flow graph is a
/// fault of the compiler, not of the program.
impl From<PassContract> for CompilerError {
    fn from(broken: PassContract) -> Self {
        CompilerError::Internal(broken.to_string())
    }
}

/// A fact read after a rewrite left it stale is a fault of the compiler.
impl From<StaleFact> for CompilerError {
    fn from(stale: StaleFact) -> Self {
        CompilerError::Internal(stale.to_string())
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            source_dir: None,
            source_path: None,
            verify_mir: false,
            run_codegen: CodegenMode::default(),
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

    /// Configure how `run` generates the code of the program it executes.
    pub fn with_run_codegen(mut self, codegen: CodegenMode) -> Self {
        self.run_codegen = codegen;
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
    ///
    /// The program is checked as written: a script is not wrapped in `main`
    /// and `main` is not given an exit status. A command deciding whether a
    /// file compiles uses [`Self::frontend_program`] instead.
    pub fn frontend(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let ast = self.parse(source)?;
        self.check_parsed(ast, None)
    }

    /// Run the frontend with `main` given the exit status `build` and `run`
    /// compile it with.
    ///
    /// `check` and `fix` judge a file through here, so neither reads a program
    /// differently from `build`: a script is wrapped in a synthetic `main` as
    /// `build` wraps it, and its top-level bindings are that `main`'s locals,
    /// which a function the script declares cannot reach. A file of
    /// declarations alone is a module and is left unwrapped, so what it
    /// imports stays what it exposes.
    ///
    /// The program is also lowered to MIR, as `build` lowers it, and the bodies
    /// thrown away: an instantiation that grows without bound, an argument no
    /// instance can be built at, or a body the verifier refuses is found only
    /// while lowering, so a check that stopped at the type checker would accept
    /// a program `build` refuses.
    pub fn frontend_program(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let mut ast = self.parse(source)?;
        let script_main = if has_executable_entry_point(&ast) {
            install_entry_point(&mut ast)
        } else {
            patch_main_return(&mut ast);
            None
        };
        let mut result = self.check_parsed(ast, script_main)?;
        self.lower_to_mir(&mut result, false)?;
        Ok(result)
    }

    fn parse(&self, source: &str) -> Result<Program, CompilerError> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let mut ast = parser.parse_all().map_err(CompilerError::Parser)?;
        crate::ast::normalize::normalize(&mut ast);
        Ok(ast)
    }

    fn check_parsed(
        &self,
        mut ast: Program,
        script_main: Option<usize>,
    ) -> Result<PipelineResult, CompilerError> {
        let mut type_checker = self.make_type_checker();
        type_checker.read_as_script(script_main);
        type_checker
            .check(&mut ast)
            .map_err(|errors| CompilerError::TypeErrors {
                errors,
                warnings: type_checker.warnings().to_vec(),
            })?;
        complete_generic_instantiation_registry(&mut type_checker, &ast)?;

        Ok(PipelineResult {
            ast,
            type_checker,
            vtable_fills: Default::default(),
            withheld_methods: Default::default(),
            device_helpers: Default::default(),
        })
    }

    /// Run the frontend with script-mode wrapping: simple programs without function
    /// declarations are wrapped in a synthetic `main`.
    ///
    /// The warnings and notes the frontend raises are printed to stderr; a
    /// caller that reports them itself uses [`Self::frontend_script_reported`].
    pub fn frontend_script(&self, source: &str) -> Result<PipelineResult, CompilerError> {
        let (result, notices) = self.frontend_script_reported(source)?;
        self.print_notices(source, &notices);
        Ok(result)
    }

    /// Run the script frontend, returning the warnings and notes it raised
    /// beside the result instead of printing them.
    fn frontend_script_reported(
        &self,
        source: &str,
    ) -> Result<(PipelineResult, Notices), CompilerError> {
        let mut ast = self.parse(source)?;

        let nothing_to_run = !has_executable_entry_point(&ast);
        let declares_tests = declares_a_test(&ast);

        let script_main = install_entry_point(&mut ast);
        let mut result = self.check_parsed(ast, script_main)?;
        let type_checker = &mut result.type_checker;

        // Reported after checking, so a file that also has real errors reports
        // those instead: a module whose imports collide should say so, not that
        // it has no entry point.
        if nothing_to_run && !declares_tests {
            type_checker.record_warning(nothing_to_run_warning());
        }

        let mut notices = type_checker.warnings().to_vec();
        // A note rather than a recorded warning: a test file is correct without
        // an entry point, and the warning channel is what a caller holding the
        // build to a no-warnings bar reads.
        if nothing_to_run && declares_tests {
            notices.push(test_file_built_note());
        }

        Ok((result, notices))
    }

    /// Print `notices` to stderr as text, against this pipeline's source path.
    pub fn print_notices(&self, source: &str, notices: &[Diagnostic]) {
        for notice in notices {
            eprintln!(
                "{}",
                crate::error::format::format_diagnostic(
                    source,
                    notice,
                    self.source_path.as_deref(),
                )
            );
        }
    }

    /// Compile and execute the source, capturing and returning output.
    ///
    /// The result carries the exit status, the captured output, and the
    /// diagnostic code of a runtime trap when the program raised one.
    ///
    /// `program_args` become the spawned program's `argv[1..]`; the program path
    /// itself is `argv[0]`, as for any executable. They are an input to this one
    /// execution rather than to compilation, which is why they are a parameter
    /// instead of builder state like the source directory or the verifier flag.
    ///
    /// The warnings and notes compiling raised are printed to stderr; a caller
    /// that reports them itself uses [`Self::run_reported`].
    pub fn run_and_capture(
        &self,
        source: &str,
        program_args: &[String],
    ) -> Result<RunCaptureResult, CompilerError> {
        let Reported { outcome, notices } = self.run_reported(source, program_args);
        self.print_notices(source, &notices);
        outcome
    }

    /// Compile and execute the source as [`Self::run_and_capture`] does,
    /// returning the warnings and notes compiling raised beside the outcome
    /// instead of printing them.
    pub fn run_reported(
        &self,
        source: &str,
        program_args: &[String],
    ) -> Reported<RunCaptureResult> {
        let temp_dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(e) => {
                return Reported::without_notices(Err(CompilerError::Codegen(format!(
                    "Failed to create temp dir: {}",
                    e
                ))))
            }
        };
        let setup = RunSetup {
            work_dir: temp_dir.path(),
            emit: Vec::new(),
            environment: Vec::new(),
        };
        self.run_in_reported(source, program_args, &setup)
    }

    /// Compile and execute the source as [`Self::run_reported`] does, but
    /// in `setup.work_dir`, which the caller owns and may read afterwards:
    /// the program is built there as [`RUN_EXECUTABLE_NAME`], with the forms
    /// `setup.emit` names beside it, and starts with `setup.environment` on
    /// top of the compiler's own.
    pub fn run_in_reported(
        &self,
        source: &str,
        program_args: &[String],
        setup: &RunSetup<'_>,
    ) -> Reported<RunCaptureResult> {
        let executable_path = setup.work_dir.join(RUN_EXECUTABLE_NAME);
        let options = self.run_build_options(&executable_path, setup.emit.clone());
        let built = self.build_reported(source, &options);
        let outcome = built
            .outcome
            .and_then(|_| Self::execute(setup, &executable_path, program_args));
        Reported {
            outcome,
            notices: built.notices,
        }
    }

    /// How `run` builds the program it executes.
    fn run_build_options(&self, executable_path: &Path, emit: Vec<Listing>) -> BuildOptions {
        BuildOptions {
            out_path: Some(executable_path.to_path_buf()),
            release: self.run_codegen.release,
            opt_level: self.run_codegen.opt_level,
            preserve_frame_pointers: self.run_codegen.preserve_frame_pointers,
            cpu_backend: CpuBackend::Cranelift,
            target: BuildTarget::Native,
            emit_native_host: false,
            emit,
        }
    }

    /// Execute the program built at `executable_path` inside the setup's
    /// work directory, capturing what it printed and how it ended.
    fn execute(
        setup: &RunSetup<'_>,
        executable_path: &Path,
        program_args: &[String],
    ) -> Result<RunCaptureResult, CompilerError> {
        let work_dir = setup.work_dir;
        let trap_report_path = work_dir.join("trap");
        let gpu_report_path = work_dir.join("gpu_telemetry");

        // Ensure canonical paths before executing to prevent traversal or symlink attacks.
        let canonical_temp = work_dir.canonicalize().map_err(|e| {
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
            .args(program_args)
            .env("MIRI_TRAP_REPORT_PATH", &trap_report_path)
            .env("MIRI_GPU_TELEMETRY_PATH", &gpu_report_path)
            .envs(setup.environment.iter().map(|(name, value)| (name, value)))
            .output()
            .map_err(|e| CompilerError::Codegen(format!("Failed to execute program: {}", e)))?;

        let exit_code = output.status.code().unwrap_or(-1);

        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            output.status.signal()
        };
        #[cfg(not(unix))]
        let signal = None;

        let trap_code = read_trap_report(&trap_report_path);
        let gpu = read_gpu_telemetry_report(&gpu_report_path);

        Ok(RunCaptureResult {
            exit_code,
            signal,
            stdout: output.stdout,
            stderr: output.stderr,
            trap_code,
            gpu,
        })
    }

    /// Compile and execute the source, returning the process exit code.
    ///
    /// In pretty mode, this prints stdout and stderr directly, matching
    /// traditional program output. The returned exit code is from the
    /// spawned process.
    ///
    /// `program_args` become the spawned program's `argv[1..]`; the program path
    /// itself is `argv[0]`, as for any executable. They are an input to this one
    /// execution rather than to compilation, which is why they are a parameter
    /// instead of builder state like the source directory or the verifier flag.
    pub fn run(&self, source: &str, program_args: &[String]) -> Result<i32, CompilerError> {
        let result = self.run_and_capture(source, program_args)?;

        if !result.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&result.stdout));
        }
        if !result.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&result.stderr));
        }

        Ok(result.exit_code)
    }

    /// Compile source code to a native executable, returning the artifact path.
    ///
    /// The warnings and notes compiling raised are printed to stderr; a caller
    /// that reports them itself uses [`Self::build_reported`].
    pub fn build(&self, source: &str, opts: &BuildOptions) -> Result<PathBuf, CompilerError> {
        let Reported { outcome, notices } = self.build_reported(source, opts);
        self.print_notices(source, &notices);
        outcome
    }

    /// Compile as [`Self::build`] does, returning the warnings and notes the
    /// frontend raised beside the outcome instead of printing them. They are
    /// kept when a later stage fails, so a refused build still reports them.
    pub fn build_reported(&self, source: &str, opts: &BuildOptions) -> Reported<PathBuf> {
        match self.frontend_script_reported(source) {
            Ok((pipeline_result, notices)) => Reported {
                outcome: self.build_checked(source, pipeline_result, opts),
                notices,
            },
            Err(error) => Reported::without_notices(Err(error)),
        }
    }

    /// Lower and emit a program the frontend accepted.
    fn build_checked(
        &self,
        source: &str,
        mut pipeline_result: PipelineResult,
        opts: &BuildOptions,
    ) -> Result<PathBuf, CompilerError> {
        pipeline_result.type_checker.entry_source = Some(std::rc::Rc::from(source));
        pipeline_result.type_checker.entry_source_path = self.source_path().map(std::rc::Rc::from);
        let (mir_bodies, _) = self.lower_to_mir(&mut pipeline_result, opts.release)?;

        match opts.target {
            BuildTarget::Native => {
                self.build_native(&pipeline_result, mir_bodies, opts, opts.out_path.clone())
            }
            BuildTarget::WebGpu => {
                let gpu_buffer_inits = &pipeline_result.type_checker.gpu_buffer_inits;
                let bundle_dir = crate::codegen::web_gpu::emit_bundle(
                    crate::codegen::web_gpu::LoweredProgram {
                        bodies: &mir_bodies,
                        device_helpers: &pipeline_result.device_helpers,
                    },
                    opts.out_path.as_ref(),
                    Some(source),
                    if gpu_buffer_inits.is_empty() {
                        None
                    } else {
                        Some(gpu_buffer_inits)
                    },
                )?;
                // Re-emit the native host binary alongside the browser bundle so
                // the same GPU program can also run on the host. It lives inside
                // the bundle directory under the bundle's own name. Bundle-only
                // callers leave `emit_native_host` off and need no linker.
                if opts.emit_native_host {
                    let native_out = native_binary_path(&bundle_dir);
                    self.build_native(&pipeline_result, mir_bodies, opts, Some(native_out))?;
                }
                Ok(bundle_dir)
            }
        }
    }

    /// Compile the lowered program to a native executable and link it.
    ///
    /// `out_override` selects the output path: `Some` writes the executable
    /// there; `None` falls back to a unique temporary `a.out`. Shared by the
    /// `native` target and the `web-gpu` target (which also emits a browser
    /// bundle before calling this).
    fn build_native(
        &self,
        pipeline_result: &PipelineResult,
        mir_bodies: Vec<(String, mir::Body)>,
        opts: &BuildOptions,
        out_override: Option<PathBuf>,
    ) -> Result<PathBuf, CompilerError> {
        let mir_text = opts
            .emit
            .contains(&Listing::Mir)
            .then(|| mir_listing(&mir_bodies));
        let object = Self::compile_object(pipeline_result, mir_bodies, opts, self.verify_mode())?;
        let (work_dir, out_path) = Self::output_paths(out_override)?;
        fs::create_dir_all(&work_dir)?;

        // Derive a unique object path from the output path so that parallel
        // builds targeting different output files don't overwrite each other's
        // intermediate `.o` file.
        let object_path = out_path.with_extension("o");
        fs::write(&object_path, &object.bytes)?;
        self.link_executable(
            &object_path,
            &out_path,
            &object.required_runtimes,
            RuntimeBuild::for_options(opts),
        )?;
        if let Some(text) = mir_text {
            write_listing(&out_path, Listing::Mir, &text)?;
        }
        if let Some(text) = object.clif {
            write_listing(&out_path, Listing::Clif, &text)?;
        }
        if opts.emit.contains(&Listing::Lines) {
            let text = self.line_map_listing(pipeline_result, &object.code_map)?;
            write_listing(&out_path, Listing::Lines, &text)?;
        }
        Ok(out_path)
    }

    /// The line map of `code_map`, read against the program's file and every
    /// module it imported.
    fn line_map_listing(
        &self,
        pipeline_result: &PipelineResult,
        code_map: &[FunctionCodeMap],
    ) -> Result<String, CompilerError> {
        let type_checker = &pipeline_result.type_checker;
        let program = line_map::SourceFile {
            name: self
                .source_path()
                .and_then(|path| Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or(line_map::UNNAMED_PROGRAM),
            text: type_checker.entry_source.as_deref().unwrap_or_default(),
        };
        let files: Vec<line_map::SourceFile> = std::iter::once(program)
            .chain(
                type_checker
                    .imported_sources()
                    .map(|module| line_map::SourceFile {
                        name: &module.name,
                        text: &module.text,
                    }),
            )
            .collect();
        Ok(line_map::LineMap::new(&files, code_map)?.to_string())
    }

    /// The directory the build works in and the executable's path: `out`
    /// when given, otherwise a unique temporary `a.out`.
    // TODO: the temporary directory is kept so the artifact outlives the
    // process, and nothing ever removes it, so every build without `-o`
    // (the test suite's included) leaves a `miri_build_*` directory behind.
    fn output_paths(out: Option<PathBuf>) -> Result<(PathBuf, PathBuf), CompilerError> {
        if let Some(out) = out {
            let work_dir = out
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            return Ok((work_dir, out));
        }
        let unique_dir = tempfile::Builder::new()
            .prefix("miri_build_")
            .tempdir()
            .map_err(|e| {
                CompilerError::Codegen(format!("Failed to create build directory: {}", e))
            })?;
        #[allow(deprecated)]
        let unique_dir = unique_dir.into_path();
        let out = unique_dir.join("a.out");
        Ok((unique_dir, out))
    }

    /// Compile the lowered bodies to native object code with the CPU backend
    /// and optimization level `opts` name, along with the runtime libraries it
    /// must be linked against.
    fn compile_object(
        pipeline_result: &PipelineResult,
        mut mir_bodies: Vec<(String, mir::Body)>,
        opts: &BuildOptions,
        verify_mode: Option<String>,
    ) -> Result<ObjectCode, CompilerError> {
        // Spell the element and closure ABIs at the hand-off: reference
        // counting, verification and every MIR reader before this point see an
        // element or a closure argument as the value it is, and every CPU
        // backend takes an element as an address and a byte count, and a
        // closure as itself and the trampoline the runtime calls it through.
        for (name, body) in &mut mir_bodies {
            let elements = mir::element_abi::pass_elements_by_address;
            run_body_pass(&passes::PASS_ELEMENTS_BY_ADDRESS, body, elements)?
                .map_err(|e| CompilerError::Internal(format!("fn {name}: {e}")))?;
            let closures = mir::closure_abi::pass_closures_with_trampolines;
            run_body_pass(&passes::PASS_CLOSURES_WITH_TRAMPOLINES, body, closures)?
                .map_err(|e| CompilerError::Internal(format!("fn {name}: {e}")))?;
        }
        // An access emitted without its bounds check is re-proven on the very
        // bodies the backend receives, after every rewrite of them.
        if let Some(mode) = verify_mode {
            let violations = mir_bodies
                .iter()
                .flat_map(|(name, body)| {
                    mir::verify::verify_in_bounds(body)
                        .into_iter()
                        .map(move |v| (name.clone(), v))
                })
                .collect();
            Self::fail_or_warn(&mir::verify::ProgramReport::of(violations), &mode)?;
        }
        match opts.cpu_backend {
            CpuBackend::Cranelift => {
                Self::compile_with_cranelift(pipeline_result, &mir_bodies, opts)
            }
            CpuBackend::Llvm => {
                use crate::codegen::LlvmBackend;
                let backend = LlvmBackend;

                let runtime_info = collect_runtime_info(
                    &pipeline_result.ast,
                    &pipeline_result.type_checker.imported_statements,
                    #[cfg(feature = "cranelift")]
                    &pipeline_result.type_checker.runtime_effects,
                    #[cfg(feature = "cranelift")]
                    None,
                )?;

                let bodies_ref: Vec<(&str, &mir::Body)> = mir_bodies
                    .iter()
                    .map(|(name, body)| (name.as_str(), body))
                    .collect();

                let artifact = backend
                    .compile(&bodies_ref, &Default::default())
                    .map_err(CompilerError::from)?;

                Ok(ObjectCode {
                    bytes: artifact.bytes,
                    required_runtimes: runtime_info.required_runtimes,
                    clif: None,
                    code_map: artifact.code_map,
                })
            }
        }
    }

    /// Compile the element-ABI-spelled bodies with Cranelift at the level
    /// `opts` names, returning the object code, the runtime libraries it must
    /// be linked against and, when `opts` asks for it, the CLIF listing.
    fn compile_with_cranelift(
        pipeline_result: &PipelineResult,
        mir_bodies: &[(String, mir::Body)],
        opts: &BuildOptions,
    ) -> Result<ObjectCode, CompilerError> {
        #[cfg(feature = "cranelift")]
        {
            use crate::codegen::cranelift::{CraneliftOptions, OptLevel};
            use crate::codegen::CraneliftBackend;
            let mut backend = CraneliftBackend::new().map_err(CompilerError::from)?;
            backend.set_type_facts(settled_type_facts(pipeline_result, mir_bodies)?);
            backend.set_device_helpers(pipeline_result.device_helpers.clone());

            let runtime_info = collect_runtime_info(
                &pipeline_result.ast,
                &pipeline_result.type_checker.imported_statements,
                &pipeline_result.type_checker.runtime_effects,
                Some(backend.pointer_type()),
            )?;
            backend.set_runtime_imports(runtime_info.imports);

            let bodies_ref: Vec<(&str, &mir::Body)> = mir_bodies
                .iter()
                .map(|(name, body)| (name.as_str(), body))
                .collect();
            let options = CraneliftOptions {
                opt_level: OptLevel::for_level(opts.opt_level),
                ir_listing: opts.emit.contains(&Listing::Clif),
                runtime_checks: !opts.release,
                preserve_frame_pointers: opts.preserve_frame_pointers,
                code_map: opts.emit.contains(&Listing::Lines),
                profile_table: opts.preserve_frame_pointers,
                inline: opts.opt_level > 0,
                ..Default::default()
            };
            let artifact = backend
                .compile(&bodies_ref, &options)
                .map_err(CompilerError::from)?;
            Ok(ObjectCode {
                bytes: artifact.bytes,
                required_runtimes: runtime_info.required_runtimes,
                clif: artifact.ir_listing,
                code_map: artifact.code_map,
            })
        }
        #[cfg(not(feature = "cranelift"))]
        {
            let _ = (pipeline_result, mir_bodies, opts);
            Err(CompilerError::Codegen(
                "Cranelift backend not enabled. Recompile with --features cranelift".to_string(),
            ))
        }
    }

    /// Lower every body the program needs.
    ///
    /// Takes the result mutably because lowering an instantiation can reach a
    /// generic-class instantiation the registry does not hold yet, which is
    /// added before its methods are emitted and before codegen reads it.
    fn lower_to_mir(
        &self,
        result: &mut PipelineResult,
        is_release: bool,
    ) -> Result<(Vec<(String, mir::Body)>, ProgramFacts), CompilerError> {
        let lowered = self.lower_program(result, is_release)?;
        let launches: Vec<_> = (lowered.iter())
            .flat_map(|lowered| lowered.requests.kernel_workgroups.iter().cloned())
            .collect();
        let mut lowered: Vec<(Symbol, mir::Body)> = (lowered.into_iter())
            .map(|lowered| (lowered.symbol, lowered.body))
            .collect();
        let mut gpu_names =
            mir::gpu_names::GpuNames::collect(&lowered).map_err(CompilerError::Lowering)?;

        // Clone user functions that are transitively called from GPU kernels into
        // GpuDevice bodies for WGSL emission. Each clone is f32-narrowed for GPU compatibility.
        mir::gpu::clone_gpu_device_helpers(&mut lowered, &mut gpu_names)
            .map_err(CompilerError::Lowering)?;

        // Apply recorded workgroup sizes from kernel launches to GPU kernel bodies.
        // When kernel(args).launch(grid, block) is lowered, the block size is
        // requested beside the caller's body; each is applied to the
        // corresponding GPU kernel body's backend metadata.
        mir::gpu::stamp_kernel_workgroups(&mut lowered, &launches)
            .map_err(|error| CompilerError::Codegen(error.to_string()))?;

        let verify_mode = self.verify_mode();
        let definitions = result.type_checker.type_definitions();
        // A vtable slot is called through without a MIR call naming its body,
        // so a body some slot points at lends every parameter too.
        let mut synthesized = mir::lowering::dispatch_symbols::synthesized_references(definitions);
        synthesized.extend(result.vtable_fills.slot_symbols().cloned());
        let mut context = PassContext::new(definitions, synthesized, verify_mode.is_some());
        let mut facts = ProgramFacts::of(&lowered);
        let manager = PassManager::new(passes::MIR_PASSES)
            .map_err(|order| CompilerError::Internal(order.to_string()))?;
        manager.run(&mut lowered, &mut facts, &mut context)?;
        let program_violations = context.violations;

        let bodies: Vec<(String, mir::Body)> = lowered
            .into_iter()
            .map(|(symbol, body)| (gpu_names.emitted_name(&symbol, &body), body))
            .collect();
        result.device_helpers = gpu_names.into_helpers();

        // Check RC invariants, cross-residency readback fencing and optional
        // layouts after Perceus, and that every call passes an owned argument
        // exactly where its callee takes one over.
        //
        // TODO: these checks run before the element and closure ABI passes
        // rewrite the bodies in `compile_object`, so they never see the MIR
        // the backend consumes; only `verify_in_bounds` runs after them. Each
        // verifier must first learn the by-address element spelling and the
        // closure trampolines, or a post-ABI run reports false violations.
        if let Some(mode) = verify_mode {
            let definitions = result.type_checker.type_definitions();
            let report =
                mir::verify::verify_program(definitions, &bodies, &facts, program_violations)?;
            Self::fail_or_warn(&report, &mode)?;
        }

        Ok((bodies, facts))
    }

    /// How optional MIR verification runs: enabled by `--verify-mir` or by
    /// setting the MIRI_VERIFY_MIR environment variable. MIRI_VERIFY_MIR=warn
    /// only warns; any other non-empty value, or the flag, is a hard error.
    /// `None` when verification is off.
    fn verify_mode(&self) -> Option<String> {
        self.verify_mir
            .then(|| "error".to_string())
            .or_else(|| std::env::var("MIRI_VERIFY_MIR").ok())
    }

    /// Print `report` under `warn` mode, fail with it otherwise; nothing
    /// when it is empty.
    fn fail_or_warn(report: &mir::verify::ProgramReport, mode: &str) -> Result<(), CompilerError> {
        let Some(message) = report.message() else {
            return Ok(());
        };
        if mode == "warn" {
            eprintln!("{}", message);
            return Ok(());
        }
        Err(CompilerError::MirVerification(message))
    }

    /// Get MIR as a string for debugging purposes (pre-RC).
    pub fn get_mir(&self, source: &str) -> Result<String, CompilerError> {
        let mut pipeline_result = self.frontend_script(source)?;
        pipeline_result.type_checker.entry_source = Some(std::rc::Rc::from(source));
        pipeline_result.type_checker.entry_source_path = self.source_path().map(std::rc::Rc::from);
        let (mir_bodies, _) = self.lower_to_mir(&mut pipeline_result, false)?;

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
        self.get_gpu_mir_bodies_in(source, false)
    }

    /// [`Self::get_gpu_mir_bodies`] lowered the way `release` builds lower,
    /// where locals carry no names.
    pub fn get_gpu_mir_bodies_in(
        &self,
        source: &str,
        release: bool,
    ) -> Result<Vec<(String, crate::mir::Body)>, CompilerError> {
        self.get_gpu_program_in(source, release)
            .map(|program| program.bodies)
    }

    /// [`Self::get_gpu_mir_bodies_in`] together with the bodies GPU code
    /// reaches as device helpers, which a WGSL module built from those bodies
    /// declares and calls under their WGSL names.
    pub fn get_gpu_program_in(
        &self,
        source: &str,
        release: bool,
    ) -> Result<GpuProgram, CompilerError> {
        let mut result = self.frontend(source)?;
        let (bodies, _) = self.lower_to_mir(&mut result, release)?;
        Ok(GpuProgram {
            bodies,
            device_helpers: result.device_helpers,
        })
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
        // `lower_to_mir` already inserts and elides RC. Running either pass again
        // here would hand tests bodies carrying two of every operation.
        Ok(self.lower_to_mir(&mut pipeline_result, false)?.0)
    }

    /// [`Self::get_mir_bodies_with_rc`], with what the passes recorded about
    /// each body beside it, in the same order.
    pub fn get_mir_program_with_rc(
        &self,
        source: &str,
    ) -> Result<(Vec<(String, crate::mir::Body)>, ProgramFacts), CompilerError> {
        let mut pipeline_result = self.frontend_script(source)?;
        pipeline_result.type_checker.entry_source = Some(std::rc::Rc::from(source));
        pipeline_result.type_checker.entry_source_path = self.source_path().map(std::rc::Rc::from);
        self.lower_to_mir(&mut pipeline_result, false)
    }
}

/// Path of the native host binary emitted inside a `web-gpu` bundle directory.
/// Named after the bundle directory so it sits next to the `<name>.json`
/// manifest without colliding with it.
fn native_binary_path(bundle_dir: &Path) -> PathBuf {
    let name = bundle_dir
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("gpu_program");
    bundle_dir.join(name)
}

/// Longest GPU telemetry report the runtime can legitimately leave: five
/// `name=count` pairs over 64-bit counters.
const MAX_GPU_REPORT_LEN: u64 = 256;

/// Read the residency counters the GPU runtime left behind, if it ran at all.
///
/// Subject to the same rule as the trap report: the spawned program is
/// untrusted and receives the report path in its own environment, so the report
/// is honoured only when it is a regular file of a plausible size, and only the
/// counters this reader knows are taken from it. A counter the runtime does not
/// yet write, or one a program invented, reads as zero rather than as a number
/// the run did not pay.
fn read_gpu_telemetry_report(path: &std::path::Path) -> Option<GpuTelemetry> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_GPU_REPORT_LEN {
        return None;
    }

    let contents = std::fs::read_to_string(path).ok()?;
    let count_of = |name: &str| {
        contents
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .find(|(key, _)| *key == name)
            .and_then(|(_, value)| value.parse().ok())
            .unwrap_or(0)
    };
    Some(GpuTelemetry {
        uploads: count_of("uploads"),
        launches: count_of("launches"),
        readbacks: count_of("readbacks"),
    })
}

/// Longest trap report the runtime can legitimately leave: a diagnostic code
/// plus a trailing newline.
const MAX_TRAP_REPORT_LEN: u64 = 32;

/// Read the diagnostic code a runtime trap handler left behind, if any.
///
/// The report is written by the spawned program, which is untrusted: it receives
/// the report path in its own environment and can write whatever it likes there,
/// or replace the file entirely. So the report is honoured only when it is a
/// regular file rather than a link aimed somewhere else, is small enough to be a
/// code, and names a code the runtime is actually able to raise. Anything else is
/// discarded and the run is reported exactly as it would be with no report at
/// all, so a program can never invent a diagnostic it did not trigger.
fn read_trap_report(path: &std::path::Path) -> Option<DiagnosticCode> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_TRAP_REPORT_LEN {
        return None;
    }

    let contents = std::fs::read_to_string(path).ok()?;
    let code: DiagnosticCode = contents.trim().parse().ok()?;
    if code.area() != "RT" {
        return None;
    }
    Some(code)
}

#[cfg(test)]
mod gpu_telemetry_report_tests {
    use super::{read_gpu_telemetry_report, GpuTelemetry, MAX_GPU_REPORT_LEN};

    /// Write `contents` into a fresh file and read it back as a report.
    fn report_of(contents: &str) -> Option<GpuTelemetry> {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("gpu_telemetry");
        std::fs::write(&path, contents).expect("write report");
        read_gpu_telemetry_report(&path)
    }

    /// The record the GPU runtime writes, read back counter for counter. This
    /// pins the text the two crates agree on: the runtime cannot reach the
    /// compiler's parser, so a rename on either side shows up here.
    #[test]
    fn the_runtime_record_reads_back_counter_for_counter() {
        assert_eq!(
            report_of("uploads=1 launches=3 readbacks=2 fences=2 releases=1"),
            Some(GpuTelemetry {
                uploads: 1,
                launches: 3,
                readbacks: 2,
            })
        );
    }

    /// A counter this reader does not know is ignored rather than mis-read as
    /// one it does, and a counter the record omits reads as zero rather than as
    /// a number the run did not pay.
    #[test]
    fn an_unknown_counter_is_ignored_and_a_missing_one_reads_as_zero() {
        assert_eq!(
            report_of("launches=4 something_new=9"),
            Some(GpuTelemetry {
                uploads: 0,
                launches: 4,
                readbacks: 0,
            })
        );
    }

    /// The program that writes the report is untrusted: a record too large to
    /// be one, or a value that is not a count, is discarded rather than
    /// believed.
    #[test]
    fn an_oversized_or_unparseable_record_is_discarded() {
        let oversized = "x".repeat(MAX_GPU_REPORT_LEN as usize + 1);
        assert_eq!(report_of(&oversized), None);
        assert_eq!(
            report_of("uploads=lots launches=-1 readbacks=2"),
            Some(GpuTelemetry {
                uploads: 0,
                launches: 0,
                readbacks: 2,
            })
        );
    }

    /// A run that never reached the GPU runtime leaves no report, and no report
    /// is not a run that paid nothing.
    #[test]
    fn an_absent_report_is_absent_rather_than_zero() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            read_gpu_telemetry_report(&dir.path().join("never_written")),
            None
        );
    }
}
