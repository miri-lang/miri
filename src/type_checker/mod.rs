// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type checker module for Miri.
//!
//! This module is responsible for validating the type safety of Miri programs.
//! It performs type inference, checks type compatibility, validates operations,
//! and ensures that all assignments and function calls use compatible types.
//!
//! # Module Structure
//!
//! ## Core Modules
//! - [`context`] - Type checking context (scopes, symbols, type definitions)
//! - [`expressions`] - Expression type inference
//! - [`statements`] - Statement type checking
//!
//! ## Support Modules
//! - [`builtins`] - Built-in types and functions (String, Dim3, Future, print)
//! - [`call_instantiation`] - Which instantiation of a generic function a call reaches
//! - [`compatibility`] - Type compatibility and subtyping checks
//! - [`generics`] - Generic type inference and substitution
//! - [`operators`] - Binary and unary operator type validation
//! - [`utils`] - Type predicates, visibility, and error reporting
//! - [`value_class`] - Which values are scalars, bitwise values, words or managed

use crate::ast::bounds::Bounds;
use crate::ast::factory::make_type;
use crate::ast::types::Type;
use crate::ast::*;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;
use crate::type_checker::context::{GenericDefinition, MethodInfo};
use std::cell::OnceCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

pub(crate) mod atomic_buffers;
pub(crate) mod attributes;
mod binding_refinement;
mod bound_list;

use crate::type_checker::attributes::Deprecation;
pub mod builtins;
pub(crate) mod call_instantiation;
mod compatibility;
mod compiled_instances;
mod derived_conformance;
pub mod element_layout;
pub use compiled_instances::CompiledInstance;
pub mod context;
pub(crate) mod diagnostics;
pub mod escape_analysis;
pub mod expressions;
pub(crate) mod float_literals;
mod format_specs;
mod function_analysis;
pub use function_analysis::{CalleeKind, DeclaredFunction, ModuleId};
pub mod body_writes;
pub(crate) mod generics;
mod gpu_buffer_init;
mod gpu_integer_width;
mod hidden_types;
pub(crate) mod hygiene;
pub(crate) mod instantiation_requirements;
pub(crate) mod int_literals;
pub(crate) mod iteration_plan;
pub mod kept_parameters;
pub(crate) mod launch_rules;
pub mod literal_hooks;
pub(crate) mod member_hints;
pub(crate) mod module_loader;
mod operators;
pub use iteration_plan::{IterationPlan, PositionalAccess, ProtocolWalk};
pub(crate) use operators::{named_class, ValueOrdering, ValueStepping};
pub(crate) mod pattern_coverage;
mod reference_cycles;
pub mod reference_sharing;
pub(crate) mod runtime_settled;
mod source_arithmetic;
pub mod statements;
pub(crate) mod trait_self;
mod type_identities;
pub(crate) mod type_mismatch;
pub(crate) mod type_set_shorthand;
mod type_table;
pub mod use_after_move;
mod used_methods;
pub mod utils;
pub mod value_class;
mod value_demand;
pub(crate) mod vtable_selectors;

use context::{Context, SymbolInfo, TypeDefinition};
use diagnostics::DiagnosticCollector;
use function_analysis::FunctionAnalysis;
use module_loader::ModuleLoader;
pub use module_loader::ModuleSource;
use type_table::TypeTable;

pub use gpu_buffer_init::GpuBufferInit;

/// Extracts a `Type` from an AST type expression without a full `TypeChecker`.
///
/// This is used by the pipeline to translate runtime function parameter/return
/// types (which are always simple, non-generic types like `Int`, `Bool`,
/// `RawPtr`) into `Type` objects suitable for codegen translation.
///
/// Returns `None` if the expression is not a type expression.
pub fn resolve_type_name(expr: &Expression) -> Option<Type> {
    match &expr.node {
        ExpressionKind::Type(ty, _) => Some(*ty.clone()),
        _ => None,
    }
}

/// The program's top-level statements, a top-level block's own statements
/// standing in for the block.
fn top_level_statements(program: &Program) -> impl Iterator<Item = &Statement> + Clone {
    program.body.iter().flat_map(|statement| {
        if let StatementKind::Block(stmts) = &statement.node {
            stmts.as_slice()
        } else {
            std::slice::from_ref(statement)
        }
    })
}

/// The type parameters `generics` declares, by name, with no bounds: what a
/// type shell records before every type its bounds may name is registered.
fn unbounded_parameters(generics: &[Expression]) -> Vec<GenericDefinition> {
    generics
        .iter()
        .filter_map(|generic| {
            let ExpressionKind::GenericType(name_expr, _, _) = &generic.node else {
                return None;
            };
            let ExpressionKind::Identifier(name, _) = &name_expr.node else {
                return None;
            };
            Some(GenericDefinition {
                name: name.clone(),
                bounds: Bounds::default(),
            })
        })
        .collect()
}

/// Residency verdict for a function: whether it can safely accept gpu-resident args.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FnResidency {
    /// Function body contains host-forcing operations (println, element access).
    HostOnly,
    /// Function body only performs buffer-untouching ops (length on fixed arrays).
    PolymorphicSafe,
    /// Function body performs buffer access only inside forall bodies (device-context).
    /// Can accept gpu-resident args via device-handle ABI and is launch-safe.
    GpuLaunchSafe,
}

/// The TypeChecker struct is responsible for validating the type safety of the program.
/// It traverses the AST, infers types for expressions, and ensures that operations
/// and assignments are performed on compatible types.
#[derive(Debug)]
pub struct TypeChecker {
    /// Internal type-state management (types, definitions, hierarchy, visibility).
    pub(crate) type_table: TypeTable,
    /// Collects type errors, warnings, and deduplication state.
    pub diagnostics: DiagnosticCollector,
    /// Module loading state, visibility tracking, and metadata.
    pub(crate) modules: ModuleLoader,
    /// Function metadata: bodies, out-parameter flags, and residency analysis.
    pub(crate) fn_analysis: FunctionAnalysis,
    /// AST statements collected from imported modules.
    /// These need to be included in MIR lowering and codegen.
    pub imported_statements: Vec<Statement>,
    /// The class each `iterator()` declared to return a trait hands back,
    /// which a `for` loop's iteration plan reads; see [`iteration_plan`].
    pub(crate) returned_classes: iteration_plan::ReturnedClasses,
    /// The `iterator()` whose returned values are being noted, while its body
    /// is checked.
    pub(crate) returning_method: Option<iteration_plan::ReturningMethod>,
    /// How each `for` statement walks its value, settled once every class is
    /// known and read by lowering; see [`iteration_plan`].
    pub(crate) iteration_plans: iteration_plan::IterationPlans,
    /// The loops walking an iterator, held until their plans are settled.
    pub(crate) protocol_loops: Vec<iteration_plan::ProtocolLoop>,
    /// The methods that write their receiver, so a call of one on a value
    /// another binding shares copies it first; see [`body_writes`].
    pub receiver_writes: body_writes::ReceiverWrites,
    /// The parameters each body writes, so lowering binds a copy of each on
    /// entry; see [`body_writes`].
    pub parameter_writes: body_writes::ParameterWrites,
    /// Each mention of a binding a closure or nested function captures,
    /// recorded as bindings are resolved; see [`body_writes`].
    pub captured_mentions: body_writes::CapturedMentions,
    /// The operands lowering reads into a temporary as they are evaluated,
    /// because a later operand of the same sequence may write them; see
    /// [`body_writes`].
    pub held_operands: body_writes::HeldOperands,
    /// The call arguments the use-after-move pass found consumed, whose
    /// caller's reference lowering releases at the call; see
    /// [`use_after_move::ConsumedArguments`].
    pub consumed_arguments: use_after_move::ConsumedArguments,
    /// The resource parameters each function owns, which both sides of a
    /// call read; see [`kept_parameters::KeptParameters`].
    pub kept_parameters: kept_parameters::KeptParameters,
    /// What each declared runtime entry point does to its arguments and its
    /// result, as its `runtime` declaration states.
    pub runtime_effects: crate::runtime_effects::RuntimeEffects,
    /// Maps call expression IDs to their inferred generic type arguments (in declaration order).
    /// Populated when a generic function is called so MIR lowering can mangle the call target.
    pub call_generic_mappings: HashMap<usize, Vec<(String, Type)>>,
    /// Generic calls whose type parameters are not all bound yet, waiting for
    /// the location their result goes into — see [`call_instantiation`].
    pub(crate) open_generic_calls: call_instantiation::OpenGenericCalls,
    /// What each body requires of its own generic parameters, keyed by the
    /// declaration that stated it. Recorded while the generic body is checked
    /// and answered at every site that pins the parameter — see
    /// [`instantiation_requirements`].
    pub(crate) instantiation_requirements: instantiation_requirements::InstantiationRequirements,
    /// Every site that pins a generic body's parameters, recorded during the
    /// body pass and answered once all requirements are known.
    pub(crate) pinning_sites: Vec<instantiation_requirements::PinningSite>,
    /// Every class instance converted to a trait, whose methods a call
    /// through that trait reaches; see [`used_methods`].
    pub(crate) trait_conversions: Vec<used_methods::TraitConversion>,
    /// Every method called through a receiver whose type is a trait.
    pub(crate) trait_method_calls: Vec<used_methods::TraitMethodCall>,
    /// The program's global scope, kept once the check completes so a
    /// compiled instance is answered where every site was; see
    /// [`compiled_instances`].
    pub(crate) global_scope: Option<compiled_instances::GlobalScope>,
    /// Source text of the entry-point file, populated by the pipeline right
    /// before MIR lowering. Used by lowering passes (notably the testing
    /// intrinsic lowering) to convert byte spans into human-readable line
    /// numbers in runtime diagnostic messages.
    pub entry_source: Option<std::rc::Rc<str>>,
    /// Source path of the entry-point file. See [`entry_source`].
    pub entry_source_path: Option<std::rc::Rc<str>>,
    /// The statement declaring the synthetic `main` a script's top-level
    /// statements were wrapped in, whose body keeps the rules of top-level
    /// code; `None` for a program that wrote its own `main`.
    pub(crate) script_main: Option<usize>,
    /// Initial host data for `gpu` buffers bound to compile-time constant
    /// literals, keyed by where the binding's name is written (two functions
    /// may each declare a buffer of the same name). Populated at the end of
    /// [`check`] and consumed by the web-gpu bundle emitter.
    pub gpu_buffer_inits: HashMap<Span, GpuBufferInit>,
    /// Distinct resolved type-argument tuples each generic class is instantiated
    /// with, keyed by class name (e.g. `"Box"` → `[[int], [float]]`). Populated
    /// at constructor inference so a later monomorphization pass can emit one
    /// specialized body per instantiation without re-walking the AST. Tuples are
    /// deduplicated by their type kinds (source spans are ignored).
    pub generic_class_instantiations: HashMap<String, Vec<Vec<Type>>>,
    /// Expression ids of integer literals that appear directly under a unary
    /// negation. `i64::MIN` can only be written as `-9223372036854775808`, whose
    /// bare operand (2^63) is one past `i64::MAX`; that magnitude is in range
    /// only in this negated position, so its literal range check is relaxed.
    pub(crate) negated_int_literals: HashSet<usize>,
    /// Integer literals held until the type they were written into is known. A
    /// literal is inferred before the declaration, argument, field, element,
    /// assignment or return that records its width is resolved, so the bound
    /// cannot be applied where the check runs; these are judged once the bodies
    /// are checked.
    pub(crate) deferred_int_literal_ranges:
        Vec<crate::type_checker::expressions::literals::DeferredIntLiteralRange>,
    /// Kernel-code integer literals past `i32::MAX`, held until their recorded
    /// type decides which 32-bit device lane (signed or unsigned) bounds them.
    pub(crate) deferred_gpu_int_literal_ranges:
        Vec<crate::type_checker::expressions::literals::DeferredIntLiteralRange>,
    /// Names of top-level (`module scope`) `const`/`let`/`var` bindings already
    /// registered by the declaration-collection pass. Function bodies checked
    /// earlier in source order resolve these through the shared context, so a
    /// forward reference (`fn f(): return G` before `const G = ...`) type-checks.
    /// A binding in this set is re-registered without a fresh shadow check when
    /// the body pass revisits its declaration, since that is the same binding,
    /// not a redeclaration.
    pub(crate) hoisted_top_level: HashSet<String>,
    /// When true, [`report_error`](Self::report_error) and its siblings drop the
    /// diagnostic instead of recording it. Set only while the declaration-collection
    /// pass hoists a top-level binding or provisionally declares a class, both of
    /// which are fully re-checked (and their diagnostics emitted) later; suppressing
    /// here avoids duplicate or premature errors from that speculative first pass.
    pub(crate) suppress_diagnostics: bool,
    /// Set while a value returned at a written `Self` is judged: it reaches
    /// the caller at the receiver's own class, so it hands no trait value on.
    pub(crate) returning_at_written_self: bool,
    /// The refined types of local bindings a later store bound an open type
    /// argument of, and whether the body being checked needs another pass.
    pub(crate) binding_refinements: binding_refinement::BindingRefinements,
    /// When true, a `Custom` type name that is not visible in the importing
    /// scope still resolves from `global_type_definitions`.
    ///
    /// Set only while reading a *callee's declared* signature. Such a type was
    /// written by the module that declares the callee, not by the caller, so
    /// gating it on what the caller imported is wrong: a selective import like
    /// `use system.json.{Json}` would otherwise fail to resolve the `JsonError`
    /// in `Json.parse`'s declared `Result<Json, JsonError>`, even though the
    /// caller never names that type. User-written type expressions keep the
    /// visibility gate, so an unimported name is still rejected where the author
    /// actually wrote it.
    pub(crate) resolving_declared_signature: bool,
    /// Where the call whose callee's declared signature is being read is
    /// written, while one is.
    pub(crate) declared_signature_site: Option<Span>,
    /// Maps the name of each `@deprecated` declaration to what it is and why it
    /// was deprecated. Populated during declaration collection so that use
    /// sites — a call, an instantiation, an enum-variant reference — can emit
    /// warnings.
    pub(crate) deprecated_declarations: HashMap<String, Deprecation>,
    /// The argument count at a method call site, used only for arity-aware
    /// member suggestions. Set temporarily in `infer_call` when the function
    /// being called is a member access (`Member(..)` expression), then consumed
    /// (via `take()`) at the start of member-access inference. This ensures
    /// arity information is only visible when inferring the receiver's members,
    /// not during sub-expression inference (preventing leakage from nested calls).
    /// Diagnostic-only: does not affect what resolves or compiles.
    pub(crate) call_site_arity: Option<usize>,
    /// The expression a call being inferred calls, by id. A generic function
    /// named there is instantiated by the call; named anywhere else it is a
    /// value no call instantiates, which is refused.
    pub(crate) callee_expr_id: Option<usize>,
    /// The expression whose type a member access is reading its member off,
    /// while it is being inferred: `self` there is a receiver, not a value
    /// handed on.
    pub(crate) member_receiver_expr_id: Option<usize>,
    /// Whether the member access being resolved reads its member off a
    /// parameter of the function being checked.
    pub(crate) member_receiver_is_parameter: bool,
    /// Where to report a type the compiler resolved on the reader's behalf.
    ///
    /// A callee's declared type reaches the caller as an expression the
    /// compiler built, not one anybody wrote, so it carries no source range.
    /// A diagnostic left to speak for such a node renders at the file's first
    /// byte — pointing at whatever happens to sit on line 1 instead of at the
    /// expression that needed the type. Set for the length of one such
    /// resolution by `resolve_type_argument_at`.
    pub(crate) synthesized_type_use_site: Option<Span>,
    /// Whether the type expression about to be resolved may be a type set.
    ///
    /// A set (`f32 or float`) bounds a type parameter and is never the type of
    /// a value, so only the two places that write one — a type parameter's
    /// `is` bound and a `type` declaration's target — set this, for the one
    /// resolution that follows. Every other resolution refuses a set.
    pub(crate) type_set_allowed: bool,
    /// The public type-set names each module file declares, read once per
    /// file by the signature shorthand (see [`type_set_shorthand`]).
    pub(crate) type_set_exports: HashMap<std::path::PathBuf, Vec<String>>,
    /// A shared copy of the global type definitions, taken the first time a
    /// later stage asks for one and dropped whenever a definition changes, so
    /// every body lowered from one finished check reads one table without
    /// copying it.
    pub(crate) shared_type_definitions: OnceCell<Rc<HashMap<String, TypeDefinition>>>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    /// Creates a new type checker with built-in types and functions pre-loaded.
    ///
    /// The type checker is initialized with:
    /// - Built-in types: `String`, `Dim3`, `GpuContext`, `Kernel`, `Future<T>`
    /// - Built-in functions: `print<T>`
    pub fn new() -> Self {
        let (global_scope, global_type_definitions) = builtins::initialize_builtins();
        Self {
            type_table: TypeTable::new(global_scope, global_type_definitions),
            diagnostics: DiagnosticCollector::new(),
            modules: ModuleLoader::new(),
            fn_analysis: FunctionAnalysis::new(),
            imported_statements: Vec::new(),
            returned_classes: HashMap::new(),
            returning_method: None,
            iteration_plans: HashMap::new(),
            protocol_loops: Vec::new(),
            receiver_writes: body_writes::ReceiverWrites::default(),
            parameter_writes: body_writes::ParameterWrites::default(),
            captured_mentions: body_writes::CapturedMentions::default(),
            held_operands: body_writes::HeldOperands::default(),
            consumed_arguments: use_after_move::ConsumedArguments::default(),
            kept_parameters: kept_parameters::KeptParameters::default(),
            runtime_effects: HashMap::new(),
            call_generic_mappings: HashMap::new(),
            open_generic_calls: call_instantiation::OpenGenericCalls::default(),
            instantiation_requirements: HashMap::new(),
            pinning_sites: Vec::new(),
            trait_conversions: Vec::new(),
            trait_method_calls: Vec::new(),
            global_scope: None,
            entry_source: None,
            entry_source_path: None,
            script_main: None,
            gpu_buffer_inits: HashMap::new(),
            generic_class_instantiations: HashMap::new(),
            negated_int_literals: HashSet::new(),
            deferred_int_literal_ranges: Vec::new(),
            deferred_gpu_int_literal_ranges: Vec::new(),
            hoisted_top_level: HashSet::new(),
            suppress_diagnostics: false,
            returning_at_written_self: false,
            binding_refinements: binding_refinement::BindingRefinements::default(),
            resolving_declared_signature: false,
            declared_signature_site: None,
            deprecated_declarations: HashMap::new(),
            call_site_arity: None,
            callee_expr_id: None,
            member_receiver_expr_id: None,
            member_receiver_is_parameter: false,
            synthesized_type_use_site: None,
            type_set_allowed: false,
            type_set_exports: HashMap::new(),
            shared_type_definitions: OnceCell::new(),
        }
    }

    /// Read the body of the function statement `id` declares as the top-level
    /// code of a script, which the pipeline wrapped in it.
    pub fn read_as_script(&mut self, id: Option<usize>) {
        self.script_main = id;
    }

    /// Creates a new type checker with a known source-file directory.
    ///
    /// The directory is used to resolve `local.*` module imports relative to
    /// the project root (i.e. the directory that contains the entry-point file).
    pub fn with_source_dir(source_dir: PathBuf) -> Self {
        let mut tc = Self::new();
        tc.modules.source_dir = Some(source_dir);
        tc
    }

    /// Sets the current module name for scoping declarations.
    pub fn set_current_module(&mut self, name: String) {
        self.modules.current_module = name;
    }

    /// Returns the inferred type for a given expression ID.
    pub fn get_type(&self, id: usize) -> Option<&Type> {
        self.type_table.types.get(&id)
    }

    /// Every type the checker inferred for an expression, in no fixed order.
    ///
    /// A type a program reaches but never writes down — the `List<W>` an
    /// `Array<W, 3>.reversed()` hands back — appears only here, so the
    /// monomorphization registry is completed from this.
    pub fn inferred_types(&self) -> impl Iterator<Item = &Type> {
        self.type_table.types.values()
    }

    /// Returns the type of a global variable by name.
    pub fn get_variable_type(&self, name: &str) -> Option<&Type> {
        self.type_table.global_scope.get(name).map(|info| &info.ty)
    }

    /// Returns the module name where the given variable/function is defined.
    pub fn get_variable_module(&self, name: &str) -> Option<&str> {
        self.type_table
            .global_scope
            .get(name)
            .map(|info| info.module.as_str())
    }

    /// Returns whether a global variable is a constant.
    pub fn is_constant(&self, name: &str) -> bool {
        self.type_table
            .global_scope
            .get(name)
            .map(|info| info.is_constant)
            .unwrap_or(false)
    }

    /// How a call through the identifier expression `expr_id` is compiled,
    /// as the declaration it resolved to where it was written says.
    pub fn callee_kind(&self, expr_id: usize) -> CalleeKind {
        self.fn_analysis
            .callee_kinds
            .get(&expr_id)
            .copied()
            .unwrap_or(CalleeKind::Program)
    }

    /// The name the intrinsic the identifier expression `expr_id` resolved to
    /// was declared under, which an alias it was imported by does not change.
    pub fn intrinsic_name(&self, expr_id: usize) -> Option<&str> {
        self.fn_analysis
            .intrinsic_names
            .get(&expr_id)
            .map(String::as_str)
    }

    /// The function the identifier expression `expr_id` resolved to where it
    /// was written, when that is a function this compilation lowers.
    pub fn declared_callee(&self, expr_id: usize) -> Option<&DeclaredFunction> {
        self.fn_analysis.declared_callees.get(&expr_id)
    }

    /// Which parameters the function the callee expression `expr_id` names
    /// owns ([`kept_parameters::KeptParameters`]); empty when it names none
    /// or the function owns none.
    pub fn kept_parameters_of_callee(&self, expr_id: usize) -> &[bool] {
        self.declared_callee(expr_id)
            .map_or(&[], |function| self.kept_parameters.of(function))
    }

    /// Which parameters the top-level function or method `statement_id`
    /// declares owns, in the order a call passes its operands, a method's
    /// receiver first; empty when it owns none.
    pub fn kept_parameters_of_declaration(&self, statement_id: usize) -> &[bool] {
        self.kept_parameters.of_declaration(statement_id)
    }

    /// Which parameters the method `method` of the type `owner` owns, in the
    /// order a call passes its operands, the receiver first; empty when it
    /// owns none.
    pub fn kept_parameters_of_method(&self, owner: &str, method: &str) -> &[bool] {
        self.kept_parameters.of_method(owner, method)
    }

    /// The module the top-level function declaration `statement_id` is
    /// written in: the module a `use` loaded it from, or the program's own
    /// file. `None` for a statement the type checker never recorded, for
    /// which no module is guessed.
    pub fn declaring_module(&self, statement_id: usize) -> Option<&ModuleId> {
        self.fn_analysis.declaring_modules.get(&statement_id)
    }

    /// The imported file that declares the function `statement_id`. `None`
    /// for the program's own functions, whose spans index the program's file.
    pub fn declaration_module_source(&self, statement_id: usize) -> Option<&ModuleSource> {
        self.modules
            .declaration_sources
            .get(&statement_id)
            .map(AsRef::as_ref)
    }

    /// The (file_path, source_text) of the imported file that declares the
    /// function `statement_id`, as [`Self::declaration_module_source`] finds it.
    pub fn declaration_source(&self, statement_id: usize) -> Option<(String, String)> {
        self.declaration_module_source(statement_id)
            .map(|source| (source.path.clone(), source.text.clone()))
    }

    /// The file the module `module` was loaded from, when it is an imported
    /// module this check loaded.
    pub fn module_source(&self, module: &ModuleId) -> Option<&ModuleSource> {
        self.modules
            .imported_sources
            .iter()
            .map(AsRef::as_ref)
            .find(|source| source.module == *module)
    }

    /// Whether the program's own file declares the function `statement_id`:
    /// a function or method written there, or the `main` a script's
    /// top-level statements were wrapped in.
    pub fn declares_in_program(&self, statement_id: usize) -> bool {
        self.modules.program_declarations.contains(&statement_id)
            || self.script_main == Some(statement_id)
    }

    /// Every imported module's file, in [`crate::error::SourceId`] order from 1.
    pub fn imported_sources(&self) -> impl Iterator<Item = &ModuleSource> {
        self.modules.imported_sources.iter().map(AsRef::as_ref)
    }

    /// Record the callee the identifier expression `expr_id`, written as
    /// `written`, names, having resolved to the declaration `info`.
    pub(crate) fn record_callee(&mut self, expr_id: usize, written: &str, info: &SymbolInfo) {
        if self.is_declared_diverging(written, info) {
            self.fn_analysis.diverging_callees.insert(expr_id);
        }
        match CalleeKind::of(info) {
            CalleeKind::Program => {
                if matches!(info.ty.kind, TypeKind::Function(_)) {
                    let declared = DeclaredFunction::resolved(written, info);
                    if self.callee_expr_id != Some(expr_id) {
                        self.fn_analysis.function_values.insert(declared.clone());
                    }
                    self.fn_analysis.declared_callees.insert(expr_id, declared);
                }
            }
            CalleeKind::Intrinsic => {
                let declared = info.original_name.as_deref().unwrap_or(written);
                self.fn_analysis
                    .intrinsic_names
                    .insert(expr_id, declared.to_string());
                self.fn_analysis
                    .callee_kinds
                    .insert(expr_id, CalleeKind::Intrinsic);
            }
            kind @ (CalleeKind::Runtime | CalleeKind::VariantConstructor) => {
                self.fn_analysis.callee_kinds.insert(expr_id, kind);
            }
        }
    }

    /// Returns the global type definitions.
    pub fn type_definitions(&self) -> &HashMap<String, TypeDefinition> {
        &self.type_table.global_type_definitions
    }

    /// The global type definitions as one shared table, copied once per state
    /// of the table rather than once per reader.
    pub fn shared_type_definitions(&self) -> Rc<HashMap<String, TypeDefinition>> {
        Rc::clone(
            self.shared_type_definitions
                .get_or_init(|| Rc::new(self.type_table.global_type_definitions.clone())),
        )
    }

    /// Forgets the shared copy of the global type definitions, which a change
    /// to any of them makes stale.
    pub(crate) fn invalidate_shared_type_definitions(&mut self) {
        self.shared_type_definitions.take();
    }

    /// Returns the global scope (function/variable declarations).
    pub fn global_scope(&self) -> &HashMap<String, SymbolInfo> {
        &self.type_table.global_scope
    }

    /// Returns the current module name.
    pub fn current_module(&self) -> &str {
        &self.modules.current_module
    }

    /// The declaration a call's callee expression resolved to: a bare name, or
    /// a member read through a module alias (`M.f`).
    pub fn declaration_called_by(&self, callee: &Expression) -> Option<&DeclaredFunction> {
        match &callee.node {
            ExpressionKind::Identifier(..) => self.declared_callee(callee.id),
            ExpressionKind::Member(_, member) => self.declared_callee(member.id),
            _ => None,
        }
    }

    /// The residency verdict of the function `callee` names.
    pub fn callee_residency(&self, callee: &Expression) -> Option<FnResidency> {
        let declared = self.declaration_called_by(callee)?;
        self.fn_analysis.fn_residencies.get(declared).copied()
    }

    /// The out-parameter flags of the function `callee` names.
    pub fn callee_out_params(&self, callee: &Expression) -> Option<&Vec<bool>> {
        let declared = self.declaration_called_by(callee)?;
        self.fn_analysis.function_out_params.get(declared)
    }

    /// Whether `info`, the declaration `written` resolved to, is `@diverges`.
    fn is_declared_diverging(&self, written: &str, info: &SymbolInfo) -> bool {
        !self.fn_analysis.diverging_functions.is_empty()
            && self
                .fn_analysis
                .diverging_functions
                .contains(&DeclaredFunction::resolved(written, info))
    }

    /// Records the declaration `name` of the module being checked as one that
    /// never returns, when its `attributes` say so.
    pub(crate) fn note_diverging_declaration(&mut self, name: &str, attributes: &[Attribute]) {
        if attributes::has_attribute(attributes, DIVERGES_ATTRIBUTE) {
            let declared = self.declared_here(name);
            self.fn_analysis.diverging_functions.insert(declared);
        }
    }

    /// The one answer to whether a call through `callee` can come back: its
    /// declaration says, and a name that shadows it is a different declaration.
    pub(crate) fn callee_never_returns(&self, callee: &Expression) -> bool {
        let named = if let ExpressionKind::Member(_, member) = &callee.node {
            member.id
        } else {
            callee.id
        };
        self.fn_analysis.diverging_callees.contains(&named)
    }

    /// The function a declaration named `name` in the module being checked is.
    pub(crate) fn declared_here(&self, name: &str) -> DeclaredFunction {
        DeclaredFunction {
            module: ModuleId::checked_as(&self.modules.current_module),
            name: name.to_string(),
        }
    }

    /// Returns the type-checking warnings.
    pub fn warnings(&self) -> &[crate::error::diagnostic::Diagnostic] {
        &self.diagnostics.warnings
    }

    /// Records a warning raised outside the checking passes.
    ///
    /// The pipeline uses this for a warning about the program as a whole that
    /// only `run` and `build` should raise, so it travels with the rest rather
    /// than through a channel of its own.
    pub fn record_warning(&mut self, warning: crate::error::diagnostic::Diagnostic) {
        self.diagnostics.push_warning(warning);
    }

    /// Walk the inheritance chain to find a static method, avoiding ClassDefinition clones.
    ///
    /// Returns `Some((defining_class_name, method_info))` if found, `None` otherwise.
    /// Only clones the single `MethodInfo` found, never the entire class chain.
    /// For most use cases, use the default resolver; for visibility-checked access, pass a custom resolver.
    pub(crate) fn find_static_method_in_chain(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<(String, MethodInfo)> {
        self.find_static_method_in_chain_with_resolver(class_name, method_name, &|name| {
            self.type_table.global_type_definitions.get(name).cloned()
        })
    }

    /// Walk the inheritance chain to find a static method using a custom type resolver.
    ///
    /// The `resolver` function is called with each class name in the chain to resolve
    /// its TypeDefinition. This allows both direct lookups and visibility-checked lookups.
    /// Only clones the single `MethodInfo` found, never the entire class chain.
    pub(crate) fn find_static_method_in_chain_with_resolver<F>(
        &self,
        class_name: &str,
        method_name: &str,
        resolver: &F,
    ) -> Option<(String, MethodInfo)>
    where
        F: Fn(&str) -> Option<TypeDefinition>,
    {
        let mut current_class_name = class_name.to_string();

        loop {
            if let Some(TypeDefinition::Class(class_def)) = resolver(&current_class_name) {
                if let Some(method_info) = class_def.methods.get(method_name) {
                    return Some((current_class_name, method_info.clone()));
                }

                // Move to base class if it exists
                if let Some(base_name) = &class_def.base_class {
                    current_class_name = base_name.clone();
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    }

    /// Main entry point for type checking a program.
    pub fn check(&mut self, program: &mut Program) -> Result<(), Vec<TypeError>> {
        self.desugar_type_set_parameters(program);
        let program = &*program;
        let mut context = Context::new();
        self.load_prelude(&mut context);

        self.record_declaring_modules(ModuleId::Program, program);
        self.record_program_declarations(program);
        self.run_pass_collect_type_shells(program);
        self.load_shadowable_prelude(&mut context);
        self.load_regex_prelude_for(&program.body, &mut context);
        self.run_pass_collect_type_aliases(program, &mut context);
        self.run_pass_collect_declarations(program, &mut context);
        self.check_top_level_shape(program);
        self.run_pass_check_bodies(program, &mut context);
        self.settle_iteration_plans();
        self.report_deferred_int_literal_ranges();
        self.report_deferred_gpu_int_literal_ranges();
        self.answer_pinning_sites(&context);
        self.run_pass_escape_summaries(program, &mut context);
        self.run_pass_use_after_move(program, &context);
        self.run_pass_kept_parameters(program, &context);
        self.check_hygiene(program);
        self.receiver_writes = body_writes::ReceiverWrites::of(
            top_level_statements(program).chain(&self.imported_statements),
            |name| self.type_identity_named_by(name).map(str::to_string),
            &self.runtime_effects,
        );
        let lost_capture_writes;
        (
            self.parameter_writes,
            self.held_operands,
            lost_capture_writes,
        ) = body_writes::analyse(
            top_level_statements(program).chain(&self.imported_statements),
            self,
            &context,
        );
        self.report_lost_capture_writes(lost_capture_writes);
        self.global_scope = Some(compiled_instances::GlobalScope(context));

        if self.diagnostics.is_empty() {
            self.collect_gpu_buffer_initializers(program);
            Ok(())
        } else {
            Err(self.diagnostics.clone_errors())
        }
    }

    fn run_pass_collect_type_shells(&mut self, program: &Program) {
        self.collect_type_shells(top_level_statements(program));
    }

    fn run_pass_collect_type_aliases(&mut self, program: &Program, context: &mut Context) {
        for statement in &program.body {
            self.collect_type_aliases_in(statement, context);
        }
    }

    /// Registers the aliases a top-level statement, or a block of them,
    /// declares whose targets already resolve.
    pub(crate) fn collect_type_aliases_in(&mut self, statement: &Statement, context: &mut Context) {
        let statements = if let StatementKind::Block(stmts) = &statement.node {
            stmts.as_slice()
        } else {
            std::slice::from_ref(statement)
        };
        for stmt in statements {
            if let StatementKind::Type(exprs, _) = &stmt.node {
                self.collect_type_aliases(exprs, context);
            }
        }
    }

    fn run_pass_collect_declarations(&mut self, program: &Program, context: &mut Context) {
        for statement in &program.body {
            if let StatementKind::Block(stmts) = &statement.node {
                for stmt in stmts {
                    self.collect_declaration(stmt, context);
                }
            } else {
                self.collect_declaration(statement, context);
            }
        }
    }

    fn run_pass_check_bodies(&mut self, program: &Program, context: &mut Context) {
        let defined = self.define_types(top_level_statements(program), context);
        self.check_iterator_bodies(top_level_statements(program), &defined, context);
        for statement in top_level_statements(program) {
            self.check_statement_after_definitions(statement, &defined, context);
        }
        // A call opened outside any statement — an initializer the hoisting
        // pass checked and the body pass never reached again — is refused
        // here rather than lowered at no instantiation.
        self.refuse_open_generic_calls_since(0);
    }

    fn run_pass_escape_summaries(&mut self, program: &Program, context: &mut Context) {
        if !self.diagnostics.is_empty() {
            return;
        }
        let facts = escape_analysis::ProgramFacts {
            types: &self.type_table.types,
            type_defs: &self.type_table.global_type_definitions,
            callees: &self.fn_analysis.declared_callees,
            modules: &self.fn_analysis.declaring_modules,
        };
        context.escape_summaries = escape_analysis::compute_escape_summaries(
            &program.body,
            &self.imported_statements,
            &facts,
            escape_analysis::runtime_escape_summaries(&self.runtime_effects),
        );
    }

    fn run_pass_use_after_move(&mut self, program: &Program, context: &Context) {
        if !self.diagnostics.is_empty() {
            return;
        }
        let findings = use_after_move::UseAfterMoveChecker::new(
            self,
            &self.type_table.types,
            &self.type_table.global_type_definitions,
            &context.escape_summaries,
            &self.fn_analysis.declared_callees,
        )
        .check_program(program);
        self.diagnostics.extend_errors(findings.errors);
        self.diagnostics.extend_warnings(findings.warnings);
        self.consumed_arguments.extend(findings.consumed_arguments);
    }

    /// Decides which resource parameters each of the program's own
    /// top-level functions owns ([`kept_parameters`]), from the converged
    /// escape summaries and the functions named as values.
    ///
    /// Only the program's bodies are decided: the consumption check walks
    /// those alone, and an owned parameter needs its consumption recorded to
    /// be handed on by `move`. A program already refused decides nothing:
    /// its escape summaries and types may be incomplete, and no MIR is
    /// lowered from it.
    fn run_pass_kept_parameters(&mut self, program: &Program, context: &Context) {
        if !self.diagnostics.is_empty() {
            return;
        }
        let type_definitions = &self.type_table.global_type_definitions;
        let selectors = vtable_selectors::vtable_selectors(type_definitions);
        let facts = kept_parameters::KeepFacts {
            type_definitions,
            escape_summaries: &context.escape_summaries,
            modules: &self.fn_analysis.declaring_modules,
            function_values: &self.fn_analysis.function_values,
            selectors: &selectors,
        };
        self.kept_parameters =
            kept_parameters::decide_kept_parameters(top_level_statements(program), &facts);
    }

    /// Registers every class, trait, struct and enum among `statements` as
    /// an empty shell, then resolves the bounds of each shell's type
    /// parameters. Runs before `collect_declaration` so that signatures can
    /// resolve forward references to types declared later in the same module
    /// (or in a module that recursively imports us back); a bound is read
    /// only once every shell is registered, so it may name a later type too.
    pub(crate) fn collect_type_shells<'a>(
        &mut self,
        statements: impl Iterator<Item = &'a Statement>,
    ) {
        let shells: Vec<(String, &[Expression])> = statements
            .filter_map(|statement| self.collect_type_shell(statement))
            .collect();
        for (name, generics) in shells {
            self.resolve_shell_bounds(&name, generics);
        }
    }

    /// Registers the type `statement` declares as a shell whose type
    /// parameters carry no bounds yet, and returns the registered name with
    /// the written parameter list when the shell declares parameters.
    fn collect_type_shell<'a>(
        &mut self,
        statement: &'a Statement,
    ) -> Option<(String, &'a [Expression])> {
        match &statement.node {
            StatementKind::Class(class_data) => self
                .shell_class(class_data)
                .zip(class_data.generics.as_deref()),
            StatementKind::Trait(name_expr, generics_expr, _, _, visibility) => self
                .shell_trait(name_expr, generics_expr.as_deref(), visibility)
                .zip(generics_expr.as_deref()),
            StatementKind::Struct(name_expr, generics_expr, _, _, visibility, _) => self
                .shell_struct(name_expr, generics_expr.as_deref(), visibility)
                .zip(generics_expr.as_deref()),
            StatementKind::Enum(name_expr, generics_expr, _, _, visibility, _) => self
                .shell_enum(name_expr, generics_expr.as_deref(), visibility)
                .zip(generics_expr.as_deref()),
            _ => None,
        }
    }

    /// Gives the shell registered as `name` the type parameters `generics`
    /// declares, each bound resolved.
    fn resolve_shell_bounds(&mut self, name: &str, generics: &[Expression]) {
        let mut context = Context::new();
        let resolved = Some(self.extract_generic_definitions(generics, &mut context));
        match self.type_table.global_type_definitions.get_mut(name) {
            Some(TypeDefinition::Class(definition)) => definition.generics = resolved,
            Some(TypeDefinition::Trait(definition)) => definition.generics = resolved,
            Some(TypeDefinition::Struct(definition)) => definition.generics = resolved,
            Some(TypeDefinition::Enum(definition)) => definition.generics = resolved,
            Some(TypeDefinition::Generic(_) | TypeDefinition::Alias(_)) | None => {}
        }
        self.invalidate_shared_type_definitions();
    }

    fn shell_class(&mut self, class_data: &crate::ast::statement::ClassData) -> Option<String> {
        let name = self.register_declared_identity(&class_data.name, &class_data.visibility)?;
        if self.type_table.global_type_definitions.contains_key(&name) {
            return None;
        }
        let generics = class_data.generics.as_deref().map(unbounded_parameters);
        let base_class_name: Option<String> = class_data
            .base_class
            .as_ref()
            .and_then(|b| self.extract_type_identity(b).ok().map(String::from));
        let trait_names: Vec<String> = class_data
            .traits
            .iter()
            .filter_map(|t| self.extract_type_identity(t).ok().map(String::from))
            .collect();
        self.register_type_definition(
            name.clone(),
            TypeDefinition::Class(context::ClassDefinition {
                name: name.clone(),
                generics,
                base_class: base_class_name,
                base_class_args: None,
                traits: trait_names,
                trait_args: HashMap::new(),
                fields: Vec::new(),
                methods: BTreeMap::new(),
                module: self.modules.current_module.clone(),
                is_abstract: class_data.is_abstract,
                runtime_settled_methods: std::collections::BTreeSet::new(),
            }),
        );
        self.modules.pre_registered_types.insert(name.clone());
        Some(name)
    }

    fn shell_trait(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&[Expression]>,
        visibility: &MemberVisibility,
    ) -> Option<String> {
        let name_str = self.register_declared_identity(name_expr, visibility)?;
        if self
            .type_table
            .global_type_definitions
            .contains_key(&name_str)
        {
            return None;
        }
        let generics = generics_expr.map(unbounded_parameters);
        self.register_type_definition(
            name_str.clone(),
            TypeDefinition::Trait(context::TraitDefinition {
                name: name_str.clone(),
                generics,
                parent_traits: vec![],
                parent_trait_args: BTreeMap::new(),
                methods: BTreeMap::new(),
                module: self.modules.current_module.clone(),
            }),
        );
        self.modules.pre_registered_types.insert(name_str.clone());
        Some(name_str)
    }

    fn shell_struct(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&[Expression]>,
        visibility: &MemberVisibility,
    ) -> Option<String> {
        let name = self.register_declared_identity(name_expr, visibility)?;
        if self.type_table.global_type_definitions.contains_key(&name) {
            return None;
        }
        let generics = generics_expr.map(unbounded_parameters);
        self.register_type_definition(
            name.clone(),
            TypeDefinition::Struct(context::StructDefinition {
                fields: vec![],
                generics,
                module: self.modules.current_module.clone(),
            }),
        );
        self.modules.pre_registered_types.insert(name.clone());
        Some(name)
    }

    fn shell_enum(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&[Expression]>,
        visibility: &MemberVisibility,
    ) -> Option<String> {
        let name = self.register_declared_identity(name_expr, visibility)?;
        if self.type_table.global_type_definitions.contains_key(&name) {
            return None;
        }
        let generics = generics_expr.map(unbounded_parameters);
        self.register_type_definition(
            name.clone(),
            TypeDefinition::Enum(context::EnumDefinition {
                variants: BTreeMap::new(),
                generics,
                methods: BTreeMap::new(),
                module: self.modules.current_module.clone(),
                must_use: false,
                non_exhaustive: false,
            }),
        );
        self.modules.pre_registered_types.insert(name.clone());
        Some(name)
    }

    /// Preliminary pass to register declarations without checking their bodies.
    fn collect_declaration(&mut self, statement: &Statement, context: &mut Context) {
        match &statement.node {
            StatementKind::FunctionDeclaration(decl) => {
                self.collect_function_decl(decl, context);
            }
            StatementKind::IntrinsicFunctionDeclaration(
                name,
                generics,
                params,
                return_type,
                visibility,
            ) => {
                self.collect_intrinsic_decl(
                    name,
                    generics,
                    params,
                    return_type,
                    visibility,
                    context,
                );
            }
            StatementKind::Class(class_data) => self.collect_class_decl(class_data, context),
            StatementKind::Struct(..) | StatementKind::Enum(..) | StatementKind::Trait(..) => {
                self.collect_type_shells(std::iter::once(statement));
            }
            StatementKind::Use(path_expr, alias) => {
                self.check_use(path_expr, alias, context);
            }
            StatementKind::Variable(decls, visibility) => {
                self.collect_variable_decl(decls, visibility, context, statement.span);
            }
            StatementKind::Type(exprs, _) => self.collect_type_aliases(exprs, context),
            _ => {}
        }
    }

    /// Hoists top-level `const`/`let`/`var` bindings so a function body checked
    /// earlier in source order can resolve them (e.g. `Array<T, SIZE>()` or a
    /// bare `return G` referencing a const declared further down the module).
    ///
    /// The shadow diagnostics run here, in source order, against any pre-existing
    /// binding (prelude, imports, or an earlier top-level declaration). The first
    /// occurrence of each name is then registered with diagnostics suppressed —
    /// its initializer is fully re-checked in the body pass, which owns the real
    /// error reporting. A repeated name is left registered under its first
    /// declaration; the body pass skips its shadow check via [`hoisted_top_level`].
    fn collect_variable_decl(
        &mut self,
        decls: &[crate::ast::statement::VariableDeclaration],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        for decl in decls {
            let is_mutable = matches!(
                decl.declaration_type,
                crate::ast::statement::VariableDeclarationType::Mutable
                    | crate::ast::statement::VariableDeclarationType::Unmarked
            );
            let is_constant = matches!(
                decl.declaration_type,
                crate::ast::statement::VariableDeclarationType::Constant
            );
            let first_occurrence = self.hoisted_top_level.insert(decl.name.clone());
            self.check_shadowing(&decl.name, is_mutable, is_constant, context, span);
            if !first_occurrence {
                continue;
            }
            let prev = self.suppress_diagnostics;
            self.suppress_diagnostics = true;
            self.register_variable_decl(decl, visibility, context, span, None);
            self.suppress_diagnostics = prev;
        }
    }

    fn collect_function_decl(
        &mut self,
        decl: &crate::ast::statement::FunctionDeclarationData,
        context: &mut Context,
    ) {
        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: decl.generics.clone(),
            params: decl.params.to_vec(),
            return_type: decl.return_type.clone(),
        })));

        if context.scopes.len() == 1 {
            self.type_table.global_scope.insert(
                decl.name.clone(),
                SymbolInfo::new(
                    func_type.clone(),
                    false,
                    false,
                    decl.properties.visibility.clone(),
                    self.modules.current_module.clone(),
                    None,
                ),
            );
        }

        self.note_diverging_declaration(&decl.name, &decl.attributes);
        context.define(
            decl.name.clone(),
            SymbolInfo::new(
                func_type,
                false,
                false,
                decl.properties.visibility.clone(),
                self.modules.current_module.clone(),
                None,
            ),
        );
    }

    fn collect_intrinsic_decl(
        &mut self,
        name: &str,
        generics: &Option<Vec<Expression>>,
        params: &[crate::ast::Parameter],
        return_type: &Option<Box<Expression>>,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: generics.clone(),
            params: params.to_vec(),
            return_type: return_type.clone(),
        })));

        if context.scopes.len() == 1 {
            self.type_table.global_scope.insert(
                name.to_string(),
                SymbolInfo::new_intrinsic(
                    func_type.clone(),
                    visibility.clone(),
                    self.modules.current_module.clone(),
                ),
            );
        }

        context.define(
            name.to_string(),
            SymbolInfo::new_intrinsic(
                func_type,
                visibility.clone(),
                self.modules.current_module.clone(),
            ),
        );
    }

    fn collect_class_decl(
        &mut self,
        class_data: &crate::ast::statement::ClassData,
        context: &mut Context,
    ) {
        let Some(name) = self.register_declared_identity(&class_data.name, &class_data.visibility)
        else {
            return;
        };
        let is_pre_shell = self.modules.pre_registered_types.contains(&name)
            && matches!(
                self.type_table.global_type_definitions.get(&name),
                Some(TypeDefinition::Class(c)) if c.methods.is_empty()
            );
        if self.type_table.global_type_definitions.contains_key(&name) && !is_pre_shell {
            return;
        }
        // This is a provisional declaration, made so a module importing this
        // one back sees the methods before the class is defined. A signature
        // may name a type the declaring module has yet to import, so nothing
        // is reported here: `define_class` resolves every member again, with
        // the module's imports in scope, and reports what still fails.
        let prev = std::mem::replace(&mut self.suppress_diagnostics, true);
        self.collect_provisional_class(name, class_data, context);
        self.suppress_diagnostics = prev;
    }

    fn collect_provisional_class(
        &mut self,
        name: String,
        class_data: &crate::ast::statement::ClassData,
        context: &mut Context,
    ) {
        let generics = class_data
            .generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));
        let base_class_name: Option<String> = class_data
            .base_class
            .as_ref()
            .and_then(|b| self.extract_type_identity(b).ok().map(String::from));
        let trait_names: Vec<String> = class_data
            .traits
            .iter()
            .filter_map(|t| self.extract_type_identity(t).ok().map(String::from))
            .collect();

        self.register_class_shell(&name, &generics, &base_class_name, &trait_names, class_data);

        let (methods, base_direct_args) =
            self.scan_class_body(&name, base_class_name.as_deref(), class_data, context);

        self.register_type_definition(
            name.clone(),
            TypeDefinition::Class(context::ClassDefinition {
                name,
                generics,
                base_class: base_class_name,
                base_class_args: base_direct_args,
                traits: trait_names,
                trait_args: HashMap::new(),
                fields: Vec::new(),
                methods,
                module: self.modules.current_module.clone(),
                is_abstract: class_data.is_abstract,
                runtime_settled_methods: std::collections::BTreeSet::new(),
            }),
        );
    }

    fn register_class_shell(
        &mut self,
        name: &str,
        generics: &Option<Vec<context::GenericDefinition>>,
        base_class_name: &Option<String>,
        trait_names: &[String],
        class_data: &crate::ast::statement::ClassData,
    ) {
        self.register_type_definition(
            name.to_string(),
            TypeDefinition::Class(context::ClassDefinition {
                name: name.to_string(),
                generics: generics.clone(),
                base_class: base_class_name.clone(),
                base_class_args: None,
                traits: trait_names.to_vec(),
                trait_args: HashMap::new(),
                fields: Vec::new(),
                methods: BTreeMap::new(),
                module: self.modules.current_module.clone(),
                is_abstract: class_data.is_abstract,
                runtime_settled_methods: std::collections::BTreeSet::new(),
            }),
        );
        self.modules.pre_registered_types.insert(name.to_string());
    }

    fn scan_class_body(
        &mut self,
        name: &str,
        base_class_name: Option<&str>,
        class_data: &crate::ast::statement::ClassData,
        context: &mut Context,
    ) -> (
        BTreeMap<String, context::MethodInfo>,
        Option<Vec<crate::ast::types::Type>>,
    ) {
        context.enter_scope();
        if let Some(gens) = &class_data.generics {
            self.define_generics(gens, context);
        }
        let class_type = self.type_at_own_parameters(name, class_data.generics.as_deref(), context);
        context.enter_class(
            name.to_string(),
            base_class_name.map(String::from),
            class_type,
        );

        let base_direct_args = class_data.base_class.as_ref().and_then(|be| {
            if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) = &be.node {
                Some(
                    args.iter()
                        .map(|arg| self.resolve_type_expression(arg, context))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        });

        let methods = self.collect_class_method_signatures(&class_data.body, context);
        context.exit_class();
        context.exit_scope();
        (methods, base_direct_args)
    }

    fn collect_class_method_signatures(
        &mut self,
        body: &[Statement],
        context: &mut Context,
    ) -> BTreeMap<String, context::MethodInfo> {
        let mut methods = BTreeMap::new();
        for stmt in body {
            let StatementKind::FunctionDeclaration(decl) = &stmt.node else {
                continue;
            };
            let (generics, (params, return_ty)) =
                self.with_member_generics(decl.generics.as_deref(), context, |checker, context| {
                    checker.resolve_member_signature(decl, context)
                });
            let is_out_flags: Vec<bool> = decl.explicit_params().iter().map(|p| p.is_out).collect();
            let is_abstract = decl.body.as_ref().is_none_or(|b| {
                matches!(&b.node, StatementKind::Empty)
                    || matches!(&b.node, StatementKind::Block(stmts) if stmts.is_empty())
            });
            methods.insert(
                decl.name.clone(),
                context::MethodInfo {
                    generics,
                    params,
                    is_out_flags,
                    return_type: return_ty,
                    visibility: decl.properties.visibility.clone(),
                    is_constructor: decl.name == "init",
                    is_abstract,
                    is_static: false,
                    attributes: decl.attributes.clone(),
                },
            );
        }
        methods
    }
}
